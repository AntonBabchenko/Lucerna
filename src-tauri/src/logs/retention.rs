//! Log retention + deletion. Pure selection logic (no Tauri) lives in
//! `select_for_clear_old` / `select_for_policy` and is unit-tested here;
//! the AppHandle-bound wrappers (`clear_old`, `apply_policy`,
//! `apply_from_settings`, `delete_one`) are exercised via the command
//! layer + manual GUI.
//!
//! Protected files (`latest.log`, `debug.log`) are never removed by
//! auto-retention or "clear old". They CAN still be deleted by the
//! explicit per-file delete command (the user asked for that file).

use crate::error::{Error, Result};
use crate::instances::schema::LogRetentionPolicy;
use crate::logs::files::{allowed_roots, list_log_files, LogFileMeta, LogSource};
use serde::Serialize;
use specta::Type;
use std::path::{Path, PathBuf};

const PROTECTED: [&str; 2] = ["latest.log", "debug.log"];

/// What a cleanup removed. `f64` for the specta IPC boundary.
#[derive(Debug, Clone, Copy, Serialize, Type, PartialEq)]
pub struct CleanupResult {
    pub deleted_count: f64,
    pub freed_bytes: f64,
}

/// True iff `name` is a protected log that auto-cleanup must keep.
pub fn is_protected(name: &str) -> bool {
    PROTECTED.contains(&name)
}

/// True iff a file may be removed by auto-retention / "clear old". Excludes
/// the protected game logs AND the launcher's own log (`LogSource::Launcher`,
/// i.e. `lucerna.log`) — the launcher log is app-wide and self-rotates in the
/// `diag` module; retention must never delete it (it would wipe the very log
/// the user opened the viewer to read). Game-console (`launch-*.log`) files
/// stay eligible — clearing those accumulating per-launch dumps is the point.
fn is_retention_eligible(f: &LogFileMeta) -> bool {
    !is_protected(&f.name) && f.source != LogSource::Launcher
}

/// Paths to delete for "clear old": every retention-eligible file across the
/// roots. Pure — caller supplies the enumerated files.
pub fn select_for_clear_old(files: &[LogFileMeta]) -> Vec<LogFileMeta> {
    files
        .iter()
        .filter(|f| is_retention_eligible(f))
        .cloned()
        .collect()
}

/// A retention candidate reduced to the only two facts the selector needs:
/// modification time and size. Lets one selector serve both instance logs
/// (`LogFileMeta`) and server logs (`server-<ts>.log` archives), so the two
/// retention paths can't silently diverge again.
#[derive(Debug, Clone, Copy)]
pub struct RetentionInput {
    pub mtime_ms: f64,
    pub size_bytes: u64,
}

/// Pure core shared by instance- and server-log retention. Returns the indices
/// (into `items`, in the caller's original order) to delete so the survivors
/// satisfy BOTH the count cap and the byte budget. Newest-first semantics:
/// - **count:** every file ranked at or beyond `max_files` is dropped;
/// - **bytes:** keep the newest contiguous prefix whose cumulative size stays
///   within `max_total_bytes` — once a file overflows the budget, it and every
///   older file are dropped (a prefix cut, not a sparse best-fit). The single
///   newest file always survives the byte rule.
///
/// The caller pre-filters eligibility (e.g. protected/launcher logs, or the
/// live `server-latest.log`). No I/O.
///
/// Assumes `mtime_ms` is finite (production log mtimes always are — they come
/// from `as_millis() as f64` / `unwrap_or(0.0)`). Files sharing an identical
/// `mtime_ms` are ranked arbitrarily, so the specific victim within a
/// same-timestamp tie is unspecified; the survivor *count* is still
/// deterministic.
pub fn select_excess(
    items: &[RetentionInput],
    max_files: usize,
    max_total_bytes: u64,
) -> Vec<usize> {
    let mut ranked: Vec<usize> = (0..items.len()).collect();
    ranked.sort_by(|&a, &b| items[b].mtime_ms.total_cmp(&items[a].mtime_ms)); // newest first

    let mut running: u64 = 0;
    let mut over_budget = false;
    let mut to_delete = Vec::new();
    for (rank, &i) in ranked.iter().enumerate() {
        if rank >= max_files {
            to_delete.push(i); // count cap: drop everything older than the newest `max_files`
            continue;
        }
        running = running.saturating_add(items[i].size_bytes);
        if rank > 0 && (over_budget || running > max_total_bytes) {
            over_budget = true; // prefix cut: latch so all older files are dropped too
            to_delete.push(i);
        }
    }
    to_delete
}

/// Files to delete to satisfy `policy` — delegates to [`select_excess`].
/// Eligible = non-protected, non-launcher. Disabled policy returns an empty
/// list. Pure — no I/O.
pub fn select_for_policy(files: &[LogFileMeta], policy: &LogRetentionPolicy) -> Vec<LogFileMeta> {
    if !policy.enabled {
        return Vec::new();
    }
    let eligible: Vec<&LogFileMeta> = files.iter().filter(|f| is_retention_eligible(f)).collect();
    let inputs: Vec<RetentionInput> = eligible
        .iter()
        .map(|f| RetentionInput {
            mtime_ms: f.modified_unix_ms,
            size_bytes: f.size_bytes.max(0.0) as u64,
        })
        .collect();
    let budget = (policy.max_total_mb as u64).saturating_mul(1024 * 1024);
    select_excess(&inputs, policy.max_files as usize, budget)
        .into_iter()
        .map(|idx| eligible[idx].clone())
        .collect()
}

/// Delete one file after confirming it is under one of `roots`. Returns
/// the freed byte count (best-effort; 0 if size can't be read).
pub fn delete_one(path: &Path, roots: &[PathBuf]) -> Result<u64> {
    crate::logs::files::assert_under_allowed_roots(path, roots)?;
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    std::fs::remove_file(path).map_err(|e| Error::io(path.display().to_string(), e))?;
    Ok(size)
}

fn delete_all(paths: &[LogFileMeta], roots: &[PathBuf]) -> CleanupResult {
    let mut count = 0u64;
    let mut freed = 0u64;
    for f in paths {
        match delete_one(Path::new(&f.path), roots) {
            Ok(bytes) => {
                count += 1;
                freed = freed.saturating_add(bytes);
            }
            Err(e) => crate::diag!("log-retention: could not delete {}: {e}", f.path),
        }
    }
    CleanupResult {
        deleted_count: count as f64,
        freed_bytes: freed as f64,
    }
}

/// Delete every retention-eligible log for `instance_id` across the roots
/// (excludes protected game logs and the launcher's own `lucerna.log`).
pub fn clear_old(app: &tauri::AppHandle, instance_id: &str) -> Result<CleanupResult> {
    let files = list_log_files(app, instance_id)?;
    let roots = allowed_roots(app, instance_id)?;
    let victims = select_for_clear_old(&files);
    Ok(delete_all(&victims, &roots))
}

/// Apply `policy` to `instance_id`'s logs.
pub fn apply_policy(
    app: &tauri::AppHandle,
    instance_id: &str,
    policy: &LogRetentionPolicy,
) -> Result<CleanupResult> {
    if !policy.enabled {
        return Ok(CleanupResult {
            deleted_count: 0.0,
            freed_bytes: 0.0,
        });
    }
    let files = list_log_files(app, instance_id)?;
    let roots = allowed_roots(app, instance_id)?;
    let victims = select_for_policy(&files, policy);
    Ok(delete_all(&victims, &roots))
}

/// Read the global policy from app.json and apply it. Used by the
/// game-exit watcher and the Logs-open command. No-op when disabled.
pub fn apply_from_settings(app: &tauri::AppHandle, instance_id: &str) -> Result<CleanupResult> {
    let path = crate::paths::app_file(app).map_err(|e| Error::io("<app_file>", e))?;
    let settings = crate::instances::store::read_app_json(&path)?;
    apply_policy(app, instance_id, &settings.general.log_retention)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(path: &str, name: &str, size: f64, mtime: f64) -> LogFileMeta {
        LogFileMeta {
            path: path.into(),
            name: name.into(),
            source: crate::logs::files::LogSource::Game,
            size_bytes: size,
            modified_unix_ms: mtime,
        }
    }

    #[test]
    fn clear_old_selects_everything_except_protected() {
        let files = vec![
            meta("/l/latest.log", "latest.log", 10.0, 5.0),
            meta("/l/debug.log", "debug.log", 10.0, 4.0),
            meta("/l/2024-01-01-1.log.gz", "2024-01-01-1.log.gz", 10.0, 3.0),
            meta("/c/crash-1.txt", "crash-1.txt", 10.0, 2.0),
        ];
        let del = select_for_clear_old(&files);
        assert_eq!(del.len(), 2);
        assert!(del.iter().any(|f| f.name == "2024-01-01-1.log.gz"));
        assert!(del.iter().any(|f| f.name == "crash-1.txt"));
        assert!(!del.iter().any(|f| is_protected(&f.name)));
    }

    #[test]
    fn launcher_log_is_never_selected_for_deletion() {
        // The app-wide launcher log (LogSource::Launcher) must survive both
        // "clear old" and policy retention — it self-rotates in `diag`, and
        // deleting it would wipe the log the user opened the viewer to read.
        let launcher = LogFileMeta {
            path: "/app/logs/lucerna.log".into(),
            name: "lucerna.log".into(),
            source: LogSource::Launcher,
            size_bytes: 9_999_999.0,
            modified_unix_ms: 1.0, // oldest → would be the first victim if eligible
        };
        let console = meta("/i/logs/2026-launch.log", "2026-launch.log", 10.0, 2.0);
        let files = vec![launcher.clone(), console];

        let cleared = select_for_clear_old(&files);
        assert!(cleared.iter().all(|f| f.source != LogSource::Launcher));
        assert!(cleared.iter().any(|f| f.name == "2026-launch.log"));

        let policy = LogRetentionPolicy {
            enabled: true,
            max_files: 0,
            max_total_mb: 0,
        };
        let by_policy = select_for_policy(&files, &policy);
        assert!(by_policy.iter().all(|f| f.source != LogSource::Launcher));
    }

    #[test]
    fn policy_disabled_deletes_nothing() {
        let files = vec![meta("/l/a.log", "a.log", 999.0, 1.0)];
        let policy = LogRetentionPolicy {
            enabled: false,
            max_files: 0,
            max_total_mb: 0,
        };
        assert!(select_for_policy(&files, &policy).is_empty());
    }

    #[test]
    fn policy_count_limit_drops_oldest_beyond_n() {
        let files = vec![
            meta("/l/a.log", "a.log", 1.0, 40.0),
            meta("/l/b.log", "b.log", 1.0, 30.0),
            meta("/l/c.log", "c.log", 1.0, 20.0),
            meta("/l/d.log", "d.log", 1.0, 10.0),
        ];
        let policy = LogRetentionPolicy {
            enabled: true,
            max_files: 2,
            max_total_mb: 9999,
        };
        let del = select_for_policy(&files, &policy);
        let names: Vec<&str> = del.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"c.log"));
        assert!(names.contains(&"d.log"));
    }

    #[test]
    fn policy_protects_latest_and_debug_regardless_of_count() {
        let files = vec![
            meta("/l/latest.log", "latest.log", 1.0, 40.0),
            meta("/l/debug.log", "debug.log", 1.0, 39.0),
            meta("/l/old.log", "old.log", 1.0, 10.0),
        ];
        let policy = LogRetentionPolicy {
            enabled: true,
            max_files: 0,
            max_total_mb: 9999,
        };
        let del = select_for_policy(&files, &policy);
        assert_eq!(del.len(), 1);
        assert_eq!(del[0].name, "old.log");
    }

    #[test]
    fn policy_size_limit_trims_to_budget_but_keeps_newest() {
        let mb = 1024.0 * 1024.0;
        let files = vec![
            meta("/l/new.log", "new.log", mb, 30.0),
            meta("/l/mid.log", "mid.log", mb, 20.0),
            meta("/l/old.log", "old.log", mb, 10.0),
        ];
        let policy = LogRetentionPolicy {
            enabled: true,
            max_files: 9999,
            max_total_mb: 1,
        };
        let del = select_for_policy(&files, &policy);
        let names: Vec<&str> = del.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"mid.log"));
        assert!(names.contains(&"old.log"));
        assert!(
            !names.contains(&"new.log"),
            "newest eligible is always retained"
        );
    }

    #[test]
    fn policy_empty_input_is_noop() {
        let policy = LogRetentionPolicy {
            enabled: true,
            max_files: 1,
            max_total_mb: 1,
        };
        assert!(select_for_policy(&[], &policy).is_empty());
    }

    // --- select_excess: the shared pure core (also backs server-log pruning) ---

    fn ri(mtime_ms: f64, size_bytes: u64) -> RetentionInput {
        RetentionInput {
            mtime_ms,
            size_bytes,
        }
    }

    #[test]
    fn select_excess_count_cap_drops_oldest_beyond_n() {
        // newest-first by mtime: idx0(40), idx1(30), idx2(20), idx3(10); keep 2.
        let items = vec![ri(40.0, 1), ri(30.0, 1), ri(20.0, 1), ri(10.0, 1)];
        let del = select_excess(&items, 2, u64::MAX);
        assert_eq!(del.len(), 2);
        assert!(del.contains(&2));
        assert!(del.contains(&3));
    }

    #[test]
    fn select_excess_byte_budget_is_a_prefix_cut_keeping_newest() {
        // newest-first new(6), mid(6), old(2), budget 10. Newest always kept;
        // mid overflows (6+6 > 10) → mid AND every older file (old) are dropped.
        // This is the prefix-cut semantics (NOT greedy best-fit, which would
        // have kept `old` because 6+2 <= 10).
        let items = vec![ri(5.0, 6), ri(4.0, 6), ri(3.0, 2)];
        let del = select_excess(&items, usize::MAX, 10);
        assert_eq!(del, vec![1, 2]);
    }

    #[test]
    fn select_excess_count_zero_deletes_all() {
        let items = vec![ri(2.0, 1), ri(1.0, 1)];
        let del = select_excess(&items, 0, u64::MAX);
        assert_eq!(del.len(), 2);
    }

    #[test]
    fn select_excess_empty_is_noop() {
        assert!(select_excess(&[], 5, 1000).is_empty());
    }

    #[test]
    fn select_excess_sorts_unsorted_input_and_returns_original_indices() {
        // Provided out of order (idx0 oldest, idx1 newest, idx2 middle): the
        // selector must rank by mtime internally and return indices into the
        // ORIGINAL slice.
        let items = vec![ri(10.0, 1), ri(40.0, 1), ri(20.0, 1)];
        let del = select_excess(&items, 1, u64::MAX); // keep only the newest (idx1)
        assert_eq!(del.len(), 2);
        assert!(del.contains(&0));
        assert!(del.contains(&2));
        assert!(!del.contains(&1));
    }

    #[test]
    fn select_excess_count_and_bytes_both_bind() {
        // newest-first idx0..idx4 (size 4 each), max_files=3, budget 10.
        // byte rule cuts idx2 (4+4+4=12 > 10) and latches; count rule cuts
        // idx3, idx4 (rank >= 3) — the `continue` must NOT add their size to
        // the running total. Survivors: idx0, idx1.
        let items = vec![
            ri(50.0, 4),
            ri(40.0, 4),
            ri(30.0, 4),
            ri(20.0, 4),
            ri(10.0, 4),
        ];
        let del = select_excess(&items, 3, 10);
        assert_eq!(del, vec![2, 3, 4]);
    }
}
