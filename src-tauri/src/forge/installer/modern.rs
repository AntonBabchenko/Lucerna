//! MC 1.17.x+ installer era (modern): install_profile spec=1 + an embedded
//! /version.json + processors. The processors (installertools MCP_DATA /
//! DOWNLOAD_MOJMAPS / MERGE_MAPPING, jarsplitter, FART, binarypatcher, and the
//! server-only ones the sides filter skips) run in the shared stage
//! `super::processors`, as Forge's own installer runs them. This file parses
//! the profile, reads the embedded version.json and hands over.

use crate::error::{Error, Result};
use crate::versions::version_json::VersionDetails;

pub async fn install(
    install_profile_value: &serde_json::Value,
    installer_bytes: &[u8],
    mc: &str,
    fv: &str,
    app: &tauri::AppHandle,
) -> Result<VersionDetails> {
    use super::processors::{prepare, run_client_processors, stage_lock, AppStageIo};
    use super::transitional::parse_install_profile;

    // 1. Parse install_profile.json (re-use Phase 2 parser).
    let raw_text = serde_json::to_string(install_profile_value).map_err(|e| {
        Error::ForgeInstallerCorrupted {
            mc: mc.into(),
            fv: fv.into(),
            details: format!("re-serialise install_profile: {e}"),
        }
    })?;
    let profile = parse_install_profile(&raw_text).map_err(|e| match e {
        Error::ForgeInstallerCorrupted { details, .. } => Error::ForgeInstallerCorrupted {
            mc: mc.into(),
            fv: fv.into(),
            details,
        },
        other => other,
    })?;

    // 2. Extract embedded /version.json from the installer.
    let version_details = {
        use std::io::Read;
        let mut archive =
            zip::ZipArchive::new(std::io::Cursor::new(installer_bytes)).map_err(|e| {
                Error::ForgeInstallerCorrupted {
                    mc: mc.into(),
                    fv: fv.into(),
                    details: format!("zip: {e}"),
                }
            })?;
        let mut entry =
            archive
                .by_name("version.json")
                .map_err(|_| Error::ForgeInstallerCorrupted {
                    mc: mc.into(),
                    fv: fv.into(),
                    details: "missing /version.json".into(),
                })?;
        let mut buf = String::new();
        entry
            .read_to_string(&mut buf)
            .map_err(|e| Error::ForgeInstallerCorrupted {
                mc: mc.into(),
                fv: fv.into(),
                details: format!("read version.json: {e}"),
            })?;
        crate::versions::version_json::parse(&buf).map_err(|e| Error::ForgeInstallerCorrupted {
            mc: mc.into(),
            fv: fv.into(),
            details: format!("parse version.json: {e}"),
        })?
    };

    // 3. Inputs (vanilla jar, Lucerna's runtime, maven tree, profile libraries),
    // then every client processor as Forge's installer runs it, its declared
    // outputs checked (spec 2026-10-16 §5.1).
    let env = prepare(&profile, installer_bytes, mc, fv, app).await?;
    let _stage = stage_lock(&env.installer_path).await;
    run_client_processors(
        &profile,
        installer_bytes,
        &env,
        &AppStageIo { app: app.clone() },
    )
    .await?;

    // 8. Assemble final VersionDetails (modern seam — pass-through for now).
    // Nothing is added to the profile's libraries: the patched client jar is
    // not a classpath entry unless Forge's own version.json lists it (Forge
    // 1.20.6+ does). FML finds it through `-DlibraryDirectory` (spec 2026-10-16
    // §3.1).
    Ok(crate::forge::profile::assemble_from_modern(version_details))
}
