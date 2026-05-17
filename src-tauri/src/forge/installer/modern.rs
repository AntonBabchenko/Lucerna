//! MC 1.17.x+ installer era (modern). install_profile spec=1 + embedded
//! /version.json + 9 processors (3 server-only, 6 client-applicable).
//!
//! The pipeline is structurally identical to Phase 2's transitional era;
//! the differences are entirely in WHICH processors run and HOW they
//! compose:
//!   - installertools EXTRACT_FILES        (server-only, sides filter)
//!   - installertools BUNDLER_EXTRACT × 2  (server-only, sides filter)
//!   - installertools MCP_DATA             (both — SRG mappings)
//!   - installertools DOWNLOAD_MOJMAPS     (both — Mojang official)
//!   - installertools MERGE_MAPPING        (both — combine the two)
//!   - FART (server side)                  (server-only, sides filter)
//!   - FART (client side)                  (CLIENT — remap vanilla jar)
//!   - binarypatcher                       (both — apply Forge patches)
//!
//! Phase 2 helpers (`parse_install_profile`, `substitute_args`,
//! `classpath_coords_to_libraries`, the data resolver, the maven-tree
//! extractor) are imported from `super::transitional::*` — no
//! duplication. The only modern-era-specific code lives in this file
//! and is limited to: dispatching to the modern profile-assembly seam
//! and (for clarity) re-asserting the runtime preconditions.

use crate::error::{Error, Result};
use crate::versions::version_json::VersionDetails;

pub async fn install(
    install_profile_value: &serde_json::Value,
    installer_bytes: &[u8],
    mc: &str,
    fv: &str,
    app: &tauri::AppHandle,
) -> Result<VersionDetails> {
    use super::transitional::{
        classpath_coords_to_libraries, parse_install_profile, substitute_args,
    };
    use crate::forge::patcher::{run_processor, ProcessorContext};
    use crate::versions::libraries::ensure_libraries;

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
        let mut entry = archive.by_name("version.json").map_err(|_| {
            Error::ForgeInstallerCorrupted {
                mc: mc.into(),
                fv: fv.into(),
                details: "missing /version.json".into(),
            }
        })?;
        let mut buf = String::new();
        entry
            .read_to_string(&mut buf)
            .map_err(|e| Error::ForgeInstallerCorrupted {
                mc: mc.into(),
                fv: fv.into(),
                details: format!("read version.json: {e}"),
            })?;
        crate::versions::version_json::parse(&buf).map_err(|e| {
            Error::ForgeInstallerCorrupted {
                mc: mc.into(),
                fv: fv.into(),
                details: format!("parse version.json: {e}"),
            }
        })?
    };

    // 3. Compute paths (identical layout to Phase 2).
    let app_data = crate::paths::app_dir(app).map_err(|e| Error::io("<app_data>", e))?;
    let cache_dir = app_data.join("forge").join("cache").join(format!("{mc}-{fv}"));
    tokio::fs::create_dir_all(&cache_dir)
        .await
        .map_err(|e| Error::io(cache_dir.display().to_string(), e))?;
    let cache_dir_str = cache_dir.display().to_string();
    let libs_root = crate::paths::libraries_dir(app).map_err(|e| Error::io("<libraries>", e))?;
    let installer_cache_path = app_data
        .join("forge")
        .join("installers")
        .join(format!("{mc}-{fv}.jar"));
    let installer_path_str = installer_cache_path.display().to_string();
    let minecraft_jar = app_data
        .join("versions")
        .join(mc)
        .join(format!("{mc}.jar"))
        .display()
        .to_string();

    // 4. Vanilla MC client.jar + JRE must exist BEFORE processors run.
    //    FART consumes {MINECRAFT_JAR}; FART + DOWNLOAD_MOJMAPS both need
    //    the JRE on disk + a working network for the Mojang manifest.
    let vanilla_jre_component;
    {
        let vanilla_details = crate::versions::install::ensure_version_json(mc, app).await?;
        vanilla_jre_component = vanilla_details
            .java_version
            .as_ref()
            .map(|jv| jv.component.clone())
            .unwrap_or_else(|| crate::jre::DEFAULT_LEGACY_COMPONENT.to_string());
        let client_download = vanilla_details
            .downloads
            .as_ref()
            .ok_or_else(|| Error::ForgeInstallerCorrupted {
                mc: mc.into(),
                fv: fv.into(),
                details: "vanilla version JSON has no downloads field".into(),
            })?;
        crate::versions::client::ensure_client(mc, &client_download.client, app).await?;
    }
    crate::jre::ensure_jre(&vanilla_jre_component, app, |_, _, _| {}).await?;
    let java_bin = crate::jre::java_executable_path(&vanilla_jre_component, app)?;

    // 5. Extract maven/ subtree from installer (the shim jar for modern era).
    super::transitional::extract_installer_maven_tree_pub(installer_bytes, &libs_root).await?;

    // 6. Fetch install_profile.libraries[] (filter URL-less, same as Phase 2).
    let os = crate::versions::install::current_os();
    let arch = crate::versions::install::current_arch();
    let downloadable: Vec<crate::versions::version_json::Library> = profile
        .libraries
        .iter()
        .filter(|l| {
            let from_top = l.url.as_deref().map(|s| !s.is_empty()).unwrap_or(false);
            let from_artifact = l
                .downloads
                .as_ref()
                .and_then(|d| d.artifact.as_ref())
                .map(|a| !a.url.is_empty())
                .unwrap_or(false);
            from_top || from_artifact
        })
        .cloned()
        .collect();
    ensure_libraries(&downloadable, os, arch, app).await?;

    // 7. Sequence processors (sides filter skips the 3 server-only entries).
    for p in &profile.processors {
        if let Some(sides) = &p.sides {
            if !sides.iter().any(|s| s == "client") {
                continue;
            }
        }
        let cp_libs = classpath_coords_to_libraries(&p.classpath);
        ensure_libraries(&cp_libs, os, arch, app).await?;
        let resolved = substitute_args(
            &p.args,
            &profile.data,
            "client",
            &libs_root,
            installer_bytes,
            &installer_path_str,
            &cache_dir_str,
            &minecraft_jar,
        )
        .await?;
        let mut cp_paths: Vec<std::path::PathBuf> = p
            .classpath
            .iter()
            .filter_map(|c| crate::forge::patcher::maven_coord_to_relative_path(c))
            .map(|rel| libs_root.join(rel))
            .collect();
        if let Some(rel) = crate::forge::patcher::maven_coord_to_relative_path(&p.jar) {
            cp_paths.push(libs_root.join(rel));
        }
        let ctx = ProcessorContext {
            classpath: cp_paths,
            cache_dir: cache_dir.clone(),
            java_bin: Some(java_bin.clone()),
        };
        run_processor(&p.jar, resolved, &ctx).await?;
    }

    // 8. Assemble final VersionDetails (modern seam — pass-through for now).
    Ok(crate::forge::profile::assemble_from_modern(version_details))
}

#[cfg(test)]
mod tests {
    // Production code is covered by:
    //   - forge_modern_era_integration.rs (no-network shield)
    //   - forge_modern_era_e2e.rs         (#[ignore]'d full pipeline)
    // No unit tests here — install() is irreducible glue.
}
