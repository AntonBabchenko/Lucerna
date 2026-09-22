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
/// Then the GPU preference: "Automatic" touches nothing; a choice is
/// applied to every installed runtime; leaving a choice puts back what
/// Lucerna replaced (`gpu_pref`).
#[tauri::command]
#[specta::specta]
pub async fn app_settings_set_general(
    app: tauri::AppHandle,
    general: crate::instances::schema::GeneralSettings,
) -> crate::error::Result<()> {
    let path =
        crate::paths::app_file(&app).map_err(|e| crate::error::Error::io("<app_file>", e))?;
    let mut current = crate::instances::store::read_app_json(&path)?;
    let old = current.general.gpu_preference;
    let new = general.gpu_preference;
    current.general = general;
    crate::instances::store::write_app_json(&path, &current)?;

    // Best-effort from here: the setting is persisted; the OS store follows.
    let record = match crate::gpu_pref::record_path(&app) {
        Ok(p) => p,
        Err(e) => {
            crate::diag!("[gpu] cannot resolve the record path, OS store untouched: {e}");
            return Ok(());
        }
    };
    // A record that cannot be read counts as empty here: the explicit
    // transitions report it themselves, and (Auto, Auto) must not turn a
    // theme flip into a logged failure on every save.
    let non_empty = crate::gpu_pref::has_entries(&record).unwrap_or(false);
    match gpu_transition(old, new, non_empty) {
        GpuTransition::Nothing => {}
        GpuTransition::Apply(pref) => sweep_installed_jres_gpu(&app, &record, pref),
        GpuTransition::Retire => {
            match crate::gpu_pref::retire_all(&crate::platform::gpu::OsRegistry, &record) {
                Ok(r) => crate::diag!(
                    "[gpu] retired: {} restored, {} forgotten, {} kept",
                    r.restored,
                    r.forgotten,
                    r.kept
                ),
                Err(e) => crate::diag!("[gpu] retire failed: {e}"),
            }
        }
    }
    Ok(())
}

/// Apply `pref` to every installed `jres/*/bin/javaw.exe` (`bin/java` off
/// Windows). Best-effort — every failure is logged with the exe and the
/// step; a missing `jres/` is "nothing installed yet", any other error is
/// said, and `NotFound` on an exe is told apart from a stat failure.
fn sweep_installed_jres_gpu(
    app: &tauri::AppHandle,
    record: &std::path::Path,
    pref: crate::instances::schema::GpuPreference,
) {
    use crate::gpu_pref::Applied;
    use std::io::ErrorKind::NotFound;
    let jres = match crate::paths::jres_dir(app) {
        Ok(d) => d,
        Err(e) => {
            crate::diag!("[gpu] cannot resolve the jres dir: {e}");
            return;
        }
    };
    let entries = match std::fs::read_dir(&jres) {
        Ok(entries) => entries,
        Err(e) if e.kind() == NotFound => return,
        Err(e) => {
            crate::diag!("[gpu] cannot list {}: {e}", jres.display());
            return;
        }
    };
    for entry in entries {
        let dir = match entry {
            Ok(e) => e.path(),
            Err(e) => {
                crate::diag!("[gpu] cannot read an entry of {}: {e}", jres.display());
                continue;
            }
        };
        #[cfg(target_os = "windows")]
        let exe = dir.join("bin").join("javaw.exe");
        #[cfg(not(target_os = "windows"))]
        let exe = dir.join("bin").join("java");
        match std::fs::metadata(&exe) {
            Ok(_) => {}
            Err(e) if e.kind() == NotFound => continue,
            Err(e) => {
                crate::diag!("[gpu] cannot stat {}: {e}", exe.display());
                continue;
            }
        }
        match crate::gpu_pref::apply(&crate::platform::gpu::OsRegistry, record, &exe, pref) {
            Ok(Applied::Refused(why)) => {
                crate::diag!("[gpu] refused for {}: {why}", exe.display())
            }
            Ok(_) => {}
            Err(e) => crate::diag!("[gpu] apply failed for {}: {e}", exe.display()),
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
    use crate::instances::schema::GpuPreference::Auto;
    match (old, new) {
        (Auto, Auto) if !record_non_empty => GpuTransition::Nothing,
        (_, Auto) => GpuTransition::Retire,
        (_, pref) => GpuTransition::Apply(pref),
    }
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
