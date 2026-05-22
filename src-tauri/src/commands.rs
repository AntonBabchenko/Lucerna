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
        message: format!("Hello, {name}! — FTlauncher is alive."),
    }
}

/// List all stored accounts.
#[tauri::command]
#[specta::specta]
pub fn list_accounts(app: tauri::AppHandle) -> Result<Vec<crate::accounts::store::Account>, crate::error::Error> {
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
    let account = crate::accounts::get_active_account(&app)?
        .ok_or(crate::error::Error::AccountNotSet)?;
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
        .map_err(|e| {
            crate::error::Error::io(dir.display().to_string(), format!("opener: {e}"))
        })?;
    Ok(())
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
pub fn set_active_instance(
    app: tauri::AppHandle,
    id: String,
) -> Result<(), crate::error::Error> {
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
pub fn delete_instance(
    app: tauri::AppHandle,
    id: String,
) -> Result<(), crate::error::Error> {
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

#[tauri::command]
#[specta::specta]
pub fn set_instance_version(
    app: tauri::AppHandle,
    id: String,
    mc_version: String,
) -> Result<crate::instances::schema::InstanceWithStatus, crate::error::Error> {
    crate::instances::set_instance_version(&app, &id, mc_version)
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
    mc_version: String,
    loader: LoaderKind,
) -> crate::error::Result<Vec<ModVersion>> {
    platform_for(source)
        .versions(&project_id, &mc_version, loader)
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
/// `{root}/ftlauncher/installed-mods.json`.
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
    let vs = platform.versions(&vr.project_id, mc, loader).await?;
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

/// Install `primary` plus all server-resolved required dependencies, plus
/// any user-checked `optional_deps`. Emits:
///   - `mod-install-progress` repeatedly during downloads,
///   - `mod-installed` once per mod that lands successfully,
///   - `mod-install-failed` if any single install errors (the run halts
///     after the first failure; previously-installed mods are kept).
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
) -> crate::error::Result<()> {
    let inst_root = instance_root(&app, &instance_id)?;
    let dd = data_dir(&app)?;
    let (mc_version, loader) = read_active_mc_and_loader(&app, &instance_id)?;

    let mut platform = platform_for(primary.source);
    let primary_v = find_version(&mut platform, &primary, &mc_version, loader).await?;
    let resolved = platform
        .resolve_deps(&primary_v, &mc_version, loader)
        .await?;

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

    // Install required deps first, then primary, then user-checked optional deps.
    let mut install_seq: Vec<ModVersion> =
        resolved.required.into_iter().map(|r| r.version).collect();
    install_seq.push(primary_v.clone());
    for opt in optional_deps {
        if let Some(v) = resolved
            .optional
            .iter()
            .find(|r| version_matches(&r.version, &opt))
            .cloned()
        {
            install_seq.push(v.version);
        }
    }

    for v in install_seq {
        let v_project_id = v.project_id.clone();
        match crate::mods::install::install_one(&dd, &inst_root, v.clone(), &prog).await {
            Ok(inst) => {
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
    Ok(())
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
    use crate::mods::updates::{classify_update, eligible_identity, ModUpdateCheck, ModUpdateState};

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
            .versions(&project_id, &mc_version, loader)
            .await
        {
            Ok(versions) => classify_update(m, &versions),
            Err(e) => ModUpdateState::CheckFailed { reason: e.to_string() },
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
    let required_deps: Vec<ModVersion> =
        resolved.required.into_iter().map(|r| r.version).collect();

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
    match crate::mods::install::update_one(
        &dd,
        &inst_root,
        &old_sha1,
        target,
        required_deps,
        &prog,
    )
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
    let bytes = tokio::fs::read(&jar_path)
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
    let bytes = tokio::fs::read(&jar_path)
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
/// persist anything. Only a successful ping causes the key to be written
/// to the OS keyring.
#[tauri::command]
#[specta::specta]
pub async fn mods_set_curseforge_key(key: String) -> crate::error::Result<()> {
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
    cf_keyring::set(&key)
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
            let url = format!(
                "https://api.modrinth.com/v2/project/{project_id}/version/{version_id}"
            );
            let resp = crate::network::request::get(
                &url,
                &[("user-agent", "AntonBabchenko/FTlauncher")],
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
        .join("ftlauncher")
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
    let summary = crate::mods::modpack::import::inspect(&bytes, "https://api.curseforge.com").await?;
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
        .ok_or_else(|| crate::error::Error::ModsNotFound { platform: "pack_origin".into() })?;
    let bytes = tokio::fs::read(&mrpack_path).await.map_err(|e| crate::error::Error::Io {
        path: mrpack_path.clone(),
        details: e.to_string(),
    })?;
    let summary = crate::mods::modpack::import::inspect(&bytes, "https://api.curseforge.com").await?;
    let diff = crate::mods::modpack::import::compute_update_diff(
        &summary, &origin, &inst.mc_version, inst.loader, &inst.loader_version,
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
            &dd, &f.url, &f.sha1.to_ascii_lowercase(), f.size, "modpacks", &install_progress,
        )
        .await?;
    }

    // ---- Phase 2: apply locally (cache is warm). ----
    for f in diff.removed.iter().chain(diff.updated.iter().map(|e| &e.old)) {
        remove_pack_file(&inst_root, f).await?;
    }
    for f in diff.added.iter().chain(diff.updated.iter().map(|e| &e.new)) {
        if f.install_path.starts_with("mods/") {
            let mv = crate::mods::modpack::import::modpack_file_to_mod_version(
                f, &summary.game_version, summary.loader,
            );
            crate::mods::install::install_one(&dd, &inst_root, mv, &install_progress).await?;
        } else {
            crate::mods::install::install_asset(
                &dd, &inst_root, &f.url, &f.sha1, f.size, &f.install_path, &install_progress,
            )
            .await?;
        }
    }

    // Rewrite pack_origin: new files[] entries + carried-over bundled.
    let bundled: Vec<crate::mods::installed::PackOriginFile> =
        origin.files.iter().filter(|f| f.url.is_empty()).cloned().collect();
    let selected: Vec<&crate::mods::modpack::schema::ModpackFile> =
        summary.files.iter().filter(|f| !f.url.is_empty()).collect();
    let mut new_origin = crate::mods::modpack::import::build_pack_origin(
        &summary, &selected, origin.project_id.clone(), &origin.project_name,
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
    let _ = on_progress.send(ModpackProgress::Done { instance_id: instance_id.clone() });
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
        _ => return Err(crate::error::Error::ModsNotFound { platform: "modrinth".into() }),
    };

    let temp_path = modpack_fetch_to_temp(
        app.clone(),
        crate::mods::platform::ModSource::Modrinth,
        project_id,
        version_id,
    )
    .await?;
    let bytes = tokio::fs::read(&temp_path).await.map_err(|e| crate::error::Error::Io {
        path: temp_path.clone(),
        details: e.to_string(),
    })?;
    crate::mods::modpack::overrides::extract(&bytes, &inst_root, |c, t| {
        let _ = on_progress.send(ModpackProgress::ExtractingOverrides { current: c, total: t });
    })
    .await?;
    let _ = on_progress.send(ModpackProgress::Done { instance_id: instance_id.clone() });
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
    let path = crate::paths::app_file(&app)
        .map_err(|e| crate::error::Error::io("<app_file>", e))?;
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
    let path = crate::paths::app_file(&app)
        .map_err(|e| crate::error::Error::io("<app_file>", e))?;
    let mut current = crate::instances::store::read_app_json(&path)?;
    current.onboarding.tour_completed_version = Some(version);
    crate::instances::store::write_app_json(&path, &current)
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
        assert!(g.message.contains("FTlauncher"));
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
            Err(Error::InstanceNameTooLong { max: 32, actual: 33 })
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
            Err(Error::InstanceNameTooLong { max: 32, actual: 33 })
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
        std::env::set_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let entries =
            crate::commands::fetch_modpack_versions(&server.uri(), "abc").await.unwrap();
        std::env::remove_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS");
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
        std::env::set_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let err = crate::commands::fetch_modpack_versions(&server.uri(), "missing")
            .await
            .unwrap_err();
        std::env::remove_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS");
        assert!(matches!(err, crate::error::Error::ModsNotFound { .. }), "got: {err:?}");
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
        let list = vec![ver("1.2", "2026-03-01T00:00:00Z"), ver("1.0", "2026-01-01T00:00:00Z")];
        // current id IS the newest → no update
        assert!(crate::commands::latest_newer(list, "id-1.2").is_none());
    }

    #[test]
    fn latest_newer_none_for_empty_list() {
        assert!(crate::commands::latest_newer(vec![], "id-1.0").is_none());
    }
}
