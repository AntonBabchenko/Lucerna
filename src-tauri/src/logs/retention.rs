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
use crate::logs::files::{allowed_roots, list_log_files, LogFileMeta};
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

/// Paths to delete for "clear old": every non-protected file across all
/// three roots. Pure — caller supplies the enumerated files.
pub fn select_for_clear_old(files: &[LogFileMeta]) -> Vec<LogFileMeta> {
    files
        .iter()
        .filter(|f| !is_protected(&f.name))
        .cloned()
        .collect()
}

/// Files to delete to satisfy `policy`. Eligible = non-protected, newest
/// first. Keep the newest `max_files`; then, within that kept set, keep
/// the newest run whose cumulative size stays under `max_total_mb` (the
/// single newest eligible file is always retained even if it alone
/// exceeds the budget). Everything not kept is returned for deletion.
/// Disabled policy returns an empty list. Pure — no I/O.
pub fn select_for_policy(files: &[LogFileMeta], policy: &LogRetentionPolicy) -> Vec<LogFileMeta> {
    if !policy.enabled {
        return Vec::new();
    }
    let mut eligible: Vec<LogFileMeta> = files
        .iter()
        .filter(|f| !is_protected(&f.name))
        .cloned()
        .collect();
    eligible.sort_by(|a, b| {
        b.modified_unix_ms
            .partial_cmp(&a.modified_unix_ms)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let keep_n = (policy.max_files as usize).min(eligible.len());
    let mut to_delete: Vec<LogFileMeta> = eligible.split_off(keep_n);

    let budget: u64 = (policy.max_total_mb as u64).saturating_mul(1024 * 1024);
    let mut running: u64 = 0;
    let mut over = false;
    for (i, f) in eligible.into_iter().enumerate() {
        let size = f.size_bytes.max(0.0) as u64;
        running = running.saturating_add(size);
        if i == 0 || (!over && running <= budget) {
            // kept — no action
        } else {
            over = true;
            to_delete.push(f);
        }
    }
    to_delete
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
            Err(e) => eprintln!("log-retention: could not delete {}: {e}", f.path),
        }
    }
    CleanupResult {
        deleted_count: count as f64,
        freed_bytes: freed as f64,
    }
}

/// Delete every non-protected log for `instance_id` across all 3 roots.
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
}
