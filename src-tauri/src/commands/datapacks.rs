//! IPC surface for client-side datapacks. Every command is a one-line
//! delegation into `crate::datapacks::*` — no business logic lives here.
//!
//! Read commands (`list_library`, `list_for_world`) are unguarded: they never
//! touch `level.dat`. They are not, however, read-only in the strictest
//! sense — both call `registry::list`, which reconciles against the library
//! dir and persists the result when reconciliation changes anything. The
//! only write this can ever produce is to the launcher-owned
//! `installed-datapacks.json`; the game never reads that file, so this
//! cannot race or corrupt anything Minecraft touches. Every command that
//! writes to `level.dat` or the library dir's *content* opens with [`guard`]
//! — see `datapacks::guard`'s module doc for why this feature needs a hard
//! gate the mods commands don't.

/// Fully-qualified per this file's neighbours (`commands::instances`): a
/// re-export exists, but every existing guard call site spells out
/// `crate::launch::spawn::is_running`.
fn guard(instance_id: &str) -> Result<(), crate::error::Error> {
    crate::datapacks::guard::datapack_write_allowed(crate::launch::spawn::is_running(instance_id))
}

/// Best-effort expected pack_format for `instance_id`'s installed Minecraft,
/// read from the client jar's own bundled `version.json`. `None` for any
/// failure along the way — no instance, no `mc_version` yet, no versions
/// dir, no client jar, an unreadable jar — this must never fail the world
/// listing it feeds. `compat::expected_data_format` is sync (the `zip` crate
/// is sync), so it runs in `spawn_blocking` off the IPC thread.
async fn expected_pack_format(app: &tauri::AppHandle, instance_id: &str) -> Option<u32> {
    let versions_dir = crate::paths::versions_dir(app).ok()?;
    let instance = crate::instances::read_instance(app, instance_id).ok()?;
    if instance.mc_version.is_empty() {
        return None;
    }
    tokio::task::spawn_blocking(move || {
        crate::datapacks::compat::expected_data_format(&versions_dir, &instance.mc_version)
    })
    .await
    .ok()
    .flatten()
}

/// List the instance's datapack library (`<instance>/datapacks/`), reconciled
/// against disk. Unguarded — read-only.
#[tauri::command]
#[specta::specta]
pub async fn datapacks_list_library(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<Vec<crate::datapacks::InstalledDatapack>, crate::error::Error> {
    crate::datapacks::library::list_at(&crate::datapacks::instance_root(&app, &instance_id)?).await
}

/// Install a `.zip` file or folder datapack from `src_path` (a file-picker
/// result) into the instance's library.
#[tauri::command]
#[specta::specta]
pub async fn datapacks_install_from_file(
    app: tauri::AppHandle,
    instance_id: String,
    src_path: String,
) -> Result<crate::datapacks::InstalledDatapack, crate::error::Error> {
    guard(&instance_id)?;
    crate::datapacks::library::install_local_at(
        &crate::datapacks::instance_root(&app, &instance_id)?,
        std::path::Path::new(&src_path),
    )
    .await
}

/// Remove a datapack from the instance's library and its registry entry.
#[tauri::command]
#[specta::specta]
pub async fn datapacks_remove_from_library(
    app: tauri::AppHandle,
    instance_id: String,
    filename: String,
) -> Result<(), crate::error::Error> {
    guard(&instance_id)?;
    crate::datapacks::library::remove_at(
        &crate::datapacks::instance_root(&app, &instance_id)?,
        &filename,
    )
    .await
}

/// List every datapack relevant to one world (library ∪ on-disk ∪ level.dat
/// names), with each entry's enabled/disabled/orphaned state and pack_format
/// compatibility against the instance's installed Minecraft. Unguarded —
/// read-only.
#[tauri::command]
#[specta::specta]
pub async fn datapacks_list_for_world(
    app: tauri::AppHandle,
    instance_id: String,
    world: String,
) -> Result<Vec<crate::datapacks::WorldDatapack>, crate::error::Error> {
    let expected = expected_pack_format(&app, &instance_id).await;
    crate::datapacks::world_link::list_for_world_at(
        &crate::datapacks::instance_root(&app, &instance_id)?,
        &world,
        expected,
    )
    .await
}

/// Link a library datapack into a world's `datapacks/` folder and enable it
/// in level.dat.
#[tauri::command]
#[specta::specta]
pub async fn datapacks_add_to_world(
    app: tauri::AppHandle,
    instance_id: String,
    world: String,
    filename: String,
) -> Result<crate::mods::store::Placement, crate::error::Error> {
    guard(&instance_id)?;
    crate::datapacks::world_link::add_to_world_at(
        &crate::datapacks::instance_root(&app, &instance_id)?,
        &world,
        &filename,
    )
    .await
}

/// Unlink a datapack from a world and drop its level.dat entry. Also the
/// repair path for an `Orphaned` row.
#[tauri::command]
#[specta::specta]
pub async fn datapacks_remove_from_world(
    app: tauri::AppHandle,
    instance_id: String,
    world: String,
    filename: String,
) -> Result<(), crate::error::Error> {
    guard(&instance_id)?;
    crate::datapacks::world_link::remove_from_world_at(
        &crate::datapacks::instance_root(&app, &instance_id)?,
        &world,
        &filename,
    )
    .await
}

/// Toggle a datapack's enabled/disabled state for one world. level.dat only —
/// the file itself is never touched.
#[tauri::command]
#[specta::specta]
pub async fn datapacks_set_enabled_in_world(
    app: tauri::AppHandle,
    instance_id: String,
    world: String,
    filename: String,
    enabled: bool,
) -> Result<(), crate::error::Error> {
    guard(&instance_id)?;
    crate::datapacks::world_link::set_enabled_in_world_at(
        &crate::datapacks::instance_root(&app, &instance_id)?,
        &world,
        &filename,
        enabled,
    )
    .await
}
