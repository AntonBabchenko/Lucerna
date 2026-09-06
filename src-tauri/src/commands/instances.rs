use super::*;
use crate::launch::guardrail::{ram_warning, RamWarning, RAM_WARN_PERCENT};

/// Install (idempotently) the given instance's version. Does NOT launch
/// — the UI shows an Install button when the instance is not ready and
/// a Play button once it is. Emits `installProgress` during the run.
/// Resolves version+loader from `instance.json` server-side.
///
/// Returns the per-file install report. It rides the RETURN VALUE, never the
/// `installProgress` event: a Tauri payload of 8192 bytes or more is delivered
/// AFTER the command response, so a report carried on the progress path would
/// race the caller that needs it. Same reasoning as `InstallSummary::details`.
///
/// An empty vec is impossible on success — the version JSON row alone is
/// always pushed — so the frontend can treat `Ok` as "there is a report".
#[tauri::command]
#[specta::specta]
pub async fn install_instance(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<Vec<crate::tasks::TaskDetail>, crate::error::Error> {
    crate::data_root::reject_if_fallen_back(&app)?;
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
    crate::data_root::reject_if_fallen_back(&app)?;
    // Don't launch on top of a repair that's rewriting this instance's shared
    // library/client jars — the JVM could read a half-written file and crash.
    // Same reasoning for a datapack update mid-flight: it is a download plus
    // one level.dat rewrite per world, the game rewrites level.dat on save
    // and exit, and whichever side loses the race silently reverts the
    // user's enable/disable choices. The forward direction (no datapack
    // write while the game runs) is `datapacks::guard`. And the same again
    // for a world migration holding this instance's maintenance claim
    // (`instances::maintenance`): its world is mid-rename or mid-copy in
    // saves/. Deliberately NOT `write_allowed` — launch must not refuse on
    // `is_running` here (a same-id relaunch is refused inside `launch::start`
    // by `claim_start`, and other ids may run side by side); only the claim
    // is checked, and `start` re-checks it after claiming.
    if crate::verify::repair_in_progress()
        || crate::datapacks::guard::update_in_progress()
        || crate::instances::maintenance::maintenance_is_active(&instance_id)
    {
        return Err(crate::error::Error::InstanceBusy);
    }
    // Stop here rather than letting the JVM die on an InvalidPathException. We
    // pass the game dir and the natives dir as argv, and the Windows JVM
    // launcher decodes argv through the system ANSI code page — so a path that
    // code page cannot express reaches java.exe with `?` substituted and the
    // game never starts. `pre_launch_check` is advisory by design and cannot
    // block; this is the gate.
    match crate::instances::path_status(&app, &instance_id)? {
        crate::instances::PathStatus::Ok => {}
        crate::instances::PathStatus::InstanceDir => {
            return Err(crate::error::Error::PathNotLaunchable { data_root: false })
        }
        crate::instances::PathStatus::DataRoot => {
            return Err(crate::error::Error::PathNotLaunchable { data_root: true })
        }
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
    // If the active Microsoft account's MC token is at/near expiry, renew it
    // now so the JVM launches with a live token instead of one that expires
    // mid-session. Never blocks launch: if the refresh fails (offline, MS
    // down), we fall back to the stored token/account.
    let account = refresh_microsoft_if_expiring(&app, account).await;
    // Gate accounts Minecraft can't actually play with (e.g. an offline name it
    // rejects, like Cyrillic) — see ensure_account_launchable for the rationale.
    crate::accounts::ops::ensure_account_launchable(&account)?;
    crate::launch::start(
        &instance,
        &effective_id,
        &account,
        &app,
        quick_play.as_ref(),
    )
    .await
}

/// Seconds of headroom before token expiry at which we proactively refresh a
/// Microsoft account's tokens on launch. A token that expires within this
/// window is treated as "expiring" so we renew before the JVM starts rather
/// than letting it lapse mid-session.
const MS_TOKEN_REFRESH_BUFFER_SECS: f64 = 120.0;

/// If `account` is a Microsoft account whose token is at/near expiry, refresh
/// it and return the refreshed account. On any refresh failure (offline, MS
/// unreachable, account since removed) this degrades to the passed-in account
/// so launch is never blocked — a possibly-stale token is better than refusing
/// to launch. Non-Microsoft accounts (offline) are returned unchanged.
async fn refresh_microsoft_if_expiring(
    app: &tauri::AppHandle,
    account: crate::accounts::Account,
) -> crate::accounts::Account {
    if account.kind != crate::accounts::AccountKind::Microsoft {
        return account;
    }
    // `expires_at == None` on a Microsoft account is unexpected (sign-in always
    // sets it); treat it as "not expiring" and leave the account untouched.
    let Some(expires_at) = account.expires_at else {
        return account;
    };
    if expires_at > crate::accounts::now_secs() + MS_TOKEN_REFRESH_BUFFER_SECS {
        return account;
    }
    match crate::accounts::microsoft::refresh(app, &account.id).await {
        Ok(refreshed) => refreshed,
        Err(e) => {
            crate::diag!(
                "launch: MS token refresh failed for {}, launching with stored token: {e}",
                account.id
            );
            account
        }
    }
}

/// Whether this instance's installed version supports MC 1.20+ Quick Play.
/// Honest signal: parses the effective version JSON and checks for a
/// quick-play feature-gated game arg (robust across release/snapshot/loader).
/// Returns `false` (not an error) when the version JSON is absent (instance
/// not yet installed, or merged profile not yet written) or unparseable —
/// the UI simply hides the entry points. An UNREADABLE file is an error:
/// answering `false` there would claim knowledge the read never produced.
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
    quick_play_support_at(&json_path)
}

/// Pure-path core of `instance_quick_play_support`, split out so the fs
/// discrimination is unit-testable without an `AppHandle` (the house
/// pattern — there is no `AppHandle` test harness).
fn quick_play_support_at(json_path: &std::path::Path) -> Result<bool, crate::error::Error> {
    let json = match std::fs::read_to_string(json_path) {
        Ok(json) => json,
        // Absent is a fact: instance not yet installed, or merged profile
        // not yet written — Quick Play is genuinely unavailable and the UI
        // hides the entry points. Any other error is ignorance, not
        // "unsupported".
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(crate::error::Error::io(json_path.display().to_string(), e)),
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
    // gates on ANY running instance: repair/verify touch SHARED libraries/versions dirs, not per-instance files
    if crate::launch::spawn::is_any_running() {
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
    // gates on ANY running instance: repair/verify touch SHARED libraries/versions dirs, not per-instance files
    if crate::launch::spawn::is_any_running() {
        return Err(crate::error::Error::InstanceBusy);
    }
    // Mark repair-in-progress for the whole rewrite so a concurrent launch is
    // rejected (closes the TOCTOU between the is_running() check above and the
    // minutes-long file rewrite below). Also rejects a second concurrent repair.
    let _repair_guard =
        crate::verify::RepairGuard::acquire().ok_or(crate::error::Error::InstanceBusy)?;
    let effective_id = resolve_instance_effective_id(&app, &instance_id)?;
    let (report, fixed) =
        crate::verify::repair_instance_report(&instance_id, &effective_id, &app).await?;
    // Best-effort: a successful verify/repair is valuable even if we can't
    // persist the badge status. Log, don't fail the command.
    if let Err(e) = persist_integrity(&app, &instance_id, &report) {
        crate::diag!("verify: failed to persist integrity for {instance_id}: {e}");
    }
    // Journal only what was actually fixed. A verify that found nothing wrong
    // changed no files, and a repair that degraded without fixing anything
    // changed none either — a history row for those would claim work that never
    // happened.
    if fixed > 0 {
        if let Ok(inst_root) = instance_root(&app, &instance_id) {
            crate::journal::record(
                &inst_root,
                crate::journal::content_bulk(
                    crate::journal::ContentAction::IntegrityRepaired,
                    "",
                    fixed,
                ),
            );
        }
    }
    Ok(report)
}

/// Kill the running Minecraft process for `instance_id` if any. Idempotent.
#[tauri::command]
#[specta::specta]
pub fn stop_instance(instance_id: String) -> Result<(), crate::error::Error> {
    crate::launch::spawn::stop(&instance_id)
}

/// Whether this instance's Minecraft can load data packs at all (the system
/// arrived in 1.13). `true` when `mc_version` is empty or unparseable —
/// uncertainty must not hide the feature. Derived from `instance.json` alone,
/// deliberately NOT from the client jar (`l10n::pack_format` reads the jar and
/// answers unknown for a never-installed instance, which would hide the kind
/// for a perfectly legitimate 1.21 instance).
#[tauri::command]
#[specta::specta]
pub fn instance_supports_datapacks(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<bool, crate::error::Error> {
    let instance = crate::instances::read_instance(&app, &instance_id)?;
    Ok(crate::datapacks::compat::supports_datapacks(
        &instance.mc_version,
    ))
}

/// Whether a Minecraft version can load data packs at all (they arrived in
/// 1.13). `true` for an empty or unparseable version — uncertainty must not
/// hide the feature.
///
/// Version-keyed rather than id-keyed, unlike its instance twin above. That
/// one reads `instance.json` because the Manage modal changes an instance's
/// Minecraft IN PLACE, so a list row can be stale. A server has no such flow —
/// `mc_version` is written at create/import and no command edits it — and
/// `ServerWithStatus.mc_version` is already on the object the caller holds. So
/// this takes the string, needs no `AppHandle` and no file read, and its
/// answer caches under one key for every server on that version.
#[tauri::command]
#[specta::specta]
pub fn mc_version_supports_datapacks(mc_version: String) -> bool {
    crate::datapacks::compat::supports_datapacks(&mc_version)
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
/// writes `instance.json`. `max_heap_mb` is the heap picked in the create form;
/// `None` means "assign the adaptive default". Any supplied value is clamped
/// onto the slider grid and range — it arrives over IPC and is not trusted.
/// `extra_jvm_args` defaults to `""`.
#[tauri::command]
#[specta::specta]
pub fn create_instance(
    app: tauri::AppHandle,
    name: String,
    mc_version: String,
    loader: crate::instances::schema::LoaderKind,
    loader_version: Option<String>,
    max_heap_mb: Option<u32>,
) -> Result<crate::instances::schema::InstanceWithStatus, crate::error::Error> {
    crate::data_root::reject_if_fallen_back(&app)?;
    validate_instance_name(&name)?;
    crate::instances::create_instance(
        &app,
        name,
        mc_version,
        loader,
        loader_version,
        max_heap_mb,
        crate::instances::schema::PackOrigin::default(),
        None,
        None,
    )
}

/// Delete an instance. If it was active, auto-switches to oldest remaining.
/// Errors `LastInstance` if it's the only one left.
#[tauri::command]
#[specta::specta]
pub async fn delete_instance(app: tauri::AppHandle, id: String) -> Result<(), crate::error::Error> {
    // Refuse while a game is running or starting: on Windows the live JVM
    // holds OS locks on the instance dir, so remove_dir_all can partially fail
    // and corrupt it. And while a maintenance claim is held — a world
    // migration reading from or writing into this instance — the tree is in
    // flight. One gate for all three (`instances::maintenance::write_allowed`),
    // the same one every world and datapack writer opens with.
    crate::instances::maintenance::write_allowed(&id)?;
    // Async + spawn_blocking (mirrors `world_import` in commands/worlds.rs): the
    // domain call walks the whole instance list, then remove_dir_all's a
    // potentially multi-GB instance dir — a sync command spends all of that on
    // the main thread with the window frozen. The `LastInstance` check stays
    // INSIDE `instances::delete_instance`, unchanged: it must be evaluated
    // against the same listing that picks the auto-switch target, so moving it
    // out here would open a gap between the check and the switch.
    tokio::task::spawn_blocking(move || crate::instances::delete_instance(&app, &id))
        .await
        .map_err(|e| crate::error::Error::io("<delete_instance>", format!("join: {e}")))?
}

/// Rename an instance's directory. Also the "repair unlaunchable folder name"
/// action — one operation, the dialog just pre-fills it differently.
///
/// Renaming changes the instance's identity, because the directory name IS the
/// id. Shortcuts survive it: they carry the instance's `uid`, which this
/// generates first for instances that predate the field.
#[tauri::command]
#[specta::specta]
pub fn rename_instance_dir(
    app: tauri::AppHandle,
    id: String,
    new_name: String,
) -> Result<crate::instances::schema::InstanceWithStatus, crate::error::Error> {
    crate::data_root::reject_if_fallen_back(&app)?;
    // Running, starting, or under a maintenance claim (a world migration holds
    // every path under this root) — the shared gate every writer opens with.
    // Plus the repair guard `launch_instance` also checks: a repair is
    // rewriting files under the root about to move. The op queue is frontend
    // state and cannot be consulted from here; a mod install racing this is a
    // known limitation whose worst case is a failed `fs::rename` with a typed
    // error, not corruption.
    crate::instances::maintenance::write_allowed(&id)?;
    if crate::verify::repair_in_progress() {
        return Err(crate::error::Error::InstanceBusy);
    }
    // Before the rename, so shortcuts created afterwards survive the NEXT one.
    crate::instances::ensure_uid(&app, &id)?;

    let parent = crate::paths::instances_dir(&app)
        .map_err(|e| crate::error::Error::io("<instances_dir>", e))?;
    let new_id = crate::instances::rename::rename_dir(&parent, &id, &new_name)?;

    crate::instances::repoint_active_instance(&app, &id, &new_id)?;
    crate::instances::read_with_status(&app, &new_id)
}

/// Whether this instance's path can be handed to the JVM, and if not, which
/// part is at fault. Drives the Manage warning banner, so the user learns
/// before pressing Play rather than from a Fabric crash window.
#[tauri::command]
#[specta::specta]
pub fn instance_path_status(
    app: tauri::AppHandle,
    id: String,
) -> Result<crate::instances::PathStatus, crate::error::Error> {
    crate::instances::path_status(&app, &id)
}

/// What directory name a display name would produce.
///
/// Exists so the rename dialog can preview the slug without duplicating the
/// rules in TypeScript, where they would drift. Returns an empty string when
/// nothing usable survives, which the dialog renders as "cannot use this name".
#[tauri::command]
#[specta::specta]
pub fn preview_instance_dir_name(name: String) -> String {
    crate::naming::derive_base(&name, None, "")
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

    // Running, starting, or under a maintenance claim: a world migration's
    // plan was computed against THIS version, and the game rewrites
    // `instance.json`-adjacent state on exit. The shared gate, same as every
    // other writer.
    instances::maintenance::write_allowed(&id)?;
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
    if crate::launch::spawn::is_running(&id) {
        return Err(crate::error::Error::InstanceBusy);
    }
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

/// Set the optional JVM initial heap (`-Xms`). `None`/0 clears it.
#[tauri::command]
#[specta::specta]
pub fn set_instance_min_heap(
    app: tauri::AppHandle,
    id: String,
    min_heap_mb: Option<u32>,
) -> Result<crate::instances::schema::InstanceWithStatus, crate::error::Error> {
    crate::instances::set_instance_min_heap(&app, &id, min_heap_mb)
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
    /// Heap a NEW instance gets when the user doesn't pick one. Exposed so the
    /// create form can seed its slider without re-deriving the policy in TS.
    pub default_mb: u32,
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
        default_mb: crate::instances::memory::default_heap_mb(ram),
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
    if crate::launch::spawn::is_running(&id) {
        return Err(crate::error::Error::InstanceBusy);
    }
    crate::instances::detach_instance_pack(&app, &id)
}

/// Clone an instance: same MC version + loader, granular content options.
/// Mods (with their installed-mods registry) and the custom icon always
/// travel with the clone; see `instances::clone` for the exact mapping.
/// The clone does NOT become the active instance.
#[tauri::command]
#[specta::specta]
pub async fn clone_instance(
    app: tauri::AppHandle,
    source_id: String,
    new_name: String,
    options: crate::instances::clone::CloneOptions,
    on_progress: tauri::ipc::Channel<crate::instances::clone::CloneProgress>,
) -> Result<crate::instances::schema::InstanceWithStatus, crate::error::Error> {
    crate::data_root::reject_if_fallen_back(&app)?;
    validate_instance_name(&new_name)?;
    // Copying files a live JVM is writing produces torn saves; copying saves/
    // while a world migration is staging a world in it clones a half-copied
    // tree. The shared gate (running, starting, or under a maintenance claim)
    // — the same one delete/verify use.
    crate::instances::maintenance::write_allowed(&source_id)?;
    crate::instances::clone::clone_instance(
        &app,
        &source_id,
        new_name,
        &options,
        move |category, current, total| {
            let _ = on_progress.send(crate::instances::clone::CloneProgress {
                category,
                current,
                total,
            });
        },
    )
}

/// Content categories present in an instance (file counts + byte totals) for
/// the clone dialog's checkbox labels. Reuses the launcher-import scanner.
#[tauri::command]
#[specta::specta]
pub async fn clone_instance_scan(
    app: tauri::AppHandle,
    id: String,
) -> Result<Vec<crate::instances::import::model::ContentEntry>, crate::error::Error> {
    // Existence check so an unknown id errors rather than returning [].
    let _ = crate::instances::read_instance(&app, &id)?;
    let mc = crate::paths::minecraft_dir(&app, &id)
        .map_err(|e| crate::error::Error::io("<minecraft_dir>", e))?;
    Ok(crate::instances::import::model::scan_content(&mc))
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

/// Store a custom picture for an instance. `png_base64` is a PNG produced by
/// the crop UI; it is normalized to 256x256 before it is written.
#[tauri::command]
#[specta::specta]
pub fn set_instance_icon(
    app: tauri::AppHandle,
    instance_id: String,
    png_base64: String,
) -> Result<(), crate::error::Error> {
    let path = crate::paths::instance_icon_png(&app, &instance_id)
        .map_err(|e| crate::error::Error::io("<instance icon path>", e))?;
    crate::instances::icon::write_icon(&path, &png_base64)?;
    refresh_shortcut_icon(&app, &instance_id, &path)
}

/// Remove an instance's custom picture (back to the letter avatar). Idempotent.
#[tauri::command]
#[specta::specta]
pub fn clear_instance_icon(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<(), crate::error::Error> {
    let path = crate::paths::instance_icon_png(&app, &instance_id)
        .map_err(|e| crate::error::Error::io("<instance icon path>", e))?;
    crate::instances::icon::clear_icon(&path)?;
    refresh_shortcut_icon(&app, &instance_id, &path)
}

/// Follow a picture change into `<instance>/icon.ico`, so desktop shortcuts made
/// earlier keep showing the right image — and keep showing *something* when the
/// picture is cleared. A no-op for instances that never had a shortcut.
fn refresh_shortcut_icon(
    app: &tauri::AppHandle,
    instance_id: &str,
    png: &std::path::Path,
) -> Result<(), crate::error::Error> {
    let ico = crate::paths::instance_icon_ico(app, instance_id)
        .map_err(|e| crate::error::Error::io("<instance icon path>", e))?;
    crate::shortcuts::icon::refresh_if_present(png, &ico)
}

/// The instance's custom picture as a base64 PNG, or `None` when it has none.
/// Cosmetic: mirrors `account_skin`.
#[tauri::command]
#[specta::specta]
pub fn instance_icon(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<Option<crate::instances::icon::InstanceIcon>, crate::error::Error> {
    let path = crate::paths::instance_icon_png(&app, &instance_id)
        .map_err(|e| crate::error::Error::io("<instance icon path>", e))?;
    crate::instances::icon::read_icon(&path)
}

// ---------------------------------------------------------------------------
// Multi-instance launch: soft pre-launch checks + running-instance snapshot
// ---------------------------------------------------------------------------

/// The active account is already launching a running instance — the same
/// account can't hold two live online sessions on a real server.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct AccountConflict {
    pub account_name: String,
    pub running_instance_id: String,
    /// The candidate account's kind. The FE picks the warning copy from this:
    /// Microsoft means a real online-session drop; Offline means only a
    /// same-name collision risk on `online-mode=false` servers.
    pub account_kind: crate::accounts::AccountKind,
}

/// Aggregate of the soft, non-blocking warnings shown before a launch.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct PreLaunchCheck {
    pub resource_warning: Option<RamWarning>,
    pub account_conflict: Option<AccountConflict>,
}

/// One currently-running instance, for the aggregate running-instances popover.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct RunningInstanceInfo {
    pub instance_id: String,
    pub pid: u32,
    pub max_heap_mb: u32,
    /// Unix ms the in-flight playtime session started (for live elapsed
    /// display). `f64` not `i64` — specta forbids BigInt-style exports; a
    /// millisecond timestamp is exact in `f64` (well under 2^53).
    pub started_unix_ms: Option<f64>,
}

/// Soft, non-blocking pre-launch warnings for `instance_id`. The UI decides
/// whether to proceed; `launch_instance` does NOT re-run these checks.
#[tauri::command]
#[specta::specta]
pub fn pre_launch_check(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<PreLaunchCheck, crate::error::Error> {
    // Advisory read-only check: intentionally does NOT call
    // data_root::reject_if_fallen_back — the real launch_instance still gates on it.
    let inst = crate::instances::read_instance(&app, &instance_id)?;
    // `total_system_ram_mb` is `Option<u64>` (None when the OS query fails).
    // `clamp_heap_mb` consumes the Option directly; `ram_warning` wants a plain
    // `u64` and treats 0 as "unknown → never warn", so map None to 0.
    let total_ram_mb = crate::platform::total_system_ram_mb();
    let candidate_mb = crate::launch::args::clamp_heap_mb(inst.max_heap_mb, total_ram_mb);
    // Exclude the candidate itself: a double-click Play (before the button
    // flips) or a future "restart" must not count its own heap or conflict with
    // its own account.
    let running_heaps: Vec<u32> = crate::launch::spawn::running_snapshot()
        .iter()
        .filter(|(id, _, _)| id != &instance_id)
        .map(|(_, _, h)| *h)
        .collect();
    let resource_warning = ram_warning(
        &running_heaps,
        candidate_mb,
        total_ram_mb.unwrap_or(0),
        RAM_WARN_PERCENT,
    );

    // Determine the candidate account the SAME way `launch_instance` does — the
    // active account (see `commands::launch_instance`). A missing active account
    // is not a conflict here (the launch itself will surface `AccountNotSet`).
    let account_conflict = match crate::accounts::get_active_account(&app)? {
        Some(acct) => crate::launch::spawn::account_in_use(&acct.id, &instance_id).map(
            |running_instance_id| AccountConflict {
                account_name: acct.name.clone(),
                running_instance_id,
                account_kind: acct.kind,
            },
        ),
        None => None,
    };

    Ok(PreLaunchCheck {
        resource_warning,
        account_conflict,
    })
}

/// Every running instance, for the aggregate popover.
#[tauri::command]
#[specta::specta]
pub fn running_instances() -> Vec<RunningInstanceInfo> {
    crate::launch::spawn::running_snapshot()
        .into_iter()
        .map(|(instance_id, pid, max_heap_mb)| {
            // i64 session start → f64 for the IPC boundary (lossless for ms).
            let started_unix_ms =
                crate::launch::spawn::session_started_ms(&instance_id).map(|ms| ms as f64);
            RunningInstanceInfo {
                instance_id,
                pid,
                max_heap_mb,
                started_unix_ms,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_play_support_missing_json_is_false() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!quick_play_support_at(&dir.path().join("1.21.json")).unwrap());
    }

    #[test]
    fn quick_play_support_unparseable_json_is_false() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("1.21.json");
        std::fs::write(&path, "{ not json").unwrap();
        assert!(!quick_play_support_at(&path).unwrap());
    }

    #[test]
    fn quick_play_support_unreadable_json_is_an_error_not_false() {
        let dir = tempfile::tempdir().unwrap();
        // A directory at the file's path makes read_to_string fail with
        // something other than NotFound on every platform — the same shape
        // as a file we have no permission to read. Same trick as
        // `read_properties_file_unreadable_is_an_error_not_empty`.
        let path = dir.path().join("1.21.json");
        std::fs::create_dir(&path).unwrap();
        assert!(quick_play_support_at(&path).is_err());
    }
}
