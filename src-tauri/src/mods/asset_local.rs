//! Local resource-pack / shader install: validate a `.zip`, copy it into
//! `{instance}/.minecraft/{resourcepacks|shaderpacks}/`, and record it in
//! `installed-assets.json` as a manual (`source: None`) asset. The asset-side
//! parallel of `mods/local.rs::install_local`.
//!
//! Validation is deliberately shallow (validity-only): resource packs and
//! shaders carry no reliable Minecraft-version metadata, so we do not imitate a
//! compatibility check we cannot stand behind — we only reject a file that is
//! not a usable pack at all.

use std::io::Cursor;
use std::path::Path;

use chrono::Utc;
use sha1::{Digest, Sha1};
use tokio::fs;

use crate::error::Error;
use crate::mods::platform::{ContentKind, InstalledAsset};

/// Validate that `bytes` is a readable zip suitable for `kind`. A resource
/// pack must additionally contain a `pack.mcmeta` entry (Minecraft requires it
/// at the archive root). A shader only needs to be a readable zip. `kind ==
/// Mod` and `kind == Plugin` never reach here (the command guards with
/// `require_asset_kind`, which rejects both — plugins are server-only
/// content); neither imposes an extra check here.
pub fn validate_asset_zip(bytes: &[u8], kind: ContentKind) -> Result<(), Error> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| Error::ModsDecode {
        platform: "local asset zip".into(),
        details: e.to_string(),
    })?;
    if kind == ContentKind::ResourcePack && zip.by_name("pack.mcmeta").is_err() {
        return Err(Error::ModsDecode {
            platform: "resource pack".into(),
            details: "missing pack.mcmeta".into(),
        });
    }
    Ok(())
}

/// Install a local resource-pack / shader `.zip` into the instance and record
/// it as a manual asset (`source: None`). Mirrors `mods/local.rs::install_local`
/// for the safe-filename guard and `mods/install.rs::install_asset` for the
/// `.minecraft/` containment guards.
///
/// Overwrite semantics: a same-`(kind, filename)` asset is replaced — the file
/// is rewritten and `assets::add` replaces the registry row. `name` is the
/// filename with a trailing `.zip` removed (a zip carries no reliable display
/// name; mirrors `install_local`'s filename fallback).
pub async fn install_asset_local(
    instance_root: &Path,
    kind: ContentKind,
    filename: &str,
    bytes: &[u8],
) -> Result<InstalledAsset, Error> {
    // Safe single-segment filename FIRST, before any I/O (mirrors install_local).
    if !crate::mods::modpack::path_safety::is_safe_filename(filename) {
        return Err(Error::ModsUnsafeFilename {
            filename: filename.to_string(),
        });
    }
    // Validate the zip (resource packs require pack.mcmeta) before writing.
    validate_asset_zip(bytes, kind)?;

    let sha = hex::encode(Sha1::digest(bytes));

    // Resolve the destination under `.minecraft/<asset_dir>/` and verify it
    // stays inside `.minecraft/` (string guard + canonical-parent guard,
    // mirroring install::install_asset's defense-in-depth).
    let rel = crate::mods::install::asset_subpath(kind, filename);
    // `rel` is `<asset_dir>/<filename>`. Given the `is_safe_filename` guard
    // above, this can't fail today — kept as defense-in-depth mirroring
    // install::install_asset, which validates an arbitrary modpack-supplied path.
    if !crate::mods::modpack::path_safety::is_safe_relative_path(&rel) {
        return Err(Error::ModpackOverridesPathEscape { entry: rel });
    }
    let mc_dir = instance_root.join(".minecraft");
    let dest = mc_dir.join(&rel);
    let parent = dest.parent().ok_or_else(|| Error::ModsInstancePath {
        path: dest.display().to_string(),
        details: "asset path has no parent directory".into(),
    })?;
    fs::create_dir_all(parent)
        .await
        .map_err(|e| Error::ModsInstancePath {
            path: parent.display().to_string(),
            details: e.to_string(),
        })?;
    let mc_canon = dunce::canonicalize(&mc_dir).map_err(|e| Error::ModsInstancePath {
        path: mc_dir.display().to_string(),
        details: e.to_string(),
    })?;
    let parent_canon = dunce::canonicalize(parent).map_err(|e| Error::ModsInstancePath {
        path: parent.display().to_string(),
        details: e.to_string(),
    })?;
    if !parent_canon.starts_with(&mc_canon) {
        return Err(Error::ModpackOverridesPathEscape { entry: rel });
    }
    // A hand-installed pack exists nowhere else, so it is never linked — but it
    // still must not be written in place: the destination may already be a
    // hardlink, and truncating one would corrupt every instance sharing it.
    crate::mods::store::place_bytes(&dest, bytes)
        .await
        .map_err(|e| Error::ModsInstancePath {
            path: e.path.display().to_string(),
            details: e.details(),
        })?;

    let name = filename
        .strip_suffix(".zip")
        .unwrap_or(filename)
        .to_string();
    let asset = InstalledAsset {
        kind,
        filename: filename.to_string(),
        sha1: sha,
        source: None,
        project_id: None,
        version_id: None,
        name,
        version_number: None,
        installed_at: Utc::now().to_rfc3339(),
    };
    crate::mods::assets::add(instance_root, asset.clone()).await?;
    Ok(asset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;
    use zip::write::SimpleFileOptions;

    /// Build an in-memory `.zip` from (name, bytes) entries.
    fn zip_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(Cursor::new(&mut buf));
            for (name, body) in entries {
                w.start_file(*name, SimpleFileOptions::default()).unwrap();
                w.write_all(body).unwrap();
            }
            w.finish().unwrap();
        }
        buf
    }

    #[test]
    fn resource_pack_with_mcmeta_is_valid() {
        let z = zip_with(&[("pack.mcmeta", br#"{"pack":{"pack_format":15}}"#)]);
        assert!(validate_asset_zip(&z, ContentKind::ResourcePack).is_ok());
    }

    #[test]
    fn resource_pack_without_mcmeta_is_rejected() {
        let z = zip_with(&[("assets/minecraft/x.png", b"x")]);
        let err = validate_asset_zip(&z, ContentKind::ResourcePack).unwrap_err();
        assert!(matches!(err, Error::ModsDecode { .. }));
    }

    #[test]
    fn shader_only_needs_a_readable_zip() {
        let z = zip_with(&[("shaders/final.fsh", b"void main(){}")]);
        assert!(validate_asset_zip(&z, ContentKind::Shader).is_ok());
    }

    #[test]
    fn non_zip_bytes_are_rejected() {
        let err = validate_asset_zip(b"not a zip at all", ContentKind::Shader).unwrap_err();
        assert!(matches!(err, Error::ModsDecode { .. }));
    }

    fn rp_zip() -> Vec<u8> {
        zip_with(&[("pack.mcmeta", br#"{"pack":{"pack_format":15}}"#)])
    }

    #[tokio::test]
    async fn installs_resource_pack_under_resourcepacks_as_manual() {
        let td = TempDir::new().unwrap();
        let asset = install_asset_local(
            td.path(),
            ContentKind::ResourcePack,
            "Faithful.zip",
            &rp_zip(),
        )
        .await
        .unwrap();
        assert_eq!(asset.source, None); // manual
        assert_eq!(asset.name, "Faithful"); // .zip stripped
        assert!(td
            .path()
            .join(".minecraft/resourcepacks/Faithful.zip")
            .exists());
        let listed = crate::mods::assets::list(td.path(), ContentKind::ResourcePack)
            .await
            .unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].filename, "Faithful.zip");
    }

    #[tokio::test]
    async fn installs_shader_under_shaderpacks() {
        let td = TempDir::new().unwrap();
        let z = zip_with(&[("shaders/final.fsh", b"void main(){}")]);
        install_asset_local(td.path(), ContentKind::Shader, "BSL.zip", &z)
            .await
            .unwrap();
        assert!(td.path().join(".minecraft/shaderpacks/BSL.zip").exists());
    }

    #[tokio::test]
    async fn resource_pack_without_mcmeta_writes_nothing() {
        let td = TempDir::new().unwrap();
        let z = zip_with(&[("assets/x.png", b"x")]);
        let err = install_asset_local(td.path(), ContentKind::ResourcePack, "Bad.zip", &z)
            .await
            .unwrap_err();
        assert!(matches!(err, Error::ModsDecode { .. }));
        assert!(!td.path().join(".minecraft/resourcepacks/Bad.zip").exists());
        assert!(
            crate::mods::assets::list(td.path(), ContentKind::ResourcePack)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn unsafe_filename_rejected_before_any_io() {
        let td = TempDir::new().unwrap();
        let err = install_asset_local(td.path(), ContentKind::Shader, "../../evil.zip", &rp_zip())
            .await
            .unwrap_err();
        assert!(matches!(err, Error::ModsUnsafeFilename { .. }));
        assert!(!td.path().join(".minecraft").exists());
    }

    #[tokio::test]
    async fn same_name_overwrites_single_registry_row() {
        let td = TempDir::new().unwrap();
        install_asset_local(td.path(), ContentKind::ResourcePack, "P.zip", &rp_zip())
            .await
            .unwrap();
        let z2 = zip_with(&[
            ("pack.mcmeta", br#"{"pack":{"pack_format":18}}"#),
            ("extra.txt", b"v2"),
        ]);
        install_asset_local(td.path(), ContentKind::ResourcePack, "P.zip", &z2)
            .await
            .unwrap();
        let listed = crate::mods::assets::list(td.path(), ContentKind::ResourcePack)
            .await
            .unwrap();
        assert_eq!(listed.len(), 1, "same (kind, filename) must not duplicate");
        let on_disk = std::fs::read(td.path().join(".minecraft/resourcepacks/P.zip")).unwrap();
        assert_eq!(on_disk, z2);
    }
}
