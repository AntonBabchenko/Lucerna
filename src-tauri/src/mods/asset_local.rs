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

use crate::error::Error;
use crate::mods::platform::ContentKind;

/// Validate that `bytes` is a readable zip suitable for `kind`. A resource
/// pack must additionally contain a `pack.mcmeta` entry (Minecraft requires it
/// at the archive root). A shader only needs to be a readable zip. `kind ==
/// Mod` never reaches here (the command guards with `require_asset_kind`); it
/// imposes no extra check.
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
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
}
