//! Tauri commands for importing an instance from another launcher.

use std::path::PathBuf;

use tauri::ipc::Channel;

use crate::error::Error;
use crate::instances::import::discovery;
use crate::instances::import::model::{
    build_import_plan, ContentCategory, DiscoverResult, ForeignInstance, ImportProgress,
};
use crate::instances::import::pipeline;
use crate::instances::schema::InstanceFile;

/// Auto-discover importable instances across known launcher install paths,
/// plus launchers found-but-empty (for the empty-state message).
#[tauri::command]
#[specta::specta]
pub async fn launcher_import_discover() -> Result<DiscoverResult, Error> {
    Ok(discovery::discover_summary())
}

/// Inspect a single user-picked folder (manual fallback). Returns the
/// normalized instance, or an error if the folder is unrecognized.
#[tauri::command]
#[specta::specta]
pub async fn launcher_import_inspect_folder(path: String) -> Result<ForeignInstance, Error> {
    discovery::detect_folder(std::path::Path::new(&path))
        .ok_or(Error::ImportSourceUnrecognized { path })
}

/// Run the import. The wizard shows pre-filled, editable version/loader
/// fields for every source, so `mc_version_override` / `loader_override`
/// arrive populated (seeded from the detected values, possibly user-edited)
/// and are applied when present. They are blank only for a bare `.minecraft`
/// the user never filled — guarded below by the empty-version check.
#[tauri::command]
#[specta::specta]
#[allow(clippy::too_many_arguments)]
pub async fn launcher_import_run(
    app: tauri::AppHandle,
    foreign: ForeignInstance,
    selected: Vec<ContentCategory>,
    target_name: String,
    mc_version_override: Option<String>,
    loader_override: Option<crate::instances::schema::LoaderKind>,
    loader_version_override: Option<String>,
    on_progress: Channel<ImportProgress>,
) -> Result<crate::instances::schema::InstanceWithStatus, Error> {
    let mut foreign = foreign;
    if let Some(v) = mc_version_override {
        foreign.mc_version = v;
    }
    if let Some(l) = loader_override {
        foreign.loader = l;
        foreign.loader_version = loader_version_override;
    }
    if foreign.mc_version.trim().is_empty() {
        return Err(Error::ImportInstanceUnreadable {
            launcher: "raw_minecraft".into(),
            details: "Minecraft version is required".into(),
        });
    }

    // Adaptive per-instance heap bounds (same source as the
    // `instance_memory_bounds` command). The source heap is clamped into
    // these by `build_import_plan`.
    let ram = crate::platform::total_system_ram_mb();
    let heap_min = crate::instances::memory::slider_min_mb();
    let heap_max = crate::instances::memory::slider_max_mb(ram);
    let heap_default = crate::instances::memory::default_heap_mb(ram);
    let plan = build_import_plan(
        &foreign,
        &selected,
        &target_name,
        heap_min,
        heap_max,
        heap_default,
    );

    // Base URLs + CF key mirror the established mod/modpack pattern
    // (`mods_enrich_pack_mods`): production passes the canonical hosts as
    // literals; the network layer / tests handle env overrides downstream.
    let cf_key = crate::mods::curseforge::keyring::resolve();

    let emit = move |p: ImportProgress| {
        let _ = on_progress.send(p);
    };
    let id = pipeline::run_import(
        &app,
        &foreign,
        &plan,
        "https://api.modrinth.com",
        "https://api.curseforge.com",
        cf_key.as_deref(),
        &emit,
    )
    .await?;

    let instance = crate::instances::read_instance(&app, &id)?;
    let versions_dir =
        crate::paths::versions_dir(&app).map_err(|e| Error::io("<versions_dir>", e))?;
    let ready = crate::instances::status::ready_status(&versions_dir, &instance);
    Ok(crate::instances::schema::InstanceWithStatus::from_file(
        &instance, ready,
    ))
}

/// Resolve the still-existing source directory recorded at import time.
/// Errors when the instance has no provenance, or the folder was removed.
fn resolve_source_dir(file: &InstanceFile) -> Result<PathBuf, Error> {
    let prov = file
        .imported_from
        .as_ref()
        .ok_or_else(|| Error::ImportNoProvenance {
            id: file.id.clone(),
        })?;
    let path = PathBuf::from(&prov.source_path);
    if !path.is_dir() {
        return Err(Error::ImportSourceMissing {
            path: prov.source_path.clone(),
        });
    }
    Ok(path)
}

/// Open the folder the instance was imported from, so the user can find and
/// clean up the original files. The path is read server-side from the
/// instance's stored provenance — the UI passes only the id.
#[tauri::command]
#[specta::specta]
pub async fn open_imported_source_folder(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<(), Error> {
    use tauri_plugin_opener::OpenerExt;
    let file = crate::instances::read_instance(&app, &instance_id)?;
    let dir = resolve_source_dir(&file)?;
    app.opener()
        .open_path(dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| Error::io(dir.display().to_string(), format!("opener: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::resolve_source_dir;
    use crate::instances::schema::{ForeignLauncher, ImportProvenance, InstanceFile, LoaderKind};

    fn instance_file(imported_from: Option<ImportProvenance>) -> InstanceFile {
        InstanceFile {
            id: "abc-123".into(),
            name: "Test".into(),
            mc_version: "1.20.1".into(),
            loader: LoaderKind::Vanilla,
            loader_version: None,
            max_heap_mb: 2048,
            extra_jvm_args: String::new(),
            created_unix_ms: 1_700_000_000_000.0,
            mrpack_name: None,
            mrpack_version: None,
            mrpack_project_id: None,
            mrpack_source: None,
            mrpack_summary: None,
            mrpack_version_id: None,
            integrity: None,
            imported_from,
            handled_log_sig: None,
        }
    }

    fn provenance(source_path: String) -> ImportProvenance {
        ImportProvenance {
            launcher: ForeignLauncher::MojangLauncher,
            source_name: "test".into(),
            source_path,
            imported_unix_ms: 0.0,
        }
    }

    #[test]
    fn errors_when_no_provenance() {
        let f = instance_file(None);
        assert!(resolve_source_dir(&f).is_err());
    }

    #[test]
    fn errors_when_source_missing() {
        let f = instance_file(Some(provenance(r"C:\definitely\not\here\xyz123".into())));
        assert!(resolve_source_dir(&f).is_err());
    }

    #[test]
    fn returns_path_when_source_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let f = instance_file(Some(provenance(tmp.path().to_string_lossy().to_string())));
        assert_eq!(resolve_source_dir(&f).unwrap(), tmp.path());
    }
}
