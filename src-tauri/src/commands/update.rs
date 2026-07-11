// =========================================================================
// Self-update
// =========================================================================

/// Check GitHub Releases for a newer version. Returns `UpdateInfo` with
/// `available=false` when up-to-date; `Err` on network/parse failure
/// (the startup caller swallows it silently — a failed check never nags).
#[tauri::command]
#[specta::specta]
pub async fn update_check() -> crate::error::Result<crate::update::UpdateInfo> {
    crate::update::check::check_for_update(env!("CARGO_PKG_VERSION")).await
}

/// Re-check, then download + verify + launch the latest installer and
/// exit. Re-checks server-side rather than trusting a client-supplied
/// `UpdateInfo`, so the URLs to download are always derived from the
/// live release on `api.github.com`. No-op if already up-to-date.
#[tauri::command]
#[specta::specta]
pub async fn update_install(app: tauri::AppHandle) -> crate::error::Result<()> {
    // In-app install runs on Windows and on Linux AppImage builds; a .deb/.rpm
    // or macOS run is check-and-notify (the UI opens the release page instead).
    // Refuse rather than attempt a no-asset install.
    if !crate::platform::supports_in_app_install() {
        return Err(crate::error::Error::UpdateInstallFailed {
            details: "in-app install is not supported on this platform".into(),
        });
    }
    let info = crate::update::check::check_for_update(env!("CARGO_PKG_VERSION")).await?;
    if !info.available {
        return Ok(());
    }
    crate::update::install::download_and_install(&app, &info).await
}

/// Persist that the user dismissed the update toast for `version`, so it
/// is not shown again until a newer release appears. Read-modify-write
/// of app.json — leaves everything else untouched.
#[tauri::command]
#[specta::specta]
pub async fn update_dismiss(app: tauri::AppHandle, version: String) -> crate::error::Result<()> {
    let path =
        crate::paths::app_file(&app).map_err(|e| crate::error::Error::io("<app_file>", e))?;
    let mut current = crate::instances::store::read_app_json(&path)?;
    current.update_dismissed_version = Some(version);
    crate::instances::store::write_app_json(&path, &current)
}
