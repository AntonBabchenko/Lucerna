//! Multi-instance support.

pub mod ids;
pub mod migrate;
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
    let files = scan::list_all(&app_root.join("instances"));
    Ok(files
        .into_iter()
        .map(|f| {
            let ready = status::ready_status(&versions_dir, &f);
            InstanceWithStatus::from_file(&f, ready)
        })
        .collect())
}

/// Resolve `app.json.active_instance` to a fully-hydrated
/// `InstanceWithStatus`. Auto-repairs stale pointer (UUID not on disk):
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

fn unix_ms_f64() -> f64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

/// Generate a UUID v4, mkdir `<instance>/.minecraft/`, write
/// `instance.json`. Returns the new instance with its (always-false-
/// initially) ready status.
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
#[allow(clippy::too_many_arguments)]
pub fn create_instance(
    app: &tauri::AppHandle,
    name: String,
    mc_version: String,
    loader: LoaderKind,
    loader_version: Option<String>,
    mrpack: Option<(String, String)>,
    mrpack_project_id: Option<String>,
    mrpack_source: Option<crate::mods::platform::ModSource>,
    mrpack_summary: Option<String>,
    mrpack_version_id: Option<String>,
) -> Result<InstanceWithStatus> {
    let id = ids::new_id();
    let dir = paths::instance_dir(app, &id).map_err(|e| Error::io("<instance_dir>", e))?;
    std::fs::create_dir_all(dir.join(".minecraft"))
        .map_err(|e| Error::io(dir.display().to_string(), e))?;
    let (mrpack_name, mrpack_version) = match mrpack {
        Some((n, v)) => (Some(n), Some(v)),
        None => (None, None),
    };
    let inst = InstanceFile {
        version: 1,
        id: id.clone(),
        name,
        mc_version,
        loader,
        loader_version,
        max_heap_mb: 2048,
        extra_jvm_args: String::new(),
        created_unix_ms: unix_ms_f64(),
        mrpack_name,
        mrpack_version,
        mrpack_project_id,
        mrpack_source,
        mrpack_summary,
        mrpack_version_id,
    };
    let json_path = paths::instance_json(app, &id).map_err(|e| Error::io("<instance_json>", e))?;
    store::write_instance_json(&json_path, &inst)?;
    let versions_dir = paths::versions_dir(app).map_err(|e| Error::io("<versions_dir>", e))?;
    let ready = status::ready_status(&versions_dir, &inst);
    Ok(InstanceWithStatus::from_file(&inst, ready))
}

fn mutate<F>(app: &tauri::AppHandle, id: &str, mutator: F) -> Result<InstanceWithStatus>
where
    F: FnOnce(&mut InstanceFile),
{
    let mut inst = read_one(app, id)?;
    mutator(&mut inst);
    let path = paths::instance_json(app, id).map_err(|e| Error::io("<instance_json>", e))?;
    store::write_instance_json(&path, &inst)?;
    let versions_dir = paths::versions_dir(app).map_err(|e| Error::io("<versions_dir>", e))?;
    let ready = status::ready_status(&versions_dir, &inst);
    Ok(InstanceWithStatus::from_file(&inst, ready))
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

pub fn set_instance_memory(
    app: &tauri::AppHandle,
    id: &str,
    max_heap_mb: u32,
) -> Result<InstanceWithStatus> {
    mutate(app, id, |i| i.max_heap_mb = max_heap_mb)
}

pub fn set_instance_jvm_args(
    app: &tauri::AppHandle,
    id: &str,
    extra_jvm_args: String,
) -> Result<InstanceWithStatus> {
    mutate(app, id, |i| i.extra_jvm_args = extra_jvm_args)
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
