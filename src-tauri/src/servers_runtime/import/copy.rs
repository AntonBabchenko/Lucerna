//! Копирование staged-данных в `runtime/` со скип-сетом бинарников загрузчика,
//! защита от zip-bomb (капы), и поиск «корня сервера» в распакованном дереве.

use crate::error::{Error, Result};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

/// Per-file / aggregate caps. Серверы с модами+миром легитимно большие.
pub const PER_FILE_CAP: u64 = 4 * 1024 * 1024 * 1024;
pub const AGGREGATE_CAP: u64 = 20 * 1024 * 1024 * 1024;

/// Топ-левел имена, которые НЕ копируются (регенерируемые бинарники загрузчика
/// + мусор). Сравнение без учёта регистра.
pub const SKIP_TOP_LEVEL: &[&str] = &[
    "server.jar",
    "installer.jar",
    "libraries",
    "versions",
    "run.bat",
    "run.sh",
    "user_jvm_args.txt",
    "fabric-server-launch.jar",
    "fabric-server-launcher.jar",
    "quilt-server-launch.jar",
    ".fabric",
    ".quilt",
    "logs",
    "server.json",
    "server.json.tmp",
    "backups",
    // Reprovision regenerates eula.txt (create_*_server writes eula=true after
    // the user accepted in the wizard). The source's eula.txt is often eula=false
    // (a freshly downloaded server pack), which would otherwise overwrite our
    // eula=true and make Minecraft exit on launch — so skip it on the reprovision
    // copy. (The preserve path uses SKIP_PRESERVE + a write_eula afterwards.)
    "eula.txt",
    // Server-pack metadata + overrides (#10): the manifests are launcher input,
    // not runnable state, and overrides are merged into the root by
    // `pack::apply_overrides` — never copied verbatim as a stray subdir.
    "manifest.json",
    "modrinth.index.json",
    "overrides",
    "server-overrides",
    "client-overrides",
];

/// Признаки «здесь корень сервера»: server.jar / любой *.jar лаунчера /
/// libraries / eula.txt / server.properties / папка world с level.dat, ИЛИ
/// манифест серверного пака (CurseForge `manifest.json` / Modrinth
/// `modrinth.index.json`) — последние ещё не запускаемы, но материализуются
/// при commit (#10).
fn looks_like_server_root(dir: &Path) -> bool {
    dir.join("server.jar").exists()
        || dir.join("fabric-server-launch.jar").exists()
        || dir.join("quilt-server-launch.jar").exists()
        || dir.join("libraries").is_dir()
        || dir.join("eula.txt").exists()
        || dir.join("server.properties").exists()
        || dir.join("world/level.dat").exists()
        || dir.join("manifest.json").is_file()
        || dir.join("modrinth.index.json").is_file()
}

/// BFS: самый «мелкий» каталог с признаками сервера (zip может содержать один
/// верхний подкаталог). Симлинки не следуются. `None` если ничего не похоже.
pub fn find_server_root(root: &Path) -> Option<PathBuf> {
    let mut q: VecDeque<PathBuf> = VecDeque::new();
    q.push_back(root.to_path_buf());
    while let Some(dir) = q.pop_front() {
        if looks_like_server_root(&dir) {
            return Some(dir);
        }
        let mut subdirs: Vec<PathBuf> = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                if let Ok(ft) = e.file_type() {
                    if ft.is_dir() && !ft.is_symlink() {
                        subdirs.push(e.path());
                    }
                }
            }
        }
        subdirs.sort();
        for s in subdirs {
            q.push_back(s);
        }
    }
    None
}

/// Skip-set for the PRESERVE path: omit only what Lucerna manages itself
/// (its own metadata + logs + backups). Everything else — server.jar,
/// libraries/, user_jvm_args.txt, args files — is the runnable state and is kept.
pub const SKIP_PRESERVE: &[&str] = &["logs", "server.json", "server.json.tmp", "backups"];

/// Скопировать `src` → `runtime`, пропуская топ-левел скип-сет, с дефолтными капами.
pub fn copy_into_runtime(src: &Path, runtime: &Path) -> Result<()> {
    copy_into_runtime_with_skip(src, runtime, SKIP_TOP_LEVEL, PER_FILE_CAP, AGGREGATE_CAP)
}

pub fn copy_into_runtime_capped(
    src: &Path,
    runtime: &Path,
    per_file_cap: u64,
    aggregate_cap: u64,
) -> Result<()> {
    copy_into_runtime_with_skip(src, runtime, SKIP_TOP_LEVEL, per_file_cap, aggregate_cap)
}

/// Copy `src` → `runtime`, preserving loader binaries (PRESERVE path).
pub fn copy_into_runtime_preserving(src: &Path, runtime: &Path) -> Result<()> {
    copy_into_runtime_with_skip(src, runtime, SKIP_PRESERVE, PER_FILE_CAP, AGGREGATE_CAP)
}

fn copy_into_runtime_with_skip(
    src: &Path,
    runtime: &Path,
    skip: &[&str],
    per_file_cap: u64,
    aggregate_cap: u64,
) -> Result<()> {
    std::fs::create_dir_all(runtime).map_err(|e| Error::io(runtime.display().to_string(), e))?;
    let mut aggregate: u64 = 0;
    for entry in std::fs::read_dir(src).map_err(|e| Error::io(src.display().to_string(), e))? {
        let entry = entry.map_err(|e| Error::io(src.display().to_string(), e))?;
        let name = entry.file_name();
        let lower = name.to_string_lossy().to_ascii_lowercase();
        if skip.iter().any(|s| *s == lower) {
            continue;
        }
        let ft = entry
            .file_type()
            .map_err(|e| Error::io(entry.path().display().to_string(), e))?;
        if ft.is_symlink() {
            continue;
        }
        let to = runtime.join(&name);
        if ft.is_dir() {
            copy_tree(
                &entry.path(),
                &to,
                &mut aggregate,
                per_file_cap,
                aggregate_cap,
            )?;
        } else if ft.is_file() {
            copy_file_capped(
                &entry.path(),
                &to,
                &mut aggregate,
                per_file_cap,
                aggregate_cap,
            )?;
        }
    }
    Ok(())
}

fn copy_tree(
    src: &Path,
    dst: &Path,
    aggregate: &mut u64,
    per_file_cap: u64,
    aggregate_cap: u64,
) -> Result<()> {
    std::fs::create_dir_all(dst).map_err(|e| Error::io(dst.display().to_string(), e))?;
    for entry in std::fs::read_dir(src).map_err(|e| Error::io(src.display().to_string(), e))? {
        let entry = entry.map_err(|e| Error::io(src.display().to_string(), e))?;
        let ft = entry
            .file_type()
            .map_err(|e| Error::io(entry.path().display().to_string(), e))?;
        if ft.is_symlink() {
            continue;
        }
        let to = dst.join(entry.file_name());
        if ft.is_dir() {
            copy_tree(&entry.path(), &to, aggregate, per_file_cap, aggregate_cap)?;
        } else if ft.is_file() {
            copy_file_capped(&entry.path(), &to, aggregate, per_file_cap, aggregate_cap)?;
        }
    }
    Ok(())
}

fn copy_file_capped(
    from: &Path,
    to: &Path,
    aggregate: &mut u64,
    per_file_cap: u64,
    aggregate_cap: u64,
) -> Result<()> {
    let size = from.metadata().map(|m| m.len()).unwrap_or(0);
    if size > per_file_cap {
        return Err(Error::ServerImportTooLarge {
            size: size as f64,
            cap: per_file_cap as f64,
        });
    }
    *aggregate = aggregate.saturating_add(size);
    if *aggregate > aggregate_cap {
        return Err(Error::ServerImportTooLarge {
            size: *aggregate as f64,
            cap: aggregate_cap as f64,
        });
    }
    std::fs::copy(from, to).map_err(|e| Error::io(to.display().to_string(), e))?;
    Ok(())
}

/// Pre-extract zip-bomb defense: reject an archive whose declared (central-directory)
/// uncompressed sizes exceed the caps. The on-disk copy enforces real bytes as a
/// second layer. Mirrors `worlds::import::check_zip_size` (kept local to avoid
/// coupling). Non-zip / unreadable → `ServerImportInvalidArchive`.
pub fn check_archive_size(zip_path: &Path, per_file_cap: u64, aggregate_cap: u64) -> Result<()> {
    let file =
        std::fs::File::open(zip_path).map_err(|e| Error::io(zip_path.display().to_string(), e))?;
    let mut archive = zip::ZipArchive::new(std::io::BufReader::new(file)).map_err(|e| {
        Error::ServerImportInvalidArchive {
            details: format!("open: {e}"),
        }
    })?;
    let mut total: u64 = 0;
    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .map_err(|e| Error::ServerImportInvalidArchive {
                details: format!("entry {i}: {e}"),
            })?;
        let size = entry.size();
        if size > per_file_cap {
            return Err(Error::ServerImportTooLarge {
                size: size as f64,
                cap: per_file_cap as f64,
            });
        }
        total = total.saturating_add(size);
        if total > aggregate_cap {
            return Err(Error::ServerImportTooLarge {
                size: total as f64,
                cap: aggregate_cap as f64,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn touch(p: &Path) {
        if let Some(d) = p.parent() {
            fs::create_dir_all(d).unwrap();
        }
        fs::write(p, b"x").unwrap();
    }

    #[test]
    fn copy_skips_loader_binaries_keeps_data() {
        let src = tempdir().unwrap();
        touch(&src.path().join("server.jar"));
        touch(&src.path().join("libraries/foo.jar"));
        touch(&src.path().join("logs/latest.log"));
        touch(&src.path().join("world/level.dat"));
        touch(&src.path().join("mods/cool.jar"));
        touch(&src.path().join("server.properties"));
        touch(&src.path().join("config/foo.toml"));
        touch(&src.path().join("eula.txt"));
        let dst = tempdir().unwrap();
        copy_into_runtime(src.path(), dst.path()).unwrap();
        assert!(dst.path().join("world/level.dat").is_file());
        assert!(dst.path().join("mods/cool.jar").is_file());
        assert!(dst.path().join("server.properties").is_file());
        assert!(dst.path().join("config/foo.toml").is_file());
        assert!(!dst.path().join("server.jar").exists());
        assert!(!dst.path().join("libraries").exists());
        assert!(!dst.path().join("logs").exists());
        // eula.txt is skipped on the reprovision copy — provision_loader writes
        // the correct eula=true; a source eula=false must not overwrite it.
        assert!(!dst.path().join("eula.txt").exists());
    }

    #[test]
    fn copy_skips_pack_manifests_and_overrides() {
        // #10: pack metadata + overrides are applied separately (pack::apply_overrides),
        // never copied verbatim. Guards the SKIP_TOP_LEVEL additions.
        let src = tempdir().unwrap();
        touch(&src.path().join("manifest.json"));
        touch(&src.path().join("modrinth.index.json"));
        touch(&src.path().join("overrides/config/a.toml"));
        touch(&src.path().join("server-overrides/server.properties"));
        touch(&src.path().join("client-overrides/options.txt"));
        touch(&src.path().join("mods/keep.jar"));
        let dst = tempdir().unwrap();
        copy_into_runtime(src.path(), dst.path()).unwrap();
        assert!(dst.path().join("mods/keep.jar").is_file());
        assert!(!dst.path().join("manifest.json").exists());
        assert!(!dst.path().join("modrinth.index.json").exists());
        assert!(!dst.path().join("overrides").exists());
        assert!(!dst.path().join("server-overrides").exists());
        assert!(!dst.path().join("client-overrides").exists());
    }

    #[test]
    fn copy_rejects_over_aggregate_cap() {
        let src = tempdir().unwrap();
        fs::write(src.path().join("a.bin"), vec![0u8; 100]).unwrap();
        fs::write(src.path().join("b.bin"), vec![0u8; 100]).unwrap();
        let dst = tempdir().unwrap();
        let r = copy_into_runtime_capped(src.path(), dst.path(), PER_FILE_CAP, 150);
        assert!(
            matches!(r, Err(Error::ServerImportTooLarge { .. })),
            "got {r:?}"
        );
    }

    #[test]
    fn find_server_root_at_top() {
        let d = tempdir().unwrap();
        touch(&d.path().join("server.properties"));
        assert_eq!(find_server_root(d.path()).as_deref(), Some(d.path()));
    }

    #[test]
    fn find_server_root_nested_one_level() {
        let d = tempdir().unwrap();
        touch(&d.path().join("MyServer/server.jar"));
        assert_eq!(find_server_root(d.path()), Some(d.path().join("MyServer")));
    }

    #[test]
    fn find_server_root_none_when_no_markers() {
        let d = tempdir().unwrap();
        touch(&d.path().join("readme.txt"));
        assert_eq!(find_server_root(d.path()), None);
    }

    #[test]
    fn find_server_root_accepts_pack_manifests() {
        // CurseForge pack (#10).
        let cf = tempdir().unwrap();
        touch(&cf.path().join("Pack/manifest.json"));
        assert_eq!(find_server_root(cf.path()), Some(cf.path().join("Pack")));
        // Modrinth pack (#10).
        let mr = tempdir().unwrap();
        touch(&mr.path().join("modrinth.index.json"));
        assert_eq!(find_server_root(mr.path()).as_deref(), Some(mr.path()));
    }

    #[test]
    fn preserve_keeps_loader_binaries() {
        let src = tempdir().unwrap();
        touch(&src.path().join("server.jar"));
        touch(
            &src.path()
                .join("libraries/net/neoforged/neoforge/20.4.237/win_args.txt"),
        );
        touch(&src.path().join("user_jvm_args.txt"));
        touch(&src.path().join("logs/latest.log"));
        touch(&src.path().join("world/level.dat"));
        let dst = tempdir().unwrap();
        copy_into_runtime_preserving(src.path(), dst.path()).unwrap();
        assert!(dst.path().join("server.jar").is_file());
        assert!(dst
            .path()
            .join("libraries/net/neoforged/neoforge/20.4.237/win_args.txt")
            .is_file());
        assert!(dst.path().join("user_jvm_args.txt").is_file());
        assert!(dst.path().join("world/level.dat").is_file());
        assert!(!dst.path().join("logs").exists()); // logs still skipped
    }
}
