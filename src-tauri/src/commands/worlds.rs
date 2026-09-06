// --- Worlds tab (backlog #16) -----------------------------------

/// List singleplayer worlds in `instance_id`, newest-first by mtime.
/// Empty Vec for an instance with no `.minecraft/saves/` dir yet.
#[tauri::command]
#[specta::specta]
pub async fn list_worlds(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<Vec<crate::worlds::World>, crate::error::Error> {
    // Async + spawn_blocking (mirrors `world_import` below): the domain walk
    // stats every file of every world for size+mtime plus a per-world backups
    // scan — seconds on a cold FS cache, and a sync command spends them on the
    // main thread with the window frozen.
    tokio::task::spawn_blocking(move || crate::worlds::list_worlds(&app, &instance_id))
        .await
        .map_err(|e| crate::error::Error::io("<list_worlds>", format!("join: {e}")))?
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
    // on exit — a mutation now is clobbered at best. One gate for every
    // instance, world and datapack writer (`instances::maintenance`): running,
    // starting, or under a maintenance claim — a world migration in flight —
    // all refuse with `InstanceBusy`.
    crate::instances::maintenance::write_allowed(&instance_id)?;
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
    // Same gate as `backup_world` — see there: running, starting, or under a
    // maintenance claim all refuse.
    crate::instances::maintenance::write_allowed(&instance_id)?;
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
    // A maintenance claim is different — a Move migration is relocating this
    // very set file by file (`worlds::backup::move_set_at`), and a zip deleted
    // under it would be counted and reported as "left behind" by a mover that
    // never saw it. Refuse for the claim alone, so deleting a backup while the
    // game runs stays allowed exactly as before. Direction: a claim that
    // cannot be observed does not exist (`maintenance_is_active` reads a set
    // under a poison-tolerant lock; there is no "could not tell" state).
    if crate::instances::maintenance::maintenance_is_active(&instance_id) {
        return Err(crate::error::Error::InstanceBusy);
    }
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
    // Same gate as `backup_world` — see there: running, starting, or under a
    // maintenance claim all refuse.
    crate::instances::maintenance::write_allowed(&instance_id)?;
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
    // Same gate as `backup_world` — see there: running, starting, or under a
    // maintenance claim all refuse.
    crate::instances::maintenance::write_allowed(&instance_id)?;
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
    // Same gate as `backup_world` — see there. The maintenance half matters
    // most here: a migration's own `.tmp-migrate-*` stage in this instance is
    // listed as stranded while the copy is in flight, and "recovering" it
    // would rename a half-copied tree into saves/ under a real name.
    crate::instances::maintenance::write_allowed(&instance_id)?;
    let saves = crate::worlds::saves_dir(&app, &instance_id)?;
    crate::worlds::orphans::recover_stranded_at(&saves, &dir_name)
}

// --- World migration between instances (spec 2026-08-16, v2) ---------------

/// Both endpoints of a migration, resolved the way every other
/// instance-scoped command resolves an id (`read_active_mc_and_loader` in
/// `commands/mod.rs`): an id absent from the instance listing is
/// `InstanceNotFound`, not the `Io` a bare `read_instance` reports for a
/// missing `instance.json`. One listing serves both ids.
///
/// `from == to` is unreachable from the UI (the target picker excludes the
/// source, spec §7) and is refused with a plain `Io` carrying the detail: no
/// dictionary key exists for a state the UI cannot produce (spec §3.4).
fn read_migration_endpoints(
    app: &tauri::AppHandle,
    from: &str,
    to: &str,
) -> Result<
    (
        crate::instances::schema::InstanceFile,
        crate::instances::schema::InstanceFile,
    ),
    crate::error::Error,
> {
    let all = crate::instances::list_instances_with_status(app)?;
    for id in [from, to] {
        if !all.iter().any(|i| i.id == id) {
            return Err(crate::error::Error::InstanceNotFound { id: id.to_string() });
        }
    }
    if from == to {
        return Err(crate::error::Error::io(
            "<world_migrate>",
            "source and target are the same instance",
        ));
    }
    let source = crate::instances::read_instance(app, from)?;
    let target = crate::instances::read_instance(app, to)?;
    Ok((source, target))
}

/// Every path the core needs, resolved from the two ids. `src_root` and
/// `dst_root` are the instance roots `datapacks::instance_root` hands the
/// datapack library and registry (`<instances>/<id>`, i.e. `paths::instance_dir`),
/// so the re-link step sees exactly the library the datapack commands see.
/// `saves_dir` / `backups_root` are the same helpers every world command uses.
fn migration_locations(
    app: &tauri::AppHandle,
    from: &str,
    to: &str,
    world_folder: &str,
    target_instance_name: &str,
) -> Result<crate::worlds::migrate::MigrationLocations, crate::error::Error> {
    Ok(crate::worlds::migrate::MigrationLocations {
        src_saves: crate::worlds::saves_dir(app, from)?,
        src_backups_root: crate::worlds::backups_root(app, from)?,
        src_root: crate::datapacks::instance_root(app, from)?,
        dst_saves: crate::worlds::saves_dir(app, to)?,
        dst_backups_root: crate::worlds::backups_root(app, to)?,
        dst_root: crate::datapacks::instance_root(app, to)?,
        world_folder: world_folder.to_string(),
        target_instance_name: target_instance_name.to_string(),
    })
}

/// Compatibility plan for moving or copying `world_folder` from
/// `from_instance` into `to_instance`: version verdict, loader pair, mods
/// the target lacks, and what will happen to each datapack. Read-only: no
/// maintenance claim and no running-instance gate — it reads `level.dat`,
/// both mod lists and both datapack libraries, and writes nothing. On
/// Windows a `level.dat` held open by a running game surfaces as
/// `WorldInUse` from the reader itself, which is the honest outcome; on
/// Linux and macOS the read simply succeeds. Async because the core awaits
/// the datapack registry and mod listings and runs its own file reads under
/// `spawn_blocking` (spec §3.4: the lexical heavy-sync guard cannot see
/// through the call, so review owns the offload).
#[tauri::command]
#[specta::specta]
pub async fn world_migration_plan(
    app: tauri::AppHandle,
    from_instance: String,
    world_folder: String,
    to_instance: String,
) -> Result<crate::worlds::migrate::MigrationPlan, crate::error::Error> {
    crate::data_root::reject_if_fallen_back(&app)?;
    // Defence in depth, same as `open_backups_folder`: the core re-validates
    // through `world_dir_at`, but a dot-name or path-shaped segment is refused
    // here before any listing or library read happens.
    crate::worlds::fs::validate_segment(&world_folder)?;
    let (source, target) = read_migration_endpoints(&app, &from_instance, &to_instance)?;
    let loc = migration_locations(
        &app,
        &from_instance,
        &to_instance,
        &world_folder,
        &target.name,
    )?;
    let versions_dir = crate::paths::versions_dir(&app)
        .map_err(|e| crate::error::Error::io("<versions_dir>", e))?;
    crate::worlds::migrate::plan_migration_at(
        &loc,
        &versions_dir,
        &source.mc_version,
        &target.mc_version,
        source.loader,
        target.loader,
    )
    .await
}

/// Move or copy `world_folder` from `from_instance` into `to_instance`
/// (spec §4). Both instances stay under a maintenance claim for the whole
/// command. The claims are taken BEFORE the running/starting check, and
/// `launch::start` re-checks `maintenance_is_active` after its own claim —
/// the Dekker pairing `DatapackUpdateGuard` uses — so a launch racing this
/// command is refused by exactly one side, never admitted by both. Progress
/// arrives on `on_progress`; the outcome states what actually happened to
/// the source, the datapacks and the backups, because after the point of
/// no return nothing is an error (spec §4.2). The core is async and runs
/// every blocking phase under `spawn_blocking` itself (spec §3.4: the
/// lexical heavy-sync guard cannot see through the call; review owns the
/// offload).
#[tauri::command]
#[specta::specta]
pub async fn world_migrate(
    app: tauri::AppHandle,
    from_instance: String,
    world_folder: String,
    to_instance: String,
    mode: crate::worlds::migrate::MigrationMode,
    on_progress: tauri::ipc::Channel<crate::worlds::migrate::MigrationProgress>,
) -> Result<crate::worlds::migrate::MigrationOutcome, crate::error::Error> {
    crate::data_root::reject_if_fallen_back(&app)?;
    // Defence in depth, same as `open_backups_folder`: the core re-validates
    // through `world_dir_at`, but a dot-name or path-shaped segment must never
    // reach the maintenance claim.
    crate::worlds::fs::validate_segment(&world_folder)?;
    let (source, target) = read_migration_endpoints(&app, &from_instance, &to_instance)?;
    let loc = migration_locations(
        &app,
        &from_instance,
        &to_instance,
        &world_folder,
        &target.name,
    )?;

    // Claim both slots first (spec §4.0). `None` means another migration —
    // or any other maintenance holder — already owns that id: `InstanceBusy`,
    // the restrictive answer. When the second claim fails, the `?` drops
    // `from_claim` on the way out, so the source slot is released (RAII).
    let from_claim = crate::instances::maintenance::maintenance_begin(&from_instance)
        .ok_or(crate::error::Error::InstanceBusy)?;
    let to_claim = crate::instances::maintenance::maintenance_begin(&to_instance)
        .ok_or(crate::error::Error::InstanceBusy)?;

    // Only now the running/starting check. `launch::start` claims its start
    // slot and then re-checks `maintenance_is_active`; with the claims already
    // held above, whichever side raced first is the one that proceeds. The
    // display NAME goes into the error — the user never sees an id.
    for (id, name, role) in [
        (
            &from_instance,
            &source.name,
            crate::error::MigrationRole::Source,
        ),
        (
            &to_instance,
            &target.name,
            crate::error::MigrationRole::Target,
        ),
    ] {
        if crate::launch::spawn::is_running(id) || crate::launch::spawn::is_starting(id) {
            // Early return drops both claims (RAII) before the error leaves.
            return Err(crate::error::Error::WorldMigrateInstanceRunning {
                instance_name: name.clone(),
                role,
            });
        }
    }

    let progress: std::sync::Arc<dyn Fn(crate::worlds::migrate::MigrationProgress) + Send + Sync> =
        std::sync::Arc::new(move |p| {
            // Same discipline as `clone_instance`: a failed send means the webview
            // dropped its listener (window closed, page reloaded). Progress is
            // advisory; the outcome still returns through the command result, and
            // a delivery failure must never abort a migration that is already
            // moving the user's world. Direction chosen: continue, because the
            // restrictive alternative (abort mid-flight) is the one that risks data.
            let _ = on_progress.send(p);
        });

    let result = crate::worlds::migrate::migrate_world_at(
        loc,
        mode,
        progress,
        crate::worlds::migrate::MigrationSeams::real(),
    )
    .await;

    // The claims live across the await above (named locals, not `let _`) and
    // are released here in reverse claim order on Ok and Err alike; every
    // earlier `?` / `return` released them through `Drop`.
    drop(to_claim);
    drop(from_claim);
    result
}
