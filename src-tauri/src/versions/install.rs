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

    // Detect platform
    let os = current_os();
    let arch = current_arch();

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
    let app_clone = app.clone();
    let version_id_owned = version_id.to_string();
    super::assets::ensure_assets(&details.asset_index, app, move |done, total, bytes| {
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
    emit(app, version_id, InstallPhase::Client, 0, 1, 0.0);
    super::client::ensure_client(version_id, &details.downloads.client, app).await?;
    emit(app, version_id, InstallPhase::Client, 1, 1, 0.0);

    emit(app, version_id, InstallPhase::Complete, 1, 1, 0.0);
    Ok(())
}

/// Fetch + cache the per-version JSON. Stored at
/// `<versions_dir>/<id>/<id>.json`.
async fn ensure_version_json(
    version_id: &str,
    app: &tauri::AppHandle,
) -> Result<VersionDetails> {
    let dir = versions_dir(app).map_err(|e| Error::io("<versions_dir>", e))?;
    let path = dir.join(version_id).join(format!("{version_id}.json"));

    // If file exists, parse it (no SHA check here — the manifest
    // provides one but a re-download on every install is wasteful;
    // hash-mismatch is caught at file-write time within download_with_sha).
    if let Ok(raw) = tokio::fs::read_to_string(&path).await {
        return parse(&raw).map_err(|e| Error::io(path.display().to_string(), format!("parse: {e}")));
    }

    // Not on disk — look up the URL in the manifest.
    let entries = list_manifest().await?;
    let entry = entries
        .iter()
        .find(|e| e.id == version_id)
        .ok_or_else(|| Error::UnknownVersion {
            id: version_id.to_string(),
        })?;

    // Stream the JSON via get_json — it's audited.
    let json: serde_json::Value = get_json(&entry.url, "versions").await?;
    let text = serde_json::to_string(&json)
        .map_err(|e| Error::io(path.display().to_string(), format!("serialise: {e}")))?;

    // Persist to disk
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| Error::io(parent.display().to_string(), e))?;
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
}
