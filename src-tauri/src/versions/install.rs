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
    let asset_index = details
        .asset_index
        .as_ref()
        .expect("merged JSON should have assetIndex — vanilla parent must provide it");
    let app_clone = app.clone();
    let version_id_owned = version_id.to_string();
    super::assets::ensure_assets(asset_index, app, move |done, total, bytes| {
        InstallProgress {
            version_id: version_id_owned.clone(),
            phase: InstallPhase::Assets,
            files_done: done,
            files_total: total,
            bytes_done: bytes as f64,
        }
        .emit(&app_clone)
        .ok();
    })
    .await?;

    // Phase 4: client
    // Same invariant: vanilla parent always supplies downloads.
    let client_download = details
        .downloads
        .as_ref()
        .expect("merged JSON should have downloads — vanilla parent must provide it");
    emit(app, version_id, InstallPhase::Client, 0, 1, 0.0);
    super::client::ensure_client(version_id, &client_download.client, app).await?;
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
async fn ensure_version_json(
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

    // Disk fast-path — both vanilla and synth ids share this.
    if let Ok(raw) = tokio::fs::read_to_string(&path).await {
        return parse(&raw)
            .map_err(|e| Error::io(path.display().to_string(), format!("parse: {e}")));
    }

    // Synthetic id dispatch
    if let Some((loader, loader_ver, mc_ver)) =
        crate::versions::loaders::parse_synth_id(version_id)
    {
        let child = crate::versions::loaders::fetch_profile(loader, &mc_ver, &loader_ver).await?;
        let parent = ensure_version_json_inner(&mc_ver, app, depth + 1).await?;
        let merged = crate::versions::resolve::merge_inherits(child, parent);
        let text = serde_json::to_string(&merged)
            .map_err(|e| Error::io(path.display().to_string(), format!("serialise: {e}")))?;
        if let Some(parent_dir) = path.parent() {
            tokio::fs::create_dir_all(parent_dir)
                .await
                .map_err(|e| Error::io(parent_dir.display().to_string(), e))?;
        }
        tokio::fs::write(&path, &text)
            .await
            .map_err(|e| Error::io(path.display().to_string(), e))?;
        return Ok(merged);
    }

    // Vanilla path — manifest lookup + fetch.
    let entries = list_manifest().await?;
    let entry = entries
        .iter()
        .find(|e| e.id == version_id)
        .ok_or_else(|| Error::UnknownVersion {
            id: version_id.to_string(),
        })?;

    let json: serde_json::Value = get_json(&entry.url, "versions").await?;
    let text = serde_json::to_string(&json)
        .map_err(|e| Error::io(path.display().to_string(), format!("serialise: {e}")))?;

    if let Some(parent_dir) = path.parent() {
        tokio::fs::create_dir_all(parent_dir)
            .await
            .map_err(|e| Error::io(parent_dir.display().to_string(), e))?;
    }
    tokio::fs::write(&path, &text)
        .await
        .map_err(|e| Error::io(path.display().to_string(), e))?;

    parse(&text).map_err(|e| Error::io(path.display().to_string(), format!("parse: {e}")))
}

fn current_os() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

fn current_arch() -> &'static str {
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
        };
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains(r#""phase":"assets""#), "got: {json}");
    }

    #[test]
    fn max_inherits_depth_is_four() {
        // Smoke check that the cap is non-trivial. The actual cap is
        // exercised by an integration test (loaders_integration.rs).
        assert!(MAX_INHERITS_DEPTH >= 2, "must allow at least loader → vanilla");
        assert!(MAX_INHERITS_DEPTH <= 10, "must not be unboundedly large");
    }
}
