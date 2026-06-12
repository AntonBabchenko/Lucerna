use super::*;

// Onboarding (v0.5.0 sub-feature 5):

/// Read the persisted app-level settings (currently: onboarding state).
/// Returns `AppFile::default()` if `app.json` is missing — a fresh
/// install has never written settings.
#[tauri::command]
#[specta::specta]
pub async fn app_settings_get(
    app: tauri::AppHandle,
) -> crate::error::Result<crate::instances::schema::AppFile> {
    let path =
        crate::paths::app_file(&app).map_err(|e| crate::error::Error::io("<app_file>", e))?;
    crate::instances::store::read_app_json(&path)
}

/// Persist that the user finished or skipped the onboarding tour on
/// the given launcher version. Idempotent — overwrites whatever was
/// there (replay-from-Settings does NOT call this; only finish / skip
/// from the tour itself does).
#[tauri::command]
#[specta::specta]
pub async fn app_settings_mark_tour_completed(
    app: tauri::AppHandle,
    version: String,
) -> crate::error::Result<()> {
    let path =
        crate::paths::app_file(&app).map_err(|e| crate::error::Error::io("<app_file>", e))?;
    let mut current = crate::instances::store::read_app_json(&path)?;
    current.onboarding.tour_completed_version = Some(version);
    crate::instances::store::write_app_json(&path, &current)
}

/// Persist the GeneralSettings block. Read-modify-write of app.json
/// — leaves `active_instance`, `onboarding`, and `version` untouched.
/// After persisting, re-syncs GPU preference to every installed JRE.
#[tauri::command]
#[specta::specta]
pub async fn app_settings_set_general(
    app: tauri::AppHandle,
    general: crate::instances::schema::GeneralSettings,
) -> crate::error::Result<()> {
    let path =
        crate::paths::app_file(&app).map_err(|e| crate::error::Error::io("<app_file>", e))?;
    let mut current = crate::instances::store::read_app_json(&path)?;
    let pref = general.gpu_preference;
    current.general = general;
    crate::instances::store::write_app_json(&path, &current)?;

    // Keep installed JREs' GPU preference in sync (Windows writes the registry;
    // no-op on Linux/macOS). Best-effort — the setting is already persisted.
    sweep_installed_jres_gpu(&app, pref);
    Ok(())
}

/// Best-effort: apply `pref` to every installed `jres/*/bin/javaw.exe`
/// (or `bin/java` off Windows). Errors are logged, never surfaced.
fn sweep_installed_jres_gpu(app: &tauri::AppHandle, pref: crate::instances::schema::GpuPreference) {
    let Ok(jres) = crate::paths::jres_dir(app) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(&jres) else {
        return;
    };
    for e in entries.flatten() {
        if !e.path().is_dir() {
            continue;
        }
        #[cfg(target_os = "windows")]
        let exe = e.path().join("bin").join("javaw.exe");
        #[cfg(not(target_os = "windows"))]
        let exe = e.path().join("bin").join("java");
        if exe.exists() {
            if let Err(err) = crate::platform::gpu::sync_for_exe(&exe, pref) {
                eprintln!("gpu: sweep sync failed for {}: {err}", exe.display());
            }
        }
    }
}

/// Probe GPU-selection capability for the Settings UI. Read-only; returns
/// `Unsupported`/`SingleGpu` (UI hides the control) or `Available` with labels.
#[tauri::command]
#[specta::specta]
pub async fn gpu_capability() -> crate::error::Result<crate::platform::gpu::GpuCapability> {
    Ok(crate::platform::gpu::capability())
}
