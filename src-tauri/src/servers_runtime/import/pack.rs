//! Server-pack import (#10): CurseForge (`manifest.json`) and Modrinth
//! (`modrinth.index.json`) packs, which `find_server_root` would otherwise
//! reject (they carry no runnable-server markers yet).
//!
//! - **Modrinth** packs reference their mods by URL in `modrinth.index.json`, so
//!   nothing is on disk: we download the server-side files, apply `overrides/`
//!   then `server-overrides/`, and let the caller provision the loader.
//! - **CurseForge** packs that **bundle** their mods (a "Server Files" download)
//!   flow through the existing reprovision path (provision loader + copy data).
//!   A CF pack that is a *client* manifest (mods are `fileID` references, not on
//!   disk) needs the CurseForge mod API + key; that is out of scope here and the
//!   caller surfaces clear guidance instead.

use crate::error::{Error, Result};
use crate::instances::schema::LoaderKind;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Which pack format a staged import root is, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackKind {
    Modrinth,
    Curseforge,
}

/// Detect a server-pack manifest at `root`. Modrinth wins if both are present
/// (mirrors `mods::modpack::detect`).
pub fn detect_pack(root: &Path) -> Option<PackKind> {
    if root.join("modrinth.index.json").is_file() {
        Some(PackKind::Modrinth)
    } else if root.join("manifest.json").is_file() {
        Some(PackKind::Curseforge)
    } else {
        None
    }
}

/// A single file to materialize into the server runtime.
#[derive(Debug, Clone, PartialEq)]
pub struct PackFile {
    /// Install path relative to the runtime dir (e.g. `mods/sodium.jar`).
    pub path: String,
    pub sha1: String,
    pub url: String,
}

/// Parsed Modrinth server pack: loader/version + the server-side file plan.
#[derive(Debug, Clone, PartialEq)]
pub struct ModrinthServerPack {
    pub name: String,
    pub mc_version: String,
    pub loader: LoaderKind,
    pub loader_version: Option<String>,
    pub files: Vec<PackFile>,
}

/// Parsed CurseForge pack header (for loader/version detection + a bundled-mods
/// check). The mod `files` are not modelled — we only need to know whether they
/// are present on disk.
#[derive(Debug, Clone, PartialEq)]
pub struct CfServerPack {
    pub name: String,
    pub mc_version: String,
    pub loader: LoaderKind,
    pub loader_version: Option<String>,
    /// True when `mods/` already contains jars (a CF "Server Files" pack), so it
    /// can be installed from local data. False for a client manifest whose mods
    /// are `fileID` references that would require the CurseForge API.
    pub bundled_mods: bool,
}

const MODRINTH_CDN: &str = "cdn.modrinth.com";

// ── Modrinth ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct MrIndex {
    #[serde(rename = "formatVersion")]
    format_version: u32,
    game: String,
    name: String,
    files: Vec<MrFile>,
    dependencies: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct MrFile {
    path: String,
    hashes: MrHashes,
    env: Option<MrEnv>,
    downloads: Vec<String>,
}

#[derive(Deserialize)]
struct MrHashes {
    sha1: String,
}

#[derive(Deserialize)]
struct MrEnv {
    #[serde(default = "env_required")]
    server: String,
}

fn env_required() -> String {
    "required".into()
}

/// Parse `modrinth.index.json` into a server file plan. Files whose
/// `env.server == "unsupported"` are dropped (client-only mods); everything else
/// (required / optional) is included. Non-`cdn.modrinth.com` or path-unsafe
/// entries are skipped (the allowlist would reject them anyway).
pub fn parse_modrinth(root: &Path) -> Result<ModrinthServerPack> {
    let raw = std::fs::read_to_string(root.join("modrinth.index.json")).map_err(|e| {
        Error::ServerImportInvalidArchive {
            details: format!("read modrinth.index.json: {e}"),
        }
    })?;
    let index: MrIndex =
        serde_json::from_str(&raw).map_err(|e| Error::ServerImportInvalidArchive {
            details: format!("modrinth.index.json: {e}"),
        })?;
    if index.format_version != 1 || index.game != "minecraft" {
        return Err(Error::ServerImportInvalidArchive {
            details: format!(
                "unsupported modrinth pack (formatVersion={}, game={})",
                index.format_version, index.game
            ),
        });
    }
    let mc_version =
        index
            .dependencies
            .get("minecraft")
            .cloned()
            .ok_or(Error::ServerImportInvalidArchive {
                details: "modrinth pack missing minecraft dependency".into(),
            })?;
    let (loader, loader_version) = modrinth_loader(&index.dependencies);

    let mut files = Vec::new();
    for f in &index.files {
        let server = f
            .env
            .as_ref()
            .map(|e| e.server.as_str())
            .unwrap_or("required");
        if server == "unsupported" {
            continue; // client-only file: never goes on a server
        }
        if !crate::mods::modpack::path_safety::is_safe_relative_path(&f.path) {
            continue; // defensive: never let a pack escape the runtime dir
        }
        let Some(url) = f.downloads.first().cloned() else {
            continue;
        };
        let host = url::Url::parse(&url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_ascii_lowercase));
        if host.as_deref() != Some(MODRINTH_CDN) {
            continue; // only the allowlisted Modrinth CDN is reachable
        }
        files.push(PackFile {
            path: f.path.clone(),
            sha1: f.hashes.sha1.to_ascii_lowercase(),
            url,
        });
    }

    Ok(ModrinthServerPack {
        name: index.name,
        mc_version,
        loader,
        loader_version,
        files,
    })
}

fn modrinth_loader(deps: &BTreeMap<String, String>) -> (LoaderKind, Option<String>) {
    for (k, v) in deps {
        match k.as_str() {
            "fabric-loader" => return (LoaderKind::Fabric, Some(v.clone())),
            "quilt-loader" => return (LoaderKind::Quilt, Some(v.clone())),
            "forge" => return (LoaderKind::Forge, Some(v.clone())),
            "neoforge" => return (LoaderKind::NeoForge, Some(v.clone())),
            _ => {}
        }
    }
    (LoaderKind::Vanilla, None)
}

/// Download every server-side file into `dest_runtime`, then apply `overrides/`
/// and `server-overrides/` (server-overrides win). `staged_root` is the
/// extracted pack; `dest_runtime` is the new server's `runtime/`.
pub async fn materialize_modrinth(
    pack: &ModrinthServerPack,
    staged_root: &Path,
    dest_runtime: &Path,
) -> Result<()> {
    for f in &pack.files {
        let dest = dest_runtime.join(&f.path);
        crate::network::download::download_no_emit(
            &f.url,
            &dest,
            &f.sha1,
            "server_import_modrinth",
        )
        .await?;
    }
    apply_overrides(staged_root, dest_runtime)
}

/// Apply a pack's `overrides/` then `server-overrides/` into the runtime
/// (server-overrides win). Used by both the Modrinth and CurseForge paths.
/// Missing dirs are no-ops.
pub fn apply_overrides(staged_root: &Path, dest_runtime: &Path) -> Result<()> {
    copy_overrides(&staged_root.join("overrides"), dest_runtime)?;
    copy_overrides(&staged_root.join("server-overrides"), dest_runtime)?;
    Ok(())
}

// ── CurseForge ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CfManifest {
    minecraft: CfMc,
    name: String,
    #[serde(default)]
    files: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct CfMc {
    version: String,
    #[serde(rename = "modLoaders")]
    mod_loaders: Vec<CfLoader>,
}

#[derive(Deserialize)]
struct CfLoader {
    id: String,
    #[serde(default)]
    primary: bool,
}

/// Parse a CurseForge `manifest.json` for loader/version, and report whether the
/// pack already bundles its mods on disk.
pub fn parse_cf(root: &Path) -> Result<CfServerPack> {
    let raw = std::fs::read_to_string(root.join("manifest.json")).map_err(|e| {
        Error::ServerImportInvalidArchive {
            details: format!("read manifest.json: {e}"),
        }
    })?;
    let manifest: CfManifest =
        serde_json::from_str(&raw).map_err(|e| Error::ServerImportInvalidArchive {
            details: format!("manifest.json: {e}"),
        })?;
    let chosen = manifest
        .minecraft
        .mod_loaders
        .iter()
        .find(|l| l.primary)
        .or_else(|| manifest.minecraft.mod_loaders.first());
    let (loader, loader_version) = chosen
        .map(|l| cf_loader_id(&l.id))
        .unwrap_or((LoaderKind::Vanilla, None));
    Ok(CfServerPack {
        name: manifest.name,
        mc_version: manifest.minecraft.version,
        loader,
        loader_version,
        // A pack whose manifest lists mod files but ships none on disk is a
        // client manifest (fileID refs); one with jars already in mods/ is a
        // runnable-after-install server pack.
        bundled_mods: dir_has_jars(&root.join("mods")) || manifest.files.is_empty(),
    })
}

/// CurseForge loader id, e.g. `forge-47.2.0`, `neoforge-20.4.237`,
/// `fabric-0.15.7`, `quilt-0.20.0` → (LoaderKind, version).
pub fn cf_loader_id(id: &str) -> (LoaderKind, Option<String>) {
    let (name, ver) = id.split_once('-').unwrap_or((id, ""));
    let version = (!ver.is_empty()).then(|| ver.to_string());
    let kind = match name.to_ascii_lowercase().as_str() {
        "forge" => LoaderKind::Forge,
        "neoforge" => LoaderKind::NeoForge,
        "fabric" => LoaderKind::Fabric,
        "quilt" => LoaderKind::Quilt,
        _ => LoaderKind::Vanilla,
    };
    (kind, version)
}

fn dir_has_jars(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten().any(|e| {
                e.file_name()
                    .to_str()
                    .map(|n| n.to_ascii_lowercase().ends_with(".jar"))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

// ── shared ───────────────────────────────────────────────────────────────────

/// Recursive merge-copy of an overrides directory into the runtime (skips
/// symlinks; later callers overwrite earlier files — server-overrides win).
/// A missing `src` is a no-op.
fn copy_overrides(src: &Path, dst_runtime: &Path) -> Result<()> {
    if !src.is_dir() {
        return Ok(());
    }
    copy_tree(src, dst_runtime)
}

fn copy_tree(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst).map_err(|e| Error::io(dst.display().to_string(), e))?;
    for entry in std::fs::read_dir(src).map_err(|e| Error::io(src.display().to_string(), e))? {
        let entry = entry.map_err(|e| Error::io(src.display().to_string(), e))?;
        let ft = entry
            .file_type()
            .map_err(|e| Error::io(entry.path().display().to_string(), e))?;
        if ft.is_symlink() {
            continue;
        }
        let to: PathBuf = dst.join(entry.file_name());
        if ft.is_dir() {
            copy_tree(&entry.path(), &to)?;
        } else if ft.is_file() {
            std::fs::copy(entry.path(), &to).map_err(|e| Error::io(to.display().to_string(), e))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    const MR_INDEX: &str = r#"{
        "formatVersion": 1,
        "game": "minecraft",
        "name": "My Server Pack",
        "versionId": "1.0.0",
        "dependencies": { "minecraft": "1.20.1", "fabric-loader": "0.15.7" },
        "files": [
            {
                "path": "mods/serverside.jar",
                "hashes": { "sha1": "AABBCC" },
                "env": { "client": "optional", "server": "required" },
                "downloads": ["https://cdn.modrinth.com/data/AA/versions/BB/serverside.jar"]
            },
            {
                "path": "mods/clientonly.jar",
                "hashes": { "sha1": "DDEEFF" },
                "env": { "client": "required", "server": "unsupported" },
                "downloads": ["https://cdn.modrinth.com/data/CC/versions/DD/clientonly.jar"]
            },
            {
                "path": "mods/offsite.jar",
                "hashes": { "sha1": "112233" },
                "env": { "client": "required", "server": "required" },
                "downloads": ["https://example.com/offsite.jar"]
            }
        ]
    }"#;

    fn write(root: &Path, name: &str, body: &str) {
        if let Some(d) = Path::new(name).parent() {
            fs::create_dir_all(root.join(d)).unwrap();
        }
        fs::write(root.join(name), body).unwrap();
    }

    #[test]
    fn detect_modrinth_and_curseforge() {
        let d = tempdir().unwrap();
        write(d.path(), "modrinth.index.json", "{}");
        assert_eq!(detect_pack(d.path()), Some(PackKind::Modrinth));
        let c = tempdir().unwrap();
        write(c.path(), "manifest.json", "{}");
        assert_eq!(detect_pack(c.path()), Some(PackKind::Curseforge));
        let n = tempdir().unwrap();
        assert_eq!(detect_pack(n.path()), None);
    }

    #[test]
    fn modrinth_keeps_server_files_drops_client_and_offsite() {
        let d = tempdir().unwrap();
        write(d.path(), "modrinth.index.json", MR_INDEX);
        let pack = parse_modrinth(d.path()).unwrap();
        assert_eq!(pack.mc_version, "1.20.1");
        assert_eq!(pack.loader, LoaderKind::Fabric);
        assert_eq!(pack.loader_version.as_deref(), Some("0.15.7"));
        // Only the server-required, modrinth-hosted, path-safe file survives.
        assert_eq!(pack.files.len(), 1);
        assert_eq!(pack.files[0].path, "mods/serverside.jar");
        assert_eq!(pack.files[0].sha1, "aabbcc"); // lowercased
    }

    #[test]
    fn modrinth_rejects_unsupported_format() {
        let d = tempdir().unwrap();
        write(
            d.path(),
            "modrinth.index.json",
            &MR_INDEX.replace("\"formatVersion\": 1", "\"formatVersion\": 2"),
        );
        assert!(matches!(
            parse_modrinth(d.path()),
            Err(Error::ServerImportInvalidArchive { .. })
        ));
    }

    #[test]
    fn modrinth_path_escape_is_dropped() {
        let d = tempdir().unwrap();
        let escaped = MR_INDEX.replace("mods/serverside.jar", "../escape.jar");
        write(d.path(), "modrinth.index.json", &escaped);
        let pack = parse_modrinth(d.path()).unwrap();
        assert!(!pack.files.iter().any(|f| f.path.contains("..")));
    }

    #[test]
    fn cf_loader_id_parsing() {
        assert_eq!(
            cf_loader_id("forge-47.2.0"),
            (LoaderKind::Forge, Some("47.2.0".into()))
        );
        assert_eq!(
            cf_loader_id("neoforge-20.4.237"),
            (LoaderKind::NeoForge, Some("20.4.237".into()))
        );
        assert_eq!(
            cf_loader_id("fabric-0.15.7"),
            (LoaderKind::Fabric, Some("0.15.7".into()))
        );
    }

    #[test]
    fn cf_parse_detects_loader_and_bundled_mods() {
        let d = tempdir().unwrap();
        write(
            d.path(),
            "manifest.json",
            r#"{
                "minecraft": { "version": "1.20.1", "modLoaders": [{ "id": "forge-47.2.0", "primary": true }] },
                "name": "CF Pack",
                "files": [{ "projectID": 1, "fileID": 2, "required": true }]
            }"#,
        );
        // No mods/ jars on disk + non-empty files[] → client manifest.
        let info = parse_cf(d.path()).unwrap();
        assert_eq!(info.loader, LoaderKind::Forge);
        assert_eq!(info.loader_version.as_deref(), Some("47.2.0"));
        assert_eq!(info.mc_version, "1.20.1");
        assert!(!info.bundled_mods);

        // Add a bundled mod jar → now treated as a runnable server pack.
        write(d.path(), "mods/cool.jar", "jar");
        assert!(parse_cf(d.path()).unwrap().bundled_mods);
    }

    #[test]
    fn copy_overrides_merges_and_is_noop_when_absent() {
        let d = tempdir().unwrap();
        let runtime = d.path().join("runtime");
        fs::create_dir_all(&runtime).unwrap();
        // server-overrides absent → no-op.
        copy_overrides(&d.path().join("server-overrides"), &runtime).unwrap();
        // overrides present → merged in.
        write(d.path(), "overrides/config/a.toml", "k=1");
        copy_overrides(&d.path().join("overrides"), &runtime).unwrap();
        assert_eq!(fs::read(runtime.join("config/a.toml")).unwrap(), b"k=1");
    }
}
