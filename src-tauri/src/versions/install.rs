//! Orchestrator for the version install pipeline.
//!
//! Fetches the per-version JSON, runs (libraries, assets, client) in
//! sequence, and emits typed `InstallProgress` events along the way.
//! Idempotent: re-running on a complete install does no network work
//! (every file passes the SHA precheck and is skipped).

use crate::error::{Error, Result};
use crate::network::get_json;
use crate::paths::versions_dir;
use crate::versions::manifest::list_manifest;
use crate::versions::version_json::{parse, VersionDetails};
use serde::Serialize;
use specta::Type;
use tauri_specta::Event;

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InstallPhase {
    Manifest,
    ForgeInstall,
    Jre,
    Libraries,
    Assets,
    Client,
    Complete,
}

#[derive(Debug, Clone, Serialize, Type, Event)]
pub struct InstallProgress {
    pub version_id: String,
    pub phase: InstallPhase,
    pub files_done: u32,
    pub files_total: u32,
    /// Cumulative bytes within the current phase.
    pub bytes_done: f64,
    /// Free-form sub-step label shown by the UI during long-running
    /// phases (currently only `ForgeInstall`). `None` for phases that
    /// don't have meaningful sub-steps.
    pub current_step: Option<String>,
}

/// Borrow the `asset_index` field, returning a typed error if the
/// merged JSON did not provide it. Used to be a `.expect()` — replaced
/// to keep a tampered or schema-shifted upstream manifest from
/// panicking the installer.
fn require_asset_index(
    details: &VersionDetails,
) -> Result<&crate::versions::version_json::AssetIndexRef> {
    details.asset_index.as_ref().ok_or_else(|| {
        Error::io(
            "<version_json>",
            "merged JSON missing assetIndex — upstream schema change?",
        )
    })
}

/// Borrow the `downloads` field, returning a typed error if the
/// merged JSON did not provide it. Same rationale as
/// `require_asset_index`.
fn require_client_downloads(
    details: &VersionDetails,
) -> Result<&crate::versions::version_json::Downloads> {
    details.downloads.as_ref().ok_or_else(|| {
        Error::io(
            "<version_json>",
            "merged JSON missing downloads — upstream schema change?",
        )
    })
}

/// Drive the full install pipeline for `version_id`.
///
/// Phases:
/// 1. Manifest — fetch + cache the per-version JSON.
/// 2. Libraries — download every library that should install on the
///    current platform.
/// 3. Assets — download the asset index and every missing object
///    concurrently (8 at a time).
/// 4. Client — download the client.jar.
pub async fn install_version(version_id: &str, app: &tauri::AppHandle) -> Result<()> {
    emit(app, version_id, InstallPhase::Manifest, 0, 1, 0.0);

    // Phase 1: per-version JSON
    let details = ensure_version_json(version_id, app).await?;
    emit(app, version_id, InstallPhase::Manifest, 1, 1, 0.0);

    // Detect platform (used by libraries below; JRE has its own
    // platform detection inside the jre module).
    let os = current_os();
    let arch = current_arch();

    // Phase 1b: JRE. Pre-1.13 versions have no `javaVersion` field;
    // fall back to Mojang's `jre-legacy` (Java 8) which is what
    // Prism Launcher does and what those MC versions need.
    let component = details
        .java_version
        .as_ref()
        .map(|jv| jv.component.as_str())
        .unwrap_or(crate::jre::DEFAULT_LEGACY_COMPONENT)
        .to_string();
    emit(app, version_id, InstallPhase::Jre, 0, 1, 0.0);
    {
        let app_clone = app.clone();
        let version_id_owned = version_id.to_string();
        crate::jre::ensure_jre(&component, app, move |done, total, bytes| {
            InstallProgress {
                version_id: version_id_owned.clone(),
                phase: InstallPhase::Jre,
                files_done: done,
                files_total: total,
                bytes_done: bytes as f64,
                current_step: None,
            }
            .emit(&app_clone)
            .ok();
        })
        .await?;
    }
    // No explicit "Jre done" emit — the per-file callback fires
    // total/total on the last file. For a fully-cached install,
    // ensure_jre's marker-fast-path itself emits (1, 1, 0).

    // Phase 2: libraries
    let lib_count = details.libraries.len() as u32;
    emit(app, version_id, InstallPhase::Libraries, 0, lib_count, 0.0);
    super::libraries::ensure_libraries(&details.libraries, os, arch, app).await?;
    emit(
        app,
        version_id,
        InstallPhase::Libraries,
        lib_count,
        lib_count,
        0.0,
    );

    // Phase 3: assets
    // After ensure_version_json returns, the merged JSON always has
    // asset_index as Some — vanilla parent supplies it; loader profiles
    // inherit it via merge_inherits.
    let asset_index = require_asset_index(&details)?;
    let app_clone = app.clone();
    let version_id_owned = version_id.to_string();
    // No instance context here (we install a version, not an instance), so
    // `game_dir` is None. Legacy `virtual` indexes still materialise into the
    // version-scoped `<assets>/virtual/<id>/`; `map_to_resources` needs a
    // per-instance dir and is handled at launch time (see deferral note).
    super::assets::ensure_assets(asset_index, None, app, move |done, total, bytes| {
        InstallProgress {
            version_id: version_id_owned.clone(),
            phase: InstallPhase::Assets,
            files_done: done,
            files_total: total,
            bytes_done: bytes as f64,
            current_step: None,
        }
        .emit(&app_clone)
        .ok();
    })
    .await?;

    // Phase 4: client
    // Same invariant: vanilla parent always supplies downloads.
    let client_download = require_client_downloads(&details)?;
    emit(app, version_id, InstallPhase::Client, 0, 1, 0.0);
    // Vanilla MC client.jar must live at `versions/<mc>/<mc>.jar` only.
    // For synth installs (Fabric/Quilt/Forge/NeoForge), route to the parent
    // MC's id; otherwise the vanilla client would be duplicated at
    // `versions/<synth>/<synth>.jar`, where it conflicts with NeoForge 21.x's
    // FML 4.0.42 module-name heuristic (which takes the filename's first
    // dash-segment as the module name → `neoforge` → collides with the
    // universal jar that's also module `neoforge`).
    let client_dest_id = crate::versions::loaders::parse_synth_id(version_id)
        .map(|(_loader, _lv, mc)| mc)
        .unwrap_or_else(|| version_id.to_string());
    super::client::ensure_client(&client_dest_id, &client_download.client, app).await?;
    emit(app, version_id, InstallPhase::Client, 1, 1, 0.0);

    emit(app, version_id, InstallPhase::Complete, 1, 1, 0.0);
    Ok(())
}

/// Fetch + cache the per-version JSON. Stored at
/// `<versions_dir>/<id>/<id>.json`.
///
/// Handles two id flavours:
/// 1. Vanilla — looked up in the Mojang manifest.
/// 2. Synthesised loader id (`fabric-loader-...`, `quilt-loader-...`)
///    — recursively resolves the parent and merges via
///    `versions::resolve::merge_inherits`. Recursion depth capped at
///    4 to defend against loop-shaped `inheritsFrom` chains.
pub(crate) async fn ensure_version_json(
    version_id: &str,
    app: &tauri::AppHandle,
) -> Result<VersionDetails> {
    ensure_version_json_inner(version_id, app, 0).await
}

const MAX_INHERITS_DEPTH: u32 = 4;

#[async_recursion::async_recursion]
async fn ensure_version_json_inner(
    version_id: &str,
    app: &tauri::AppHandle,
    depth: u32,
) -> Result<VersionDetails> {
    if depth > MAX_INHERITS_DEPTH {
        return Err(Error::io(
            "<resolve>",
            format!("inheritsFrom chain too deep (>{MAX_INHERITS_DEPTH}) starting at {version_id}"),
        ));
    }

    let dir = versions_dir(app).map_err(|e| Error::io("<versions_dir>", e))?;
    let path = dir.join(version_id).join(format!("{version_id}.json"));

    // Disk fast-path — both vanilla and synth ids share this. A parse
    // failure means the cached file is corrupt (truncated write, disk
    // fault, hand-edit): self-heal by deleting it and falling through to
    // the network/synth path rather than surfacing a permanent error that
    // no amount of retrying could clear.
    if let Ok(raw) = tokio::fs::read_to_string(&path).await {
        match parse(&raw) {
            Ok(details) => return Ok(details),
            Err(e) => {
                crate::diag!(
                    "versions: cached {} is corrupt ({e}) — deleting and re-fetching",
                    path.display()
                );
                let _ = tokio::fs::remove_file(&path).await;
                // fall through to re-fetch
            }
        }
    }

    // Synthetic id dispatch
    if let Some((loader, loader_ver, mc_ver)) = crate::versions::loaders::parse_synth_id(version_id)
    {
        if matches!(
            loader,
            crate::versions::loaders::Loader::Forge | crate::versions::loaders::Loader::NeoForge
        ) {
            InstallProgress {
                version_id: version_id.to_string(),
                phase: InstallPhase::ForgeInstall,
                files_done: 0,
                files_total: 1,
                bytes_done: 0.0,
                current_step: Some(format!(
                    "Installing {} {loader_ver} for {mc_ver}",
                    loader.display_name()
                )),
            }
            .emit(app)
            .ok();
        }
        let child =
            crate::versions::loaders::fetch_profile(loader, &mc_ver, &loader_ver, app).await?;
        if matches!(
            loader,
            crate::versions::loaders::Loader::Forge | crate::versions::loaders::Loader::NeoForge
        ) {
            InstallProgress {
                version_id: version_id.to_string(),
                phase: InstallPhase::ForgeInstall,
                files_done: 1,
                files_total: 1,
                bytes_done: 0.0,
                current_step: None,
            }
            .emit(app)
            .ok();
        }
        let parent = ensure_version_json_inner(&mc_ver, app, depth + 1).await?;
        let merged = crate::versions::resolve::merge_inherits(child, parent);
        let text = serde_json::to_string(&merged)
            .map_err(|e| Error::io(path.display().to_string(), format!("serialise: {e}")))?;
        write_cache_atomic(&path, &text).await?;
        return Ok(merged);
    }

    // Vanilla path — manifest lookup + fetch.
    let entries = list_manifest().await?;
    let entry =
        entries
            .iter()
            .find(|e| e.id == version_id)
            .ok_or_else(|| Error::UnknownVersion {
                id: version_id.to_string(),
            })?;

    let json: serde_json::Value = get_json(&entry.url, "versions").await?;
    let text = serde_json::to_string(&json)
        .map_err(|e| Error::io(path.display().to_string(), format!("serialise: {e}")))?;

    write_cache_atomic(&path, &text).await?;

    parse(&text).map_err(|e| Error::io(path.display().to_string(), format!("parse: {e}")))
}

/// Write the cached version JSON atomically: create the parent dir, stream
/// to a sibling `<path>.tmp`, then rename onto `path`. This prevents a
/// crash/power-loss mid-write from leaving a truncated JSON that the disk
/// fast-path would later reject (self-healed above, but avoided entirely
/// here). Same-directory temp keeps the rename a cheap same-volume move.
async fn write_cache_atomic(path: &std::path::Path, text: &str) -> Result<()> {
    if let Some(parent_dir) = path.parent() {
        tokio::fs::create_dir_all(parent_dir)
            .await
            .map_err(|e| Error::io(parent_dir.display().to_string(), e))?;
    }
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    let tmp = std::path::PathBuf::from(tmp);
    tokio::fs::write(&tmp, text)
        .await
        .map_err(|e| Error::io(tmp.display().to_string(), e))?;
    tokio::fs::rename(&tmp, path).await.map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        Error::io(path.display().to_string(), e)
    })
}

pub(crate) fn current_os() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

pub(crate) fn current_arch() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "x86"
    }
}

fn emit(
    app: &tauri::AppHandle,
    version_id: &str,
    phase: InstallPhase,
    done: u32,
    total: u32,
    bytes: f64,
) {
    InstallProgress {
        version_id: version_id.to_string(),
        phase,
        files_done: done,
        files_total: total,
        bytes_done: bytes,
        current_step: None,
    }
    .emit(app)
    .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_os_returns_known_value() {
        let os = current_os();
        assert!(["windows", "macos", "linux"].contains(&os));
    }

    #[test]
    fn current_arch_returns_known_value() {
        let arch = current_arch();
        assert!(["x64", "aarch64", "x86"].contains(&arch));
    }

    #[test]
    fn install_progress_serializes_with_snake_case_phase() {
        let p = InstallProgress {
            version_id: "1.20.4".into(),
            phase: InstallPhase::Assets,
            files_done: 100,
            files_total: 600,
            bytes_done: 12345.0,
            current_step: None,
        };
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains(r#""phase":"assets""#), "got: {json}");
    }

    #[test]
    fn forge_install_phase_serializes_as_snake_case() {
        let p = InstallProgress {
            version_id: "forge-49.0.49-1.20.4".into(),
            phase: InstallPhase::ForgeInstall,
            files_done: 1,
            files_total: 5,
            bytes_done: 0.0,
            current_step: Some("Running BinaryPatcher".into()),
        };
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains(r#""phase":"forge_install""#), "got: {json}");
        assert!(
            json.contains(r#""current_step":"Running BinaryPatcher""#),
            "got: {json}"
        );
    }

    #[test]
    fn install_progress_default_current_step_is_none() {
        let p = InstallProgress {
            version_id: "1.20.4".into(),
            phase: InstallPhase::Assets,
            files_done: 0,
            files_total: 0,
            bytes_done: 0.0,
            current_step: None,
        };
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains(r#""current_step":null"#), "got: {json}");
    }

    #[test]
    fn max_inherits_depth_is_four() {
        // Smoke check that the cap is non-trivial. The actual cap is
        // exercised by an integration test (loaders_integration.rs).
        assert!(
            MAX_INHERITS_DEPTH >= 2,
            "must allow at least loader → vanilla"
        );
        assert!(MAX_INHERITS_DEPTH <= 10, "must not be unboundedly large");
    }

    fn synth_details() -> VersionDetails {
        // Minimal VersionDetails with asset_index = None and downloads = None.
        // Mirrors a Fabric/Quilt loader profile before merge_inherits runs.
        VersionDetails {
            id: "fabric-loader-0.15.7-1.20.4".into(),
            inherits_from: Some("1.20.4".into()),
            main_class: "net.fabricmc.loader.impl.launch.knot.KnotClient".into(),
            java_version: None,
            asset_index: None,
            assets: None,
            libraries: vec![],
            downloads: None,
            arguments: None,
            minecraft_arguments: None,
        }
    }

    #[test]
    fn require_asset_index_errors_when_missing() {
        let d = synth_details();
        assert!(
            require_asset_index(&d).is_err(),
            "expected Err when asset_index = None"
        );
    }

    #[test]
    fn require_client_downloads_errors_when_missing() {
        let d = synth_details();
        assert!(
            require_client_downloads(&d).is_err(),
            "expected Err when downloads = None"
        );
    }
}
