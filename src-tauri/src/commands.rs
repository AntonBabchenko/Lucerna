use specta::Type;

#[derive(Debug, serde::Serialize, Type)]
pub struct Greeting {
    pub message: String,
}

const MAX_INSTANCE_NAME_LEN: u32 = 32;

/// Validate instance name at the IPC boundary.
///
/// Reasons live as typed Error variants so the UI doesn't string-parse.
/// Count uses unicode scalar values (chars), not bytes — a 32-char
/// cyrillic name is 64 bytes but 32 graphemes, and that's fine.
fn validate_instance_name(name: &str) -> Result<(), crate::error::Error> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(crate::error::Error::InstanceNameEmpty);
    }
    let count = trimmed.chars().count() as u32;
    if count > MAX_INSTANCE_NAME_LEN {
        return Err(crate::error::Error::InstanceNameTooLong {
            max: MAX_INSTANCE_NAME_LEN,
            actual: count,
        });
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn greet(name: String) -> Greeting {
    Greeting {
        message: format!("Hello, {name}! — Lucerna is alive."),
    }
}

/// List all stored accounts.
#[tauri::command]
#[specta::specta]
pub fn list_accounts(
    app: tauri::AppHandle,
) -> Result<Vec<crate::accounts::store::Account>, crate::error::Error> {
    crate::accounts::list_accounts(&app)
}

/// Currently active account, or None if no account is set.
#[tauri::command]
#[specta::specta]
pub fn get_active_account(
    app: tauri::AppHandle,
) -> Result<Option<crate::accounts::store::Account>, crate::error::Error> {
    crate::accounts::get_active_account(&app)
}

/// Set the active account by id. Errors AccountNotSet if id is unknown.
#[tauri::command]
#[specta::specta]
pub fn set_active_account(app: tauri::AppHandle, id: String) -> Result<(), crate::error::Error> {
    crate::accounts::set_active_account(&app, &id)
}

/// Remove an account. If it was active, the next account becomes active;
/// if none remain, active_id becomes None.
#[tauri::command]
#[specta::specta]
pub fn remove_account(app: tauri::AppHandle, id: String) -> Result<(), crate::error::Error> {
    crate::accounts::remove_account(&app, &id)
}

/// Add an offline account. Idempotent — same name produces same UUID.
#[tauri::command]
#[specta::specta]
pub fn add_offline_account(
    app: tauri::AppHandle,
    name: String,
) -> Result<crate::accounts::store::Account, crate::error::Error> {
    crate::accounts::add_offline_account(&app, &name)
}

/// Fetch the Mojang version manifest. Cached for 5 minutes — repeated
/// calls within that window are zero-network.
#[tauri::command]
#[specta::specta]
pub async fn list_versions() -> Result<Vec<crate::versions::VersionEntry>, crate::error::Error> {
    crate::versions::list_manifest().await
}

/// Install a Minecraft version: downloads the per-version JSON,
/// libraries, assets, and client.jar. Emits `installProgress` events
/// throughout. Idempotent — files already present with matching SHA-1
/// are skipped.
#[tauri::command]
#[specta::specta]
pub async fn install_version(
    app: tauri::AppHandle,
    version_id: String,
) -> Result<(), crate::error::Error> {
    crate::versions::install_version(&version_id, &app).await
}

/// Install (idempotently) the given instance's version. Does NOT launch
/// — the UI shows an Install button when the instance is not ready and
/// a Play button once it is. Emits `installProgress` during the run.
/// Resolves version+loader from `instance.json` server-side.
#[tauri::command]
#[specta::specta]
pub async fn install_instance(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<(), crate::error::Error> {
    let effective_id = resolve_instance_effective_id(&app, &instance_id)?;
    crate::versions::install_version(&effective_id, &app).await
}

/// Launch the given instance. Assumes it is already installed (the UI
/// only enables Play when `instance.ready == true`, which checks the
/// effective version's profile JSON + parent MC client jar). Does NOT
/// re-run install_version — earlier attempts to add it as a safety net
/// re-hashed every library and asset on every click, surfacing as a
/// visible "Downloading…" flash. If launch fails on missing files, the
/// error surfaces and the user can click Install (Manage → re-pick the
/// loader, or the Install button reappears once ready_status flips).
#[tauri::command]
#[specta::specta]
pub async fn launch_instance(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<u32, crate::error::Error> {
    let effective_id = resolve_instance_effective_id(&app, &instance_id)?;
    let json_path = crate::paths::instance_json(&app, &instance_id)
        .map_err(|e| crate::error::Error::io("<instance_json>", e))?;
    let instance = crate::instances::store::read_instance_json(&json_path)?;
    let account =
        crate::accounts::get_active_account(&app)?.ok_or(crate::error::Error::AccountNotSet)?;
    crate::launch::start(&instance, &effective_id, &account, &app).await
}

/// Shared prelude for install_instance and launch_instance: confirm the
/// instance exists, read its JSON, and resolve the effective version id.
/// Returns the version id only; callers that need the full Instance read
/// it again (cheap; same file on disk).
fn resolve_instance_effective_id(
    app: &tauri::AppHandle,
    instance_id: &str,
) -> Result<String, crate::error::Error> {
    let all = crate::instances::list_instances_with_status(app)?;
    if !all.iter().any(|i| i.id == instance_id) {
        return Err(crate::error::Error::InstanceNotFound {
            id: instance_id.to_string(),
        });
    }
    let json_path = crate::paths::instance_json(app, instance_id)
        .map_err(|e| crate::error::Error::io("<instance_json>", e))?;
    let instance = crate::instances::store::read_instance_json(&json_path)?;
    crate::instances::status::effective_version_id(&instance)
        .ok_or(crate::error::Error::NoVersionSelected)
}

/// Kill the running Minecraft process if any. Idempotent.
#[tauri::command]
#[specta::specta]
pub fn stop_minecraft() -> Result<(), crate::error::Error> {
    crate::launch::stop()
}

/// List every log file under `instance_id`'s three documented roots.
/// Sorted by mtime descending.
#[tauri::command]
#[specta::specta]
pub fn list_log_files(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<Vec<crate::logs::files::LogFileMeta>, crate::error::Error> {
    crate::logs::files::list_log_files(&app, &instance_id)
}

/// Read up to `max_bytes` of a log file. `max_bytes` is clamped to
/// `[64 KB, 100 MB]`; `0` becomes the 5 MB default. `path` must be
/// under one of SOME instance's allowed log roots — anything else is
/// rejected with `Error::Io`.
#[tauri::command]
#[specta::specta]
pub fn read_log_file(
    app: tauri::AppHandle,
    path: String,
    max_bytes: f64,
) -> Result<String, crate::error::Error> {
    let all = crate::instances::list_instances_with_status(&app)?;
    let mut roots: Vec<std::path::PathBuf> = Vec::new();
    for inst in &all {
        let mut r = crate::logs::files::allowed_roots(&app, &inst.id)?;
        roots.append(&mut r);
    }
    let path = std::path::PathBuf::from(&path);
    crate::logs::files::assert_under_allowed_roots(&path, &roots)?;
    let cap = if !max_bytes.is_finite() || max_bytes < 0.0 {
        0
    } else {
        max_bytes as u64
    };
    crate::logs::read::read_with_cap(&path, cap)
}

/// Newest crash report (if any) for `instance_id`. Used by the UI to
/// show a banner on non-zero MC exit.
#[tauri::command]
#[specta::specta]
pub fn latest_crash(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<Option<crate::logs::files::CrashReport>, crate::error::Error> {
    crate::logs::files::latest_crash(&app, &instance_id)
}

/// Run the diagnoser over `path`. Returns `Ok(None)` when no known
/// pattern matches or the file is empty/too short. Path must be
/// under one of `instance_id`'s allowed log roots — anything else
/// is rejected with `Error::Io` (mirrors `read_log_file` semantics
/// but scoped to a single instance rather than all instances).
#[tauri::command]
#[specta::specta]
pub async fn diagnose_log(
    app: tauri::AppHandle,
    instance_id: String,
    path: String,
) -> Result<Option<crate::logs::diagnose::Diagnosis>, crate::error::Error> {
    let roots = crate::logs::files::allowed_roots(&app, &instance_id)?;
    let path_buf = std::path::PathBuf::from(&path);
    crate::logs::files::assert_under_allowed_roots(&path_buf, &roots)?;
    crate::logs::diagnose::diagnose(&path_buf).await
}

/// Anonymise a log body and upload it to mclo.gs. Returns the
/// shareable URL. Frontend caller is the Logs popover "Share"
/// button. Anonymisation runs server-side so the frontend can't
/// accidentally bypass it.
#[tauri::command]
#[specta::specta]
pub async fn share_log_to_mclogs(content: String) -> crate::error::Result<String> {
    let anon = crate::logs::share::anonymise(&content);
    crate::logs::share::upload_to_mclogs(&anon).await
}

/// Ensure `<instance>/.minecraft/mods/` exists, then open it in the OS
/// file manager. Idempotent — safe to click repeatedly. Vanilla MC
/// does not load mods; the UI carries a caveat below the button.
#[tauri::command]
#[specta::specta]
pub async fn open_mods_folder(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<(), crate::error::Error> {
    use tauri_plugin_opener::OpenerExt;
    let dir = crate::paths::mods_dir(&app, &instance_id)
        .map_err(|e| crate::error::Error::io("<mods_dir>", e))?;
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| crate::error::Error::io(dir.display().to_string(), e))?;
    app.opener()
        .open_path(dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| crate::error::Error::io(dir.display().to_string(), format!("opener: {e}")))?;
    Ok(())
}

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

/// Open the parent directory of `path` (a log file path the popover is
/// currently viewing) in the OS file manager. The path is validated
/// against every instance's allowed log roots (mirrors `read_log_file`
/// semantics) so a crafted path cannot escape the log directories. This
/// lets a user click "Open folder" on either an MC game log
/// (`.minecraft/logs/`), a crash report (`.minecraft/crash-reports/`),
/// or a launcher capture (`<instance>/logs/`) and land in the exact dir
/// that contains the file they're looking at.
#[tauri::command]
#[specta::specta]
pub async fn open_log_folder(
    app: tauri::AppHandle,
    path: String,
) -> Result<(), crate::error::Error> {
    use tauri_plugin_opener::OpenerExt;
    let path = std::path::PathBuf::from(&path);
    let all = crate::instances::list_instances_with_status(&app)?;
    let mut roots: Vec<std::path::PathBuf> = Vec::new();
    for inst in &all {
        let mut r = crate::logs::files::allowed_roots(&app, &inst.id)?;
        roots.append(&mut r);
    }
    crate::logs::files::assert_under_allowed_roots(&path, &roots)?;
    let dir = path.parent().ok_or_else(|| {
        crate::error::Error::io(path.display().to_string(), "log file has no parent dir")
    })?;
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

/// List Fabric loader versions compatible with `mc_id`. Sorted
/// newest-first by build. Empty list → `Error::LoaderUnavailable`.
/// Cached 5 minutes per `mc_id`.
#[tauri::command]
#[specta::specta]
pub async fn list_fabric_loaders(
    mc_id: String,
) -> Result<Vec<crate::versions::LoaderVersion>, crate::error::Error> {
    crate::versions::loaders::list_loaders(crate::versions::Loader::Fabric, &mc_id).await
}

/// List Quilt loader versions compatible with `mc_id`. Same semantics
/// as `list_fabric_loaders`. Stability is inferred from the version
/// string (Quilt meta does not expose a `stable` flag).
#[tauri::command]
#[specta::specta]
pub async fn list_quilt_loaders(
    mc_id: String,
) -> Result<Vec<crate::versions::LoaderVersion>, crate::error::Error> {
    crate::versions::loaders::list_loaders(crate::versions::Loader::Quilt, &mc_id).await
}

/// List Forge loader versions compatible with `mc_id`. Cached
/// 5 minutes per MC version. Empty list → `LoaderUnavailable`.
#[tauri::command]
#[specta::specta]
pub async fn list_forge_loaders(
    mc_id: String,
) -> Result<Vec<crate::versions::LoaderVersion>, crate::error::Error> {
    crate::versions::loaders::list_loaders(crate::versions::Loader::Forge, &mc_id).await
}

/// List NeoForge loader versions compatible with `mc_id`. Cached
/// 5 minutes per MC version. Empty list → `LoaderUnavailable`.
#[tauri::command]
#[specta::specta]
pub async fn list_neoforge_loaders(
    mc_id: String,
) -> Result<Vec<crate::versions::LoaderVersion>, crate::error::Error> {
    crate::versions::loaders::list_loaders(crate::versions::Loader::NeoForge, &mc_id).await
}

/// All instances on disk with precomputed `ready` status. Sorted
/// oldest-first by `created_unix_ms`.
#[tauri::command]
#[specta::specta]
pub fn list_instances(
    app: tauri::AppHandle,
) -> Result<Vec<crate::instances::schema::InstanceWithStatus>, crate::error::Error> {
    crate::instances::list_instances_with_status(&app)
}

/// Currently active instance, or `None` if no instances exist.
#[tauri::command]
#[specta::specta]
pub fn get_active_instance(
    app: tauri::AppHandle,
) -> Result<Option<crate::instances::schema::InstanceWithStatus>, crate::error::Error> {
    crate::instances::get_active_instance(&app)
}

/// Set the active instance by id. Errors `InstanceNotFound` if id is unknown.
#[tauri::command]
#[specta::specta]
pub fn set_active_instance(app: tauri::AppHandle, id: String) -> Result<(), crate::error::Error> {
    crate::instances::set_active_instance(&app, &id)
}

/// Create a new instance. Generates a UUID, mkdirs `.minecraft/`,
/// writes `instance.json`. Defaults: `max_heap_mb=2048`, `extra_jvm_args=""`.
#[tauri::command]
#[specta::specta]
pub fn create_instance(
    app: tauri::AppHandle,
    name: String,
    mc_version: String,
    loader: crate::instances::schema::LoaderKind,
    loader_version: Option<String>,
) -> Result<crate::instances::schema::InstanceWithStatus, crate::error::Error> {
    validate_instance_name(&name)?;
    crate::instances::create_instance(
        &app,
        name,
        mc_version,
        loader,
        loader_version,
        None,
        None,
        None,
        None,
        None,
    )
}

/// Delete an instance. If it was active, auto-switches to oldest remaining.
/// Errors `LastInstance` if it's the only one left.
#[tauri::command]
#[specta::specta]
pub fn delete_instance(app: tauri::AppHandle, id: String) -> Result<(), crate::error::Error> {
    crate::instances::delete_instance(&app, &id)
}

#[tauri::command]
#[specta::specta]
pub fn set_instance_name(
    app: tauri::AppHandle,
    id: String,
    name: String,
) -> Result<crate::instances::schema::InstanceWithStatus, crate::error::Error> {
    validate_instance_name(&name)?;
    crate::instances::set_instance_name(&app, &id, name)
}

/// Change the MC version of an instance, re-resolving the loader version for
/// the new MC. If the loader is not supported on the new MC version, the
/// instance is automatically reset to Vanilla and the report reflects that.
/// Returns the updated instance plus a `LoaderOutcome` describing what changed.
#[tauri::command]
#[specta::specta]
pub async fn change_instance_mc(
    app: tauri::AppHandle,
    id: String,
    mc: String,
) -> crate::error::Result<crate::instances::McChangeReport> {
    use crate::instances::schema::LoaderKind;
    use crate::instances::{self, LoaderDecision, LoaderOutcome};
    use crate::versions::loaders::{list_loaders, Loader};

    let current = instances::read_instance(&app, &id)?;
    let loader = current.loader;

    if loader == LoaderKind::Vanilla {
        // Vanilla instance: nothing to re-resolve, just set the MC version.
        let instance = instances::set_instance_version(&app, &id, mc)?;
        return Ok(crate::instances::McChangeReport {
            instance,
            loader_outcome: LoaderOutcome::Unchanged,
        });
    }

    let as_loader = match loader {
        LoaderKind::Fabric => Loader::Fabric,
        LoaderKind::Quilt => Loader::Quilt,
        LoaderKind::Forge => Loader::Forge,
        LoaderKind::NeoForge => Loader::NeoForge,
        // SAFETY: guarded by the `loader == Vanilla` branch above
        LoaderKind::Vanilla => unreachable!(),
    };
    // Resolve over the network BEFORE any write, then apply MC + loader +
    // loader_version in a SINGLE atomic mutate — a torn write must never leave
    // the new MC paired with the old loader version (that is the bug).
    match instances::decide_loader(loader, list_loaders(as_loader, &mc).await) {
        LoaderDecision::Use(v) => {
            let instance = instances::apply_mc_and_loader(&app, &id, mc, loader, Some(v.clone()))?;
            Ok(crate::instances::McChangeReport {
                instance,
                loader_outcome: LoaderOutcome::LoaderUpdated { loader, version: v },
            })
        }
        LoaderDecision::ResetToVanilla => {
            let instance =
                instances::apply_mc_and_loader(&app, &id, mc, LoaderKind::Vanilla, None)?;
            Ok(crate::instances::McChangeReport {
                instance,
                loader_outcome: LoaderOutcome::LoaderResetToVanilla {
                    previous_loader: loader,
                },
            })
        }
        LoaderDecision::Error(e) => Err(e),
    }
}

#[tauri::command]
#[specta::specta]
pub fn set_instance_loader(
    app: tauri::AppHandle,
    id: String,
    loader: crate::instances::schema::LoaderKind,
    loader_version: Option<String>,
) -> Result<crate::instances::schema::InstanceWithStatus, crate::error::Error> {
    crate::instances::set_instance_loader(&app, &id, loader, loader_version)
}

#[tauri::command]
#[specta::specta]
pub fn set_instance_memory(
    app: tauri::AppHandle,
    id: String,
    max_heap_mb: u32,
) -> Result<crate::instances::schema::InstanceWithStatus, crate::error::Error> {
    crate::instances::set_instance_memory(&app, &id, max_heap_mb)
}

#[tauri::command]
#[specta::specta]
pub fn set_instance_jvm_args(
    app: tauri::AppHandle,
    id: String,
    args: String,
) -> Result<crate::instances::schema::InstanceWithStatus, crate::error::Error> {
    crate::instances::set_instance_jvm_args(&app, &id, args)
}

/// Clear all modpack provenance fields (`mrpack_*`) from an instance,
/// detaching it from its origin pack. Safe to call on non-pack instances
/// (idempotent no-op). The UI offers this when the user changes MC or
/// loader on a pack-imported instance.
#[tauri::command]
#[specta::specta]
pub fn detach_instance_pack(
    app: tauri::AppHandle,
    id: String,
) -> Result<crate::instances::schema::InstanceWithStatus, crate::error::Error> {
    crate::instances::detach_instance_pack(&app, &id)
}

/// Ensure `<instance>/.minecraft/` exists, then open it in the OS
/// file manager.
#[tauri::command]
#[specta::specta]
pub async fn open_instance_folder(
    app: tauri::AppHandle,
    id: String,
) -> Result<(), crate::error::Error> {
    crate::instances::open_instance_folder(&app, &id).await
}

// =========================================================================
// Mod browser commands (v0.5.0 sub-feature 3)
// =========================================================================

use crate::mods::curseforge::CurseForgeClient;
use crate::mods::modrinth::ModrinthClient;
use crate::mods::platform::*;

fn platform_for(source: ModSource) -> Box<dyn ModPlatform> {
    match source {
        ModSource::Modrinth => Box::new(ModrinthClient::new()),
        ModSource::Curseforge => Box::new(CurseForgeClient::new()),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn mods_search(query: ModSearchQuery) -> crate::error::Result<ModSearchPage> {
    platform_for(query.source).search(&query).await
}

#[tauri::command]
#[specta::specta]
pub async fn mods_project(
    source: ModSource,
    project_id: String,
) -> crate::error::Result<ModProject> {
    crate::mods::project_cache::get_or_fetch(source, &project_id, || async {
        platform_for(source).project(&project_id).await
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn mods_versions(
    source: ModSource,
    project_id: String,
    mc_version: Option<String>,
    loader: Option<LoaderKind>,
) -> crate::error::Result<Vec<ModVersion>> {
    platform_for(source)
        .versions(&project_id, mc_version.as_deref(), loader)
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn mods_resolve_deps(
    version: ModVersion,
    mc_version: String,
    loader: LoaderKind,
) -> crate::error::Result<ResolvedDeps> {
    platform_for(version.source)
        .resolve_deps(&version, &mc_version, loader)
        .await
}

// =========================================================================
// Transitive-dependency resolver adapter helpers
// =========================================================================

use crate::mods::deps::{FetchedDeps, ProjectKey, ResolvedNode};

/// Backend loader-project slugs (mirrors the frontend LOADER_SLUGS in
/// ModBrowseView.svelte). A dep whose project slug is one of these is a
/// loader — managed at the instance level, never installed as a mod jar.
const LOADER_SLUGS: &[&str] = &[
    "neoforge", "forge", "fabric", "fabric-loader", "quilt", "quilt-loader", "minecraft",
];

/// Is `version`'s project a loader? Looks up the project slug, memoized in
/// `loader_cache`. One `project()` call per distinct project, amortized.
/// Fails open: an un-classifiable project is treated as a normal mod.
async fn is_loader_project(
    platform: &dyn ModPlatform,
    loader_cache: &mut std::collections::HashMap<ProjectKey, bool>,
    v: &ModVersion,
) -> bool {
    let key = ProjectKey::of_version(v);
    if let Some(hit) = loader_cache.get(&key) {
        return *hit;
    }
    let is_loader = match platform.project(&v.project_id).await {
        Ok(p) => p
            .summary
            .slug
            .as_deref()
            .map(|s| LOADER_SLUGS.contains(&s.to_ascii_lowercase().as_str()))
            .unwrap_or(false),
        Err(_) => false,
    };
    loader_cache.insert(key, is_loader);
    is_loader
}

/// Build `FetchedDeps` for one version: call the platform's one-level
/// `resolve_deps` and classify each resolved dep as loader / normal.
async fn fetch_one_level(
    platform: &dyn ModPlatform,
    loader_cache: &mut std::collections::HashMap<ProjectKey, bool>,
    v: &ModVersion,
    mc: &str,
    loader: LoaderKind,
) -> Result<FetchedDeps, crate::error::Error> {
    let rd = platform.resolve_deps(v, mc, loader).await?;
    let mut required = Vec::new();
    for r in rd.required {
        let is_loader = is_loader_project(platform, loader_cache, &r.version).await;
        required.push(ResolvedNode { version: r.version, is_loader });
    }
    let mut optional = Vec::new();
    for o in rd.optional {
        let is_loader = is_loader_project(platform, loader_cache, &o.version).await;
        optional.push(ResolvedNode { version: o.version, is_loader });
    }
    Ok(FetchedDeps {
        required,
        optional,
        incompatible: rd.incompatible,
        unresolvable: rd.unresolvable,
    })
}

// =========================================================================
// Mod install / list / disable / enable / uninstall (v0.5.0 sub-feature 3)
// =========================================================================

use serde::Serialize;
use std::path::PathBuf;
use tauri_specta::Event;

/// Streamed progress for a single mod install operation. Tagged union so
/// the UI can switch on `phase` and show a progress bar / spinner.
#[derive(Debug, Clone, Serialize, Type, Event)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum ModInstallProgress {
    Downloading {
        instance_id: String,
        project_id: String,
        /// f64 not u64 — specta forbids BigInt-style exports.
        bytes_done: f64,
        bytes_total: Option<f64>,
    },
    Verifying {
        instance_id: String,
        project_id: String,
        bytes_done: f64,
    },
    Copying {
        instance_id: String,
        project_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Type, Event)]
pub struct ModInstalled {
    pub instance_id: String,
    pub sha1: String,
    pub filename: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Type, Event)]
pub struct ModUninstalled {
    pub instance_id: String,
    pub sha1: String,
}

#[derive(Debug, Clone, Serialize, Type, Event)]
pub struct ModToggle {
    pub instance_id: String,
    pub sha1: String,
    /// True iff the mod is now enabled. UI uses this to drive the toggle
    /// switch without re-querying `mods_list_installed`.
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Type, Event)]
pub struct ModInstallFailed {
    pub instance_id: String,
    pub project_id: String,
    pub error: crate::error::Error,
}

/// Per-instance root, e.g. `<app_data>/instances/<id>/`. The mod install
/// pipeline writes under `{root}/.minecraft/mods/` and tracks state in
/// `{root}/lucerna/installed-mods.json`.
fn instance_root(
    app: &tauri::AppHandle,
    instance_id: &str,
) -> Result<PathBuf, crate::error::Error> {
    crate::paths::instance_dir(app, instance_id)
        .map_err(|e| crate::error::Error::io("<instance_dir>", e))
}

/// Launcher app-data directory — host of the shared mod cache.
fn data_dir(app: &tauri::AppHandle) -> Result<PathBuf, crate::error::Error> {
    crate::paths::app_dir(app).map_err(|e| crate::error::Error::io("<app_dir>", e))
}

/// Read the active MC version + loader for an instance from
/// `instance.json`. Returns `InstanceNotFound` if the file is missing.
fn read_active_mc_and_loader(
    app: &tauri::AppHandle,
    instance_id: &str,
) -> Result<(String, LoaderKind), crate::error::Error> {
    let all = crate::instances::list_instances_with_status(app)?;
    if !all.iter().any(|i| i.id == instance_id) {
        return Err(crate::error::Error::InstanceNotFound {
            id: instance_id.to_string(),
        });
    }
    let json_path = crate::paths::instance_json(app, instance_id)
        .map_err(|e| crate::error::Error::io("<instance_json>", e))?;
    let instance = crate::instances::store::read_instance_json(&json_path)?;
    Ok((instance.mc_version, instance.loader))
}

/// Resolve a `VersionRef` to a full `ModVersion` by querying the platform
/// for the project's available versions (filtered by MC + loader).
async fn find_version(
    platform: &mut Box<dyn ModPlatform>,
    vr: &VersionRef,
    mc: &str,
    loader: LoaderKind,
) -> crate::error::Result<ModVersion> {
    let vs = platform
        .versions(&vr.project_id, Some(mc), Some(loader))
        .await?;
    vs.into_iter()
        .find(|v| v.version_id == vr.version_id)
        .ok_or_else(|| crate::error::Error::ModsNotFound {
            platform: match vr.source {
                ModSource::Modrinth => "modrinth",
                ModSource::Curseforge => "curseforge",
            }
            .into(),
        })
}

fn version_matches(v: &ModVersion, vr: &VersionRef) -> bool {
    v.source == vr.source && v.project_id == vr.project_id && v.version_id == vr.version_id
}

/// Install `primary` plus the TRANSITIVE required closure of the primary and
/// each chosen optional, deduped, installed deps-first, then primary, then
/// chosen optionals. Emits:
///   - `mod-install-progress` repeatedly during downloads,
///   - `mod-installed` once per mod that lands successfully,
///   - `mod-install-failed` if any single install errors (the run halts
///     after the first failure; previously-installed mods are kept).
///
/// Returns an `InstallSummary` so the UI can show which dependencies were
/// pulled in automatically.
///
/// `primary` is a `VersionRef` (not a full `ModVersion`) so the caller
/// doesn't need to keep a heavy struct around — we re-fetch from the
/// platform here. This also re-validates against the live API.
#[tauri::command]
#[specta::specta]
pub async fn mods_install_with_deps(
    app: tauri::AppHandle,
    instance_id: String,
    primary: VersionRef,
    optional_deps: Vec<VersionRef>,
) -> crate::error::Result<crate::mods::platform::InstallSummary> {
    use std::sync::Arc;
    use crate::mods::deps::{resolve_closure, ProjectKey};

    let inst_root = instance_root(&app, &instance_id)?;
    let dd = data_dir(&app)?;
    let (mc_version, loader) = read_active_mc_and_loader(&app, &instance_id)?;

    // Two handles: Box for find_version calls, Arc for make_fetch closure.
    let mut platform_box = platform_for(primary.source);
    let primary_v = find_version(&mut platform_box, &primary, &mc_version, loader).await?;

    // Build the set of already-installed mods so resolve_closure can prune them.
    let installed: std::collections::HashSet<ProjectKey> =
        crate::mods::installed::list(&inst_root)
            .await?
            .into_iter()
            .filter_map(|m| match (m.source, m.project_id) {
                (Some(ModSource::Modrinth), Some(pid)) => Some(ProjectKey::Modrinth(pid)),
                (Some(ModSource::Curseforge), Some(pid)) => {
                    pid.parse().ok().map(ProjectKey::Curseforge)
                }
                _ => None,
            })
            .collect();

    // Shared Arc platform + loader-slug cache for the make_fetch factory.
    let platform_arc: Arc<dyn crate::mods::platform::ModPlatform> =
        platform_for(primary.source).into();
    let loader_cache = Arc::new(tokio::sync::Mutex::new(
        std::collections::HashMap::<ProjectKey, bool>::new(),
    ));

    // Factory: produce a fresh fetch closure that shares the Arc'd platform + cache.
    let make_fetch = || {
        let platform = platform_arc.clone();
        let loader_cache = loader_cache.clone();
        let mc = mc_version.clone();
        move |v: ModVersion| {
            let platform = platform.clone();
            let loader_cache = loader_cache.clone();
            let mc = mc.clone();
            async move {
                let mut cache = loader_cache.lock().await;
                fetch_one_level(platform.as_ref(), &mut cache, &v, &mc, loader).await
            }
        }
    };

    // Progress callback closes over a clone of the AppHandle and the
    // primary's project_id (used to tag every progress event so the UI
    // can route the bar to the right card). Dep installs reuse the same
    // project_id tag — the UI shows them as part of the same operation.
    let app_for_progress = app.clone();
    let instance_id_for_progress = instance_id.clone();
    let project_id_for_progress = primary_v.project_id.clone();
    let prog: crate::mods::install::ProgressFn = Box::new(move |phase, done, total| {
        let payload = match phase {
            crate::mods::install::ModInstallPhase::Downloading => ModInstallProgress::Downloading {
                instance_id: instance_id_for_progress.clone(),
                project_id: project_id_for_progress.clone(),
                bytes_done: done as f64,
                bytes_total: total.map(|t| t as f64),
            },
            crate::mods::install::ModInstallPhase::Verifying => ModInstallProgress::Verifying {
                instance_id: instance_id_for_progress.clone(),
                project_id: project_id_for_progress.clone(),
                bytes_done: done as f64,
            },
            crate::mods::install::ModInstallPhase::Copying => ModInstallProgress::Copying {
                instance_id: instance_id_for_progress.clone(),
                project_id: project_id_for_progress.clone(),
            },
        };
        let _ = payload.emit(&app_for_progress);
    });

    // Compute the primary's transitive required closure.
    let primary_required =
        resolve_closure(std::slice::from_ref(&primary_v), &installed, make_fetch())
            .await?
            .required;

    // For each chosen optional: resolve it to a full version, then compute its
    // transitive sub-closure (excluding installed + already-collected deps).
    let mut dep_versions: Vec<ModVersion> = primary_required;
    let mut chosen_optionals: Vec<ModVersion> = Vec::new();
    for opt in &optional_deps {
        let ov = find_version(&mut platform_box, opt, &mc_version, loader).await?;
        let mut excl = installed.clone();
        for v in &dep_versions {
            excl.insert(ProjectKey::of_version(v));
        }
        for v in &chosen_optionals {
            excl.insert(ProjectKey::of_version(v));
        }
        excl.insert(ProjectKey::of_version(&ov));
        let sub = resolve_closure(std::slice::from_ref(&ov), &excl, make_fetch()).await?;
        dep_versions.extend(sub.required);
        chosen_optionals.push(ov);
    }
    let dep_versions = dedup_versions(dep_versions.into_iter());

    // Install sequence: required deps first, then primary, then chosen optionals.
    let mut install_seq = dep_versions.clone();
    install_seq.push(primary_v.clone());
    install_seq.extend(chosen_optionals.iter().cloned());

    let mut installed_dependencies: Vec<String> = Vec::new();
    for v in install_seq {
        let is_primary = version_matches(&v, &primary);
        let v_project_id = v.project_id.clone();
        match crate::mods::install::install_one(&dd, &inst_root, v.clone(), &prog).await {
            Ok(inst) => {
                if !is_primary {
                    installed_dependencies.push(inst.name.clone());
                }
                let _ = ModInstalled {
                    instance_id: instance_id.clone(),
                    sha1: inst.sha1,
                    filename: inst.filename,
                    name: inst.name,
                }
                .emit(&app);
            }
            Err(e) => {
                let _ = ModInstallFailed {
                    instance_id: instance_id.clone(),
                    project_id: v_project_id,
                    error: e.clone(),
                }
                .emit(&app);
                return Err(e);
            }
        }
    }
    Ok(crate::mods::platform::InstallSummary {
        primary_name: primary_v.name.clone(),
        installed_dependencies,
    })
}

// =========================================================================
// Transitive install-plan command (Task 3)
// =========================================================================

/// Resolve the full `InstallPlan` for `primary`:
/// - `required`: primary's transitive required closure (all must be installed)
/// - `optional`: each direct optional dep + its own transitive required sub-closure
/// - `incompatible` / `unresolvable`: refs from the primary's one-level scan
/// - `loader_requirements`: loader project refs (informational, not installed)
///
/// Already-installed mods are pruned from all lists.
#[tauri::command]
#[specta::specta]
pub async fn mods_resolve_install_plan(
    app: tauri::AppHandle,
    instance_id: String,
    primary: ModVersion,
    mc_version: String,
    loader: LoaderKind,
) -> crate::error::Result<InstallPlan> {
    use std::sync::Arc;
    use crate::mods::deps::{resolve_closure, ProjectKey};

    let root = instance_root(&app, &instance_id)?;
    let installed: std::collections::HashSet<ProjectKey> =
        crate::mods::installed::list(&root)
            .await?
            .into_iter()
            .filter_map(|m| match (m.source, m.project_id) {
                (Some(ModSource::Modrinth), Some(pid)) => Some(ProjectKey::Modrinth(pid)),
                (Some(ModSource::Curseforge), Some(pid)) => {
                    pid.parse().ok().map(ProjectKey::Curseforge)
                }
                _ => None,
            })
            .collect();

    // Shared platform + loader-slug cache, cloned into each closure via Arc.
    let platform: Arc<dyn crate::mods::platform::ModPlatform> =
        platform_for(primary.source).into();
    let loader_cache = Arc::new(tokio::sync::Mutex::new(
        std::collections::HashMap::<ProjectKey, bool>::new(),
    ));

    // Factory: produce a fresh fetch closure that shares the Arc'd platform + cache.
    let make_fetch = || {
        let platform = platform.clone();
        let loader_cache = loader_cache.clone();
        let mc = mc_version.clone();
        move |v: ModVersion| {
            let platform = platform.clone();
            let loader_cache = loader_cache.clone();
            let mc = mc.clone();
            async move {
                let mut cache = loader_cache.lock().await;
                fetch_one_level(platform.as_ref(), &mut cache, &v, &mc, loader).await
            }
        }
    };

    // 1. One-level deps of the primary (required/optional/incompat/unresolvable).
    let top = {
        let mut cache = loader_cache.lock().await;
        fetch_one_level(platform.as_ref(), &mut cache, &primary, &mc_version, loader).await?
    };

    // 2. Primary's transitive required closure; prune already-installed.
    //    The closure walker enqueues the root's required deps, so
    //    `primary_closure.required` already contains everything that a
    //    separate `direct_required` collect would have added — a second
    //    independent fetch would also open a theoretical version-skew
    //    window between two network calls to the same endpoint.
    let primary_closure =
        resolve_closure(std::slice::from_ref(&primary), &installed, make_fetch()).await?;
    // The transitive closure already includes the primary's direct requireds,
    // is deduplicated, and has installed mods pruned by `resolve_closure`.
    let required = primary_closure.required;

    // 3. Each direct optional + its transitive required sub-closure,
    //    excluding primary's requireds + installed.
    let mut exclude = installed.clone();
    for v in &required {
        exclude.insert(ProjectKey::of_version(v));
    }
    let mut optional = Vec::new();
    for opt in top.optional.iter().filter(|n| !n.is_loader) {
        let sub =
            resolve_closure(std::slice::from_ref(&opt.version), &exclude, make_fetch()).await?;
        optional.push(OptionalDep {
            version: opt.version.clone(),
            requires: dedup_versions(sub.required.into_iter()),
        });
    }

    // 4. Loader refs seen at the primary's top level.
    let loader_requirements = top
        .required
        .iter()
        .chain(top.optional.iter())
        .filter(|n| n.is_loader)
        .map(|n| version_to_ref(&n.version))
        .collect();

    Ok(InstallPlan {
        required,
        optional,
        incompatible: top.incompatible,
        unresolvable: top.unresolvable,
        loader_requirements,
    })
}

fn dedup_versions(
    it: impl Iterator<Item = crate::mods::platform::ModVersion>,
) -> Vec<crate::mods::platform::ModVersion> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for v in it {
        if seen.insert(crate::mods::deps::ProjectKey::of_version(&v)) {
            out.push(v);
        }
    }
    out
}

fn version_to_ref(v: &crate::mods::platform::ModVersion) -> crate::mods::platform::DepProjectRef {
    match v.source {
        crate::mods::platform::ModSource::Modrinth => crate::mods::platform::DepProjectRef::Modrinth {
            project_id: v.project_id.clone(),
            version_id: Some(v.version_id.clone()),
        },
        crate::mods::platform::ModSource::Curseforge => {
            crate::mods::platform::DepProjectRef::Curseforge {
                mod_id: v.project_id.parse().unwrap_or(0),
                file_id: v.version_id.parse().ok(),
            }
        }
    }
}

/// Reconciled view of `{instance}/.minecraft/mods/`: any jar present is
/// listed (with synthesized metadata if it wasn't installed via the
/// launcher), and stale registry entries with no file on disk are dropped.
#[tauri::command]
#[specta::specta]
pub async fn mods_list_installed(
    app: tauri::AppHandle,
    instance_id: String,
) -> crate::error::Result<Vec<InstalledMod>> {
    let inst_root = instance_root(&app, &instance_id)?;
    crate::mods::installed::list(&inst_root).await
}

/// Rename `<name>.jar` to `<name>.jar.disabled` and flip the registry
/// flag so the next launch skips this mod. Emits `mod-toggle` with
/// `enabled: false`.
#[tauri::command]
#[specta::specta]
pub async fn mods_disable(
    app: tauri::AppHandle,
    instance_id: String,
    sha1: String,
) -> crate::error::Result<()> {
    let inst_root = instance_root(&app, &instance_id)?;
    crate::mods::install::disable(&inst_root, &sha1).await?;
    let _ = ModToggle {
        instance_id,
        sha1,
        enabled: false,
    }
    .emit(&app);
    Ok(())
}

/// Inverse of `mods_disable`. Emits `mod-toggle` with `enabled: true`.
#[tauri::command]
#[specta::specta]
pub async fn mods_enable(
    app: tauri::AppHandle,
    instance_id: String,
    sha1: String,
) -> crate::error::Result<()> {
    let inst_root = instance_root(&app, &instance_id)?;
    crate::mods::install::enable(&inst_root, &sha1).await?;
    let _ = ModToggle {
        instance_id,
        sha1,
        enabled: true,
    }
    .emit(&app);
    Ok(())
}

/// Remove the jar (enabled or disabled flavor) and drop the registry
/// entry. The shared cache copy survives. Emits `mod-uninstalled`.
#[tauri::command]
#[specta::specta]
pub async fn mods_uninstall(
    app: tauri::AppHandle,
    instance_id: String,
    sha1: String,
) -> crate::error::Result<()> {
    let inst_root = instance_root(&app, &instance_id)?;
    crate::mods::install::uninstall(&inst_root, &sha1).await?;
    let _ = ModUninstalled { instance_id, sha1 }.emit(&app);
    Ok(())
}

/// Check every eligible installed user-mod for a newer version. For
/// each mod with platform identity that is not a modpack-origin mod,
/// query its source platform for the versions available on the
/// instance's MC + loader and classify the result. A single mod's
/// query failure becomes that mod's `CheckFailed` state — the command
/// fails wholesale only on a catastrophic error (instance missing,
/// registry unreadable). Modpack-origin and hand-dropped mods are
/// absent from the result.
#[tauri::command]
#[specta::specta]
pub async fn mods_check_updates(
    app: tauri::AppHandle,
    instance_id: String,
) -> crate::error::Result<Vec<crate::mods::updates::ModUpdateCheck>> {
    use crate::mods::updates::{
        classify_update, eligible_identity, ModUpdateCheck, ModUpdateState,
    };

    let inst_root = instance_root(&app, &instance_id)?;
    let (mc_version, loader) = read_active_mc_and_loader(&app, &instance_id)?;
    let installed = crate::mods::installed::list(&inst_root).await?;
    let pack_origin = crate::mods::installed::get_pack_origin(&inst_root).await?;

    let mut out = Vec::new();
    for m in &installed {
        let Some((source, project_id, version_id)) = eligible_identity(m, pack_origin.as_ref())
        else {
            continue;
        };
        let state = match platform_for(source)
            .versions(&project_id, Some(&mc_version), Some(loader))
            .await
        {
            Ok(versions) => classify_update(m, &versions),
            Err(e) => ModUpdateState::CheckFailed {
                reason: e.to_string(),
            },
        };
        out.push(ModUpdateCheck {
            sha1: m.sha1.clone(),
            name: m.name.clone(),
            source,
            project_id,
            current_version_id: version_id,
            current_version_number: m.version_number.clone(),
            state,
        });
    }
    Ok(out)
}

/// For each installed mod in `id`, report whether any platform version
/// exists for the given target `mc` + `loader`. Non-destructive — no
/// files are modified.
///
/// Mods with no platform identity (hand-dropped jars) and pack-origin
/// mods report [`ModCompatStatus::Unknown`]. A single mod's query
/// failure becomes [`ModCompatStatus::Unknown`] for that mod — the
/// command fails wholesale only on a catastrophic error (instance
/// missing, registry unreadable).
#[tauri::command]
#[specta::specta]
pub async fn check_instance_mod_compat(
    app: tauri::AppHandle,
    id: String,
    mc: String,
    loader: crate::instances::schema::LoaderKind,
) -> crate::error::Result<Vec<crate::mods::compat::ModCompat>> {
    use crate::mods::updates::eligible_identity;

    let inst_root = instance_root(&app, &id)?;
    let installed = crate::mods::installed::list(&inst_root).await?;
    let pack_origin = crate::mods::installed::get_pack_origin(&inst_root).await?;

    let mut out = Vec::new();
    for m in &installed {
        let status = match eligible_identity(m, pack_origin.as_ref()) {
            None => crate::mods::compat::ModCompatStatus::Unknown,
            Some((source, project_id, _vid)) => crate::mods::compat::classify_compat(
                platform_for(source)
                    .versions(&project_id, Some(&mc), Some(loader))
                    .await,
            ),
        };
        out.push(crate::mods::compat::ModCompat {
            sha1: m.sha1.clone(),
            name: m.name.clone(),
            status,
        });
    }
    Ok(out)
}

/// The instance's modpack origin reduced to chip data: the pack name
/// and the SHA-1s of its bundled `mods/` files. `None` for an instance
/// that was not created from a modpack import.
#[tauri::command]
#[specta::specta]
pub async fn mods_pack_origin_summary(
    app: tauri::AppHandle,
    instance_id: String,
) -> crate::error::Result<Option<crate::mods::updates::PackOriginSummary>> {
    let inst_root = instance_root(&app, &instance_id)?;
    let pack_origin = crate::mods::installed::get_pack_origin(&inst_root).await?;
    Ok(pack_origin
        .as_ref()
        .map(crate::mods::updates::pack_origin_summary))
}

/// Identify the instance's modpack override-bundled mods by file hash
/// and backfill their platform identity into the registry. Returns the
/// number of mods newly resolved. Best-effort and idempotent: a no-op
/// (returns 0) for instances that were not modpack-imported, or whose
/// pack mods are all already identified or already attempted.
#[tauri::command]
#[specta::specta]
pub async fn mods_enrich_pack_mods(
    app: tauri::AppHandle,
    instance_id: String,
) -> crate::error::Result<u32> {
    let inst_root = instance_root(&app, &instance_id)?;
    let cf_key = crate::mods::curseforge::keyring::get().ok().flatten();
    crate::mods::enrich::enrich_instance(
        &inst_root,
        "https://api.modrinth.com",
        "https://api.curseforge.com",
        cf_key.as_deref(),
    )
    .await
}

/// Apply one mod update: resolve `target`'s required dependencies,
/// pre-warm the cache, swap the old jar (`old_sha1`) for `target` plus
/// its required deps, and preserve the old mod's enabled state. Emits
/// `mod-install-progress` during downloads, `mod-uninstalled` for the
/// old jar, `mod-installed` per landed mod, and `mod-install-failed`
/// on error. Optional dependencies are intentionally not installed —
/// see the spec ("Dependencies on update").
#[tauri::command]
#[specta::specta]
pub async fn mods_update_one(
    app: tauri::AppHandle,
    instance_id: String,
    old_sha1: String,
    target: ModVersion,
) -> crate::error::Result<()> {
    let inst_root = instance_root(&app, &instance_id)?;
    let dd = data_dir(&app)?;
    let (mc_version, loader) = read_active_mc_and_loader(&app, &instance_id)?;

    // Required dependencies of the target version (optional deps skipped).
    let platform = platform_for(target.source);
    let resolved = platform.resolve_deps(&target, &mc_version, loader).await?;
    let required_deps: Vec<ModVersion> = resolved.required.into_iter().map(|r| r.version).collect();

    // Progress events tagged with the target's project_id so the UI can
    // route the bar to the right card (same pattern as install).
    let app_for_progress = app.clone();
    let instance_id_for_progress = instance_id.clone();
    let project_id_for_progress = target.project_id.clone();
    let prog: crate::mods::install::ProgressFn = Box::new(move |phase, done, total| {
        let payload = match phase {
            crate::mods::install::ModInstallPhase::Downloading => ModInstallProgress::Downloading {
                instance_id: instance_id_for_progress.clone(),
                project_id: project_id_for_progress.clone(),
                bytes_done: done as f64,
                bytes_total: total.map(|t| t as f64),
            },
            crate::mods::install::ModInstallPhase::Verifying => ModInstallProgress::Verifying {
                instance_id: instance_id_for_progress.clone(),
                project_id: project_id_for_progress.clone(),
                bytes_done: done as f64,
            },
            crate::mods::install::ModInstallPhase::Copying => ModInstallProgress::Copying {
                instance_id: instance_id_for_progress.clone(),
                project_id: project_id_for_progress.clone(),
            },
        };
        let _ = payload.emit(&app_for_progress);
    });

    let target_project_id = target.project_id.clone();
    match crate::mods::install::update_one(&dd, &inst_root, &old_sha1, target, required_deps, &prog)
        .await
    {
        Ok(outcome) => {
            let _ = ModUninstalled {
                instance_id: instance_id.clone(),
                sha1: outcome.removed_sha1,
            }
            .emit(&app);
            for inst in std::iter::once(outcome.primary).chain(outcome.deps) {
                let _ = ModInstalled {
                    instance_id: instance_id.clone(),
                    sha1: inst.sha1,
                    filename: inst.filename,
                    name: inst.name,
                }
                .emit(&app);
            }
            Ok(())
        }
        Err(e) => {
            let _ = ModInstallFailed {
                instance_id: instance_id.clone(),
                project_id: target_project_id,
                error: e.clone(),
            }
            .emit(&app);
            Err(e)
        }
    }
}

/// Inspect a local mod `.jar`: read its descriptor and judge loader/MC
/// compatibility against the target instance. No filesystem writes.
#[tauri::command]
#[specta::specta]
pub async fn mods_inspect_local(
    app: tauri::AppHandle,
    instance_id: String,
    jar_path: String,
) -> crate::error::Result<crate::mods::local::CompatVerdict> {
    let inst = crate::instances::read_instance(&app, &instance_id)?;
    let bytes =
        tokio::fs::read(&jar_path)
            .await
            .map_err(|e| crate::error::Error::ModsInstancePath {
                path: jar_path.clone(),
                details: format!("{} ({})", e, e.kind()),
            })?;
    let meta = crate::mods::local::read_jar_meta(&bytes)?;
    Ok(crate::mods::local::compat_verdict(
        &meta,
        inst.loader,
        &inst.mc_version,
    ))
}

/// Install a local mod `.jar` into the instance as a manual mod. Emits
/// `mod-installed` on success so the Installed view refreshes the same
/// way it does after a platform install.
#[tauri::command]
#[specta::specta]
pub async fn mods_install_local(
    app: tauri::AppHandle,
    instance_id: String,
    jar_path: String,
) -> crate::error::Result<crate::mods::platform::InstalledMod> {
    let inst_root = instance_root(&app, &instance_id)?;
    let bytes =
        tokio::fs::read(&jar_path)
            .await
            .map_err(|e| crate::error::Error::ModsInstancePath {
                path: jar_path.clone(),
                details: format!("{} ({})", e, e.kind()),
            })?;
    let filename = std::path::Path::new(&jar_path)
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| crate::error::Error::ModsInstancePath {
            path: jar_path.clone(),
            details: "dropped path has no filename".into(),
        })?
        .to_string();
    let meta = crate::mods::local::read_jar_meta(&bytes)?;
    let inst = crate::mods::local::install_local(
        &inst_root,
        &filename,
        &bytes,
        meta.display_name.as_deref(),
    )
    .await?;
    let _ = ModInstalled {
        instance_id,
        sha1: inst.sha1.clone(),
        filename: inst.filename.clone(),
        name: inst.name.clone(),
    }
    .emit(&app);
    Ok(inst)
}

// =========================================================================
// CurseForge key management + shared cache management (v0.5.0 sub-feature 3)
// =========================================================================

use crate::mods::curseforge::keyring as cf_keyring;

/// Report whether a CurseForge API key is currently stored in the OS
/// keyring. `Invalid` is reserved for future "key was rejected" surfacing —
/// today this command only distinguishes Missing vs Set.
#[tauri::command]
#[specta::specta]
pub async fn mods_get_curseforge_key_status() -> crate::error::Result<KeyStatus> {
    Ok(match cf_keyring::get()? {
        Some(_) => KeyStatus::Set,
        None => KeyStatus::Missing,
    })
}

/// Validate a candidate CurseForge API key by pinging `/v1/games/432`
/// (the Minecraft game id) with `x-api-key`. On a non-success HTTP
/// response we return `ModsPlatformAuth { kind: Invalid }` and do NOT
/// persist anything. Only a successful ping causes the key to be
/// written to the OS keyring.
///
/// After a successful key set, this command also iterates every
/// instance and resets `enrich_attempted = false` on each instance's
/// `source = None` mods, so any mods that were Modrinth-only-attempted
/// under a keyless install are retried (now with CF) on the next
/// Installed-tab open. Reset failures are logged and swallowed — a
/// single instance's registry write failure must not fail the key set.
#[tauri::command]
#[specta::specta]
pub async fn mods_set_curseforge_key(
    app: tauri::AppHandle,
    key: String,
) -> crate::error::Result<()> {
    let url = "https://api.curseforge.com/v1/games/432";
    let resp = crate::network::request::get(url, &[("x-api-key", key.as_str())], "mods")
        .await
        .map_err(|e| crate::error::Error::ModsNetwork {
            url: url.into(),
            details: e.to_string(),
        })?;
    if !(200..300).contains(&resp.status) {
        return Err(crate::error::Error::ModsPlatformAuth {
            kind: crate::error::ModsAuthKind::Invalid,
        });
    }
    cf_keyring::set(&key)?;

    // Self-heal: any instance whose source=None mods were attempted
    // under a keyless or CF-down pass becomes eligible for backfill
    // again on the next Installed-tab open. Best-effort.
    let instances = match crate::instances::list_instances_with_status(&app) {
        Ok(xs) => xs,
        Err(e) => {
            eprintln!("[mods_set_curseforge_key] could not list instances for reset: {e}");
            return Ok(());
        }
    };
    for inst in instances {
        let root = match crate::paths::instance_dir(&app, &inst.id) {
            Ok(p) => p,
            Err(e) => {
                eprintln!(
                    "[mods_set_curseforge_key] no instance_dir for {}: {e}",
                    inst.id
                );
                continue;
            }
        };
        if let Err(e) =
            crate::mods::installed::reset_enrichment_attempts_for_unresolved(&root).await
        {
            eprintln!(
                "[mods_set_curseforge_key] reset failed for {}: {e}",
                inst.id
            );
        }
    }
    Ok(())
}

/// Remove the stored CurseForge API key. No-op if no key is set.
#[tauri::command]
#[specta::specta]
pub async fn mods_clear_curseforge_key() -> crate::error::Result<()> {
    cf_keyring::clear()
}

/// Size in bytes of the shared mod cache directory (under the launcher's
/// app-data dir). Used by the Settings panel to show "Cache: X MB".
#[tauri::command]
#[specta::specta]
pub async fn mods_cache_size_bytes(app: tauri::AppHandle) -> crate::error::Result<f64> {
    // f64 not u64: specta forbids exporting BigInt-style types to TS.
    // 2^53 bytes (~9 PiB) is far beyond any plausible mod cache size.
    let dd = data_dir(&app)?;
    let n = crate::mods::cache::size_bytes(&dd).await?;
    Ok(n as f64)
}

/// Delete every cached mod jar. Returns the number of bytes reclaimed.
/// Installed instance copies are untouched — only the shared cache.
#[tauri::command]
#[specta::specta]
pub async fn mods_clear_cache(app: tauri::AppHandle) -> crate::error::Result<f64> {
    // f64 not u64 — same reason as mods_cache_size_bytes.
    let dd = data_dir(&app)?;
    let n = crate::mods::cache::clear(&dd).await?;
    Ok(n as f64)
}

// =========================================================================
// Modpack import (v0.5.0 sub-feature 4)
// =========================================================================

use crate::mods::modpack;
use crate::mods::modpack::schema::{
    ModpackProgress, ModpackSearchPage, ModpackSort, ModpackStatus, ModpackSummary,
};
use tauri::ipc::Channel;
use tauri::Manager;

/// Read a `.mrpack` / `.zip` from disk and return a parsed summary
/// (resolved mod files, overrides count, loader, mc version). The UI
/// uses this for the picker dialog before the user commits to import.
#[tauri::command]
#[specta::specta]
pub async fn modpack_inspect(path: String) -> Result<ModpackSummary, crate::error::Error> {
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|e| crate::error::Error::Io {
            path: path.clone(),
            details: e.to_string(),
        })?;
    modpack::import::inspect(&bytes, "https://api.curseforge.com").await
}

/// Run the full import: create an instance, download every selected
/// mod (subject to license / distribution allowance), then optionally
/// extract overrides into the instance's `.minecraft/`. Streams two
/// kinds of progress over typed channels:
/// - `on_progress`: coarse-grained `ModpackProgress` phases.
/// - `on_install_progress`: per-mod `ProgressTick` (download / verify
///   / copy bytes). The per-mod stream is keyed by phase only, not by
///   `project_id` — the UI correlates it with the `InstallingFile`
///   phase emitted on `on_progress`.
#[tauri::command]
#[specta::specta]
#[allow(clippy::too_many_arguments)]
pub async fn modpack_import(
    app: tauri::AppHandle,
    path: String,
    selected_shas: Vec<String>,
    apply_overrides: bool,
    // Optional provenance hints from the Browse sub-tab. When the user
    // imports straight off a `ModpackHit` the UI already has these and
    // can pass them through, letting the orchestrator skip a Modrinth
    // /v2/version round-trip. Drag-drop imports pass `null` and the
    // orchestrator auto-looks-up.
    hint_project_id: Option<String>,
    hint_source: Option<crate::mods::platform::ModSource>,
    hint_version_id: Option<String>,
    on_progress: Channel<ModpackProgress>,
    on_install_progress: Channel<crate::mods::install::ProgressTick>,
) -> Result<crate::instances::schema::InstanceWithStatus, crate::error::Error> {
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|e| crate::error::Error::Io {
            path: path.clone(),
            details: e.to_string(),
        })?;
    let install_progress: crate::mods::install::ProgressFn =
        Box::new(move |phase, current, total| {
            let _ = on_install_progress.send(crate::mods::install::ProgressTick {
                phase,
                current: current as f64,
                total: total.map(|t| t as f64),
            });
        });
    modpack::import::import(
        &app,
        &bytes,
        &selected_shas,
        apply_overrides,
        "https://api.curseforge.com",
        hint_project_id,
        hint_source,
        hint_version_id,
        &|p| {
            let _ = on_progress.send(p);
        },
        install_progress,
    )
    .await
}

/// Search a modpack catalogue. `source` selects Modrinth (anonymous)
/// or CurseForge (requires a stored API key — a missing key surfaces
/// as `ModsPlatformAuth`, which the UI maps to the key banner).
#[tauri::command]
#[specta::specta]
pub async fn modpack_search(
    source: crate::mods::platform::ModSource,
    query: String,
    page: u32,
    mc_version: Option<String>,
    loader: Option<crate::mods::platform::LoaderKind>,
    sort: ModpackSort,
    page_size: u32,
) -> Result<ModpackSearchPage, crate::error::Error> {
    match source {
        crate::mods::platform::ModSource::Modrinth => {
            modpack::search::search(
                "https://api.modrinth.com",
                &query,
                page,
                mc_version.as_deref(),
                loader,
                sort,
                page_size,
            )
            .await
        }
        crate::mods::platform::ModSource::Curseforge => {
            let key = crate::mods::curseforge::keyring::get().ok().flatten();
            modpack::cf_api::search(
                "https://api.curseforge.com",
                key.as_deref(),
                &query,
                page,
                mc_version.as_deref(),
                loader,
                sort,
                page_size,
            )
            .await
        }
    }
}

/// Pull a modpack version's archive to a temp path under the OS temp
/// dir, and return the absolute path so the UI can hand it to
/// `modpack_inspect` / `modpack_import`. Modrinth versions resolve to a
/// primary `.mrpack`; CurseForge versions resolve a file's
/// `downloadUrl` to a `.zip`. The temp file is left in place after
/// import — a successful import has already copied every byte that
/// matters into the instance.
#[tauri::command]
#[specta::specta]
pub async fn modpack_fetch_to_temp(
    app: tauri::AppHandle,
    source: crate::mods::platform::ModSource,
    project_id: String,
    version_id: String,
) -> Result<String, crate::error::Error> {
    let (bytes, ext) = match source {
        crate::mods::platform::ModSource::Modrinth => {
            let url =
                format!("https://api.modrinth.com/v2/project/{project_id}/version/{version_id}");
            let resp = crate::network::request::get(
                &url,
                &[("user-agent", "AntonBabchenko/Lucerna")],
                "modpacks",
            )
            .await
            .map_err(|e| crate::error::Error::ModsNetwork {
                url: url.clone(),
                details: e.to_string(),
            })?;
            if !(200..300).contains(&resp.status) {
                return Err(crate::error::Error::ModsNetwork {
                    url,
                    details: format!("HTTP {}", resp.status),
                });
            }
            #[derive(serde::Deserialize)]
            struct V {
                files: Vec<F>,
            }
            #[derive(serde::Deserialize)]
            struct F {
                url: String,
                filename: String,
                primary: bool,
            }
            let v: V = serde_json::from_slice(&resp.body).map_err(|e| {
                crate::error::Error::ModsDecode {
                    platform: "modrinth".into(),
                    details: e.to_string(),
                }
            })?;
            let f = v
                .files
                .iter()
                .find(|f| f.primary)
                .or_else(|| v.files.iter().find(|f| f.filename.ends_with(".mrpack")))
                .ok_or(crate::error::Error::ModpackManifestInvalid {
                    format: "modrinth".into(),
                    details: "no primary .mrpack file on version".into(),
                })?;
            let bytes = crate::network::get_bytes(&f.url, "modpacks")
                .await
                .map_err(|e| crate::error::Error::ModsNetwork {
                    url: f.url.clone(),
                    details: e.to_string(),
                })?;
            (bytes, "mrpack")
        }
        crate::mods::platform::ModSource::Curseforge => {
            let key = crate::mods::curseforge::keyring::get().ok().flatten();
            // For CurseForge, `version_id` carries the file id — the
            // command keeps the `version_id` name for symmetry with Modrinth.
            let dl = modpack::cf_api::resolve_file_download(
                "https://api.curseforge.com",
                key.as_deref(),
                &project_id,
                &version_id,
            )
            .await?;
            let bytes = crate::network::get_bytes(&dl, "modpacks")
                .await
                .map_err(|e| crate::error::Error::ModsNetwork {
                    url: dl.clone(),
                    details: e.to_string(),
                })?;
            (bytes, "zip")
        }
    };

    let temp_dir = app
        .path()
        .temp_dir()
        .map_err(|e| crate::error::Error::Io {
            path: "<temp>".into(),
            details: e.to_string(),
        })?
        .join("lucerna")
        .join("modpack");
    tokio::fs::create_dir_all(&temp_dir)
        .await
        .map_err(|e| crate::error::Error::Io {
            path: temp_dir.display().to_string(),
            details: e.to_string(),
        })?;
    let dest = temp_dir.join(format!("{}.{ext}", uuid::Uuid::new_v4()));
    tokio::fs::write(&dest, &bytes)
        .await
        .map_err(|e| crate::error::Error::Io {
            path: dest.display().to_string(),
            details: e.to_string(),
        })?;
    Ok(dest.to_string_lossy().to_string())
}

/// Return the pack-origin snapshot + a live diff for a pack-imported
/// instance. Returns `None` for instances that were manually created
/// (no .mrpack/.zip ever ran through the import pipeline) and for
/// pre-bundle-2 imports that pre-date the `pack_origin` field. Single
/// IPC round-trip combines `get_pack_origin` + the modified-check so
/// the UI doesn't have to make two calls per card.
#[tauri::command]
#[specta::specta]
pub async fn modpack_status(
    app: tauri::AppHandle,
    instance_id: String,
) -> crate::error::Result<Option<ModpackStatus>> {
    let inst_root = instance_root(&app, &instance_id)?;
    let origin = match crate::mods::installed::get_pack_origin(&inst_root).await? {
        Some(o) => o,
        None => return Ok(None),
    };
    let installed = crate::mods::installed::list(&inst_root).await?;
    // Asset (non-mods/) origin files: an asset is "present" iff its file
    // exists at the declared path under the instance's .minecraft/.
    let mc_dir = inst_root.join(".minecraft");
    let mut asset_present: std::collections::HashSet<String> = Default::default();
    for f in &origin.files {
        if !crate::mods::modpack::import::is_tracked_mod(&f.install_path)
            && tokio::fs::try_exists(mc_dir.join(&f.install_path))
                .await
                .unwrap_or(false)
        {
            asset_present.insert(f.install_path.clone());
        }
    }
    Ok(Some(crate::mods::modpack::import::compute_status(
        origin,
        &installed,
        &asset_present,
    )))
}

/// Re-install a single file that was part of the original pack but is
/// no longer in the instance (= it shows up in `ModpackStatus.removed_files`).
/// Looks the file up by `sha1` in the frozen origin snapshot,
/// synthesises a `ModVersion` from the snapshot fields, and calls
/// `install_one`. Errors `ModsNotFound { source: "pack_origin" }` if
/// `sha1` is not in the origin (= caller has stale data, or the
/// instance has no origin at all).
#[tauri::command]
#[specta::specta]
pub async fn modpack_restore_file(
    app: tauri::AppHandle,
    instance_id: String,
    sha1: String,
) -> crate::error::Result<()> {
    let inst_root = instance_root(&app, &instance_id)?;
    let dd = data_dir(&app)?;
    let origin = crate::mods::installed::get_pack_origin(&inst_root)
        .await?
        .ok_or_else(|| crate::error::Error::ModsNotFound {
            platform: "pack_origin".into(),
        })?;
    let file = origin
        .files
        .iter()
        .find(|f| f.sha1.eq_ignore_ascii_case(&sha1))
        .cloned()
        .ok_or_else(|| crate::error::Error::ModsNotFound {
            platform: "pack_origin".into(),
        })?;
    // Bundled-from-overrides entries carry no URL — the bytes lived
    // inside the .mrpack archive and we don't keep that archive after
    // import. Tell the UI to gray out / disable the Restore button via
    // a typed error instead of trying to install_one a no-URL file.
    if file.url.is_empty() {
        return Err(crate::error::Error::ModpackBundledNoUrl {
            mod_name: file.name.clone(),
        });
    }
    // Re-use the same per-mod progress wiring as `mods_install_with_deps`
    // so the UI surfaces the same Downloading/Verifying/Copying states.
    let app_for_progress = app.clone();
    let instance_id_for_progress = instance_id.clone();
    let project_id_for_progress = file.project_id.clone();
    let prog: crate::mods::install::ProgressFn = Box::new(move |phase, done, total| {
        let payload = match phase {
            crate::mods::install::ModInstallPhase::Downloading => ModInstallProgress::Downloading {
                instance_id: instance_id_for_progress.clone(),
                project_id: project_id_for_progress.clone(),
                bytes_done: done as f64,
                bytes_total: total.map(|t| t as f64),
            },
            crate::mods::install::ModInstallPhase::Verifying => ModInstallProgress::Verifying {
                instance_id: instance_id_for_progress.clone(),
                project_id: project_id_for_progress.clone(),
                bytes_done: done as f64,
            },
            crate::mods::install::ModInstallPhase::Copying => ModInstallProgress::Copying {
                instance_id: instance_id_for_progress.clone(),
                project_id: project_id_for_progress.clone(),
            },
        };
        let _ = payload.emit(&app_for_progress);
    });

    if file.install_path.starts_with("mods/") {
        let (mc_version, loader) = read_active_mc_and_loader(&app, &instance_id)?;
        let mv = crate::mods::modpack::import::pack_origin_file_to_mod_version(
            &file,
            &mc_version,
            loader,
        );
        crate::mods::install::install_one(&dd, &inst_root, mv, &prog).await?;
    } else {
        crate::mods::install::install_asset(
            &dd,
            &inst_root,
            &file.url,
            &file.sha1,
            file.size,
            &file.install_path,
            &prog,
        )
        .await?;
    }
    let _ = ModInstalled {
        instance_id: instance_id.clone(),
        sha1: file.sha1.clone(),
        filename: file.filename.clone(),
        name: file.name.clone(),
    }
    .emit(&app);
    Ok(())
}

/// Fetch a modpack project's version list from a Modrinth-shaped base.
/// Split out from the `modpack_get_versions` command so tests can
/// inject a wiremock base URL.
pub(crate) async fn fetch_modpack_versions(
    base: &str,
    project_id: &str,
) -> crate::error::Result<Vec<crate::mods::modpack::schema::ModpackVersionEntry>> {
    let url = format!("{base}/v2/project/{project_id}/version");
    let resp = crate::network::request::get(&url, &[], "modpacks").await?;
    if resp.status == 404 {
        return Err(crate::error::Error::ModsNotFound {
            platform: "modrinth".into(),
        });
    }
    if !(200..300).contains(&resp.status) {
        return Err(crate::error::Error::ModsNetwork {
            url,
            details: format!("HTTP {}", resp.status),
        });
    }
    serde_json::from_slice(&resp.body).map_err(|e| crate::error::Error::ModsDecode {
        platform: "modrinth".into(),
        details: e.to_string(),
    })
}

/// List the published versions of a modpack project. Modrinth versions
/// come from `/v2/project/{id}/version`; CurseForge versions are the
/// project's files (`/v1/mods/{id}/files`).
#[tauri::command]
#[specta::specta]
pub async fn modpack_get_versions(
    source: crate::mods::platform::ModSource,
    project_id: String,
) -> crate::error::Result<Vec<crate::mods::modpack::schema::ModpackVersionEntry>> {
    match source {
        crate::mods::platform::ModSource::Modrinth => {
            fetch_modpack_versions("https://api.modrinth.com", &project_id).await
        }
        crate::mods::platform::ModSource::Curseforge => {
            let key = crate::mods::curseforge::keyring::get().ok().flatten();
            modpack::cf_api::list_files("https://api.curseforge.com", key.as_deref(), &project_id)
                .await
        }
    }
}

/// Minimal serde shape for the Modrinth `/v2/project/{id}` fields the
/// modpack detail modal consumes. Split out so tests inject a base URL.
#[derive(serde::Deserialize)]
struct MrModpackProject {
    body: String,
    source_url: Option<String>,
    wiki_url: Option<String>,
    #[serde(default)]
    gallery: Vec<MrGalleryEntry>,
}

#[derive(serde::Deserialize)]
struct MrGalleryEntry {
    url: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    featured: bool,
    #[serde(default)]
    ordering: Option<i64>,
}

pub(crate) async fn fetch_modrinth_modpack_project(
    base: &str,
    project_id: &str,
) -> crate::error::Result<crate::mods::modpack::schema::ModpackProject> {
    let url = format!("{base}/v2/project/{project_id}");
    let resp = crate::network::request::get(&url, &[], "modpacks").await?;
    if resp.status == 404 {
        return Err(crate::error::Error::ModsNotFound {
            platform: "modrinth".into(),
        });
    }
    if !(200..300).contains(&resp.status) {
        return Err(crate::error::Error::ModsNetwork {
            url,
            details: format!("HTTP {}", resp.status),
        });
    }
    let p: MrModpackProject =
        serde_json::from_slice(&resp.body).map_err(|e| crate::error::Error::ModsDecode {
            platform: "modrinth".into(),
            details: e.to_string(),
        })?;
    let mut entries = p.gallery;
    entries.sort_by(|a, b| {
        b.featured.cmp(&a.featured).then(
            a.ordering
                .unwrap_or(i64::MAX)
                .cmp(&b.ordering.unwrap_or(i64::MAX)),
        )
    });
    let gallery = entries
        .into_iter()
        .filter(|e| crate::mods::render::is_safe_image_url(&e.url))
        .map(|e| crate::mods::platform::GalleryImage {
            url: e.url,
            title: e.title,
        })
        .collect();
    Ok(crate::mods::modpack::schema::ModpackProject {
        body_html: crate::mods::render::markdown_to_safe_html(&p.body),
        gallery,
        website_url: p.source_url.or(p.wiki_url),
    })
}

/// Fetch a modpack project's description + gallery for the detail modal's
/// Overview tab. Modrinth: `/v2/project/{id}`. CurseForge: `/v1/mods/{id}`
/// + the description endpoint.
#[tauri::command]
#[specta::specta]
pub async fn modpack_project(
    source: crate::mods::platform::ModSource,
    project_id: String,
) -> crate::error::Result<crate::mods::modpack::schema::ModpackProject> {
    match source {
        crate::mods::platform::ModSource::Modrinth => {
            fetch_modrinth_modpack_project("https://api.modrinth.com", &project_id).await
        }
        crate::mods::platform::ModSource::Curseforge => {
            let key = crate::mods::curseforge::keyring::get().ok().flatten();
            modpack::cf_api::fetch_project_detail(
                "https://api.curseforge.com",
                key.as_deref(),
                &project_id,
            )
            .await
        }
    }
}

/// Pick the most-recently-published version, or `None` if the list is
/// empty or its newest entry's opaque Modrinth `id` already equals
/// `current_id`. Pure — split out so it is unit-testable.
pub(crate) fn latest_newer(
    mut versions: Vec<crate::mods::modpack::schema::ModpackVersionEntry>,
    current_id: &str,
) -> Option<crate::mods::modpack::schema::ModpackVersionEntry> {
    versions.sort_by(|a, b| b.date_published.cmp(&a.date_published));
    let latest = versions.into_iter().next()?;
    if latest.id == current_id {
        None
    } else {
        Some(latest)
    }
}

/// Check whether a newer version of an imported Modrinth modpack exists.
/// Returns `None` for non-Modrinth pack instances and when the instance
/// already has the latest version.
#[tauri::command]
#[specta::specta]
pub async fn modpack_check_update(
    app: tauri::AppHandle,
    instance_id: String,
) -> crate::error::Result<Option<crate::mods::modpack::schema::ModpackVersionEntry>> {
    let inst = crate::instances::read_instance(&app, &instance_id)?;
    let (project_id, current_id) = match (
        inst.mrpack_source,
        inst.mrpack_project_id.as_deref(),
        inst.mrpack_version_id.as_deref(),
    ) {
        (Some(crate::mods::platform::ModSource::Modrinth), Some(pid), Some(vid)) => {
            (pid.to_string(), vid.to_string())
        }
        _ => return Ok(None),
    };
    let versions = fetch_modpack_versions("https://api.modrinth.com", &project_id).await?;
    Ok(latest_newer(versions, &current_id))
}

/// Diff a downloaded new-version `.mrpack` (already fetched to
/// `mrpack_path` via `modpack_fetch_to_temp`) against the instance's
/// current `pack_origin`. Returns the diff for the confirm dialog.
#[tauri::command]
#[specta::specta]
pub async fn modpack_compute_update(
    app: tauri::AppHandle,
    instance_id: String,
    mrpack_path: String,
) -> crate::error::Result<crate::mods::modpack::schema::ModpackUpdateDiff> {
    let inst = crate::instances::read_instance(&app, &instance_id)?;
    let inst_root = instance_root(&app, &instance_id)?;
    let origin = crate::mods::installed::get_pack_origin(&inst_root)
        .await?
        .ok_or_else(|| crate::error::Error::ModsNotFound {
            platform: "pack_origin".into(),
        })?;
    let bytes = tokio::fs::read(&mrpack_path)
        .await
        .map_err(|e| crate::error::Error::Io {
            path: mrpack_path.clone(),
            details: e.to_string(),
        })?;
    let summary =
        crate::mods::modpack::import::inspect(&bytes, "https://api.curseforge.com").await?;
    Ok(crate::mods::modpack::import::compute_update_diff(
        &summary,
        &origin,
        &inst.mc_version,
        inst.loader,
        &inst.loader_version,
    ))
}

/// Apply a modpack update in place. Phase 1 downloads every new/changed
/// file into the shared cache (the instance is NOT touched — a failure
/// here aborts cleanly). Phase 2 removes the old files, installs the new
/// ones from the warm cache, and rewrites `pack_origin` + the instance's
/// version metadata. `overrides/`-bundled content is not touched.
#[tauri::command]
#[specta::specta]
pub async fn modpack_apply_update(
    app: tauri::AppHandle,
    instance_id: String,
    mrpack_path: String,
    new_version_id: String,
    on_progress: Channel<ModpackProgress>,
    on_install_progress: Channel<crate::mods::install::ProgressTick>,
) -> crate::error::Result<crate::instances::schema::InstanceWithStatus> {
    let inst = crate::instances::read_instance(&app, &instance_id)?;
    let inst_root = instance_root(&app, &instance_id)?;
    let dd = data_dir(&app)?;
    let origin = crate::mods::installed::get_pack_origin(&inst_root)
        .await?
        .ok_or_else(|| crate::error::Error::ModsNotFound {
            platform: "pack_origin".into(),
        })?;
    let bytes = tokio::fs::read(&mrpack_path)
        .await
        .map_err(|e| crate::error::Error::Io {
            path: mrpack_path.clone(),
            details: e.to_string(),
        })?;
    let summary =
        crate::mods::modpack::import::inspect(&bytes, "https://api.curseforge.com").await?;
    let diff = crate::mods::modpack::import::compute_update_diff(
        &summary,
        &origin,
        &inst.mc_version,
        inst.loader,
        &inst.loader_version,
    );

    let install_progress: crate::mods::install::ProgressFn = {
        let ch = on_install_progress.clone();
        Box::new(move |phase, current, total| {
            let _ = ch.send(crate::mods::install::ProgressTick {
                phase,
                current: current as f64,
                total: total.map(|t| t as f64),
            });
        })
    };

    // ---- Phase 1: download every new/changed file into the cache. ----
    let to_fetch: Vec<&crate::mods::modpack::schema::ModpackFile> = diff
        .added
        .iter()
        .chain(diff.updated.iter().map(|e| &e.new))
        .collect();
    let total = to_fetch.len() as u32;
    for (idx, f) in to_fetch.iter().enumerate() {
        let _ = on_progress.send(ModpackProgress::InstallingFile {
            current: idx as u32 + 1,
            total,
            file_name: f.name.clone(),
        });
        crate::mods::install::fetch_to_cache(
            &dd,
            &f.url,
            &f.sha1.to_ascii_lowercase(),
            f.size,
            "modpacks",
            &install_progress,
        )
        .await?;
    }

    // ---- Phase 2: apply locally (cache is warm). ----
    for f in diff
        .removed
        .iter()
        .chain(diff.updated.iter().map(|e| &e.old))
    {
        remove_pack_file(&inst_root, f).await?;
    }
    for f in diff.added.iter().chain(diff.updated.iter().map(|e| &e.new)) {
        if f.install_path.starts_with("mods/") {
            let mv = crate::mods::modpack::import::modpack_file_to_mod_version(
                f,
                &summary.game_version,
                summary.loader,
            );
            crate::mods::install::install_one(&dd, &inst_root, mv, &install_progress).await?;
        } else {
            crate::mods::install::install_asset(
                &dd,
                &inst_root,
                &f.url,
                &f.sha1,
                f.size,
                &f.install_path,
                &install_progress,
            )
            .await?;
        }
    }

    // Rewrite pack_origin: new files[] entries + carried-over bundled.
    let bundled: Vec<crate::mods::installed::PackOriginFile> = origin
        .files
        .iter()
        .filter(|f| f.url.is_empty())
        .cloned()
        .collect();
    let selected: Vec<&crate::mods::modpack::schema::ModpackFile> =
        summary.files.iter().filter(|f| !f.url.is_empty()).collect();
    let mut new_origin = crate::mods::modpack::import::build_pack_origin(
        &summary,
        &selected,
        origin.project_id.clone(),
        &origin.project_name,
    );
    new_origin.files.extend(bundled);
    crate::mods::installed::set_pack_origin(&inst_root, new_origin).await?;

    let updated_inst = crate::instances::set_instance_pack_update(
        &app,
        &instance_id,
        summary.version.clone(),
        summary.game_version.clone(),
        summary.loader,
        summary.loader_version.clone(),
        new_version_id,
    )?;
    let _ = on_progress.send(ModpackProgress::Done {
        instance_id: instance_id.clone(),
    });
    Ok(updated_inst)
}

/// Re-fetch the instance's current modpack version and re-extract its
/// `overrides/` — recovers bundled mods/files that a per-file Restore
/// cannot. Modrinth pack instances only.
#[tauri::command]
#[specta::specta]
pub async fn modpack_reimport_overrides(
    app: tauri::AppHandle,
    instance_id: String,
    on_progress: Channel<ModpackProgress>,
) -> crate::error::Result<()> {
    let inst = crate::instances::read_instance(&app, &instance_id)?;
    let inst_root = instance_root(&app, &instance_id)?;
    let (project_id, version_id) = match (
        inst.mrpack_source,
        inst.mrpack_project_id.as_deref(),
        inst.mrpack_version_id.as_deref(),
    ) {
        (Some(crate::mods::platform::ModSource::Modrinth), Some(pid), Some(vid)) => {
            (pid.to_string(), vid.to_string())
        }
        _ => {
            return Err(crate::error::Error::ModsNotFound {
                platform: "modrinth".into(),
            })
        }
    };

    let temp_path = modpack_fetch_to_temp(
        app.clone(),
        crate::mods::platform::ModSource::Modrinth,
        project_id,
        version_id,
    )
    .await?;
    let bytes = tokio::fs::read(&temp_path)
        .await
        .map_err(|e| crate::error::Error::Io {
            path: temp_path.clone(),
            details: e.to_string(),
        })?;
    crate::mods::modpack::overrides::extract(&bytes, &inst_root, |c, t| {
        let _ = on_progress.send(ModpackProgress::ExtractingOverrides {
            current: c,
            total: t,
        });
    })
    .await?;
    let _ = on_progress.send(ModpackProgress::Done {
        instance_id: instance_id.clone(),
    });
    Ok(())
}

/// Remove one pack-origin file from an instance: a `mods/` jar via the
/// mod registry, anything else by deleting the file at `install_path`.
async fn remove_pack_file(
    inst_root: &std::path::Path,
    f: &crate::mods::installed::PackOriginFile,
) -> crate::error::Result<()> {
    if f.install_path.starts_with("mods/") {
        crate::mods::installed::remove(inst_root, &f.sha1).await?;
        let jar = crate::mods::installed::mods_dir(inst_root).join(&f.filename);
        if tokio::fs::try_exists(&jar).await.unwrap_or(false) {
            let _ = tokio::fs::remove_file(&jar).await;
        }
    } else {
        let p = inst_root.join(".minecraft").join(&f.install_path);
        if tokio::fs::try_exists(&p).await.unwrap_or(false) {
            let _ = tokio::fs::remove_file(&p).await;
        }
    }
    Ok(())
}

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
#[tauri::command]
#[specta::specta]
pub async fn app_settings_set_general(
    app: tauri::AppHandle,
    general: crate::instances::schema::GeneralSettings,
) -> crate::error::Result<()> {
    let path =
        crate::paths::app_file(&app).map_err(|e| crate::error::Error::io("<app_file>", e))?;
    let mut current = crate::instances::store::read_app_json(&path)?;
    current.general = general;
    crate::instances::store::write_app_json(&path, &current)
}

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

// =========================================================================
// Microsoft authentication (task 12)
// =========================================================================

/// Begin a Microsoft sign-in flow. Opens the OAuth browser, exchanges
/// the auth code for tokens, queries the Mojang API for a profile,
/// and persists the account. Returns the new Account.
#[tauri::command]
#[specta::specta]
pub async fn begin_microsoft_signin(
    app: tauri::AppHandle,
) -> crate::error::Result<crate::accounts::store::Account> {
    crate::accounts::microsoft::sign_in(&app).await
}

/// Refresh an existing Microsoft account by id. Queries the Mojang API
/// to validate/update the profile, and re-persists. Returns the updated Account.
#[tauri::command]
#[specta::specta]
pub async fn refresh_microsoft_account(
    app: tauri::AppHandle,
    id: String,
) -> crate::error::Result<crate::accounts::store::Account> {
    crate::accounts::microsoft::refresh(&app, &id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;

    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::test_env_lock()
    }

    #[test]
    fn greet_includes_name() {
        let g = greet("World".to_string());
        assert!(g.message.contains("World"));
        assert!(g.message.contains("Lucerna"));
    }

    #[test]
    fn validate_accepts_normal_name() {
        assert!(validate_instance_name("My Pack").is_ok());
    }

    #[test]
    fn validate_rejects_empty() {
        assert!(matches!(
            validate_instance_name(""),
            Err(Error::InstanceNameEmpty)
        ));
    }

    #[test]
    fn validate_rejects_whitespace_only() {
        assert!(matches!(
            validate_instance_name("   \t  "),
            Err(Error::InstanceNameEmpty)
        ));
    }

    #[test]
    fn validate_trims_leading_trailing_whitespace_for_length_check() {
        // 32 chars surrounded by spaces — trimmed length is 32, valid.
        let name = format!("  {}  ", "a".repeat(32));
        assert!(validate_instance_name(&name).is_ok());
    }

    #[test]
    fn validate_accepts_exactly_32_chars() {
        assert!(validate_instance_name(&"a".repeat(32)).is_ok());
    }

    #[test]
    fn validate_rejects_33_chars() {
        let result = validate_instance_name(&"a".repeat(33));
        assert!(matches!(
            result,
            Err(Error::InstanceNameTooLong {
                max: 32,
                actual: 33
            })
        ));
    }

    #[test]
    fn validate_counts_unicode_scalar_values_not_bytes() {
        // 30 cyrillic chars = 60 bytes in UTF-8 but 30 scalars — valid.
        assert!(validate_instance_name(&"я".repeat(30)).is_ok());
        // 33 cyrillic chars = 66 bytes — should still reject as 33 too long.
        let result = validate_instance_name(&"я".repeat(33));
        assert!(matches!(
            result,
            Err(Error::InstanceNameTooLong {
                max: 32,
                actual: 33
            })
        ));
    }

    // These tests verify the validate_instance_name call site — they do
    // NOT exercise the full Tauri command path (no AppHandle available
    // in unit tests). For full integration use the matrix harness.

    #[test]
    fn validate_rejects_at_create_call_site_path() {
        // The shape we want: anyone calling validate_instance_name
        // before reaching crate::instances::create_instance gets the
        // correct typed error. The function's a private guard, so this
        // is a behavioural assertion via the public helper.
        let r = validate_instance_name("");
        assert!(matches!(r, Err(Error::InstanceNameEmpty)));
        let r = validate_instance_name(&"x".repeat(33));
        assert!(matches!(r, Err(Error::InstanceNameTooLong { .. })));
    }

    #[tokio::test]
    async fn modpack_get_versions_parses_modrinth_list() {
        let _g = test_lock();
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v2/project/abc/version"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"[{"id":"v1","name":"Pack 1.0","version_number":"1.0",
                     "game_versions":["1.20.1"],"loaders":["fabric"],
                     "date_published":"2026-05-01T00:00:00Z"}]"#,
            ))
            .mount(&server)
            .await;
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let entries = crate::commands::fetch_modpack_versions(&server.uri(), "abc")
            .await
            .unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "v1");
        assert_eq!(entries[0].game_versions, vec!["1.20.1"]);
    }

    #[tokio::test]
    async fn modpack_get_versions_non_2xx_is_error() {
        let _g = test_lock();
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v2/project/missing/version"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let err = crate::commands::fetch_modpack_versions(&server.uri(), "missing")
            .await
            .unwrap_err();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert!(
            matches!(err, crate::error::Error::ModsNotFound { .. }),
            "got: {err:?}"
        );
    }

    #[tokio::test]
    async fn fetch_modrinth_modpack_project_renders_body_and_gallery() {
        let _g = test_lock();
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v2/project/abc"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r##"{"body":"# Pack\n\ntext","source_url":"https://src.example","wiki_url":null,
                    "gallery":[{"url":"https://media.modrinth.com/g.png","title":"G","featured":true,"ordering":1}]}"##,
            ))
            .mount(&server)
            .await;
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let p = crate::commands::fetch_modrinth_modpack_project(&server.uri(), "abc")
            .await
            .unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert!(p.body_html.contains("<h1>"));
        assert_eq!(p.gallery[0].url, "https://media.modrinth.com/g.png");
        assert_eq!(p.website_url.as_deref(), Some("https://src.example"));
    }

    fn ver(num: &str, date: &str) -> crate::mods::modpack::schema::ModpackVersionEntry {
        crate::mods::modpack::schema::ModpackVersionEntry {
            id: format!("id-{num}"),
            name: num.into(),
            version_number: num.into(),
            game_versions: vec!["1.20.1".into()],
            loaders: vec!["fabric".into()],
            date_published: date.into(),
        }
    }

    #[test]
    fn latest_newer_picks_newest_when_different() {
        let list = vec![
            ver("1.0", "2026-01-01T00:00:00Z"),
            ver("1.2", "2026-03-01T00:00:00Z"),
            ver("1.1", "2026-02-01T00:00:00Z"),
        ];
        // current id is "id-1.0"; newest by date is "1.2" → id "id-1.2"
        let r = crate::commands::latest_newer(list, "id-1.0");
        assert_eq!(r.map(|v| v.id), Some("id-1.2".to_string()));
    }

    #[test]
    fn latest_newer_none_when_already_latest() {
        let list = vec![
            ver("1.2", "2026-03-01T00:00:00Z"),
            ver("1.0", "2026-01-01T00:00:00Z"),
        ];
        // current id IS the newest → no update
        assert!(crate::commands::latest_newer(list, "id-1.2").is_none());
    }

    #[test]
    fn latest_newer_none_for_empty_list() {
        assert!(crate::commands::latest_newer(vec![], "id-1.0").is_none());
    }
}
