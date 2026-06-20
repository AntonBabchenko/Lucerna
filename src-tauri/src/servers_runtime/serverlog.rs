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

/// Keep the newest `KEEP_LOGS` archives (`server-*.log` except `server-latest.log`);
/// delete the rest. Best-effort. `server-latest.log` is always kept.
pub fn prune_logs(logs_dir: &Path) {
    let mut archives: Vec<(std::path::PathBuf, f64)> = match std::fs::read_dir(logs_dir) {
        Ok(rd) => rd
            .flatten()
            .filter(|e| is_archive(&e.file_name().to_string_lossy()))
            .filter_map(|e| e.metadata().ok().map(|m| (e.path(), mtime_ms(&m))))
            .collect(),
        Err(_) => return,
    };
    archives.sort_by(|a, b| b.1.total_cmp(&a.1)); // newest first
    for (path, _) in archives.into_iter().skip(KEEP_LOGS) {
        let _ = std::fs::remove_file(path);
    }
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
