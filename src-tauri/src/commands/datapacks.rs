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
///
/// `is_starting` alongside `is_running`: the `running` registry is only
/// populated after the JVM process exists, so a launch is invisible to
/// `is_running` for its whole multi-second spawn pipeline — during which the
/// game will shortly open a world and rewrite its `level.dat`. A datapack
/// write admitted in that window races the boot exactly like one racing the
/// running game.
fn guard(instance_id: &str) -> Result<(), crate::error::Error> {
    crate::datapacks::guard::datapack_write_allowed(
        crate::launch::spawn::is_running(instance_id)
            || crate::launch::spawn::is_starting(instance_id),
    )
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

/// The instance-level library view: every datapack Lucerna knows about —
/// the registry UNION the packs still linked in worlds — each with its state
/// in every world and one per-instance compat verdict. Unguarded — read-only.
#[tauri::command]
#[specta::specta]
pub async fn datapacks_list_library(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<crate::datapacks::DatapackLibraryView, crate::error::Error> {
    let expected = expected_pack_format(&app, &instance_id).await;
    crate::datapacks::overview::list_at(
        &crate::datapacks::instance_root(&app, &instance_id)?,
        expected,
    )
    .await
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

/// Remove a datapack from the instance's library. With `cascade`, first unlink
/// it and drop its level.dat entries in every world holding it; without,
/// world links survive — and keep loading in game — which the library listing
/// then reports as `in_library: false`. Either way the result names each
/// affected world (F3).
#[tauri::command]
#[specta::specta]
pub async fn datapacks_remove_from_library(
    app: tauri::AppHandle,
    instance_id: String,
    filename: String,
    cascade: bool,
) -> Result<crate::datapacks::LibraryRemoval, crate::error::Error> {
    guard(&instance_id)?;
    crate::datapacks::library::remove_from_library_at(
        &crate::datapacks::instance_root(&app, &instance_id)?,
        &filename,
        cascade,
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

/// Resolve a catalog version to verified bytes: a SHA-1-checked download
/// through the shared content cache, size-capped twice (F19) — on the
/// platform's DECLARED size before any I/O, and on the file's ACTUAL size
/// before it is buffered into memory, because the declared size is remote
/// metadata that can simply be wrong. The catalog is an automated path —
/// before it, every pack came from a file the user picked themselves, which
/// is why the cap sits here at the entry rather than only inside
/// `install_named_at`.
pub(super) async fn fetch_datapack_bytes(
    data_dir: &std::path::Path,
    version: &crate::mods::platform::ModVersion,
) -> Result<Vec<u8>, crate::error::Error> {
    let f = &version.primary_file;
    if f.size > crate::datapacks::MAX_DATAPACK_BYTES as f64 {
        return Err(crate::error::Error::DatapackTooLarge {
            filename: f.filename.clone(),
            size_bytes: f.size,
            limit_bytes: crate::datapacks::MAX_DATAPACK_BYTES as f64,
        });
    }
    // Safe filename, distribution allowed, SHA-1 present (no-TOFU) — the same
    // pre-I/O gate every mod install path shares.
    let sha = crate::mods::install::guard_version(version)?;
    let progress: crate::mods::install::ProgressFn = Box::new(|_, _, _| {});
    let fetch =
        crate::mods::install::fetch_to_cache(data_dir, &f.url, &sha, f.size, "mods", &progress)
            .await?;
    // The actual size, before the read allocates: a declared size of 1 KB on
    // a 4 GB file (bad platform metadata) passed the check above and the
    // streaming download enforces only the hash — this is the last point
    // before the whole file lands in memory.
    let actual = tokio::fs::metadata(&fetch.path)
        .await
        .map_err(|e| crate::error::Error::ModsCacheIo {
            details: format!("{}: {e}", fetch.path.display()),
        })?
        .len();
    if actual > crate::datapacks::MAX_DATAPACK_BYTES as u64 {
        return Err(crate::error::Error::DatapackTooLarge {
            filename: f.filename.clone(),
            size_bytes: actual as f64,
            limit_bytes: crate::datapacks::MAX_DATAPACK_BYTES as f64,
        });
    }
    tokio::fs::read(&fetch.path)
        .await
        .map_err(|e| crate::error::Error::ModsCacheIo {
            details: format!("{}: {e}", fetch.path.display()),
        })
}

/// Extract the provenance a catalog version carries. Recording it is what
/// makes update checking live: `classify_asset_update` answers `UpToDate`
/// forever when `version_id` is `None`.
pub(super) fn datapack_provenance_of(
    version: &crate::mods::platform::ModVersion,
) -> crate::datapacks::DatapackProvenance {
    crate::datapacks::DatapackProvenance {
        source: version.source,
        project_id: version.project_id.clone(),
        version_id: version.version_id.clone(),
        version_number: Some(version.version_number.clone()),
    }
}

/// Download a datapack version from the catalog into the instance's library,
/// recording provenance. Placement into worlds is the world picker's separate
/// step (`datapacks_add_to_world`) — a fresh install touches no world; the
/// returned fan-out is non-empty only when a same-named pack was already
/// linked somewhere (the reinstall path).
#[tauri::command]
#[specta::specta]
pub async fn datapacks_install_from_version(
    app: tauri::AppHandle,
    instance_id: String,
    version: crate::mods::platform::ModVersion,
) -> Result<crate::datapacks::LibraryInstall, crate::error::Error> {
    guard(&instance_id)?;
    let root = crate::datapacks::instance_root(&app, &instance_id)?;
    let dd = super::data_dir(&app)?;
    let bytes = fetch_datapack_bytes(&dd, &version).await?;
    crate::datapacks::library::install_named_at(
        &root,
        &version.primary_file.filename,
        &bytes,
        Some(&datapack_provenance_of(&version)),
    )
    .await
}

/// Check every library pack that carries platform identity for a newer
/// version on the instance's MC version. Hand-dropped packs (no
/// `source`/`project_id`) are silently omitted — there is nothing to query.
/// A single pack's query failure becomes that pack's `CheckFailed` state.
///
/// Deliberately NOT a mirror of `assets_check_updates`: that fetches with
/// `loader: None`, the unfiltered call that returns a MOD JAR as the latest
/// version of a hybrid project like Terralith. This uses the
/// datapack-filtered listing.
#[tauri::command]
#[specta::specta]
pub async fn datapacks_check_updates(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<Vec<crate::mods::platform::AssetUpdateCheck>, crate::error::Error> {
    use crate::mods::platform::{AssetUpdateCheck, AssetUpdateState};
    let root = crate::datapacks::instance_root(&app, &instance_id)?;
    let (mc_version, _loader) = super::read_active_mc_and_loader(&app, &instance_id)?;
    let installed = crate::datapacks::library::list_at(&root).await?;
    let mut out = Vec::with_capacity(installed.len());
    for pack in installed {
        let (Some(source), Some(pid)) = (pack.source, pack.project_id.clone()) else {
            continue;
        };
        let state = match super::platform_for(source)
            .datapack_versions(&pid, Some(&mc_version))
            .await
        {
            Ok(versions) => {
                crate::mods::updates::classify_asset_update(pack.version_id.as_deref(), &versions)
            }
            Err(e) => AssetUpdateState::CheckFailed {
                reason: e.to_string(),
            },
        };
        out.push(AssetUpdateCheck {
            filename: pack.filename,
            name: pack.name,
            state,
        });
    }
    Ok(out)
}

/// Apply one datapack update: install the target version into the library and
/// propagate it into every world holding the pack, preserving each world's
/// own enabled/disabled choice (`datapacks::update::update_at`, the §8.5
/// corrected algorithm).
///
/// Holds [`crate::datapacks::guard::DatapackUpdateGuard`] for the duration so
/// `launch_instance` refuses to start mid-update: the forward [`guard`] below
/// is a one-shot `is_running` snapshot calibrated for sub-second commands,
/// and this one inserts a network download plus N level.dat writes into its
/// window. Wrapped in `with_interactive` like `asset_update_one` — the user
/// is watching this download.
#[tauri::command]
#[specta::specta]
pub async fn datapacks_update_one(
    app: tauri::AppHandle,
    instance_id: String,
    old_filename: String,
    target: crate::mods::platform::ModVersion,
) -> Result<crate::datapacks::DatapackUpdateOutcome, crate::error::Error> {
    // Own flag FIRST, then the other side's — the Dekker order `spawn::start`
    // mirrors (claim, then check this flag). Checking `guard` before
    // acquiring would leave an interleaving where both sides pass their
    // checks before either flag is visible.
    let _update_guard = crate::datapacks::guard::DatapackUpdateGuard::acquire()
        .ok_or(crate::error::Error::InstanceBusy)?;
    guard(&instance_id)?;
    let root = crate::datapacks::instance_root(&app, &instance_id)?;
    let dd = super::data_dir(&app)?;
    crate::network::throttle::with_interactive(async move {
        let bytes = fetch_datapack_bytes(&dd, &target).await?;
        crate::datapacks::update::update_at(
            &root,
            &old_filename,
            &target.primary_file.filename,
            &bytes,
            &datapack_provenance_of(&target),
        )
        .await
    })
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
