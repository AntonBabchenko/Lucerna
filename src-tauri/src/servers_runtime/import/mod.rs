//! Импорт готового сервера из `.zip` или папки (Слайс 2). Детект загрузчика/
//! версии, staging, гибрид preserve/reprovision.

pub mod copy;
pub mod detect;
pub mod pack;

use crate::error::{Error, Result};
use crate::instances::ids::new_id;
use crate::servers_runtime::schema::{ServerCore, ServerFile};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::{Path, PathBuf};

/// Превью импорта, возвращается `inspect` и потребляется визардом (Слайс 2b).
#[derive(Debug, Clone, Serialize, Type)]
pub struct ServerImportPreview {
    pub token: String,
    pub detected_name: String,
    pub mc_version: Option<String>,
    pub loader: Option<ServerCore>,
    pub loader_version: Option<String>,
    pub can_launch_as_is: bool,
    pub mod_count: u32,
    pub world_present: bool,
    pub eula_in_source: bool,
    pub size_bytes: f64,
}

/// Сайдкар staging: откуда брать данные на commit.
#[derive(Serialize, Deserialize)]
struct StagingMeta {
    /// "zip" → данные в `<staging>/content`; "folder" → в `source`.
    kind: String,
    source: String,
}

pub(crate) fn staging_dir(base: &Path, token: &str) -> PathBuf {
    base.join("servers").join(format!(".tmp-import-{token}"))
}

fn write_meta(dir: &Path, meta: &StagingMeta) -> Result<()> {
    let json = serde_json::to_string(meta)
        .map_err(|e| Error::io(dir.display().to_string(), format!("meta: {e}")))?;
    std::fs::write(dir.join("meta.json"), json).map_err(|e| Error::io(dir.display().to_string(), e))
}

fn read_meta(dir: &Path) -> Result<StagingMeta> {
    let raw = std::fs::read_to_string(dir.join("meta.json"))
        .map_err(|e| Error::io(dir.display().to_string(), e))?;
    serde_json::from_str(&raw)
        .map_err(|e| Error::io(dir.display().to_string(), format!("meta: {e}")))
}

/// Фаза 1: подготовить staging, продетектить, вернуть превью.
/// `source` — `.zip` (файл) ИЛИ папка с сервером.
pub fn inspect(base: &Path, source: &Path) -> Result<ServerImportPreview> {
    let token = new_id();
    let staging = staging_dir(base, &token);
    std::fs::create_dir_all(&staging).map_err(|e| Error::io(staging.display().to_string(), e))?;

    let result = (|| -> Result<ServerImportPreview> {
        let (root, kind, source_str, fallback_name) = if source.is_file() {
            copy::check_archive_size(source, copy::PER_FILE_CAP, copy::AGGREGATE_CAP)?;
            let content = staging.join("content");
            std::fs::create_dir_all(&content)
                .map_err(|e| Error::io(content.display().to_string(), e))?;
            crate::worlds::zip::extract_zip(source, &content).map_err(map_archive_err)?;
            let root = copy::find_server_root(&content).ok_or(Error::ServerImportNotAServer)?;
            let name = source
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Server")
                .to_string();
            (root, "zip", content.display().to_string(), name)
        } else if source.is_dir() {
            let root = copy::find_server_root(source).ok_or(Error::ServerImportNotAServer)?;
            let name = source
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("Server")
                .to_string();
            (root.clone(), "folder", source.display().to_string(), name)
        } else {
            return Err(Error::ServerImportUnsupportedSource);
        };

        write_meta(
            &staging,
            &StagingMeta {
                kind: kind.to_string(),
                source: source_str,
            },
        )?;

        let det = detect::detect(&root);
        let can = det
            .loader
            .map(|l| detect::can_launch_as_is(&root, l))
            .unwrap_or(false);
        Ok(ServerImportPreview {
            token: token.clone(),
            detected_name: fallback_name,
            mc_version: det.mc_version,
            loader: det.loader,
            loader_version: det.loader_version,
            can_launch_as_is: can,
            mod_count: count_jars(&root.join("mods")),
            world_present: root.join("world/level.dat").exists(),
            eula_in_source: eula_true(&root.join("eula.txt")),
            size_bytes: dir_size(&root) as f64,
        })
    })();

    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result
}

/// Фаза 3 (preserve): staged-данные уже в нашем launch-контракте → переносим
/// `runtime/` целиком (минус logs/server.json через скип-сет), пишем json+eula.
/// Возвращает id нового сервера. Удаляет staging.
#[allow(clippy::too_many_arguments)]
pub fn commit_preserve(
    base: &Path,
    token: &str,
    name: &str,
    mc_version: &str,
    loader: ServerCore,
    loader_version: Option<String>,
    max_heap_mb: u32,
    eula_accepted: bool,
) -> Result<String> {
    let root = staged_root(base, token)?;
    // Reserve a readable, unique directory; the reserved name is the id. Any
    // failure below removes it so a partial import never leaks the slug.
    let (id, reserved_dir) =
        crate::naming::reserve_unique_dir(&crate::paths::servers_root(base), name, "server")?;
    // Remove the reserved directory if any step below fails (`?`), so a partial
    // import never leaks the slug. Disarmed on success via `keep()`.
    let cleanup = crate::naming::DirCleanup::new(&reserved_dir);
    let p = crate::paths::server_paths(base, &id);
    // Copy all runnable state (server.jar, libraries/, user_jvm_args.txt,
    // worlds, mods, configs, etc.). SKIP_PRESERVE omits only Lucerna-managed
    // files (logs, server.json, backups) that will be re-created or are
    // irrelevant.
    copy::copy_into_runtime_preserving(&root, &p.runtime)?;
    let file = build_file(
        &id,
        name,
        mc_version,
        loader,
        loader_version,
        max_heap_mb,
        eula_accepted,
    );
    crate::servers_runtime::store::write_server_json(&p.json, &file)?;
    crate::servers_runtime::eula::write_eula(&p.runtime.join("eula.txt"), eula_accepted)?;
    let _ = std::fs::remove_dir_all(staging_dir(base, token));
    cleanup.keep();
    Ok(id)
}

/// Resolve the staged server root for an existing token (zip content or folder).
pub fn staged_root(base: &Path, token: &str) -> Result<PathBuf> {
    let staging = staging_dir(base, token);
    if !staging.exists() {
        return Err(Error::ServerImportStagingExpired {
            token: token.to_string(),
        });
    }
    let meta = read_meta(&staging)?;
    let search_base = if meta.kind == "zip" {
        staging.join("content")
    } else {
        let p = PathBuf::from(&meta.source);
        if !p.exists() {
            return Err(Error::ServerImportStagingExpired {
                token: token.to_string(),
            });
        }
        p
    };
    copy::find_server_root(&search_base).ok_or(Error::ServerImportNotAServer)
}

pub fn cancel(base: &Path, token: &str) -> Result<()> {
    let dir = staging_dir(base, token);
    match std::fs::remove_dir_all(&dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::io(dir.display().to_string(), e)),
    }
}

/// Remove stale `.tmp-import-*` staging dirs older than ~1h (orphans from a
/// crashed/abandoned import). Best-effort; never fails the caller.
pub fn sweep_stale(base: &Path) {
    let servers = base.join("servers");
    let cutoff = std::time::SystemTime::now().checked_sub(std::time::Duration::from_secs(3600));
    let Ok(rd) = std::fs::read_dir(&servers) else {
        return;
    };
    for e in rd.flatten() {
        let name = e.file_name();
        let Some(n) = name.to_str() else { continue };
        if !n.starts_with(".tmp-import-") {
            continue;
        }
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .zip(cutoff)
            .map(|(m, c)| m < c)
            .unwrap_or(true);
        if old {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_file(
    id: &str,
    name: &str,
    mc_version: &str,
    loader: ServerCore,
    loader_version: Option<String>,
    max_heap_mb: u32,
    eula_accepted: bool,
) -> ServerFile {
    ServerFile {
        id: id.to_string(),
        name: name.to_string(),
        mc_version: mc_version.to_string(),
        loader,
        loader_version,
        max_heap_mb,
        extra_jvm_args: String::new(),
        created_unix_ms: chrono::Utc::now().timestamp_millis() as f64,
        eula_accepted,
        created_from_instance: None,
        handled_log_sig: None,
        java_component: None,
        upload: None,
    }
}

fn map_archive_err(e: Error) -> Error {
    match e {
        Error::BackupCorrupt { details, .. } => Error::ServerImportInvalidArchive { details },
        other => other,
    }
}

fn count_jars(mods: &Path) -> u32 {
    std::fs::read_dir(mods)
        .map(|rd| {
            rd.flatten()
                .filter(|e| {
                    e.file_name()
                        .to_str()
                        .map(|n| n.to_ascii_lowercase().ends_with(".jar"))
                        .unwrap_or(false)
                })
                .count() as u32
        })
        .unwrap_or(0)
}

fn eula_true(path: &Path) -> bool {
    std::fs::read_to_string(path)
        .map(|s| {
            s.lines()
                .any(|l| l.trim().eq_ignore_ascii_case("eula=true"))
        })
        .unwrap_or(false)
}

fn dir_size(root: &Path) -> u64 {
    let mut total = 0u64;
    let mut q = std::collections::VecDeque::new();
    q.push_back(root.to_path_buf());
    while let Some(d) = q.pop_front() {
        if let Ok(rd) = std::fs::read_dir(&d) {
            for e in rd.flatten() {
                match e.file_type() {
                    Ok(ft) if ft.is_dir() && !ft.is_symlink() => q.push_back(e.path()),
                    Ok(ft) if ft.is_file() => {
                        total = total.saturating_add(e.metadata().map(|m| m.len()).unwrap_or(0))
                    }
                    _ => {}
                }
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write as _;
    use tempfile::tempdir;
    use zip::write::SimpleFileOptions;

    fn make_zip(entries: &[(&str, &[u8])]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("in.zip");
        let f = fs::File::create(&path).unwrap();
        let mut w = zip::ZipWriter::new(f);
        for (name, body) in entries {
            if name.ends_with('/') {
                w.add_directory(*name, SimpleFileOptions::default())
                    .unwrap();
            } else {
                w.start_file(*name, SimpleFileOptions::default()).unwrap();
                w.write_all(body).unwrap();
            }
        }
        w.finish().unwrap();
        (dir, path)
    }

    #[test]
    fn inspect_folder_detects_vanilla_and_world() {
        let base = tempdir().unwrap();
        let src = tempdir().unwrap();
        fs::create_dir_all(src.path().join("world")).unwrap();
        fs::write(src.path().join("world/level.dat"), b"x").unwrap();
        fs::write(src.path().join("server.properties"), b"server-port=25565\n").unwrap();
        fs::write(src.path().join("eula.txt"), b"eula=true\n").unwrap();
        fs::write(src.path().join("mods/a.jar"), b"x").ok(); // no mods dir => fine
        let prev = inspect(base.path(), src.path()).unwrap();
        assert!(prev.world_present);
        assert!(prev.eula_in_source);
        assert!(!prev.token.is_empty());
        // detected name defaults to the source folder name
        assert_eq!(
            prev.detected_name,
            src.path().file_name().unwrap().to_string_lossy()
        );
    }

    #[test]
    fn inspect_rejects_non_server() {
        let base = tempdir().unwrap();
        let src = tempdir().unwrap();
        fs::write(src.path().join("notes.txt"), b"x").unwrap();
        let r = inspect(base.path(), src.path());
        assert!(matches!(r, Err(Error::ServerImportNotAServer)), "got {r:?}");
    }

    #[test]
    fn inspect_zip_extracts_and_detects() {
        let base = tempdir().unwrap();
        let (_z, zip) = make_zip(&[
            ("server.properties", b"server-port=25565\n"),
            ("world/level.dat", b"x"),
        ]);
        let prev = inspect(base.path(), &zip).unwrap();
        assert!(prev.world_present);
        // staging dir created under base/servers/.tmp-import-<token>
        assert!(staging_dir(base.path(), &prev.token).exists());
    }

    #[test]
    fn cancel_removes_staging() {
        let base = tempdir().unwrap();
        let (_z, zip) = make_zip(&[("server.properties", b"x")]);
        let prev = inspect(base.path(), &zip).unwrap();
        cancel(base.path(), &prev.token).unwrap();
        assert!(!staging_dir(base.path(), &prev.token).exists());
    }

    #[test]
    fn commit_preserve_lays_down_runtime_and_writes_json() {
        let base = tempdir().unwrap();
        // A Lucerna-export-shaped vanilla server in a zip.
        let (_z, zip) = make_zip(&[
            ("server.jar", b"JAR"),
            ("server.properties", b"server-port=25565\n"),
            ("world/level.dat", b"x"),
        ]);
        let prev = inspect(base.path(), &zip).unwrap();
        let id = commit_preserve(
            base.path(),
            &prev.token,
            "My Imported",
            "1.20.4",
            crate::servers_runtime::schema::ServerCore::Vanilla,
            None,
            4096,
            true,
        )
        .unwrap();
        let p = crate::paths::server_paths(base.path(), &id);
        assert!(p.json.exists());
        assert!(p.runtime.join("server.jar").is_file());
        assert!(p.runtime.join("world/level.dat").is_file());
        let f = crate::servers_runtime::store::read_server_json(&p.json).unwrap();
        assert_eq!(f.name, "My Imported");
        assert_eq!(f.mc_version, "1.20.4");
        assert!(f.eula_accepted);
        // staging cleaned
        assert!(!staging_dir(base.path(), &prev.token).exists());
    }
}
