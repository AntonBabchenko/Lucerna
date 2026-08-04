//! Multi-instance support.

pub mod clone;
pub mod icon;
pub mod ids;
pub mod import;
pub mod memory;
pub mod migrate;
pub mod rename;
pub mod scan;
pub mod schema;
pub mod status;
pub mod store;

use crate::error::{Error, Result};
use crate::paths;
use schema::{InstanceFile, InstanceWithStatus, LoaderKind};
use std::time::SystemTime;

/// All instances on disk, each with a precomputed `ready` boolean.
/// Sorted oldest-first by `created_unix_ms`.
pub fn list_instances_with_status(app: &tauri::AppHandle) -> Result<Vec<InstanceWithStatus>> {
    let app_root = paths::app_dir(app).map_err(|e| Error::io("<app_dir>", e))?;
    let versions_dir = paths::versions_dir(app).map_err(|e| Error::io("<versions_dir>", e))?;
    let instances_dir = app_root.join("instances");
    let files = scan::list_all(&instances_dir);
    Ok(files
        .into_iter()
        .map(|f| {
            let ready = status::ready_status(&versions_dir, &f);
            let has_icon = icon::has_icon(&instances_dir.join(&f.id).join("icon.png"));
            InstanceWithStatus::from_file(&f, ready, has_icon)
        })
        .collect())
}

/// Resolve `app.json.active_instance` to a fully-hydrated
/// `InstanceWithStatus`. Auto-repairs stale pointer (id not on disk):
/// picks the oldest remaining instance and rewrites `app.json`.
///
/// `None` only when there are zero instances on disk (which should not
/// normally happen because `migrate_or_seed` always seeds one; covers the
/// resilience path documented in the spec).
pub fn get_active_instance(app: &tauri::AppHandle) -> Result<Option<InstanceWithStatus>> {
    let all = list_instances_with_status(app)?;
    if all.is_empty() {
        return Ok(None);
    }
    let app_file_path = paths::app_file(app).map_err(|e| Error::io("<app_file>", e))?;
    let mut app_state = store::read_app_json(&app_file_path)?;
    if let Some(active_id) = &app_state.active_instance {
        if let Some(found) = all.iter().find(|i| &i.id == active_id) {
            return Ok(Some(found.clone()));
        }
        // Stale pointer — auto-repair by picking the oldest.
    }
    let pick = all[0].clone();
    app_state.active_instance = Some(pick.id.clone());
    store::write_app_json(&app_file_path, &app_state)?;
    Ok(Some(pick))
}

/// Set `app.json.active_instance`. Errors `InstanceNotFound` if `id` is
/// not on disk.
pub fn set_active_instance(app: &tauri::AppHandle, id: &str) -> Result<()> {
    let all = list_instances_with_status(app)?;
    if !all.iter().any(|i| i.id == id) {
        return Err(Error::InstanceNotFound { id: id.to_string() });
    }
    let app_file_path = paths::app_file(app).map_err(|e| Error::io("<app_file>", e))?;
    let mut current = store::read_app_json(&app_file_path)?;
    current.active_instance = Some(id.to_string());
    store::write_app_json(&app_file_path, &current)
}

/// Create `<instance>/.minecraft/` if missing, then open it in the OS
/// file manager. Errors `InstanceNotFound` if `id` is unknown.
pub async fn open_instance_folder(app: &tauri::AppHandle, id: &str) -> Result<()> {
    use tauri_plugin_opener::OpenerExt;
    let all = list_instances_with_status(app)?;
    if !all.iter().any(|i| i.id == id) {
        return Err(Error::InstanceNotFound { id: id.to_string() });
    }
    let dir = paths::minecraft_dir(app, id).map_err(|e| Error::io("<minecraft_dir>", e))?;
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| Error::io(dir.display().to_string(), e))?;
    app.opener()
        .open_path(dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| Error::io(dir.display().to_string(), format!("opener: {e}")))
}

/// Helper for the in-process orchestrators below — reads a single
/// instance.json by id (assumes the directory exists).
fn read_one(app: &tauri::AppHandle, id: &str) -> Result<InstanceFile> {
    let path = paths::instance_json(app, id).map_err(|e| Error::io("<instance_json>", e))?;
    store::read_instance_json(&path)
}

/// Read a single instance's on-disk record by id.
pub fn read_instance(app: &tauri::AppHandle, id: &str) -> Result<InstanceFile> {
    read_one(app, id)
}

pub(crate) fn unix_ms_f64() -> f64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

/// Reserve a readable, unique slug directory from `name` (see
/// [`crate::naming`]), mkdir `<instance>/.minecraft/`, write `instance.json`.
/// The reserved directory name is the instance id. Returns the new instance
/// with its (always-false-initially) ready status.
///
/// `mrpack` carries the origin pack's display name and version when the
/// instance is created via modpack import. Pass `None` for manually-
/// created instances; the optional fields will be omitted from disk.
///
/// The three trailing `mrpack_*` parameters (`project_id`, `source`,
/// `summary`) carry provenance metadata for the Imported drawer in the
/// UI (Modpacks tab). They are best-effort — pass `None` if the import
/// path could not look them up. `mrpack_source` is conceptually paired
/// with `mrpack` (an import either populates both or neither).
pub fn create_instance(
    app: &tauri::AppHandle,
    name: String,
    mc_version: String,
    loader: LoaderKind,
    loader_version: Option<String>,
    max_heap_mb: Option<u32>,
    pack: crate::instances::schema::PackOrigin,
    imported_from: Option<crate::instances::schema::ImportProvenance>,
    created_from_server: Option<String>,
) -> Result<InstanceWithStatus> {
    // Reserve a readable, unique directory derived from the display name. The
    // returned name IS the id (the "dir name == id" invariant is preserved, so
    // all path resolution downstream is unchanged).
    let instances_parent =
        paths::instances_dir(app).map_err(|e| Error::io("<instances_dir>", e))?;
    let (id, dir) = crate::naming::reserve_unique_dir(
        &instances_parent,
        &name,
        pack.slug.as_deref(),
        "instance",
    )?;
    // Remove the reserved directory if any step below fails (`?`), so a partial
    // create never leaks the slug (which would force every future same-name
    // create to climb -2, -3, …). Disarmed on success via `keep()`.
    let cleanup = crate::naming::DirCleanup::new(&dir);

    std::fs::create_dir_all(dir.join(".minecraft"))
        .map_err(|e| Error::io(dir.display().to_string(), e))?;
    let (mrpack_name, mrpack_version) = match pack.name_and_version {
        Some((n, v)) => (Some(n), Some(v)),
        None => (None, None),
    };
    let inst = InstanceFile {
        id: id.clone(),
        uid: Some(crate::instances::ids::new_id()),
        name,
        mc_version,
        loader,
        loader_version,
        max_heap_mb: memory::resolve_heap_mb(max_heap_mb, crate::platform::total_system_ram_mb()),
        min_heap_mb: None,
        extra_jvm_args: String::new(),
        created_unix_ms: unix_ms_f64(),
        mrpack_name,
        mrpack_version,
        mrpack_project_id: pack.project_id,
        mrpack_source: pack.source,
        mrpack_summary: pack.summary,
        mrpack_version_id: pack.version_id,
        integrity: None,
        imported_from,
        created_from_server,
        handled_log_sig: None,
    };
    let json_path = paths::instance_json(app, &id).map_err(|e| Error::io("<instance_json>", e))?;
    store::write_instance_json(&json_path, &inst)?;
    let versions_dir = paths::versions_dir(app).map_err(|e| Error::io("<versions_dir>", e))?;
    let ready = status::ready_status(&versions_dir, &inst);
    cleanup.keep();
    // A freshly reserved instance directory cannot already contain icon.png.
    Ok(InstanceWithStatus::from_file(&inst, ready, false))
}

/// This instance's rename-proof identity, creating and persisting one if the
/// instance predates the field.
///
/// Lazy on purpose: a startup migration would rewrite every `instance.json` on
/// disk to add a field that only desktop shortcuts ever read.
pub fn ensure_uid(app: &tauri::AppHandle, id: &str) -> Result<String> {
    let mut inst = read_one(app, id)?;
    if let Some(uid) = inst.uid.clone() {
        return Ok(uid);
    }
    let uid = crate::instances::ids::new_id();
    inst.uid = Some(uid.clone());
    let path = paths::instance_json(app, id).map_err(|e| Error::io("<instance_json>", e))?;
    store::write_instance_json(&path, &inst)?;
    Ok(uid)
}

/// Resolve a `--launch` token — a `uid` written by a shortcut, or a plain
/// directory name — to the instance's CURRENT directory name.
///
/// `uid` is checked first so a renamed instance still answers to the shortcut
/// that predates the rename. `None` when neither matches, which callers must
/// surface as "instance not found" rather than launching something else.
pub fn resolve_launch_target(app: &tauri::AppHandle, token: &str) -> Option<String> {
    let all = scan::list_all(&paths::instances_dir(app).ok()?);
    all.iter()
        .find(|i| i.uid.as_deref() == Some(token))
        .or_else(|| all.iter().find(|i| i.id == token))
        .map(|i| i.id.clone())
}

/// Whether this instance can be handed to the JVM at all, and if not, which
/// part of the path is at fault — the two cases have different remedies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum PathStatus {
    /// The path survives conversion to the system code page.
    Ok,
    /// The instance's own directory name is at fault — renaming it fixes this.
    InstanceDir,
    /// A data-root segment is at fault (e.g. the Windows user name under
    /// `%APPDATA%`). Renaming the instance cannot help; the data root has to
    /// move.
    DataRoot,
}

/// Classify an instance's game directory for JVM-readability.
///
/// Shared by the launch gate and the Manage banner so the two can never
/// disagree about whether an instance is launchable.
pub fn path_status(app: &tauri::AppHandle, id: &str) -> Result<PathStatus> {
    let game_dir = paths::minecraft_dir(app, id).map_err(|e| Error::io("<minecraft_dir>", e))?;
    if crate::platform::path_launchable(&game_dir) {
        return Ok(PathStatus::Ok);
    }
    let root = paths::instances_dir(app).map_err(|e| Error::io("<instances_dir>", e))?;
    Ok(if crate::platform::path_launchable(&root) {
        PathStatus::InstanceDir
    } else {
        PathStatus::DataRoot
    })
}

/// Read one instance plus its derived status, writing nothing.
///
/// Renaming needs this: it must not rewrite `instance.json`, so it cannot go
/// through [`mutate`].
pub fn read_with_status(app: &tauri::AppHandle, id: &str) -> Result<InstanceWithStatus> {
    let inst = read_one(app, id)?;
    let versions_dir = paths::versions_dir(app).map_err(|e| Error::io("<versions_dir>", e))?;
    let ready = status::ready_status(&versions_dir, &inst);
    // Re-stat icon.png so a settings edit doesn't clobber has_icon back to
    // false for an instance that already has a custom picture.
    let icon_path = paths::instance_icon_png(app, id).map_err(|e| Error::io("<icon_path>", e))?;
    let has_icon = icon::has_icon(&icon_path);
    Ok(InstanceWithStatus::from_file(&inst, ready, has_icon))
}

/// Point `app.json.active_instance` at `new_id`, but only if it currently names
/// `old_id` — renaming a non-active instance must not rewrite `app.json`.
///
/// Runs AFTER the directory rename. If it fails, the only damage is a dangling
/// `active_instance`, which the startup repair in [`migrate`] and the lazy
/// repair in [`get_active_instance`] both already heal.
pub fn repoint_active_instance(app: &tauri::AppHandle, old_id: &str, new_id: &str) -> Result<()> {
    let app_file_path = paths::app_file(app).map_err(|e| Error::io("<app_file>", e))?;
    let mut state = store::read_app_json(&app_file_path)?;
    if state.active_instance.as_deref() != Some(old_id) {
        return Ok(());
    }
    state.active_instance = Some(new_id.to_string());
    store::write_app_json(&app_file_path, &state)
}

fn mutate<F>(app: &tauri::AppHandle, id: &str, mutator: F) -> Result<InstanceWithStatus>
where
    F: FnOnce(&mut InstanceFile),
{
    let mut inst = read_one(app, id)?;
    mutator(&mut inst);
    let path = paths::instance_json(app, id).map_err(|e| Error::io("<instance_json>", e))?;
    store::write_instance_json(&path, &inst)?;
    read_with_status(app, id)
}

pub fn set_instance_name(
    app: &tauri::AppHandle,
    id: &str,
    name: String,
) -> Result<InstanceWithStatus> {
    mutate(app, id, |i| i.name = name)
}

pub fn set_instance_version(
    app: &tauri::AppHandle,
    id: &str,
    mc_version: String,
) -> Result<InstanceWithStatus> {
    mutate(app, id, |i| i.mc_version = mc_version)
}

pub fn set_instance_loader(
    app: &tauri::AppHandle,
    id: &str,
    loader: LoaderKind,
    loader_version: Option<String>,
) -> Result<InstanceWithStatus> {
    mutate(app, id, |i| {
        i.loader = loader;
        i.loader_version = match loader {
            LoaderKind::Vanilla => None,
            _ => loader_version,
        };
    })
}

/// Apply a Minecraft-version change and its re-resolved loader in ONE
/// read-modify-write. `change_instance_mc` uses this so a torn write can't
/// leave the instance with the new MC but the old (incompatible) loader
/// version — which would re-introduce the very Forge-404 stale-state bug
/// this module exists to prevent.
pub(crate) fn apply_mc_and_loader(
    app: &tauri::AppHandle,
    id: &str,
    mc_version: String,
    loader: LoaderKind,
    loader_version: Option<String>,
) -> Result<InstanceWithStatus> {
    mutate(app, id, |i| {
        i.mc_version = mc_version;
        i.loader = loader;
        i.loader_version = match loader {
            LoaderKind::Vanilla => None,
            _ => loader_version,
        };
    })
}

pub fn set_instance_memory(
    app: &tauri::AppHandle,
    id: &str,
    max_heap_mb: u32,
) -> Result<InstanceWithStatus> {
    // Same trust boundary as create: the value comes from the UI over IPC, so
    // normalize it onto the slider grid/range instead of writing it raw.
    let clamped = memory::clamp_heap_mb(max_heap_mb, crate::platform::total_system_ram_mb());
    mutate(app, id, |i| i.max_heap_mb = clamped)
}

/// Set the optional JVM initial heap (`-Xms`). `None` clears it (JVM default).
pub fn set_instance_min_heap(
    app: &tauri::AppHandle,
    id: &str,
    min_heap_mb: Option<u32>,
) -> Result<InstanceWithStatus> {
    // Treat 0 as "unset" so an empty field and an explicit 0 behave the same.
    let normalized = min_heap_mb.filter(|&m| m > 0);
    mutate(app, id, |i| i.min_heap_mb = normalized)
}

pub fn set_instance_jvm_args(
    app: &tauri::AppHandle,
    id: &str,
    extra_jvm_args: String,
) -> Result<InstanceWithStatus> {
    mutate(app, id, |i| i.extra_jvm_args = extra_jvm_args)
}

pub fn set_instance_handled_log_sig(
    app: &tauri::AppHandle,
    id: &str,
    sig: Option<String>,
) -> Result<InstanceWithStatus> {
    mutate(app, id, |i| i.handled_log_sig = sig)
}

/// Rewrite an instance's modpack-version metadata after an in-place
/// update, optionally also bumping the Minecraft / loader version.
pub fn set_instance_pack_update(
    app: &tauri::AppHandle,
    id: &str,
    mrpack_version: String,
    mc_version: String,
    loader: LoaderKind,
    loader_version: Option<String>,
    mrpack_version_id: String,
) -> Result<InstanceWithStatus> {
    mutate(app, id, |i| {
        i.mrpack_version = Some(mrpack_version);
        i.mrpack_version_id = Some(mrpack_version_id);
        i.mc_version = mc_version;
        i.loader = loader;
        i.loader_version = match loader {
            LoaderKind::Vanilla => None,
            _ => loader_version,
        };
    })
}

// ---------------------------------------------------------------------------
// MC-change report types
// ---------------------------------------------------------------------------

/// Outcome of re-resolving the loader version when MC changes.
#[derive(Debug, Clone, serde::Serialize, specta::Type, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LoaderOutcome {
    Unchanged,
    LoaderUpdated { loader: LoaderKind, version: String },
    LoaderResetToVanilla { previous_loader: LoaderKind },
}

/// Return value of `change_instance_mc`: the updated instance plus a
/// description of what happened to the loader version.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct McChangeReport {
    pub instance: InstanceWithStatus,
    pub loader_outcome: LoaderOutcome,
}

// ---------------------------------------------------------------------------
// Pure loader-decision helper (testable without I/O)
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub(crate) enum LoaderDecision {
    Use(String),
    ResetToVanilla,
    Error(crate::error::Error),
}

/// Given the result of `list_loaders` for a new MC version, decide what
/// loader version to use.
///
/// - `Ok(list)` with a stable build → `Use` the stable build.
/// - `Ok(list)` with no stable build but at least one entry → `Use` the first.
/// - `Ok([])` is unreachable (callers use the cached list that already maps
///   empty → `LoaderUnavailable`).
/// - `Err(LoaderUnavailable { .. })` → `ResetToVanilla` (loader not supported
///   for this MC version).
/// - Any other `Err` → propagate as `Error`.
///
/// `_loader` is unused today (the decision is loader-agnostic) but kept in the
/// signature, reserved for a future per-loader version-selection policy.
pub(crate) fn decide_loader(
    _loader: LoaderKind,
    list: crate::error::Result<Vec<crate::versions::loaders::LoaderVersion>>,
) -> LoaderDecision {
    match list {
        Ok(v) => match v.iter().find(|x| x.stable).or_else(|| v.first()) {
            Some(lv) => LoaderDecision::Use(lv.version.clone()),
            None => LoaderDecision::ResetToVanilla,
        },
        Err(crate::error::Error::LoaderUnavailable { .. }) => LoaderDecision::ResetToVanilla,
        Err(e) => LoaderDecision::Error(e),
    }
}

/// Remove `<instance>/` recursively. If `id` was active, auto-switches
/// to the oldest remaining instance. Errors `LastInstance` if it's the
/// only one (UI also disables the Delete button — defence in depth).
///
/// `InstanceNotFound` is NOT raised for a missing dir; we treat that as
/// "already gone, ok". Active-pointer cleanup still runs.
pub fn delete_instance(app: &tauri::AppHandle, id: &str) -> Result<()> {
    let all = list_instances_with_status(app)?;
    if all.len() <= 1 && all.iter().any(|i| i.id == id) {
        return Err(Error::LastInstance);
    }

    let dir = paths::instance_dir(app, id).map_err(|e| Error::io("<instance_dir>", e))?;
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| Error::io(dir.display().to_string(), e))?;
    }

    let app_file_path = paths::app_file(app).map_err(|e| Error::io("<app_file>", e))?;
    let mut app_state = store::read_app_json(&app_file_path)?;
    if app_state.active_instance.as_deref() == Some(id) {
        // Auto-switch to oldest remaining.
        let remaining: Vec<_> = all.into_iter().filter(|i| i.id != id).collect();
        app_state.active_instance = remaining.first().map(|i| i.id.clone());
        store::write_app_json(&app_file_path, &app_state)?;
    }
    Ok(())
}

pub(crate) fn clear_pack_origin_fields(i: &mut schema::InstanceFile) {
    i.mrpack_name = None;
    i.mrpack_version = None;
    i.mrpack_project_id = None;
    i.mrpack_source = None;
    i.mrpack_summary = None;
    i.mrpack_version_id = None;
}

pub fn detach_instance_pack(app: &tauri::AppHandle, id: &str) -> Result<InstanceWithStatus> {
    mutate(app, id, clear_pack_origin_fields)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use crate::versions::loaders::LoaderVersion;

    fn stable(version: &str) -> LoaderVersion {
        LoaderVersion {
            version: version.to_string(),
            stable: true,
            build: 0,
        }
    }

    fn unstable(version: &str) -> LoaderVersion {
        LoaderVersion {
            version: version.to_string(),
            stable: false,
            build: 0,
        }
    }

    fn pack_instance() -> schema::InstanceFile {
        schema::InstanceFile {
            id: "aaaa-bbbb-cccc-dddd-eeeeffffaaaa".into(),
            uid: None,
            name: "Pack Instance".into(),
            mc_version: "1.20.1".into(),
            loader: schema::LoaderKind::Fabric,
            loader_version: Some("0.16.5".into()),
            max_heap_mb: 4096,
            min_heap_mb: None,
            extra_jvm_args: String::new(),
            created_unix_ms: 1_700_000_000_000.0,
            mrpack_name: Some("All The Mods 10".into()),
            mrpack_version: Some("1.4.7".into()),
            mrpack_project_id: Some("ABCD1234".into()),
            mrpack_source: Some(crate::mods::platform::ModSource::Modrinth),
            mrpack_summary: Some("A great pack".into()),
            mrpack_version_id: Some("vyRB9jtS".into()),
            integrity: None,
            imported_from: None,
            created_from_server: None,
            handled_log_sig: None,
        }
    }

    fn plain_instance() -> schema::InstanceFile {
        schema::InstanceFile {
            id: "1111-2222-3333-4444-555566667777".into(),
            uid: None,
            name: "Plain".into(),
            mc_version: "1.20.4".into(),
            loader: schema::LoaderKind::Vanilla,
            loader_version: None,
            max_heap_mb: 2048,
            min_heap_mb: None,
            extra_jvm_args: String::new(),
            created_unix_ms: 1_700_000_000_000.0,
            mrpack_name: None,
            mrpack_version: None,
            mrpack_project_id: None,
            mrpack_source: None,
            mrpack_summary: None,
            mrpack_version_id: None,
            integrity: None,
            imported_from: None,
            created_from_server: None,
            handled_log_sig: None,
        }
    }

    #[test]
    fn detach_clears_all_mrpack_fields() {
        let mut f = pack_instance();
        clear_pack_origin_fields(&mut f);
        assert!(
            f.mrpack_name.is_none()
                && f.mrpack_version.is_none()
                && f.mrpack_project_id.is_none()
                && f.mrpack_source.is_none()
                && f.mrpack_summary.is_none()
                && f.mrpack_version_id.is_none()
        );
    }

    #[test]
    fn detach_is_idempotent_on_non_pack_instance() {
        let mut f = plain_instance();
        clear_pack_origin_fields(&mut f); // must not panic; stays all-None
        assert!(f.mrpack_name.is_none());
    }

    #[test]
    fn picks_stable_build_when_available() {
        let list = vec![unstable("0.16.0"), stable("0.15.7"), unstable("0.14.0")];
        let decision = decide_loader(LoaderKind::Fabric, Ok(list));
        assert!(matches!(decision, LoaderDecision::Use(ref v) if v == "0.15.7"));
    }

    #[test]
    fn falls_back_to_first_when_none_stable() {
        let list = vec![unstable("0.16.0"), unstable("0.15.7")];
        let decision = decide_loader(LoaderKind::Quilt, Ok(list));
        assert!(matches!(decision, LoaderDecision::Use(ref v) if v == "0.16.0"));
    }

    #[test]
    fn resets_to_vanilla_when_loader_unavailable() {
        let err = Error::LoaderUnavailable {
            loader: "forge".to_string(),
            mc_version: "1.6.4".to_string(),
        };
        let decision = decide_loader(LoaderKind::Forge, Err(err));
        assert!(matches!(decision, LoaderDecision::ResetToVanilla));
    }

    #[test]
    fn propagates_network_error() {
        let err = Error::Network {
            url: "https://meta.fabricmc.net".to_string(),
            details: "connection refused".to_string(),
        };
        let decision = decide_loader(LoaderKind::Fabric, Err(err));
        assert!(matches!(decision, LoaderDecision::Error(_)));
    }
}
