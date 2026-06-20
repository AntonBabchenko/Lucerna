//! Server snapshots: zip `runtime/` (reusing transfer::export_zip) to
//! `servers/<id>/backups/<ts>.zip`, list/restore/delete + keep-newest-N prune.

use crate::error::{Error, Result};
use serde::Serialize;
use specta::Type;
use std::path::{Path, PathBuf};

/// One snapshot file shown to the UI.
#[derive(Debug, Clone, Serialize, Type)]
pub struct BackupInfo {
    pub file_name: String,
    pub created_unix_ms: f64,
    pub size_bytes: f64,
}

/// Keep at most this many snapshots per server (oldest pruned on create).
pub const KEEP_BACKUPS: usize = 10;

/// Opt-in automatic-backup policy (#29). Persisted per server in
/// `backup-policy.json` under the server root. An absent or unparseable file
/// means "disabled" (the back-compat default), so existing servers keep their
/// current behaviour until the user opts in. The interval scheduler that acts
/// on this lives in the command layer (it needs an `AppHandle`); the pure
/// due-logic and persistence live here so they are unit-testable.
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize, Type, Default)]
pub struct BackupPolicy {
    /// Master switch. When false, no automatic snapshots are taken.
    #[serde(default)]
    pub enabled: bool,
    /// Minimum minutes between automatic snapshots. `0` is treated as disabled
    /// even when `enabled` is true (guards against a hot loop).
    #[serde(default)]
    pub interval_minutes: u32,
    /// Epoch-ms of the last automatic snapshot, stamped by [`maybe_auto_backup`].
    /// `0.0` = never run, so the first check after enabling is immediately due.
    #[serde(default)]
    pub last_run_unix_ms: f64,
}

fn backups_dir(base: &Path, id: &str) -> PathBuf {
    crate::paths::server_paths(base, id).root.join("backups")
}

fn policy_path(base: &Path, id: &str) -> PathBuf {
    crate::paths::server_paths(base, id)
        .root
        .join("backup-policy.json")
}

/// Read the server's backup policy. Absent or unreadable/invalid → the disabled
/// default (never surfaces an error; a corrupt sidecar must not break the UI).
pub fn read_policy(base: &Path, id: &str) -> BackupPolicy {
    std::fs::read_to_string(policy_path(base, id))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Persist the server's backup policy (creates the server root if needed).
pub fn write_policy(base: &Path, id: &str, policy: &BackupPolicy) -> Result<()> {
    let path = policy_path(base, id);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| Error::io(dir.display().to_string(), e))?;
    }
    let json = serde_json::to_string_pretty(policy)
        .map_err(|e| Error::io(path.display().to_string(), format!("policy: {e}")))?;
    std::fs::write(&path, json).map_err(|e| Error::io(path.display().to_string(), e))
}

/// Pure due-check: an automatic backup is due iff the policy is enabled with a
/// positive interval and at least `interval_minutes` have elapsed since the last
/// run (`last_run_unix_ms == 0.0` means "never run" → due immediately).
pub fn is_due(policy: &BackupPolicy, now_unix_ms: f64) -> bool {
    if !policy.enabled || policy.interval_minutes == 0 {
        return false;
    }
    let interval_ms = policy.interval_minutes as f64 * 60_000.0;
    now_unix_ms - policy.last_run_unix_ms >= interval_ms
}

/// If a scheduled backup is due, create one and stamp `last_run_unix_ms`.
/// Returns `Some(info)` when a snapshot was taken, `None` when not due. A
/// snapshot failure propagates (the caller logs it; the stamp is NOT advanced,
/// so the next tick retries) — mirroring the manual restore safety-net stance.
pub fn maybe_auto_backup(
    base: &Path,
    id: &str,
    now_unix_ms: f64,
    stamp: &str,
) -> Result<Option<BackupInfo>> {
    let mut policy = read_policy(base, id);
    if !is_due(&policy, now_unix_ms) {
        return Ok(None);
    }
    let info = create_backup(base, id, stamp)?;
    policy.last_run_unix_ms = now_unix_ms;
    write_policy(base, id, &policy)?;
    Ok(Some(info))
}

/// True iff `name` is a single safe path component (no separators / `..` / drive).
fn is_safe_file_name(name: &str) -> bool {
    if name.contains('\\') || name.contains(':') {
        return false;
    }
    let mut c = std::path::Path::new(name).components();
    matches!(c.next(), Some(std::path::Component::Normal(_))) && c.next().is_none()
}

/// Snapshot `runtime/` into `backups/<stamp>.zip` (reusing the logs-excluding
/// export writer), then prune to the newest `KEEP_BACKUPS`. Returns the new
/// snapshot's info.
pub fn create_backup(base: &Path, id: &str, stamp: &str) -> Result<BackupInfo> {
    let p = crate::paths::server_paths(base, id);
    let dir = backups_dir(base, id);
    std::fs::create_dir_all(&dir).map_err(|e| Error::io(dir.display().to_string(), e))?;
    let file_name = format!("backup-{stamp}.zip");
    let dest = dir.join(&file_name);
    crate::servers_runtime::transfer::export_zip(&p.runtime, &dest)?;
    prune(&dir);
    let meta = std::fs::metadata(&dest).map_err(|e| Error::io(dest.display().to_string(), e))?;
    Ok(BackupInfo {
        file_name,
        created_unix_ms: mtime_ms(&meta),
        size_bytes: meta.len() as f64,
    })
}

/// All snapshots, newest first.
pub fn list_backups(base: &Path, id: &str) -> Result<Vec<BackupInfo>> {
    let dir = backups_dir(base, id);
    let mut out = Vec::new();
    let rd = match std::fs::read_dir(&dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(Error::io(dir.display().to_string(), e)),
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !name.to_ascii_lowercase().ends_with(".zip") {
            continue;
        }
        if let Ok(meta) = e.metadata() {
            out.push(BackupInfo {
                file_name: name,
                created_unix_ms: mtime_ms(&meta),
                size_bytes: meta.len() as f64,
            });
        }
    }
    out.sort_by(|a, b| b.created_unix_ms.total_cmp(&a.created_unix_ms));
    Ok(out)
}

/// Restore `file_name` into `runtime/` (reset mode: clear runtime then extract).
/// Caller MUST ensure the server is stopped. Rejects unsafe names / missing file.
pub fn restore_backup(base: &Path, id: &str, file_name: &str) -> Result<()> {
    if !is_safe_file_name(file_name) {
        return Err(Error::io("<backup>", "invalid filename"));
    }
    let p = crate::paths::server_paths(base, id);
    let src = backups_dir(base, id).join(file_name);
    if !src.is_file() {
        return Err(Error::io(src.display().to_string(), "backup not found"));
    }
    // Reset: remove runtime/ then extract the snapshot back into it.
    match std::fs::remove_dir_all(&p.runtime) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(Error::io(p.runtime.display().to_string(), e)),
    }
    std::fs::create_dir_all(&p.runtime)
        .map_err(|e| Error::io(p.runtime.display().to_string(), e))?;
    crate::worlds::zip::extract_zip(&src, &p.runtime).map_err(map_zip_err)?;
    Ok(())
}

/// Delete a snapshot. Idempotent; rejects unsafe names.
pub fn delete_backup(base: &Path, id: &str, file_name: &str) -> Result<()> {
    if !is_safe_file_name(file_name) {
        return Err(Error::io("<backup>", "invalid filename"));
    }
    let path = backups_dir(base, id).join(file_name);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::io(path.display().to_string(), e)),
    }
}

fn prune(dir: &Path) {
    let mut zips: Vec<(PathBuf, f64)> = match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .map(|n| n.to_ascii_lowercase().ends_with(".zip"))
                    .unwrap_or(false)
            })
            .filter_map(|e| e.metadata().ok().map(|m| (e.path(), mtime_ms(&m))))
            .collect(),
        Err(_) => return,
    };
    zips.sort_by(|a, b| b.1.total_cmp(&a.1)); // newest first
    for (path, _) in zips.into_iter().skip(KEEP_BACKUPS) {
        let _ = std::fs::remove_file(path);
    }
}

fn mtime_ms(meta: &std::fs::Metadata) -> f64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

fn map_zip_err(e: Error) -> Error {
    match e {
        Error::BackupCorrupt { details, .. } => {
            Error::io("<backup>", format!("corrupt: {details}"))
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn seed_runtime(base: &Path, id: &str) -> PathBuf {
        let p = crate::paths::server_paths(base, id);
        fs::create_dir_all(p.runtime.join("world")).unwrap();
        fs::write(p.runtime.join("server.jar"), b"JAR").unwrap();
        fs::write(p.runtime.join("world/level.dat"), b"LVL").unwrap();
        fs::create_dir_all(p.runtime.join("logs")).unwrap();
        fs::write(p.runtime.join("logs/server-latest.log"), b"noise").unwrap();
        p.runtime
    }

    #[test]
    fn create_then_list_roundtrip() {
        let base = tempdir().unwrap();
        seed_runtime(base.path(), "srv-1");
        let info = create_backup(base.path(), "srv-1", "20260619-120000").unwrap();
        assert!(info.file_name.ends_with(".zip"));
        assert!(info.size_bytes > 0.0);
        let list = list_backups(base.path(), "srv-1").unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].file_name, info.file_name);
        // backup excludes logs/
        let dir = crate::paths::server_paths(base.path(), "srv-1")
            .root
            .join("backups");
        assert!(dir.join(&info.file_name).is_file());
    }

    #[test]
    fn restore_replaces_runtime_from_backup() {
        let base = tempdir().unwrap();
        let rt = seed_runtime(base.path(), "srv-1");
        let info = create_backup(base.path(), "srv-1", "20260619-120000").unwrap();
        // Mutate the live world, then restore → original content back.
        fs::write(rt.join("world/level.dat"), b"CHANGED").unwrap();
        restore_backup(base.path(), "srv-1", &info.file_name).unwrap();
        assert_eq!(fs::read(rt.join("world/level.dat")).unwrap(), b"LVL");
        assert!(rt.join("server.jar").is_file());
    }

    #[test]
    fn restore_missing_file_errors() {
        let base = tempdir().unwrap();
        seed_runtime(base.path(), "srv-1");
        let r = restore_backup(base.path(), "srv-1", "nope.zip");
        assert!(r.is_err());
    }

    #[test]
    fn restore_rejects_unsafe_name() {
        let base = tempdir().unwrap();
        seed_runtime(base.path(), "srv-1");
        assert!(restore_backup(base.path(), "srv-1", "../evil.zip").is_err());
        assert!(delete_backup(base.path(), "srv-1", "../evil.zip").is_err());
    }

    #[test]
    fn delete_is_idempotent() {
        let base = tempdir().unwrap();
        seed_runtime(base.path(), "srv-1");
        let info = create_backup(base.path(), "srv-1", "20260619-120000").unwrap();
        delete_backup(base.path(), "srv-1", &info.file_name).unwrap();
        delete_backup(base.path(), "srv-1", &info.file_name).unwrap(); // gone → still Ok
        assert!(list_backups(base.path(), "srv-1").unwrap().is_empty());
    }

    #[test]
    fn prune_keeps_newest_n() {
        let base = tempdir().unwrap();
        seed_runtime(base.path(), "srv-1");
        for i in 0..(KEEP_BACKUPS + 3) {
            create_backup(base.path(), "srv-1", &format!("ts-{i:03}")).unwrap();
        }
        let list = list_backups(base.path(), "srv-1").unwrap();
        assert_eq!(list.len(), KEEP_BACKUPS, "older snapshots pruned");
    }

    // ── #29 auto/scheduled backup policy ─────────────────────────────────────

    #[test]
    fn policy_defaults_to_disabled_when_absent() {
        let base = tempdir().unwrap();
        let p = read_policy(base.path(), "srv-1");
        assert_eq!(p, BackupPolicy::default());
        assert!(!p.enabled);
        assert_eq!(p.interval_minutes, 0);
    }

    #[test]
    fn policy_roundtrips_through_disk() {
        let base = tempdir().unwrap();
        seed_runtime(base.path(), "srv-1");
        let policy = BackupPolicy {
            enabled: true,
            interval_minutes: 30,
            last_run_unix_ms: 1_700_000_000_000.0,
        };
        write_policy(base.path(), "srv-1", &policy).unwrap();
        assert_eq!(read_policy(base.path(), "srv-1"), policy);
    }

    #[test]
    fn corrupt_policy_file_reads_as_default() {
        let base = tempdir().unwrap();
        let path = policy_path(base.path(), "srv-1");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{ not json").unwrap();
        assert_eq!(read_policy(base.path(), "srv-1"), BackupPolicy::default());
    }

    #[test]
    fn is_due_false_when_disabled_or_zero_interval() {
        let disabled = BackupPolicy {
            enabled: false,
            interval_minutes: 30,
            last_run_unix_ms: 0.0,
        };
        assert!(!is_due(&disabled, 10_000_000.0));
        let zero_interval = BackupPolicy {
            enabled: true,
            interval_minutes: 0,
            last_run_unix_ms: 0.0,
        };
        assert!(!is_due(&zero_interval, 10_000_000.0));
    }

    #[test]
    fn is_due_true_on_first_check_then_false_until_interval_elapses() {
        let policy = BackupPolicy {
            enabled: true,
            interval_minutes: 10, // 600_000 ms
            last_run_unix_ms: 1_000_000.0,
        };
        // Just enabled, never run from this stamp's perspective: exactly the
        // interval later → due.
        assert!(is_due(&policy, 1_000_000.0 + 600_000.0));
        // One ms before the interval elapses → not yet due.
        assert!(!is_due(&policy, 1_000_000.0 + 599_999.0));
    }

    #[test]
    fn maybe_auto_backup_creates_when_due_and_stamps_last_run() {
        let base = tempdir().unwrap();
        seed_runtime(base.path(), "srv-1");
        write_policy(
            base.path(),
            "srv-1",
            &BackupPolicy {
                enabled: true,
                interval_minutes: 10,
                last_run_unix_ms: 0.0,
            },
        )
        .unwrap();
        let now = 5_000_000.0;
        let info = maybe_auto_backup(base.path(), "srv-1", now, "auto-1")
            .unwrap()
            .expect("backup taken when due");
        assert!(info.file_name.ends_with(".zip"));
        // last_run advanced → an immediate re-check is no longer due.
        assert_eq!(read_policy(base.path(), "srv-1").last_run_unix_ms, now);
        assert!(maybe_auto_backup(base.path(), "srv-1", now, "auto-2")
            .unwrap()
            .is_none());
    }

    #[test]
    fn maybe_auto_backup_noops_when_disabled() {
        let base = tempdir().unwrap();
        seed_runtime(base.path(), "srv-1");
        // No policy file → disabled default.
        assert!(
            maybe_auto_backup(base.path(), "srv-1", 9_999_999.0, "auto-x")
                .unwrap()
                .is_none()
        );
        assert!(list_backups(base.path(), "srv-1").unwrap().is_empty());
    }
}
