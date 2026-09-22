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

/// Where an in-app install is; the button and the progress toast follow it.
#[derive(Debug, Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct UpdateInstallPhase {
    pub phase: crate::update::install::Phase,
}

/// Re-check, then download + verify + launch the latest installer and
/// exit. Re-checks server-side rather than trusting a client-supplied
/// `UpdateInfo`, so the URLs to download are always derived from the
/// live release on `api.github.com`. No-op if already up-to-date.
///
/// Lucerna CLOSES to install, and the exit hook force-kills every running
/// game and server: the install is refused while anything runs, is starting
/// or holds a claim — and while that cannot be told — here at the top, and
/// once more right before the installer is spawned.
#[tauri::command]
#[specta::specta]
pub async fn update_install(app: tauri::AppHandle) -> crate::error::Result<()> {
    use crate::update::install::{download_and_install, install_blocked, try_begin};
    // The installer ends in `app.exit(0)`. During a move that kills the copy
    // with no cleanup; after one, the launcher must restart first anyway.
    crate::data_root::state::global().check_usable()?;
    // In-app install runs on Windows and on Linux AppImage builds; a .deb/.rpm
    // or macOS run is check-and-notify (the UI opens the release page instead).
    // Refuse rather than attempt a no-asset install.
    if !crate::platform::supports_in_app_install() {
        return Err(crate::error::Error::UpdateInstallFailed {
            details: "in-app install is not supported on this platform".into(),
        });
    }
    // One install at a time: two would clear the update dir under each other.
    let _guard = try_begin().ok_or(crate::error::Error::UpdateBlocked {
        block: crate::data_root::blockers::RestartBlock::Busy,
    })?;
    install_blocked(crate::data_root::blockers::observe(&app))?;
    let info = crate::update::check::check_for_update(env!("CARGO_PKG_VERSION")).await?;
    if !info.available {
        return Ok(());
    }
    let emitter = app.clone();
    let on_phase = std::sync::Arc::new(move |phase| {
        use tauri_specta::Event;
        // A lost phase event costs the button its label for a moment, nothing
        // more; the install itself does not depend on it.
        if let Err(e) = (UpdateInstallPhase { phase }).emit(&emitter) {
            crate::diag!("[update] phase event not delivered: {e}");
        }
    });
    download_and_install(&app, &info, on_phase).await
}

/// Persist that the user dismissed the update toast for `version`, so it
/// is not shown again until a newer release appears. Read-modify-write
/// of app.json — leaves everything else untouched.
#[tauri::command]
#[specta::specta]
pub async fn update_dismiss(app: tauri::AppHandle, version: String) -> crate::error::Result<()> {
    let path =
        crate::paths::app_file(&app).map_err(|e| crate::error::Error::io("<app_file>", e))?;
    crate::instances::store::update_app_json(&path, |af| {
        af.update_dismissed_version = Some(version);
        crate::instances::store::Verdict::Write
    })
    .map(|_| ())
}
