use specta::Type;

#[derive(Debug, serde::Serialize, Type)]
pub struct Greeting {
    pub message: String,
}

#[tauri::command]
#[specta::specta]
pub fn greet(name: String) -> Greeting {
    Greeting {
        message: format!("Hello, {name}! — FTlauncher is alive."),
    }
}

/// Return the most recent network activity entries (up to 200).
#[tauri::command]
#[specta::specta]
pub fn network_activity() -> Vec<crate::network::AuditEntry> {
    crate::network::recent()
}

/// Return the currently persisted account, or `None` if not set.
#[tauri::command]
#[specta::specta]
pub fn get_account(
    app: tauri::AppHandle,
) -> Result<Option<crate::accounts::Account>, crate::error::Error> {
    crate::accounts::get_current(&app)
}

/// Persist an offline account for the given display name. The UUID is
/// derived deterministically.
#[tauri::command]
#[specta::specta]
pub fn set_offline_account(
    app: tauri::AppHandle,
    name: String,
) -> Result<crate::accounts::Account, crate::error::Error> {
    crate::accounts::set_offline(&app, &name)
}

/// Fetch the Mojang version manifest. Cached for 5 minutes — repeated
/// calls within that window are zero-network.
#[tauri::command]
#[specta::specta]
pub async fn list_versions() -> Result<Vec<crate::versions::VersionEntry>, crate::error::Error> {
    crate::versions::list_manifest().await
}

/// Install a Minecraft version: downloads the per-version JSON,
/// libraries, assets, and client.jar. Emits `installProgress` events
/// throughout. Idempotent — files already present with matching SHA-1
/// are skipped.
#[tauri::command]
#[specta::specta]
pub async fn install_version(
    app: tauri::AppHandle,
    version_id: String,
) -> Result<(), crate::error::Error> {
    crate::versions::install_version(&version_id, &app).await
}

/// Install (idempotently) and then launch the given Minecraft version.
/// Emits `installProgress` during install and `processSpawned` /
/// `processExited` around the run.
#[tauri::command]
#[specta::specta]
pub async fn install_and_launch(
    app: tauri::AppHandle,
    version_id: String,
) -> Result<u32, crate::error::Error> {
    crate::versions::install_version(&version_id, &app).await?;
    let account = crate::accounts::get_current(&app)?
        .ok_or(crate::error::Error::AccountNotSet)?;
    crate::launch::start(&version_id, &account, &app).await
}

/// Kill the running Minecraft process if any. Idempotent.
#[tauri::command]
#[specta::specta]
pub fn stop_minecraft() -> Result<(), crate::error::Error> {
    crate::launch::stop()
}

/// List every log file under the default instance's three documented
/// roots. Sorted by mtime descending.
#[tauri::command]
#[specta::specta]
pub fn list_log_files(
    app: tauri::AppHandle,
) -> Result<Vec<crate::logs::files::LogFileMeta>, crate::error::Error> {
    crate::logs::files::list_log_files(&app)
}

/// Read up to `max_bytes` of a log file. `max_bytes` is clamped to
/// `[64 KB, 100 MB]`; `0` becomes the 5 MB default. `path` must be
/// under one of the three allowed log roots — anything else is
/// rejected with `Error::Io`.
#[tauri::command]
#[specta::specta]
pub fn read_log_file(
    app: tauri::AppHandle,
    path: String,
    max_bytes: f64,
) -> Result<String, crate::error::Error> {
    let roots = crate::logs::files::allowed_roots(&app)?;
    let path = std::path::PathBuf::from(&path);
    crate::logs::files::assert_under_allowed_roots(&path, &roots)?;
    let cap = if !max_bytes.is_finite() || max_bytes < 0.0 {
        0
    } else {
        max_bytes as u64
    };
    crate::logs::read::read_with_cap(&path, cap)
}

/// Newest crash report (if any). Used by the UI to show a banner on
/// non-zero MC exit.
#[tauri::command]
#[specta::specta]
pub fn latest_crash(
    app: tauri::AppHandle,
) -> Result<Option<crate::logs::files::CrashReport>, crate::error::Error> {
    crate::logs::files::latest_crash(&app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greet_includes_name() {
        let g = greet("World".to_string());
        assert!(g.message.contains("World"));
        assert!(g.message.contains("FTlauncher"));
    }
}
