//! MC 1.6.x — 1.12.2 installer era. Installer JSON has `install` +
//! `versionInfo` at the top level; `versionInfo` is already a
//! Mojang-format VersionDetails with `inheritsFrom: <mc>`.
//! No processors, no patches, no mappings.

use crate::error::{Error, Result};
use crate::versions::version_json::{parse, VersionDetails};
use std::io::Read;

/// Extract the `versionInfo` block from the parsed install_profile.json
/// and parse it as `VersionDetails`. Returns `ForgeInstallerCorrupted`
/// if the block is missing or malformed.
pub fn extract_version_info(
    install_profile: &serde_json::Value,
    mc: &str,
    fv: &str,
) -> Result<VersionDetails> {
    let version_info =
        install_profile
            .get("versionInfo")
            .ok_or_else(|| Error::ForgeInstallerCorrupted {
                mc: mc.to_string(),
                fv: fv.to_string(),
                details: "missing top-level versionInfo".to_string(),
            })?;
    let text = serde_json::to_string(version_info).map_err(|e| Error::ForgeInstallerCorrupted {
        mc: mc.to_string(),
        fv: fv.to_string(),
        details: format!("serialise versionInfo: {e}"),
    })?;
    parse(&text).map_err(|e| Error::ForgeInstallerCorrupted {
        mc: mc.to_string(),
        fv: fv.to_string(),
        details: format!("parse versionInfo: {e}"),
    })
}

/// Locate the universal jar entry inside the installer ZIP. Path is
/// taken from `install.filePath` in the install_profile, and looks
/// like `maven/net/minecraftforge/forge/<mc>-<fv>/forge-<mc>-<fv>-universal.jar`.
pub fn extract_universal_jar_bytes(
    install_profile: &serde_json::Value,
    installer_bytes: &[u8],
    mc: &str,
    fv: &str,
) -> Result<(String, Vec<u8>)> {
    let file_path = install_profile
        .get("install")
        .and_then(|i| i.get("filePath"))
        .and_then(|p| p.as_str())
        .ok_or_else(|| Error::ForgeInstallerCorrupted {
            mc: mc.to_string(),
            fv: fv.to_string(),
            details: "missing install.filePath".to_string(),
        })?;
    let cursor = std::io::Cursor::new(installer_bytes);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| Error::ForgeInstallerCorrupted {
        mc: mc.to_string(),
        fv: fv.to_string(),
        details: format!("zip open: {e}"),
    })?;
    let mut entry = archive
        .by_name(file_path)
        .map_err(|_| Error::ForgeInstallerCorrupted {
            mc: mc.to_string(),
            fv: fv.to_string(),
            details: format!("universal jar entry {file_path} not found"),
        })?;
    let mut buf = Vec::with_capacity(entry.size() as usize);
    entry
        .read_to_end(&mut buf)
        .map_err(|e| Error::ForgeInstallerCorrupted {
            mc: mc.to_string(),
            fv: fv.to_string(),
            details: format!("universal jar read: {e}"),
        })?;
    Ok((file_path.to_string(), buf))
}

/// Re-exported from `forge::patcher` — two eras now need this helper.
pub use crate::forge::patcher::maven_coord_to_relative_path;

pub async fn install(
    install_profile: &serde_json::Value,
    installer_bytes: &[u8],
    mc: &str,
    fv: &str,
    app: &tauri::AppHandle,
) -> Result<VersionDetails> {
    // 1. Pull versionInfo as the base VersionDetails (already Mojang-format).
    let details = extract_version_info(install_profile, mc, fv)?;

    // 2. Pull universal jar bytes from the installer ZIP. The `file_path`
    //    here is the entry name *inside* the installer (e.g. a bare
    //    `forge-<raw>-universal.jar`), NOT a maven layout — that's what
    //    `install.path` is for.
    let (_entry_path, jar_bytes) =
        extract_universal_jar_bytes(install_profile, installer_bytes, mc, fv)?;

    // 3. Resolve target placement from `install.path` (a maven
    //    coordinate). Legacy Forge installer-internal name carries a
    //    `-universal` classifier but the coordinate the version manifest
    //    references is plain `g:a:v` (no classifier). When Forge's
    //    GUI installer runs it renames the file on its way out — we do
    //    the same here so the vanilla library resolver finds it.
    let install_path = install_profile
        .get("install")
        .and_then(|i| i.get("path"))
        .and_then(|p| p.as_str())
        .ok_or_else(|| Error::ForgeInstallerCorrupted {
            mc: mc.to_string(),
            fv: fv.to_string(),
            details: "missing install.path".to_string(),
        })?;
    let rel_path = maven_coord_to_relative_path(install_path).ok_or_else(|| {
        Error::ForgeInstallerCorrupted {
            mc: mc.to_string(),
            fv: fv.to_string(),
            details: format!("install.path is not a maven coordinate: {install_path}"),
        }
    })?;
    let libs_root =
        crate::paths::libraries_dir(app).map_err(|e| Error::io("<libraries_dir>", e))?;
    let dest = libs_root.join(rel_path);
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| Error::io(parent.display().to_string(), e))?;
    }
    tokio::fs::write(&dest, &jar_bytes)
        .await
        .map_err(|e| Error::io(dest.display().to_string(), e))?;

    Ok(details)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mirror of the real 1.7.10 installer's install_profile.json
    // (quirk MC range — version segment carries the duplicate suffix,
    // filePath is a bare entry name, install.path is the coord WITHOUT
    // the `-universal` classifier even though the file inside the JAR
    // does carry it).
    const FIXTURE_INSTALL_PROFILE: &str = r#"{
      "install": {
        "profileName": "Forge",
        "target": "1.7.10-Forge10.13.4.1614-1.7.10",
        "path": "net.minecraftforge:forge:1.7.10-10.13.4.1614-1.7.10",
        "version": "Forge 10.13.4.1614-1.7.10",
        "filePath": "forge-1.7.10-10.13.4.1614-1.7.10-universal.jar",
        "minecraft": "1.7.10"
      },
      "versionInfo": {
        "id": "1.7.10-Forge10.13.4.1614-1.7.10",
        "inheritsFrom": "1.7.10",
        "mainClass": "net.minecraft.launchwrapper.Launch",
        "libraries": [
          {"name": "net.minecraftforge:forge:1.7.10-10.13.4.1614-1.7.10", "url": "https://maven.minecraftforge.net/"}
        ],
        "minecraftArguments": "--username ${auth_player_name} --version ${version_name} --tweakClass net.minecraftforge.fml.common.launcher.FMLTweaker"
      }
    }"#;

    #[test]
    fn extract_version_info_returns_parsed_version_details() {
        let profile: serde_json::Value = serde_json::from_str(FIXTURE_INSTALL_PROFILE).unwrap();
        let details = extract_version_info(&profile, "1.7.10", "10.13.4.1614").expect("extract");
        assert_eq!(details.id, "1.7.10-Forge10.13.4.1614-1.7.10");
        assert_eq!(details.inherits_from.as_deref(), Some("1.7.10"));
        assert_eq!(details.main_class, "net.minecraft.launchwrapper.Launch");
        let m = details.minecraft_arguments.expect("minecraftArguments");
        assert!(m.contains("FMLTweaker"));
    }

    #[test]
    fn extract_version_info_missing_returns_corrupted_error() {
        let profile = serde_json::json!({"install": {"profileName": "Forge"}});
        let err = extract_version_info(&profile, "1.12.2", "14.23.5.2860").unwrap_err();
        match err {
            Error::ForgeInstallerCorrupted { details, .. } => {
                assert!(details.contains("versionInfo"), "got: {details}");
            }
            other => panic!("expected ForgeInstallerCorrupted, got {other:?}"),
        }
    }

    #[test]
    fn extract_universal_jar_missing_file_path_returns_corrupted_error() {
        let profile = serde_json::json!({"install": {}});
        let err = extract_universal_jar_bytes(&profile, &[], "1.12.2", "14.23.5.2860").unwrap_err();
        match err {
            Error::ForgeInstallerCorrupted { details, .. } => {
                assert!(details.contains("filePath"), "got: {details}");
            }
            other => panic!("expected ForgeInstallerCorrupted, got {other:?}"),
        }
    }

    #[test]
    fn extract_universal_jar_invalid_zip_returns_corrupted_error() {
        let profile: serde_json::Value = serde_json::from_str(FIXTURE_INSTALL_PROFILE).unwrap();
        let err = extract_universal_jar_bytes(&profile, b"not a zip", "1.12.2", "14.23.5.2860")
            .unwrap_err();
        match err {
            Error::ForgeInstallerCorrupted { details, .. } => {
                assert!(details.contains("zip open"), "got: {details}");
            }
            other => panic!("expected ForgeInstallerCorrupted, got {other:?}"),
        }
    }
}
