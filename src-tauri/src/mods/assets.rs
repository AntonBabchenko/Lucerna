//! Tracks resource packs and shaders installed into an instance, in
//! `<instance_root>/installed-assets.json`. Parallel to `installed.rs`
//! (mods) but simpler: no enable/disable, no dependency closure.

use std::path::Path;

use tokio::fs;

use crate::error::Error;
use crate::mods::platform::{ContentKind, InstalledAsset};

fn registry_path(instance_root: &Path) -> std::path::PathBuf {
    instance_root.join("installed-assets.json")
}

/// Guard the asset commands against `ContentKind::Mod`.
///
/// The asset subsystem only manages resource packs and shaders. If `Mod`
/// reached these commands, `asset_dir(Mod)` resolves to `mods` — so an
/// uninstall would delete from `.minecraft/mods/` while the assets registry
/// (which never tracks mods) finds nothing, silently orphaning the mods
/// registry. We reject it at the boundary.
///
/// Reuses [`Error::ModpackOverridesPathEscape`] (no new variant per the
/// bindings-freeze constraint): a `Mod` kind would route the asset write/delete
/// into `.minecraft/mods/`, i.e. outside the resourcepacks/shaderpacks subtree
/// the asset commands are allowed to touch.
pub fn require_asset_kind(kind: ContentKind) -> Result<(), Error> {
    if kind == ContentKind::Mod {
        return Err(Error::ModpackOverridesPathEscape {
            entry: "mods (asset commands accept resource packs and shaders only)".to_string(),
        });
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::platform::ModSource;

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

    #[tokio::test]
    async fn list_on_missing_file_is_empty() {
        let td = tempfile::tempdir().unwrap();
        assert!(list(td.path(), ContentKind::Shader)
            .await
            .unwrap()
            .is_empty());
    }
}
