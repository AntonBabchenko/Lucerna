//! Get/set the effective data-root location, plus the running-guard that
//! blocks relocation while a game or server is live.

use crate::error::{Error, Result};
use tauri::{AppHandle, Manager};
use tauri_specta::Event;

/// True if any Minecraft instance process is currently live, or any saved
/// server reports a running status. Reuses the existing liveness
/// chokepoints — `launch::spawn::is_running` (the same check `stop_minecraft`
/// and `repair_instance` use for the single-instance game process) and
/// `commands::server_list`'s per-server `running` field (the same
/// PID-reconciled status the Servers UI and preflight diagnosis use) — so
/// this introduces no new process bookkeeping.
pub fn any_game_running(app: &AppHandle) -> bool {
    if crate::launch::spawn::is_running() {
        return true;
    }
    crate::commands::server_list(app.clone())
        .map(|servers| servers.iter().any(|s| s.running))
        .unwrap_or(false)
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct DataLocationStatus {
    pub effective: String,
    pub configured: Option<String>,
    pub fell_back: bool,
    /// f64 (specta forbids u64); within JS safe-int range.
    pub data_size_bytes: f64,
}

/// Current effective data-root location, its configured (possibly
/// unavailable) target, and the size on disk.
#[tauri::command]
#[specta::specta]
pub fn get_data_location(app: AppHandle) -> Result<DataLocationStatus> {
    let st = app.state::<crate::data_root::DataRoot>();
    let root = st.0.root.clone();
    Ok(DataLocationStatus {
        effective: root.display().to_string(),
        configured: st.0.configured.as_ref().map(|p| p.display().to_string()),
        fell_back: st.0.fell_back,
        data_size_bytes: crate::data_root::migrate::dir_size(&root) as f64,
    })
}

/// Streamed progress for a data-root relocation.
#[derive(Debug, Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct DataMigrationProgress {
    pub copied_bytes: f64,
    pub total_bytes: f64,
    /// "copying" | "verifying" | "deleting"
    pub phase: String,
}

/// Name of the bootstrap redirect file. It always lives at the OS-default
/// app-data dir and must never be copied/deleted as part of a relocation.
const REDIRECT_FILE_NAME: &str = "data-location.json";

/// Relocate the data root to `new_path`, or reset to the OS default when
/// `None`. Copies the current root to the target, verifies the copy,
/// repoints the bootstrap redirect, deletes the old data, then restarts the
/// app so every chokepoint re-resolves `paths::app_dir` against the new root.
///
/// Rejected while any game/server is running (`Error::DataLocationBusy`) or
/// when the target fails validation (`Error::DataLocationInvalid`). A copy or
/// verify failure surfaces as `Error::DataLocationMigrationFailed` — the
/// original data is left untouched because the redirect is written and the
/// old data deleted only after a complete, verified copy.
#[tauri::command]
#[specta::specta]
pub async fn set_data_location(app: AppHandle, new_path: Option<String>) -> Result<()> {
    let current = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let default =
        crate::paths::default_app_data_dir(&app).map_err(|e| Error::io("<default>", e))?;
    let target = match &new_path {
        Some(p) => std::path::PathBuf::from(p),
        None => default.clone(),
    };

    if any_game_running(&app) {
        return Err(Error::DataLocationBusy);
    }

    let empty = crate::data_root::migrate::target_is_empty(&target);
    crate::data_root::validate::validate_target(&current, &target, empty).map_err(|v| {
        Error::DataLocationInvalid {
            reason: format!("{v:?}"),
        }
    })?;

    // The redirect file itself lives at the default app-data dir and must
    // never be moved as part of the tree copy/delete — it is the bootstrap
    // pointer read *before* `DataRoot` is resolved.
    let redirect_name = std::ffi::OsString::from(REDIRECT_FILE_NAME);
    let skip = move |p: &std::path::Path| p.as_os_str() == redirect_name;

    let total = crate::data_root::migrate::dir_size(&current) as f64;
    let mut copied = 0u64;
    {
        let app_for_progress = app.clone();
        crate::data_root::migrate::copy_tree(
            &current,
            &target,
            &skip,
            &mut |c| {
                let _ = DataMigrationProgress {
                    copied_bytes: c as f64,
                    total_bytes: total,
                    phase: "copying".into(),
                }
                .emit(&app_for_progress);
            },
            &mut copied,
        )
        .map_err(|e| Error::DataLocationMigrationFailed {
            reason: e.to_string(),
        })?;
    }

    let _ = DataMigrationProgress {
        copied_bytes: copied as f64,
        total_bytes: total,
        phase: "verifying".into(),
    }
    .emit(&app);

    // Verify: the target's byte total must match what we copied. `copied`
    // already excludes the skipped redirect file, and `dir_size(&target)`
    // never contains it either (it was never copied there), so the two
    // must agree exactly regardless of whether `current == default`.
    let target_size = crate::data_root::migrate::dir_size(&target);
    if target_size != copied {
        return Err(Error::DataLocationMigrationFailed {
            reason: format!("verification failed: expected {copied} bytes, found {target_size}"),
        });
    }

    // Point the redirect at the new root ONLY after a complete, verified copy.
    let redirect_file =
        crate::paths::redirect_file(&app).map_err(|e| Error::io("<redirect>", e))?;
    match &new_path {
        Some(p) => crate::data_root::redirect::write(
            &redirect_file,
            &crate::data_root::redirect::Redirect {
                path: std::path::PathBuf::from(p),
            },
        )?,
        None => crate::data_root::redirect::remove(&redirect_file)?,
    }

    let _ = DataMigrationProgress {
        copied_bytes: copied as f64,
        total_bytes: total,
        phase: "deleting".into(),
    }
    .emit(&app);

    // Delete the old data: every top-level entry of `current` except the
    // redirect file (which only ever lives there when current == default).
    if let Ok(entries) = std::fs::read_dir(&current) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name == REDIRECT_FILE_NAME {
                continue;
            }
            let path = entry.path();
            let result = if path.is_dir() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            };
            if let Err(e) = result {
                if e.kind() != std::io::ErrorKind::NotFound {
                    return Err(Error::DataLocationMigrationFailed {
                        reason: format!("failed to remove old data at {}: {e}", path.display()),
                    });
                }
            }
        }
    }

    app.restart();
}
