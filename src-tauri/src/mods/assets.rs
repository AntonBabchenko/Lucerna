//! Tracks resource packs and shaders installed into an instance, in
//! `<instance_root>/installed-assets.json`. Parallel to `installed.rs`
//! (mods) but simpler: no enable/disable, no dependency closure.

use std::path::Path;

use tokio::fs;

use crate::error::Error;
use crate::mods::platform::{ContentKind, InstalledAsset, ModSource};

fn registry_path(instance_root: &Path) -> std::path::PathBuf {
    instance_root.join("installed-assets.json")
}

/// Guard the asset commands against non-asset `ContentKind`s.
///
/// The asset subsystem only manages resource packs and shaders. If `Mod`
/// reached these commands, `asset_dir(Mod)` resolves to `mods` — so an
/// uninstall would delete from `.minecraft/mods/` while the assets registry
/// (which never tracks mods) finds nothing, silently orphaning the mods
/// registry. `Plugin` is server-only content (a server's `runtime/plugins/`,
/// managed by the servers_runtime plugin fs module) — letting it through
/// would read/write `.minecraft/plugins/` in a CLIENT instance. We reject
/// both at the boundary.
///
/// Reuses [`Error::ModpackOverridesPathEscape`] (no new variant per the
/// bindings-freeze constraint): a rejected kind would route the asset
/// write/delete outside the resourcepacks/shaderpacks subtree the asset
/// commands are allowed to touch.
pub fn require_asset_kind(kind: ContentKind) -> Result<(), Error> {
    match kind {
        ContentKind::Mod => Err(Error::ModpackOverridesPathEscape {
            entry: "mods (asset commands accept resource packs and shaders only)".to_string(),
        }),
        ContentKind::Plugin => Err(Error::ModpackOverridesPathEscape {
            entry: "plugins (asset commands accept resource packs and shaders only)".to_string(),
        }),
        ContentKind::Datapack => Err(Error::ModpackOverridesPathEscape {
            entry: "datapacks (asset commands accept resource packs and shaders only)".to_string(),
        }),
        ContentKind::ResourcePack | ContentKind::Shader => Ok(()),
    }
}

fn io_err(path: &Path, e: std::io::Error) -> Error {
    Error::ModsInstancePath {
        path: path.display().to_string(),
        details: e.to_string(),
    }
}

pub async fn list_all(instance_root: &Path) -> Result<Vec<InstalledAsset>, Error> {
    let path = registry_path(instance_root);
    match fs::read(&path).await {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| Error::ModsDecode {
            platform: "installed-assets.json".into(),
            details: e.to_string(),
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(io_err(&path, e)),
    }
}

pub async fn list(instance_root: &Path, kind: ContentKind) -> Result<Vec<InstalledAsset>, Error> {
    Ok(list_all(instance_root)
        .await?
        .into_iter()
        .filter(|a| a.kind == kind)
        .collect())
}

async fn write_all(instance_root: &Path, items: &[InstalledAsset]) -> Result<(), Error> {
    let path = registry_path(instance_root);
    let json = serde_json::to_vec_pretty(items).map_err(|e| Error::ModsDecode {
        platform: "installed-assets.json".into(),
        details: e.to_string(),
    })?;
    // Atomic write (mirrors installed.rs): write to a temp file then rename
    // over the target, so a crash mid-write can't leave a truncated registry.
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, &json).await.map_err(|e| io_err(&tmp, e))?;
    fs::rename(&tmp, &path).await.map_err(|e| io_err(&path, e))
}

pub async fn add(instance_root: &Path, asset: InstalledAsset) -> Result<(), Error> {
    let mut items = list_all(instance_root).await?;
    items.retain(|a| !(a.kind == asset.kind && a.filename == asset.filename));
    items.push(asset);
    write_all(instance_root, &items).await
}

pub async fn remove(instance_root: &Path, kind: ContentKind, filename: &str) -> Result<(), Error> {
    let mut items = list_all(instance_root).await?;
    items.retain(|a| !(a.kind == kind && a.filename == filename));
    write_all(instance_root, &items).await
}

/// Build an `InstalledAsset` for a file discovered during modpack import or the
/// backfill. `installed_at` is supplied by the caller (tests pass a fixed
/// value; production passes `chrono::Utc::now().to_rfc3339()`). `sha1` is
/// lowercased here so registry lookups stay case-insensitive.
#[allow(clippy::too_many_arguments)]
pub fn make_asset(
    kind: ContentKind,
    filename: &str,
    sha1: &str,
    source: Option<ModSource>,
    project_id: Option<String>,
    version_id: Option<String>,
    name: &str,
    installed_at: String,
) -> InstalledAsset {
    InstalledAsset {
        kind,
        filename: filename.to_string(),
        sha1: sha1.to_ascii_lowercase(),
        source,
        project_id,
        version_id,
        name: name.to_string(),
        version_number: None,
        installed_at,
    }
}

/// Seed `installed-assets.json` from the instance's `pack_origin` resource-pack
/// / shader files — but ONLY when the registry file does not yet exist. This
/// retro-fits instances imported before assets were tracked: their packs are on
/// disk and recorded in `pack_origin`, just never registered. Guarded on
/// "file absent" so it never resurrects assets the user later uninstalled (that
/// leaves a present-but-shorter registry, which this skips). Best-effort and
/// idempotent: after the first run the file exists and this no-ops.
///
/// Note: `installed_at` is stamped at backfill time, not the original import
/// time — `PackOrigin` carries no per-import timestamp, so retro-fitted assets
/// read as "installed now". Acceptable: the field only drives display/sort for
/// these older instances, not correctness.
pub async fn backfill_from_pack_origin_if_missing(instance_root: &Path) -> Result<(), Error> {
    let path = registry_path(instance_root);
    match fs::metadata(&path).await {
        Ok(_) => return Ok(()), // registry already exists — leave it untouched
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(io_err(&path, e)),
    }
    let Some(origin) = crate::mods::installed::get_pack_origin(instance_root).await? else {
        return Ok(()); // not a pack-imported instance
    };
    let mut items: Vec<InstalledAsset> = Vec::new();
    for f in &origin.files {
        let Some(kind) = crate::mods::install::content_kind_for_install_path(&f.install_path)
        else {
            continue;
        };
        items.push(make_asset(
            kind,
            &f.filename,
            &f.sha1,
            Some(f.source),
            (!f.project_id.is_empty()).then(|| f.project_id.clone()),
            (!f.version_id.is_empty()).then(|| f.version_id.clone()),
            &f.name,
            chrono::Utc::now().to_rfc3339(),
        ));
    }
    if items.is_empty() {
        return Ok(()); // nothing to seed — avoid writing an empty registry
    }
    write_all(instance_root, &items).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(kind: ContentKind, filename: &str) -> InstalledAsset {
        InstalledAsset {
            kind,
            filename: filename.into(),
            sha1: "aa".into(),
            source: Some(ModSource::Modrinth),
            project_id: Some("p".into()),
            version_id: Some("v".into()),
            name: filename.into(),
            version_number: Some("1.0".into()),
            installed_at: "2026-06-04T00:00:00+00:00".into(),
        }
    }

    #[tokio::test]
    async fn add_list_remove_round_trip() {
        let td = tempfile::tempdir().unwrap();
        let root = td.path();
        add(root, sample(ContentKind::Shader, "BSL.zip"))
            .await
            .unwrap();
        add(root, sample(ContentKind::ResourcePack, "Faithful.zip"))
            .await
            .unwrap();
        let shaders = list(root, ContentKind::Shader).await.unwrap();
        assert_eq!(shaders.len(), 1);
        assert_eq!(shaders[0].filename, "BSL.zip");
        assert_eq!(list_all(root).await.unwrap().len(), 2);
        remove(root, ContentKind::Shader, "BSL.zip").await.unwrap();
        assert!(list(root, ContentKind::Shader).await.unwrap().is_empty());
        assert_eq!(
            list(root, ContentKind::ResourcePack).await.unwrap().len(),
            1
        );
    }

    #[tokio::test]
    async fn add_replaces_same_kind_and_filename() {
        let td = tempfile::tempdir().unwrap();
        let root = td.path();
        add(root, sample(ContentKind::Shader, "BSL.zip"))
            .await
            .unwrap();
        let mut updated = sample(ContentKind::Shader, "BSL.zip");
        updated.version_number = Some("2.0".into());
        add(root, updated).await.unwrap();
        let shaders = list(root, ContentKind::Shader).await.unwrap();
        assert_eq!(shaders.len(), 1);
        assert_eq!(shaders[0].version_number.as_deref(), Some("2.0"));
    }

    #[test]
    fn require_asset_kind_rejects_mod_accepts_assets() {
        assert!(matches!(
            require_asset_kind(ContentKind::Mod),
            Err(Error::ModpackOverridesPathEscape { .. })
        ));
        assert!(require_asset_kind(ContentKind::ResourcePack).is_ok());
        assert!(require_asset_kind(ContentKind::Shader).is_ok());
    }

    #[test]
    fn require_asset_kind_rejects_plugin() {
        // Plugins are server-only content (a server's runtime/plugins/, via
        // the servers_runtime plugin fs module) — a kind:"plugin" IPC payload
        // must never route an asset write/delete into a CLIENT instance's
        // .minecraft/plugins/.
        assert!(matches!(
            require_asset_kind(ContentKind::Plugin),
            Err(Error::ModpackOverridesPathEscape { .. })
        ));
    }

    #[test]
    fn require_asset_kind_rejects_datapack() {
        // This guard is the ONLY thing keeping Datapack away from `asset_dir`.
        // Both entry points into it — `asset_subpath` and
        // `safe_asset_remove_path` — are `pub` over an unconstrained
        // ContentKind, and the three l10n callers bypass this guard entirely
        // (they pass a hardcoded ResourcePack). So the `asset_dir` arm for
        // Datapack is unreachable by CONVENTION, not by construction, and this
        // test is the convention's only enforcement. A datapack's library lives
        // at the instance root and its real home is `saves/<world>/datapacks/`;
        // nothing about it belongs under `.minecraft/`.
        assert!(matches!(
            require_asset_kind(ContentKind::Datapack),
            Err(Error::ModpackOverridesPathEscape { .. })
        ));
    }

    #[tokio::test]
    async fn list_on_missing_file_is_empty() {
        let td = tempfile::tempdir().unwrap();
        assert!(list(td.path(), ContentKind::Shader)
            .await
            .unwrap()
            .is_empty());
    }

    #[test]
    fn make_asset_lowercases_sha_and_clears_version() {
        let a = make_asset(
            ContentKind::ResourcePack,
            "RP.zip",
            "AABBCC",
            Some(ModSource::Modrinth),
            Some("pid".into()),
            Some("vid".into()),
            "RP",
            "2026-06-12T00:00:00+00:00".into(),
        );
        assert_eq!(a.kind, ContentKind::ResourcePack);
        assert_eq!(a.sha1, "aabbcc");
        assert_eq!(a.version_number, None);
        assert_eq!(a.project_id.as_deref(), Some("pid"));
    }

    #[tokio::test]
    async fn backfill_seeds_assets_from_pack_origin_when_registry_absent() {
        use crate::mods::installed::{set_pack_origin, PackOrigin, PackOriginFile};
        use crate::mods::modpack::schema::EnvSupport;
        let td = tempfile::tempdir().unwrap();
        let root = td.path();

        let rp = PackOriginFile {
            sha1: "AABB".into(),
            name: "Faithful.zip".into(),
            filename: "Faithful.zip".into(),
            install_path: "resourcepacks/Faithful.zip".into(),
            url: "https://cdn.modrinth.com/Faithful.zip".into(),
            size: 2048.0,
            project_id: "pid".into(),
            version_id: "vid".into(),
            env_client: EnvSupport::Required,
            source: ModSource::Modrinth,
        };
        let a_jar = PackOriginFile {
            install_path: "mods/sodium.jar".into(),
            filename: "sodium.jar".into(),
            name: "Sodium".into(),
            ..rp.clone()
        };
        set_pack_origin(
            root,
            PackOrigin {
                project_id: None,
                source: ModSource::Modrinth,
                project_name: "Pack".into(),
                version: "1.0".into(),
                files: vec![rp, a_jar],
                missing_mods: vec![],
                skipped_overrides: vec![],
                resolved_missing: vec![],
                inert_loader_jars: vec![],
            },
        )
        .await
        .unwrap();

        // Registry absent → backfill seeds only the resource pack.
        backfill_from_pack_origin_if_missing(root).await.unwrap();
        let rps = list(root, ContentKind::ResourcePack).await.unwrap();
        assert_eq!(rps.len(), 1);
        assert_eq!(rps[0].filename, "Faithful.zip");

        // Second run is a no-op (file now exists) and does not duplicate.
        backfill_from_pack_origin_if_missing(root).await.unwrap();
        assert_eq!(
            list(root, ContentKind::ResourcePack).await.unwrap().len(),
            1
        );
    }
}
