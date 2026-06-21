//! Server log retention: rotate `server-latest.log` to a timestamped archive on
//! each start (so prior sessions aren't lost), keep newest N, list + read past
//! sessions. `server-latest.log` is never an archive and never pruned.

use crate::error::{Error, Result};
use crate::instances::schema::LogRetentionPolicy;
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

/// Safety-floor count cap for rotated archives (`server-<ts>.log`), applied
/// only when the user has NOT enabled log retention. It keeps a crash/restart
/// loop from growing the log dir without bound even with retention off. When
/// retention IS enabled, the user's `max_files` governs instead. The live
/// `server-latest.log` is never counted/pruned.
pub const KEEP_LOGS: usize = 15;

/// Safety-floor byte budget for rotated archives, applied only when retention
/// is disabled (the user's `max_total_mb` governs when enabled). The live
/// `server-latest.log` is excluded.
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

/// Prune rotated archives best-effort. Honors the user's log-retention policy
/// when enabled — the same `max_files` / `max_total_mb` the launcher applies to
/// instance logs (parity, via the shared [`crate::logs::retention::select_excess`]
/// selector). When retention is disabled, falls back to the built-in safety
/// floor (`KEEP_LOGS` / `MAX_TOTAL_MB`) so disk use stays bounded regardless.
/// The live `server-latest.log` is never an archive and never pruned.
pub fn prune_logs(logs_dir: &Path, policy: &LogRetentionPolicy) {
    let archives: Vec<(std::path::PathBuf, f64, u64)> = match std::fs::read_dir(logs_dir) {
        Ok(rd) => rd
            .flatten()
            .filter(|e| is_archive(&e.file_name().to_string_lossy()))
            .filter_map(|e| e.metadata().ok().map(|m| (e.path(), mtime_ms(&m), m.len())))
            .collect(),
        Err(_) => return,
    };
    let (max_files, budget_bytes): (usize, u64) = if policy.enabled {
        (
            policy.max_files as usize,
            (policy.max_total_mb as u64).saturating_mul(1024 * 1024),
        )
    } else {
        (KEEP_LOGS, MAX_TOTAL_MB.saturating_mul(1024 * 1024))
    };
    let inputs: Vec<crate::logs::retention::RetentionInput> = archives
        .iter()
        .map(|(_, mtime, size)| crate::logs::retention::RetentionInput {
            mtime_ms: *mtime,
            size_bytes: *size,
        })
        .collect();
    for idx in crate::logs::retention::select_excess(&inputs, max_files, budget_bytes) {
        let _ = std::fs::remove_file(&archives[idx].0);
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

    fn policy(enabled: bool, max_files: u32, max_total_mb: u32) -> LogRetentionPolicy {
        LogRetentionPolicy {
            enabled,
            max_files,
            max_total_mb,
        }
    }

    fn count_archives(dir: &Path) -> usize {
        list_logs(dir)
            .unwrap()
            .into_iter()
            .filter(|l| !l.is_latest)
            .count()
    }

    #[test]
    fn prune_floor_applies_when_retention_disabled() {
        let d = tempdir().unwrap();
        fs::write(d.path().join(LATEST), b"live").unwrap();
        for i in 0..(KEEP_LOGS + 4) {
            fs::write(d.path().join(format!("server-arc-{i:03}.log")), b"x").unwrap();
        }
        // Disabled → the (1, 1) values are ignored; the KEEP_LOGS floor governs.
        prune_logs(d.path(), &policy(false, 1, 1));
        assert!(d.path().join(LATEST).is_file(), "latest survives");
        assert_eq!(count_archives(d.path()), KEEP_LOGS);
    }

    #[test]
    fn prune_honors_user_policy_when_enabled() {
        let d = tempdir().unwrap();
        fs::write(d.path().join(LATEST), b"live").unwrap();
        for i in 0..6 {
            fs::write(d.path().join(format!("server-arc-{i:03}.log")), b"x").unwrap();
        }
        // Enabled with a tighter cap than the floor → user's max_files wins.
        prune_logs(d.path(), &policy(true, 2, 9999));
        assert!(d.path().join(LATEST).is_file(), "latest survives");
        assert_eq!(count_archives(d.path()), 2);
    }

    #[test]
    fn prune_honors_byte_budget_when_enabled() {
        // Count is effectively infinite (9999); only the BYTE budget can bind.
        // Six 400 KiB archives, budget 1 MiB → newest two fit (800 KiB ≤ 1 MiB),
        // the third overflows (1.2 MiB) so it and all older are prefix-cut.
        // Sizes are equal, so the survivor COUNT is deterministic regardless of
        // same-second mtime ties. A dropped/mis-scaled budget arg would keep all
        // six (huge budget) or just one (zero budget) — both caught here.
        let d = tempdir().unwrap();
        fs::write(d.path().join(LATEST), b"live").unwrap();
        let blob = vec![b'x'; 400 * 1024];
        for i in 0..6 {
            fs::write(d.path().join(format!("server-arc-{i:03}.log")), &blob).unwrap();
        }
        prune_logs(d.path(), &policy(true, 9999, 1));
        assert!(d.path().join(LATEST).is_file(), "latest survives");
        assert_eq!(count_archives(d.path()), 2);
    }

    #[test]
    fn prune_floor_uses_byte_floor_not_policy_when_disabled() {
        // Disabled → the 200 MiB byte FLOOR governs, not the policy's tiny
        // max_total_mb=1. Four 400 KiB archives total 1.6 MiB: over the policy's
        // 1 MiB (which is ignored) but well under the 200 MiB floor and the
        // KEEP_LOGS count floor → nothing is pruned. A bug that used the policy
        // budget when disabled would byte-cut down to ~2.
        let d = tempdir().unwrap();
        fs::write(d.path().join(LATEST), b"live").unwrap();
        let blob = vec![b'x'; 400 * 1024];
        for i in 0..4 {
            fs::write(d.path().join(format!("server-arc-{i:03}.log")), &blob).unwrap();
        }
        prune_logs(d.path(), &policy(false, 9999, 1));
        assert_eq!(count_archives(d.path()), 4);
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
