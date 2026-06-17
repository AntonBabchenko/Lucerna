use super::*;

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
    quick_play: Option<crate::launch::QuickPlay>,
) -> Result<u32, crate::error::Error> {
    // Don't launch on top of a repair that's rewriting this instance's shared
    // library/client jars — the JVM could read a half-written file and crash.
    if crate::verify::repair_in_progress() {
        return Err(crate::error::Error::InstanceBusy);
    }
    // Boundary-validate the quick-play target (path segment / address) before
    // doing any launch work.
    if let Some(qp) = &quick_play {
        qp.validate()?;
    }
    let effective_id = resolve_instance_effective_id(&app, &instance_id)?;
    let json_path = crate::paths::instance_json(&app, &instance_id)
        .map_err(|e| crate::error::Error::io("<instance_json>", e))?;
    let instance = crate::instances::store::read_instance_json(&json_path)?;
    let account =
        crate::accounts::get_active_account(&app)?.ok_or(crate::error::Error::AccountNotSet)?;
    crate::launch::start(
        &instance,
        &effective_id,
        &account,
        &app,
        quick_play.as_ref(),
    )
    .await
}

/// Whether this instance's installed version supports MC 1.20+ Quick Play.
/// Honest signal: parses the effective version JSON and checks for a
/// quick-play feature-gated game arg (robust across release/snapshot/loader).
/// Returns `false` (not an error) when the version JSON is absent (instance
/// not yet installed, or merged profile not yet written) or unparseable —
/// the UI simply hides the entry points.
#[tauri::command]
#[specta::specta]
pub async fn instance_quick_play_support(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<bool, crate::error::Error> {
    let effective_id = resolve_instance_effective_id(&app, &instance_id)?;
    let versions = crate::paths::versions_dir(&app)
        .map_err(|e| crate::error::Error::io("<versions_dir>", e))?;
    let json_path = versions
        .join(&effective_id)
        .join(format!("{effective_id}.json"));
    let Ok(json) = std::fs::read_to_string(&json_path) else {
        return Ok(false);
    };
    let Ok(details) = crate::versions::version_json::parse(&json) else {
        return Ok(false);
    };
    Ok(crate::launch::args::details_has_quick_play(&details))
}

/// Integrity verification of an instance's installed files. The hashing pass is
/// read-only; on a cold cache the manifest fetch inside it may write the version
/// JSON (offline no-op for an already-installed instance — the normal case).
/// Blocked while a game is running (can't hash a live game's files).
#[tauri::command]
#[specta::specta]
pub async fn verify_instance(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<crate::verify::VerifyReport, crate::error::Error> {
    if crate::launch::spawn::is_running() {
        return Err(crate::error::Error::InstanceBusy);
    }
    let effective_id = resolve_instance_effective_id(&app, &instance_id)?;
    let report = crate::verify::verify_instance_report(&instance_id, &effective_id, &app).await?;
    // Best-effort: a successful verify/repair is valuable even if we can't
    // persist the badge status. Log, don't fail the command.
    if let Err(e) = persist_integrity(&app, &instance_id, &report) {
        crate::diag!("verify: failed to persist integrity for {instance_id}: {e}");
    }
    Ok(report)
}

/// Repair the instance's broken/missing files, then return the post-repair
/// report. Blocked while a game is running.
#[tauri::command]
#[specta::specta]
pub async fn repair_instance(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<crate::verify::VerifyReport, crate::error::Error> {
    if crate::launch::spawn::is_running() {
        return Err(crate::error::Error::InstanceBusy);
    }
    // Mark repair-in-progress for the whole rewrite so a concurrent launch is
    // rejected (closes the TOCTOU between the is_running() check above and the
    // minutes-long file rewrite below). Also rejects a second concurrent repair.
    let _repair_guard =
        crate::verify::RepairGuard::acquire().ok_or(crate::error::Error::InstanceBusy)?;
    let effective_id = resolve_instance_effective_id(&app, &instance_id)?;
    let report = crate::verify::repair_instance_report(&instance_id, &effective_id, &app).await?;
    // Best-effort: a successful verify/repair is valuable even if we can't
    // persist the badge status. Log, don't fail the command.
    if let Err(e) = persist_integrity(&app, &instance_id, &report) {
        crate::diag!("verify: failed to persist integrity for {instance_id}: {e}");
    }
    Ok(report)
}

/// Kill the running Minecraft process if any. Idempotent.
#[tauri::command]
#[specta::specta]
pub fn stop_minecraft() -> Result<(), crate::error::Error> {
    crate::launch::stop()
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

/// Adaptive memory bounds for the per-instance heap slider, derived from total
/// physical RAM. All MB values; `u32` (RAM-in-MB fits) to avoid the specta
/// `u64`→`f64` IPC quirk.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct MemoryBounds {
    pub min_mb: u32,
    pub max_mb: u32,
    pub recommended_max_mb: u32,
    pub step_mb: u32,
    pub ram_known: bool,
}

#[tauri::command]
#[specta::specta]
pub fn instance_memory_bounds() -> MemoryBounds {
    let ram = crate::platform::total_system_ram_mb();
    MemoryBounds {
        min_mb: crate::instances::memory::slider_min_mb(),
        max_mb: crate::instances::memory::slider_max_mb(ram),
        recommended_max_mb: crate::instances::memory::recommended_max_mb(ram),
        step_mb: crate::instances::memory::slider_step_mb(),
        ram_known: ram.is_some(),
    }
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
