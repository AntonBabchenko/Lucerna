//! Installer dispatch. `install` is the public entrypoint; it detects
//! the era from `install_profile.json` shape and dispatches to one of
//! the era handlers.

pub mod legacy;
// pub mod transitional;  // Phase 2
// pub mod modern;        // Phase 3

use crate::error::{Error, Result};
use crate::forge::ForgeFlavor;
use crate::versions::version_json::VersionDetails;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Era {
    Legacy,
    Transitional,
    Modern,
}

/// Detect installer era from the parsed install_profile.json.
///
/// - Legacy (1.6-1.12): has `install` + `versionInfo` top-level keys.
/// - Modern (1.17+): `spec` >= 1 AND non-empty `processors` array.
/// - Transitional (1.13-1.16): `spec` is present but processors are
///   empty OR `spec` == 0.
pub fn detect_era(profile_json: &serde_json::Value) -> Era {
    if profile_json.get("install").is_some() && profile_json.get("versionInfo").is_some() {
        return Era::Legacy;
    }
    let spec = profile_json.get("spec").and_then(|v| v.as_i64()).unwrap_or(0);
    let has_processors = profile_json
        .get("processors")
        .and_then(|v| v.as_array())
        .map(|a| !a.is_empty())
        .unwrap_or(false);
    if spec >= 1 && has_processors {
        Era::Modern
    } else {
        Era::Transitional
    }
}

/// Top-level installer entrypoint. Phase 1 implements `Era::Legacy`;
/// Transitional/Modern return `ForgeUnsupportedProcessor` in Phase 1
/// (Phase 2/3 wire them).
pub async fn install(
    flavor: ForgeFlavor,
    mc: &str,
    fv: &str,
    app: &tauri::AppHandle,
) -> Result<VersionDetails> {
    let bytes = crate::forge::meta::fetch_installer_bytes(flavor, mc, fv, app).await?;
    let install_profile = read_install_profile(&bytes)
        .map_err(|details| Error::ForgeInstallerCorrupted {
            mc: mc.to_string(),
            fv: fv.to_string(),
            details,
        })?;
    let era = detect_era(&install_profile);
    match era {
        Era::Legacy => legacy::install(&install_profile, &bytes, mc, fv, app).await,
        Era::Transitional => Err(Error::ForgeUnsupportedProcessor {
            coord: format!("<transitional-era for {mc}-{fv}, Phase 2 — not yet shipped>"),
        }),
        Era::Modern => Err(Error::ForgeUnsupportedProcessor {
            coord: format!("<modern-era for {mc}-{fv}, Phase 3 — not yet shipped>"),
        }),
    }
}

/// Read `install_profile.json` from the installer ZIP and parse it as
/// raw JSON. Returns `Err(details_string)` on any failure for the
/// caller to wrap into `ForgeInstallerCorrupted`.
fn read_install_profile(installer_bytes: &[u8]) -> std::result::Result<serde_json::Value, String> {
    use std::io::Read;
    let cursor = std::io::Cursor::new(installer_bytes);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| format!("zip open: {e}"))?;
    let mut entry = archive
        .by_name("install_profile.json")
        .map_err(|_| "install_profile.json not found in installer JAR".to_string())?;
    let mut buf = String::new();
    entry
        .read_to_string(&mut buf)
        .map_err(|e| format!("install_profile.json read: {e}"))?;
    serde_json::from_str(&buf).map_err(|e| format!("install_profile.json parse: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_legacy_from_install_versioninfo() {
        let v = serde_json::json!({
            "install": {"profileName": "Forge"},
            "versionInfo": {"id": "1.12.2-forge-14.23.5.2860"}
        });
        assert_eq!(detect_era(&v), Era::Legacy);
    }

    #[test]
    fn detect_modern_from_spec_and_processors() {
        let v = serde_json::json!({
            "spec": 1,
            "processors": [{"jar": "net.minecraftforge:binarypatcher:1.1.1"}]
        });
        assert_eq!(detect_era(&v), Era::Modern);
    }

    #[test]
    fn detect_transitional_from_spec_zero() {
        let v = serde_json::json!({
            "spec": 0,
            "processors": [{"jar": "net.md-5:SpecialSource:1.10.0"}]
        });
        assert_eq!(detect_era(&v), Era::Transitional);
    }

    #[test]
    fn detect_transitional_from_empty_processors() {
        let v = serde_json::json!({
            "spec": 1,
            "processors": []
        });
        assert_eq!(detect_era(&v), Era::Transitional);
    }

    #[test]
    fn detect_transitional_from_missing_processors() {
        let v = serde_json::json!({
            "spec": 1
        });
        assert_eq!(detect_era(&v), Era::Transitional);
    }
}
