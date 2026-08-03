//! Per-task install reports — the persisted form of a [`TaskDetail`] list, so
//! a finished install/import task can be reopened later from the instance's
//! History rather than existing only for the lifetime of the progress toast
//! that first showed it.
//!
//! # Storage
//!
//! `<instance>/.lucerna/reports/<taskId>.json`, one file per task. Deliberately
//! NOT under `<instance>/logs/`: that directory is enumerated by
//! `logs::files::allowed_roots`, so a report there would be listed to the user
//! as a log file, AND `logs::retention` (which protects only `latest.log`,
//! `debug.log`, and the launcher's own log) would treat it as fair game —
//! `launch::spawn::apply_from_settings` runs retention on every game exit, so
//! a report there would be silently deleted in normal use. Living beside
//! `journal.jsonl` and `playtime.json` in `.lucerna/` keeps it out of reach of
//! every log path guard and every cleanup pass. It is also NOT under
//! `<instance>/lucerna/` (no dot) — that directory is the installed-content
//! registries (`installed-mods.json`, `installed-datapacks.json`), a
//! different convention entirely.
//!
//! # Rotation
//!
//! A directory of per-task files has no single file size to rewrite-on-grow
//! the way the journal does, so pruning runs after every write and deletes
//! whatever [`logs::retention::select_excess`] — the same pure, I/O-free
//! selector the log-retention paths share — says is excess, ranked newest
//! first by mtime. Its sibling `delete_one` is NOT reused here: it asserts the
//! victim lives under one of the four log roots, and `.lucerna/` deliberately
//! is not one of them. Deletion goes through plain `std::fs::remove_file`
//! instead. Caps: [`MAX_REPORT_FILES`] and [`MAX_REPORTS_TOTAL_BYTES`].
//!
//! # Failure policy
//!
//! Persisting a report must never fail the install/import it describes, so
//! call sites use the infallible [`record`]. Errors go to the launcher log.
//!
//! # Clones
//!
//! Reports do not travel with `instances::clone` (which copies `.lucerna/`
//! file-by-file via explicit calls, not a blanket copy). That is deliberate,
//! not an oversight: a report is the per-file detail of one journalled
//! operation on THIS instance, and the journal itself does not travel to a
//! clone either (see `clone::tests::clone_never_inherits_the_source_journal`).

use crate::error::{Error, Result};
use crate::logs::retention::{select_excess, RetentionInput};
use crate::tasks::TaskDetail;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::{Path, PathBuf};

const META_DIR: &str = ".lucerna";
const REPORTS_DIR: &str = "reports";
const FILE_EXT: &str = "json";

const FILE_VERSION: u32 = 1;

/// Kept newest-first when a task produces more reports than fit. Sized around
/// "a handful of recent installs" — enough that History can page back through
/// a meaningful stretch of activity without the directory growing unbounded.
const MAX_REPORT_FILES: usize = 50;
/// Byte budget across all kept reports. Each report is small (per-file JSON
/// rows, no jar bytes), so this is a generous ceiling meant to catch a
/// pathological case (a modpack with thousands of files) rather than bind in
/// normal use.
const MAX_REPORTS_TOTAL_BYTES: u64 = 8 * 1024 * 1024;

/// A persisted report: one task's [`TaskDetail`] rows plus the metadata
/// needed to find and order them again later.
///
/// Deliberately NOT `#[derive(Default)]` — a default `version: 0` would look
/// like a pre-`FILE_VERSION` file to a future migration and silently take the
/// migration branch on a value nothing ever wrote (see
/// `datapacks::registry::OnDisk`'s doc comment for the bug this avoids).
/// Every construction site below sets `version: FILE_VERSION` explicitly.
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
pub struct TaskReport {
    #[serde(default = "default_version")]
    pub version: u32,
    pub task_id: String,
    /// Wall-clock time the report was written. `f64` — specta-typescript
    /// forbids 64-bit integer exports (same convention as
    /// `journal::JournalEntry::at_unix_ms`).
    pub at_unix_ms: f64,
    pub details: Vec<TaskDetail>,
}

fn default_version() -> u32 {
    FILE_VERSION
}

pub fn reports_dir(instance_root: &Path) -> PathBuf {
    instance_root.join(META_DIR).join(REPORTS_DIR)
}

pub fn report_path(instance_root: &Path, task_id: &str) -> PathBuf {
    reports_dir(instance_root).join(format!("{task_id}.{FILE_EXT}"))
}

/// Persist `details` for `task_id`, stamped with the current time.
///
/// Infallible by design: a report write is never worth failing the install it
/// describes. Errors go to the launcher log.
pub fn record(instance_root: &Path, task_id: impl Into<String>, details: Vec<TaskDetail>) {
    // Timestamp NOW, not when the write lands: the runtime branch below can
    // hand the write off to the blocking pool, so stamping later would let
    // concurrent records taken in a known order come out reordered on disk.
    let report = TaskReport {
        version: FILE_VERSION,
        task_id: task_id.into(),
        at_unix_ms: chrono::Utc::now().timestamp_millis() as f64,
        details,
    };
    // Off the async worker when there is a runtime to offload onto — the write
    // is a small JSON file, but it is still open + write + a directory scan
    // for rotation, which has no business stalling a tokio worker thread.
    //
    // No runtime means a synchronous caller — app-exit teardown runs without
    // one, and a spawned task would not survive it, so the write happens
    // inline there. Mirrors `journal::record` exactly.
    match tokio::runtime::Handle::try_current() {
        Ok(_) => {
            let root = instance_root.to_path_buf();
            tokio::task::spawn_blocking(move || write_or_log(&root, &report));
        }
        Err(_) => write_or_log(instance_root, &report),
    }
}

fn write_or_log(instance_root: &Path, report: &TaskReport) {
    if let Err(e) = write_report(instance_root, report) {
        crate::diag!(
            "reports: failed to persist task {} at {}: {e}",
            report.task_id,
            instance_root.display()
        );
    }
}

/// Fallible, synchronous write of `details` for `task_id`, stamped with the
/// current time. Used by tests, which need the error and a deterministic
/// completion. Mirrors `journal::append`.
#[cfg(test)]
fn write_sync(instance_root: &Path, task_id: &str, details: Vec<TaskDetail>) -> Result<()> {
    let report = TaskReport {
        version: FILE_VERSION,
        task_id: task_id.to_string(),
        at_unix_ms: chrono::Utc::now().timestamp_millis() as f64,
        details,
    };
    write_report(instance_root, &report)
}

/// Write a fully-formed report to its path, creating `.lucerna/reports/` if
/// needed, then prune the directory back down to the caps.
fn write_report(instance_root: &Path, report: &TaskReport) -> Result<()> {
    let path = report_path(instance_root, &report.task_id);
    let json = serde_json::to_vec_pretty(report)
        .map_err(|e| Error::io(path.display().to_string(), format!("serialize: {e}")))?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent.display().to_string(), e))?;
    }
    std::fs::write(&path, &json).map_err(|e| Error::io(path.display().to_string(), e))?;

    rotate(instance_root);
    Ok(())
}

/// Read a previously persisted report. A missing file is `Ok(None)`, not an
/// error — the task may never have been reported, or its report may already
/// have rotated out.
pub fn read(instance_root: &Path, task_id: &str) -> Result<Option<TaskReport>> {
    let path = report_path(instance_root, task_id);
    match std::fs::read(&path) {
        Ok(bytes) => {
            let report = serde_json::from_slice(&bytes)
                .map_err(|e| Error::io(path.display().to_string(), format!("parse: {e}")))?;
            Ok(Some(report))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::io(path.display().to_string(), e)),
    }
}

/// Prune `.lucerna/reports/` down to the production caps. Best-effort: a
/// failed prune leaves extra files on disk, which is not worth surfacing as a
/// write error — the next successful write tries again.
fn rotate(instance_root: &Path) {
    rotate_with_caps(instance_root, MAX_REPORT_FILES, MAX_REPORTS_TOTAL_BYTES);
}

/// Core of [`rotate`], with the caps as parameters so tests can exercise real
/// rotation over real files without waiting on production-sized caps.
fn rotate_with_caps(instance_root: &Path, max_files: usize, max_total_bytes: u64) {
    let dir = reports_dir(instance_root);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };

    let mut candidates: Vec<PathBuf> = Vec::new();
    let mut inputs: Vec<RetentionInput> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some(FILE_EXT) {
            continue; // ignore anything that isn't a report file
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let mtime_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as f64)
            .unwrap_or(0.0);
        candidates.push(path);
        inputs.push(RetentionInput {
            mtime_ms,
            size_bytes: meta.len(),
        });
    }

    for idx in select_excess(&inputs, max_files, max_total_bytes) {
        let path = &candidates[idx];
        if let Err(e) = std::fs::remove_file(path) {
            crate::diag!("reports: failed to prune {}: {e}", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::store::Placement;
    use crate::tasks::{DetailOutcome, Fetched, TaskOrigin};
    use std::fs::File;
    use std::time::{Duration, SystemTime};

    fn detail(name: &str) -> TaskDetail {
        TaskDetail {
            name: name.into(),
            install_path: format!("mods/{name}.jar"),
            origin: TaskOrigin::Modrinth,
            host: Some("cdn.modrinth.com".into()),
            bytes: Some(1024.0),
            sha1: Some("deadbeef".into()),
            outcome: DetailOutcome::Installed {
                fetched: Fetched::Downloaded,
                placement: Placement::Linked,
            },
        }
    }

    #[test]
    fn reports_live_outside_every_per_instance_log_root() {
        // Mirrors journal::tests::journal_lives_outside_every_per_instance_log_root.
        // `logs::files::allowed_roots` enumerates these three per-instance
        // directories, and everything inside them is fair game for
        // `logs::retention` (which protects only `latest.log`, `debug.log`,
        // and the app-wide launcher log). A report in any of them would be
        // listed as a log file AND silently deleted by a retention pass.
        let td = tempfile::tempdir().unwrap();
        let inst = td.path();
        let log_roots = [
            inst.join(".minecraft").join("logs"),
            inst.join(".minecraft").join("crash-reports"),
            inst.join("logs"),
        ];
        let p = report_path(inst, "task-123");
        for root in &log_roots {
            assert!(
                !p.starts_with(root),
                "report must not live under log root {}: {}",
                root.display(),
                p.display()
            );
        }
        assert_eq!(
            p.parent()
                .and_then(|d| d.parent())
                .and_then(|d| d.file_name()),
            Some(META_DIR.as_ref()),
            "report must live under the .lucerna meta dir, not logs/"
        );
    }

    #[test]
    fn rotate_keeps_the_newest_files_and_prunes_the_rest() {
        let td = tempfile::tempdir().unwrap();
        let dir = reports_dir(td.path());
        std::fs::create_dir_all(&dir).unwrap();

        let base = SystemTime::now();
        for i in 0..6u64 {
            let path = dir.join(format!("task-{i}.json"));
            std::fs::write(&path, b"{}").unwrap();
            let f = File::options().write(true).open(&path).unwrap();
            f.set_modified(base + Duration::from_secs(i)).unwrap();
        }

        rotate_with_caps(td.path(), 3, u64::MAX);

        let mut remaining: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        remaining.sort();
        assert_eq!(remaining.len(), 3, "rotation must cap at max_files");
        assert_eq!(
            remaining,
            vec!["task-3.json", "task-4.json", "task-5.json"],
            "the newest files survive"
        );
    }

    #[test]
    fn write_then_read_round_trips_task_report() {
        let td = tempfile::tempdir().unwrap();
        let details = vec![detail("sodium")];
        write_sync(td.path(), "task-abc", details.clone()).unwrap();

        let got = read(td.path(), "task-abc").unwrap().expect("report exists");
        assert_eq!(got.task_id, "task-abc");
        assert_eq!(got.details, details);
        assert_eq!(got.version, FILE_VERSION);
    }

    #[test]
    fn missing_report_reads_as_none() {
        let td = tempfile::tempdir().unwrap();
        assert!(read(td.path(), "nonexistent").unwrap().is_none());
    }

    #[test]
    fn record_swallows_a_write_failure() {
        // `.lucerna` occupied by a FILE makes create_dir_all fail. `record`
        // must stay silent: a report write is never worth failing the
        // install it describes.
        let td = tempfile::tempdir().unwrap();
        std::fs::write(td.path().join(".lucerna"), b"not a dir").unwrap();
        record(td.path(), "task-x", vec![detail("sodium")]);
        assert!(write_sync(td.path(), "task-x", vec![detail("sodium")]).is_err());
    }
}
