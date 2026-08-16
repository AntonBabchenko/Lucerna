//! Server datapack commands: listing with state, catalog install, update
//! check/apply, toggle and removal.
//!
//! Every mutating command opens with
//! `servers_runtime::datapacks::guard::gate`, which is
//! `is_running || is_starting` — the plain `is_running` snapshot the shipped
//! commands used cannot see the whole of `start()`'s Java resolution and JRE
//! download.

use tauri::AppHandle;

use crate::error::{Error, Result};
use crate::mods::platform::{AssetUpdateCheck, AssetUpdateState, ModVersion};
use crate::servers_runtime::datapacks::{
    self, guard, listing, mutate, update, ServerDatapackEntry, ServerDatapackUpdateOutcome,
};

/// `runtime/<level>/` for a server, resolved from its live `server.properties`.
fn world_dir_of(app: &AppHandle, id: &str) -> Result<std::path::PathBuf> {
    let base = crate::paths::app_dir(app).map_err(|e| Error::io("<app_dir>", e))?;
    let p = crate::paths::server_paths(&base, id);
    Ok(datapacks::world_dir(
        &p.runtime,
        &super::server_props_raw(&p)?,
    ))
}

/// Every datapack this server's world knows about, with its real enabled
/// state read from `level.dat`.
#[tauri::command]
#[specta::specta]
pub fn server_list_datapacks(app: AppHandle, id: String) -> Result<Vec<ServerDatapackEntry>> {
    Ok(listing::entries(&world_dir_of(&app, &id)?))
}

/// Install a datapack `.zip` chosen from disk. Records a provenance-less
/// sidecar row so the pack lists with real state.
#[tauri::command]
#[specta::specta]
pub async fn server_install_datapack(
    app: AppHandle,
    id: String,
    zip_path: String,
) -> Result<String> {
    guard::gate(&id)?;
    let world = world_dir_of(&app, &id)?;
    datapacks::install_datapack(&world, std::path::Path::new(&zip_path)).await
}

/// Remove a datapack: the on-disk entry, its `level.dat` name and its sidecar
/// row. Idempotent, and the repair for a `level.dat` entry with no file.
#[tauri::command]
#[specta::specta]
pub async fn server_remove_datapack(app: AppHandle, id: String, filename: String) -> Result<()> {
    guard::gate(&id)?;
    mutate::remove(&world_dir_of(&app, &id)?, &filename).await
}

/// Enable or disable a datapack in the world's `level.dat`.
#[tauri::command]
#[specta::specta]
pub async fn server_set_datapack_enabled(
    app: AppHandle,
    id: String,
    filename: String,
    enabled: bool,
) -> Result<()> {
    guard::gate(&id)?;
    mutate::set_enabled(&world_dir_of(&app, &id)?, &filename, enabled).await
}

/// Download a catalog version into the server's world, recording provenance.
#[tauri::command]
#[specta::specta]
pub async fn server_install_datapack_version(
    app: AppHandle,
    id: String,
    version: ModVersion,
) -> Result<crate::servers_runtime::installed::ServerInstalledRecord> {
    guard::gate(&id)?;
    let world = world_dir_of(&app, &id)?;
    let dd = super::data_dir(&app)?;
    let bytes = super::fetch_datapack_bytes(&dd, &version).await?;
    mutate::install_bytes(
        &world,
        &version.primary_file.filename,
        &bytes,
        Some(&super::datapack_provenance_of(&version)),
    )
    .await
}

/// Check every pack carrying platform identity for a newer version on this
/// server's Minecraft.
///
/// Mirrors `server_check_mod_updates` in shape, with ONE step deliberately
/// not mirrored: that command opens with `installed::reconcile_on_list`, which
/// on a `datapacks/` dir wipes every sidecar row. The datapack sidecar has its
/// own `.zip`-aware reconcile.
#[tauri::command]
#[specta::specta]
pub async fn server_check_datapack_updates(
    app: AppHandle,
    id: String,
) -> Result<Vec<AssetUpdateCheck>> {
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let p = crate::paths::server_paths(&base, &id);
    let file = crate::servers_runtime::store::read_server_json(&p.json)?;
    let world = datapacks::world_dir(&p.runtime, &super::server_props_raw(&p)?);
    let rows = datapacks::sidecar::reconcile(&world);
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        // No identity ⇒ nothing to query. Hand-dropped packs are omitted.
        let (Some(source), Some(pid)) = (row.source, row.project_id.clone()) else {
            continue;
        };
        let state = match super::platform_for(source)
            .datapack_versions(&pid, Some(&file.mc_version))
            .await
        {
            Ok(versions) => {
                crate::mods::updates::classify_asset_update(row.version_id.as_deref(), &versions)
            }
            Err(e) => AssetUpdateState::CheckFailed {
                reason: e.to_string(),
            },
        };
        out.push(AssetUpdateCheck {
            filename: row.filename,
            name: row.name.unwrap_or_default(),
            state,
        });
    }
    Ok(out)
}

/// Apply one datapack update.
///
/// Takes the update claim BEFORE the running gate — the Dekker order
/// `runtime::start` mirrors (claim the start slot, then check this set).
/// Checking the gate first would leave an interleaving where both sides pass
/// before either flag is visible.
#[tauri::command]
#[specta::specta]
pub async fn server_update_datapack_one(
    app: AppHandle,
    id: String,
    old_filename: String,
    target: ModVersion,
) -> Result<ServerDatapackUpdateOutcome> {
    let _claim = guard::UpdateGuard::acquire(&id).ok_or(Error::ServerContentStale)?;
    guard::gate(&id)?;
    let world = world_dir_of(&app, &id)?;
    let dd = super::data_dir(&app)?;
    // See the twin in `commands::datapacks`: a Vanilla Tweaks pack has no
    // direct URL, and its filename carries the version, so both come back
    // together from a one-pack build instead of being fetched and predicted.
    let vt_family = if target.source == crate::mods::platform::ModSource::VanillaTweaks {
        let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
        let p = crate::paths::server_paths(&base, &id);
        let file = crate::servers_runtime::store::read_server_json(&p.json)?;
        Some(
            crate::datapacks::vanillatweaks::family_for(&file.mc_version).ok_or(
                Error::VanillaTweaksUnavailable {
                    mc_version: file.mc_version.clone(),
                },
            )?,
        )
    } else {
        None
    };
    crate::network::throttle::with_interactive(async move {
        let (filename, bytes) = match vt_family {
            Some(family) => {
                crate::datapacks::vanillatweaks::build_one(&family, &target.project_id).await?
            }
            None => (
                target.primary_file.filename.clone(),
                super::fetch_datapack_bytes(&dd, &target).await?,
            ),
        };
        update::update_one(
            &world,
            &old_filename,
            &filename,
            &bytes,
            &super::datapack_provenance_of(&target),
        )
        .await
    })
    .await
}
