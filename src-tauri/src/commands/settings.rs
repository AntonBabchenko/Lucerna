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
                crate::diag!("gpu: sweep sync failed for {}: {err}", exe.display());
            }
        }
    }
}

/// What a change of `gpu_preference` asks of the OS store.
#[derive(Debug, PartialEq, Eq)]
pub enum GpuTransition {
    Apply(crate::instances::schema::GpuPreference),
    Retire,
    Nothing,
}

/// `(_, non-Auto)` applies; `(non-Auto, Auto)` retires; `(Auto, Auto)` — a
/// theme flip, a language change — is nothing, UNLESS a retire could not
/// finish earlier (the record is not empty): that retry touches only values
/// Lucerna recorded writing, so it stays inside decision 3.
pub fn gpu_transition(
    old: crate::instances::schema::GpuPreference,
    new: crate::instances::schema::GpuPreference,
    record_non_empty: bool,
) -> GpuTransition {
    // RED STUB (push 1).
    let _ = (old, new, record_non_empty);
    GpuTransition::Nothing
}

/// Probe GPU-selection capability for the Settings UI, with the mechanism
/// this OS uses so the page can describe it truthfully. Read-only.
#[tauri::command]
#[specta::specta]
pub async fn gpu_capability() -> crate::error::Result<crate::platform::gpu::GpuStatus> {
    Ok(crate::platform::gpu::status())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::schema::GpuPreference::{Auto, HighPerformance, PowerSaving};

    #[test]
    fn a_theme_flip_touches_nothing() {
        assert_eq!(gpu_transition(Auto, Auto, false), GpuTransition::Nothing);
    }

    #[test]
    fn auto_to_auto_retries_an_unfinished_retire() {
        assert_eq!(gpu_transition(Auto, Auto, true), GpuTransition::Retire);
    }

    #[test]
    fn leaving_a_choice_retires_and_making_one_applies() {
        assert_eq!(
            gpu_transition(HighPerformance, Auto, false),
            GpuTransition::Retire
        );
        assert_eq!(
            gpu_transition(Auto, HighPerformance, false),
            GpuTransition::Apply(HighPerformance)
        );
        assert_eq!(
            gpu_transition(HighPerformance, PowerSaving, true),
            GpuTransition::Apply(PowerSaving)
        );
    }
}
