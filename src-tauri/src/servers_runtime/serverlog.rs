//! Server log retention: rotate `server-latest.log` to a timestamped archive on
//! each start (so prior sessions aren't lost), keep newest N, list + read past
//! sessions. `server-latest.log` is never an archive and never pruned.

use crate::error::{Error, Result};
use serde::Serialize;
use specta::Type;
use std::path::Path;

/// One log file shown to the UI.
#[derive(Debug, Clone, Serialize, Type)]
pub struct ServerLogInfo {
    pub file_name: String,
    pub modified_unix_ms: f64,
    pub size_bytes: f64,
    /// True for the current/most-recent `server-latest.log`.
    pub is_latest: bool,
}

/// Keep at most this many rotated archives (`server-<ts>.log`). The live
/// `server-latest.log` is never counted/pruned.
pub const KEEP_LOGS: usize = 15;

/// Byte budget for rotated archives (the live `server-latest.log` is excluded).
/// Bounds disk use when a crash/restart loop spams sessions, mirroring the
/// client log-retention byte budget.
pub const MAX_TOTAL_MB: u64 = 200;

pub const LATEST: &str = "server-latest.log";

/// Move `server-latest.log` to `server-<stamp>.log` if it exists (so the prior
/// session is preserved before a fresh log is created). No-op if absent.
pub fn rotate_log(logs_dir: &Path, stamp: &str) -> Result<()> {
    let latest = logs_dir.join(LATEST);
    if !latest.exists() {
        return Ok(());
    }
    let archive = logs_dir.join(format!("server-{stamp}.log"));
    std::fs::rename(&latest, &archive).map_err(|e| Error::io(archive.display().to_string(), e))
}

/// Prune rotated archives to satisfy BOTH the count cap (`KEEP_LOGS`) and the
/// byte budget (`MAX_TOTAL_MB`); the live `server-latest.log` is never pruned.
/// Best-effort.
pub fn prune_logs(logs_dir: &Path) {
    let archives: Vec<(std::path::PathBuf, f64, u64)> = match std::fs::read_dir(logs_dir) {
        Ok(rd) => rd
            .flatten()
            .filter(|e| is_archive(&e.file_name().to_string_lossy()))
            .filter_map(|e| e.metadata().ok().map(|m| (e.path(), mtime_ms(&m), m.len())))
            .collect(),
        Err(_) => return,
    };
    for path in select_archives_to_delete(archives, KEEP_LOGS, MAX_TOTAL_MB * 1024 * 1024) {
        let _ = std::fs::remove_file(path);
    }
}

/// Pure: from `(path, mtime_ms, size_bytes)` archives, choose which to delete so
/// the survivors satisfy the count cap (`keep` newest) AND the byte budget
/// (`budget_bytes`). The single newest archive is always kept regardless of
/// size. Greedy newest-first: keep an archive when it's within the count and
/// still fits the running byte total, else delete it.
fn select_archives_to_delete(
    mut archives: Vec<(std::path::PathBuf, f64, u64)>,
    keep: usize,
    budget_bytes: u64,
) -> Vec<std::path::PathBuf> {
    archives.sort_by(|a, b| b.1.total_cmp(&a.1)); // newest first
    let mut total: u64 = 0;
    let mut to_delete = Vec::new();
    for (i, (path, _mtime, size)) in archives.into_iter().enumerate() {
        let over_count = i >= keep;
        let over_bytes = i > 0 && total.saturating_add(size) > budget_bytes;
        if over_count || over_bytes {
            to_delete.push(path);
        } else {
            total = total.saturating_add(size);
        }
    }
    to_delete
}

/// All server logs (latest + archives), newest first, `is_latest` flagged.
pub fn list_logs(logs_dir: &Path) -> Result<Vec<ServerLogInfo>> {
    let mut out = Vec::new();
    let rd = match std::fs::read_dir(logs_dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(Error::io(logs_dir.display().to_string(), e)),
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let is_latest = name == LATEST;
        if !is_latest && !is_archive(&name) {
            continue;
        }
        if let Ok(meta) = e.metadata() {
            out.push(ServerLogInfo {
                file_name: name,
                modified_unix_ms: mtime_ms(&meta),
                size_bytes: meta.len() as f64,
                is_latest,
            });
        }
    }
    // latest first, then archives newest-first
    out.sort_by(|a, b| {
        b.is_latest
            .cmp(&a.is_latest)
            .then(b.modified_unix_ms.total_cmp(&a.modified_unix_ms))
    });
    Ok(out)
}

/// True iff `name` is an archive log (`server-<...>.log`, not `server-latest.log`).
fn is_archive(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("server-") && lower.ends_with(".log") && name != LATEST
}

fn mtime_ms(meta: &std::fs::Metadata) -> f64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

/// True iff `name` is a single safe path component (guards `read_log`).
pub fn is_safe_log_name(name: &str) -> bool {
    if name.contains('\\') || name.contains(':') {
        return false;
    }
    let mut c = std::path::Path::new(name).components();
    matches!(c.next(), Some(std::path::Component::Normal(_))) && c.next().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn rotate_moves_latest_to_archive() {
        let d = tempdir().unwrap();
        fs::write(d.path().join(LATEST), b"old session").unwrap();
        rotate_log(d.path(), "20260619-120000").unwrap();
        assert!(!d.path().join(LATEST).exists(), "latest moved away");
        assert!(d.path().join("server-20260619-120000.log").is_file());
        assert_eq!(
            fs::read(d.path().join("server-20260619-120000.log")).unwrap(),
            b"old session"
        );
    }

    #[test]
    fn rotate_noop_when_no_latest() {
        let d = tempdir().unwrap();
        rotate_log(d.path(), "ts").unwrap(); // no panic, nothing created
        assert!(fs::read_dir(d.path()).unwrap().next().is_none());
    }

    #[test]
    fn prune_keeps_newest_archives_not_latest() {
        let d = tempdir().unwrap();
        fs::write(d.path().join(LATEST), b"live").unwrap();
        for i in 0..(KEEP_LOGS + 4) {
            fs::write(d.path().join(format!("server-arc-{i:03}.log")), b"x").unwrap();
        }
        prune_logs(d.path());
        // latest survives
        assert!(d.path().join(LATEST).is_file());
        let archives = list_logs(d.path())
            .unwrap()
            .into_iter()
            .filter(|l| !l.is_latest)
            .count();
        assert_eq!(archives, KEEP_LOGS);
    }

    #[test]
    fn select_respects_count_cap() {
        let archives: Vec<_> = (0..5)
            .map(|i| (std::path::PathBuf::from(format!("a{i}")), i as f64, 1u64))
            .collect();
        // keep newest 2, huge budget → the 3 oldest are deleted.
        let del = select_archives_to_delete(archives, 2, 1_000_000);
        assert_eq!(del.len(), 3);
        assert!(del.contains(&std::path::PathBuf::from("a0")));
        assert!(!del.contains(&std::path::PathBuf::from("a4")));
    }

    #[test]
    fn select_respects_byte_budget_keeping_newest() {
        let archives = vec![
            (std::path::PathBuf::from("new"), 5.0, 6u64), // newest, always kept (total=6)
            (std::path::PathBuf::from("mid"), 4.0, 6u64), // 6+6=12 > 10 → delete
            (std::path::PathBuf::from("old"), 3.0, 2u64), // 6+2=8 <= 10 → kept
        ];
        let del = select_archives_to_delete(archives, 100, 10);
        assert_eq!(del, vec![std::path::PathBuf::from("mid")]);
    }

    #[test]
    fn list_marks_latest_and_sorts_newest_first() {
        let d = tempdir().unwrap();
        fs::write(d.path().join(LATEST), b"live").unwrap();
        fs::write(d.path().join("server-arc-1.log"), b"a").unwrap();
        let list = list_logs(d.path()).unwrap();
        assert!(list.iter().any(|l| l.is_latest && l.file_name == LATEST));
        assert!(list
            .iter()
            .any(|l| l.file_name == "server-arc-1.log" && !l.is_latest));
    }
}
