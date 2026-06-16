//! File enumeration + crash detection.
//!
//! Four roots:
//! - `instances/<id>/.minecraft/logs/`          → LogSource::Game (latest.log, debug.log)
//! - `instances/<id>/.minecraft/crash-reports/` → LogSource::Crash
//! - `instances/<id>/logs/`                     → LogSource::GameConsole (captured
//!   stdout/stderr of the game process — `<stamp>-launch.log`)
//! - `<app_data>/logs/`                         → LogSource::Launcher (the launcher's
//!   own `lucerna.log`; app-wide, shown under every instance)

use crate::error::{Error, Result};
use serde::Serialize;
use specta::Type;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, Serialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LogSource {
    Game,
    Crash,
    /// The game process's captured stdout/stderr (`instances/<id>/logs/`) —
    /// not the launcher's own log. Catches early/JVM-level crashes that never
    /// reach the game's `latest.log`.
    GameConsole,
    /// The launcher's own diagnostics (`<app_data>/logs/lucerna.log`).
    Launcher,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct LogFileMeta {
    pub path: String,
    pub name: String,
    pub source: LogSource,
    /// f64 because specta-typescript 0.0.12 forbids u64.
    pub size_bytes: f64,
    pub modified_unix_ms: f64,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct CrashReport {
    pub path: String,
    /// First ~500 chars of the crash report — enough to show the
    /// stack-trace head in a banner without loading the full file.
    pub preview: String,
}

const CRASH_PREVIEW_CHARS: usize = 500;

/// Return the four log roots (three per-instance + the app-wide launcher
/// log dir). Roots that don't exist on disk yet (fresh install) are NOT
/// created — callers must treat absence as "no files," not an error.
pub fn allowed_roots(app: &tauri::AppHandle, instance_id: &str) -> Result<Vec<PathBuf>> {
    let inst =
        crate::paths::instance_dir(app, instance_id).map_err(|e| Error::io("<instance_dir>", e))?;
    let app_logs = crate::paths::app_logs_dir(app).map_err(|e| Error::io("<app_logs_dir>", e))?;
    // Order matters: it maps to LogSource by index in `list_log_files`.
    Ok(vec![
        inst.join(".minecraft").join("logs"),
        inst.join(".minecraft").join("crash-reports"),
        inst.join("logs"),
        app_logs,
    ])
}

/// Enumerate every log file across the four roots. Missing roots are
/// silently skipped. Sorted by mtime descending (newest first).
pub fn list_log_files(app: &tauri::AppHandle, instance_id: &str) -> Result<Vec<LogFileMeta>> {
    let roots = allowed_roots(app, instance_id)?;
    let mut out: Vec<LogFileMeta> = Vec::new();
    for (i, root) in roots.iter().enumerate() {
        let source = match i {
            0 => LogSource::Game,
            1 => LogSource::Crash,
            2 => LogSource::GameConsole,
            _ => LogSource::Launcher,
        };
        list_root_into(root, source, &mut out);
    }
    out.sort_by(|a, b| {
        b.modified_unix_ms
            .partial_cmp(&a.modified_unix_ms)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(out)
}

fn list_root_into(root: &Path, source: LogSource, out: &mut Vec<LogFileMeta>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let name_os = entry.file_name();
        let Some(name) = name_os.to_str() else {
            continue;
        };
        // Skip dot-files (.DS_Store, .lock, etc.).
        if name.starts_with('.') {
            continue;
        }
        let modified_unix_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as f64)
            .unwrap_or(0.0);
        out.push(LogFileMeta {
            path: entry.path().to_string_lossy().into_owned(),
            name: name.to_string(),
            source,
            size_bytes: meta.len() as f64,
            modified_unix_ms,
        });
    }
}

/// Newest `crash-*.txt` in the crash-reports dir, with a short
/// preview. Returns Ok(None) when no crash reports exist.
pub fn latest_crash(app: &tauri::AppHandle, instance_id: &str) -> Result<Option<CrashReport>> {
    let roots = allowed_roots(app, instance_id)?;
    let crash_root = &roots[1];
    let Ok(entries) = std::fs::read_dir(crash_root) else {
        return Ok(None);
    };
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let name_os = entry.file_name();
        let Some(name) = name_os.to_str() else {
            continue;
        };
        // MC convention — defends against stray `notes.txt`.
        if !name.starts_with("crash-") || !name.ends_with(".txt") {
            continue;
        }
        let Ok(modified) = meta.modified() else {
            continue;
        };
        match &newest {
            Some((t, _)) if *t >= modified => {}
            _ => newest = Some((modified, entry.path())),
        }
    }
    let Some((_, path)) = newest else {
        return Ok(None);
    };
    let preview = match std::fs::read_to_string(&path) {
        Ok(s) => s.chars().take(CRASH_PREVIEW_CHARS).collect(),
        Err(_) => String::new(),
    };
    Ok(Some(CrashReport {
        path: path.to_string_lossy().into_owned(),
        preview,
    }))
}

/// Check that `path` resolves to a real file under one of the
/// allowed roots. Uses `canonicalize` on both sides to defeat
/// `..`-traversal attacks.
pub fn assert_under_allowed_roots(path: &Path, roots: &[PathBuf]) -> Result<()> {
    let canonical = path
        .canonicalize()
        .map_err(|e| Error::io(path.display().to_string(), e))?;
    for root in roots {
        if let Ok(root_canon) = root.canonicalize() {
            if canonical.starts_with(&root_canon) {
                return Ok(());
            }
        }
    }
    Err(Error::Io {
        path: path.display().to_string(),
        details: "path not in allowed log roots".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    #[test]
    fn list_root_skips_directories_and_dot_files() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("a.log"), b"hi").unwrap();
        std::fs::write(root.join(".hidden"), b"x").unwrap();
        std::fs::create_dir(root.join("subdir")).unwrap();

        let mut out = Vec::new();
        list_root_into(root, LogSource::Game, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "a.log");
        assert_eq!(out[0].source, LogSource::Game);
        assert!(out[0].size_bytes >= 2.0);
    }

    #[test]
    fn list_root_tags_the_new_sources() {
        // The GameConsole (per-instance captured stdout/stderr) and Launcher
        // (app-wide lucerna.log) sources tag their files correctly — the
        // index→source mapping in list_log_files relies on these variants.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("2026-launch.log"), b"console").unwrap();
        let mut console = Vec::new();
        list_root_into(tmp.path(), LogSource::GameConsole, &mut console);
        assert_eq!(console.len(), 1);
        assert_eq!(console[0].source, LogSource::GameConsole);

        let tmp2 = tempfile::tempdir().unwrap();
        std::fs::write(tmp2.path().join("lucerna.log"), b"launcher diag").unwrap();
        let mut launcher = Vec::new();
        list_root_into(tmp2.path(), LogSource::Launcher, &mut launcher);
        assert_eq!(launcher.len(), 1);
        assert_eq!(launcher[0].source, LogSource::Launcher);
        assert_eq!(launcher[0].name, "lucerna.log");
    }

    #[test]
    fn list_root_missing_directory_is_noop() {
        let mut out = Vec::new();
        list_root_into(
            std::path::Path::new("C:/this/path/does/not/exist"),
            LogSource::Crash,
            &mut out,
        );
        assert!(out.is_empty());
    }

    #[test]
    fn assert_under_allowed_roots_accepts_child() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let f = root.join("ok.log");
        std::fs::write(&f, b"hi").unwrap();
        let roots = vec![root.to_path_buf()];
        assert_under_allowed_roots(&f, &roots).expect("child accepted");
    }

    #[test]
    fn assert_under_allowed_roots_rejects_sibling() {
        let tmp = tempfile::tempdir().unwrap();
        let root1 = tmp.path().join("a");
        let root2 = tmp.path().join("b");
        std::fs::create_dir_all(&root1).unwrap();
        std::fs::create_dir_all(&root2).unwrap();
        let escapee = root2.join("nope.log");
        std::fs::write(&escapee, b"x").unwrap();
        let roots = vec![root1.clone()];
        let err = assert_under_allowed_roots(&escapee, &roots).unwrap_err();
        match err {
            Error::Io { details, .. } => {
                assert!(
                    details.contains("not in allowed log roots"),
                    "got: {details}"
                );
            }
            other => panic!("expected Io, got {other:?}"),
        }
    }

    #[test]
    fn assert_under_allowed_roots_defeats_dotdot_traversal() {
        let tmp = tempfile::tempdir().unwrap();
        let root1 = tmp.path().join("a");
        let outside = tmp.path().join("secrets.txt");
        std::fs::create_dir_all(&root1).unwrap();
        std::fs::write(&outside, b"top-secret").unwrap();

        // `<root1>/../secrets.txt` resolves outside root1.
        let traversal = root1.join("..").join("secrets.txt");
        let roots = vec![root1];
        assert!(
            assert_under_allowed_roots(&traversal, &roots).is_err(),
            "../-traversal must be refused"
        );
    }

    #[test]
    fn latest_among_files_picks_newest_by_mtime() {
        // Plain helper to verify the mtime ranking logic — runs without
        // an AppHandle. We exercise the public latest_crash via
        // the integration test (Task 6).
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let a = root.join("crash-1.txt");
        let b = root.join("crash-2.txt");
        std::fs::write(&a, b"first").unwrap();
        std::thread::sleep(Duration::from_millis(50));
        std::fs::write(&b, b"second").unwrap();

        let now = SystemTime::now();
        let a_modified = std::fs::metadata(&a).unwrap().modified().unwrap();
        let b_modified = std::fs::metadata(&b).unwrap().modified().unwrap();
        assert!(a_modified < now);
        assert!(b_modified >= a_modified);
    }
}
