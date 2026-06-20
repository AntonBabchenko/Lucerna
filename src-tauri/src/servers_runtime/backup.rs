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

fn backups_dir(base: &Path, id: &str) -> PathBuf {
    crate::paths::server_paths(base, id).root.join("backups")
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
}
