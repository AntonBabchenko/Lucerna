//! Tauri commands for importing an instance from another launcher.

use tauri::ipc::Channel;

use crate::error::Error;
use crate::instances::import::discovery;
use crate::instances::import::model::{
    build_import_plan, ContentCategory, ForeignInstance, ImportProgress,
};
use crate::instances::import::pipeline;

/// Auto-discover importable instances across known launcher install paths.
#[tauri::command]
#[specta::specta]
pub async fn launcher_import_discover() -> Result<Vec<ForeignInstance>, Error> {
    Ok(discovery::discover_all())
}

/// Inspect a single user-picked folder (manual fallback). Returns the
/// normalized instance, or an error if the folder is unrecognized.
#[tauri::command]
#[specta::specta]
pub async fn launcher_import_inspect_folder(path: String) -> Result<ForeignInstance, Error> {
    discovery::detect_folder(std::path::Path::new(&path))
        .ok_or(Error::ImportSourceUnrecognized { path })
}

/// Run the import. `mc_version_override` / `loader_override` are used for
/// the generic `.minecraft` reader (which leaves them blank); for
/// structured readers (Prism/MultiMC/PolyMC) the foreign instance already
/// carries a version + loader and the overrides are ignored.
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
    let plan = build_import_plan(&foreign, &selected, &target_name, heap_min, heap_max);

    // Base URLs + CF key mirror the established mod/modpack pattern
    // (`mods_enrich_pack_mods`): production passes the canonical hosts as
    // literals; the network layer / tests handle env overrides downstream.
    let cf_key = crate::mods::curseforge::keyring::get().ok().flatten();

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
