use super::*;

// --- Playtime (backlog #21) --------------------------------------------

/// Read accumulated playtime stats for `instance_id`.
/// Returns zeros when no sessions have been recorded yet.
#[tauri::command]
#[specta::specta]
pub fn get_playtime(
    app: tauri::AppHandle,
    instance_id: String,
) -> crate::error::Result<crate::playtime::PlaytimeStats> {
    let root = crate::paths::instance_dir(&app, &instance_id)
        .map_err(|e| crate::error::Error::io("<instance_dir>", e))?;
    crate::playtime::get_stats_at(&root)
}
