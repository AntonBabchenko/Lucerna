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
    // A live JVM holds OS locks on the world tree, and level.dat is rewritten
    // on exit — a mutation now is clobbered at best. Same gate the datapack
    // commands use.
    crate::datapacks::guard::datapack_write_allowed(crate::launch::spawn::is_running(
        &instance_id,
    ))?;
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
    // A live JVM holds OS locks on the world tree, and level.dat is rewritten
    // on exit — a mutation now is clobbered at best. Same gate the datapack
    // commands use.
    crate::datapacks::guard::datapack_write_allowed(crate::launch::spawn::is_running(
        &instance_id,
    ))?;
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
    // No running guard: backups live outside the world tree the JVM holds.
    crate::worlds::backup::delete_backup(&app, &instance_id, &world_folder_name, &backup_filename)
}

/// Delete a world folder AND its backups subdir (cascade).
#[tauri::command]
#[specta::specta]
pub async fn delete_world(
    app: tauri::AppHandle,
    instance_id: String,
    world_folder_name: String,
) -> Result<(), crate::error::Error> {
    // A live JVM holds OS locks on the world tree, and level.dat is rewritten
    // on exit — a mutation now is clobbered at best. Same gate the datapack
    // commands use.
    crate::datapacks::guard::datapack_write_allowed(crate::launch::spawn::is_running(
        &instance_id,
    ))?;
    // Async + spawn_blocking (mirrors `world_import` below): the cascade is two
    // remove_dir_all's — a region-file-heavy world plus its backups dir — and a
    // sync command runs them on the main thread with the window frozen.
    tokio::task::spawn_blocking(move || {
        crate::worlds::delete_world(&app, &instance_id, &world_folder_name)
    })
    .await
    .map_err(|e| crate::error::Error::io("<delete_world>", format!("join: {e}")))?
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

/// Import a world into `instance_id`'s `saves/` from a local `.zip` or folder.
/// Returns the imported World (suffixed name on collision). Runs the blocking
/// extract/copy off the IPC thread.
#[tauri::command]
#[specta::specta]
pub async fn world_import(
    app: tauri::AppHandle,
    instance_id: String,
    source_path: String,
) -> Result<crate::worlds::World, crate::error::Error> {
    // A live JVM holds OS locks on the world tree, and level.dat is rewritten
    // on exit — a mutation now is clobbered at best. Same gate the datapack
    // commands use.
    crate::datapacks::guard::datapack_write_allowed(crate::launch::spawn::is_running(
        &instance_id,
    ))?;
    crate::data_root::reject_if_fallen_back(&app)?;
    let saves = crate::worlds::saves_dir(&app, &instance_id)?;
    let source = std::path::PathBuf::from(source_path);
    tokio::task::spawn_blocking(move || crate::worlds::import::import_into_saves(&saves, &source))
        .await
        .map_err(|e| crate::error::Error::io("<world-import>", format!("join: {e}")))?
}

/// Backup sets whose world is gone from `saves/`.
///
/// Async on purpose: a sync `#[tauri::command]` doing `read_dir` plus per-entry
/// `metadata` across `backups/*/` runs on the main thread and freezes the
/// window, and `tauri-specta` renders sync and async identically — so a bindings
/// diff would not catch the mistake.
#[tauri::command]
#[specta::specta]
pub async fn list_orphaned_backup_worlds(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<Vec<crate::worlds::orphans::OrphanedBackupSet>, crate::error::Error> {
    let saves = crate::worlds::saves_dir(&app, &instance_id)?;
    let backups = crate::worlds::backups_root(&app, &instance_id)?;
    tokio::task::spawn_blocking(move || {
        crate::worlds::orphans::orphaned_backup_sets_at(&saves, &backups)
    })
    .await
    .map_err(|e| crate::error::Error::io("<orphan-scan>", format!("join: {e}")))
}

/// Worlds a restore could not put back. Invisible to every other listing.
#[tauri::command]
#[specta::specta]
pub async fn list_stranded_worlds(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<Vec<crate::worlds::orphans::StrandedWorld>, crate::error::Error> {
    let saves = crate::worlds::saves_dir(&app, &instance_id)?;
    tokio::task::spawn_blocking(move || crate::worlds::orphans::stranded_worlds_at(&saves))
        .await
        .map_err(|e| crate::error::Error::io("<stranded-scan>", format!("join: {e}")))
}

/// Rename a stranded world back into place. Returns the recovered folder name.
#[tauri::command]
#[specta::specta]
pub async fn recover_stranded_world(
    app: tauri::AppHandle,
    instance_id: String,
    dir_name: String,
) -> Result<String, crate::error::Error> {
    // A live JVM holds OS locks on the saves tree — same gate the sibling
    // mutating world commands use.
    crate::datapacks::guard::datapack_write_allowed(crate::launch::spawn::is_running(
        &instance_id,
    ))?;
    let saves = crate::worlds::saves_dir(&app, &instance_id)?;
    crate::worlds::orphans::recover_stranded_at(&saves, &dir_name)
}
