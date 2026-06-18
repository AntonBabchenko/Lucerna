use super::*;

// --- Worlds tab (backlog #16) -----------------------------------

/// List singleplayer worlds in `instance_id`, newest-first by mtime.
/// Empty Vec for an instance with no `.minecraft/saves/` dir yet.
#[tauri::command]
#[specta::specta]
pub fn list_worlds(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<Vec<crate::worlds::World>, crate::error::Error> {
    crate::worlds::list_worlds(&app, &instance_id)
}

/// Lightweight world list (folder name + recency proxy) for the sidebar
/// Play-button dropdown. Cheaper than `list_worlds` — no size/backup walk —
/// so the UI can call it on every instance switch.
#[tauri::command]
#[specta::specta]
pub fn list_world_names(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<Vec<crate::worlds::WorldQuickEntry>, crate::error::Error> {
    crate::worlds::list_world_names(&app, &instance_id)
}

/// Create a new backup zip of `world_folder_name` under
/// `<instance>/backups/<world>/`. Returns the new Backup descriptor.
#[tauri::command]
#[specta::specta]
pub async fn backup_world(
    app: tauri::AppHandle,
    instance_id: String,
    world_folder_name: String,
) -> Result<crate::worlds::Backup, crate::error::Error> {
    crate::worlds::backup::backup_world(&app, &instance_id, &world_folder_name).await
}

/// List backups of `world_folder_name`, newest-first by parsed
/// filename timestamp. Empty Vec when none exist.
#[tauri::command]
#[specta::specta]
pub fn list_backups(
    app: tauri::AppHandle,
    instance_id: String,
    world_folder_name: String,
) -> Result<Vec<crate::worlds::Backup>, crate::error::Error> {
    crate::worlds::backup::list_backups(&app, &instance_id, &world_folder_name)
}

/// Restore a backup. Mode determines the semantic — see RestoreMode
/// docs. Returns the final folder name (= original for Replace,
/// suffixed for AsCopy).
#[tauri::command]
#[specta::specta]
pub async fn restore_backup(
    app: tauri::AppHandle,
    instance_id: String,
    world_folder_name: String,
    backup_filename: String,
    mode: crate::worlds::RestoreMode,
) -> Result<crate::worlds::RestoredWorld, crate::error::Error> {
    crate::worlds::restore::restore_backup(
        &app,
        &instance_id,
        &world_folder_name,
        &backup_filename,
        mode,
    )
    .await
}

/// Delete a single backup zip.
#[tauri::command]
#[specta::specta]
pub fn delete_backup(
    app: tauri::AppHandle,
    instance_id: String,
    world_folder_name: String,
    backup_filename: String,
) -> Result<(), crate::error::Error> {
    crate::worlds::backup::delete_backup(&app, &instance_id, &world_folder_name, &backup_filename)
}

/// Delete a world folder AND its backups subdir (cascade).
#[tauri::command]
#[specta::specta]
pub fn delete_world(
    app: tauri::AppHandle,
    instance_id: String,
    world_folder_name: String,
) -> Result<(), crate::error::Error> {
    crate::worlds::delete_world(&app, &instance_id, &world_folder_name)
}

/// Open `<instance>/.minecraft/saves/` in the OS file manager.
/// Idempotent — creates the dir if missing.
#[tauri::command]
#[specta::specta]
pub async fn open_saves_folder(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<(), crate::error::Error> {
    use tauri_plugin_opener::OpenerExt;
    let dir = crate::worlds::saves_dir(&app, &instance_id)?;
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| crate::error::Error::io(dir.display().to_string(), e))?;
    app.opener()
        .open_path(dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| crate::error::Error::io(dir.display().to_string(), format!("opener: {e}")))?;
    Ok(())
}

/// Open `<instance>/backups/<world>/` in the OS file manager.
/// Idempotent — creates the dir if missing (so the user can navigate
/// even before the first backup exists).
#[tauri::command]
#[specta::specta]
pub async fn open_backups_folder(
    app: tauri::AppHandle,
    instance_id: String,
    world_folder_name: String,
) -> Result<(), crate::error::Error> {
    use tauri_plugin_opener::OpenerExt;
    crate::worlds::fs::validate_segment(&world_folder_name)?;
    let dir = crate::worlds::backups_root(&app, &instance_id)?.join(&world_folder_name);
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| crate::error::Error::io(dir.display().to_string(), e))?;
    app.opener()
        .open_path(dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| crate::error::Error::io(dir.display().to_string(), format!("opener: {e}")))?;
    Ok(())
}
