//! Settings sync in the Shared data section (spec 2026-10-15 §B7, §B9).
//! The work is `settings_sync::membership`; these commands resolve paths,
//! take the instance's claim for a write, and run it on the blocking pool.

use std::path::PathBuf;

use crate::error::Error;
use crate::settings_sync::creation::{on_created, Origin};
use crate::settings_sync::engine::KindOutcome;
use crate::settings_sync::membership::{self, SectionState};
use crate::settings_sync::sidecar::Join;
use crate::settings_sync::version::VersionFacts;
use crate::settings_sync::Kind;

struct Roots {
    data_root: PathBuf,
    instance_root: PathBuf,
    versions: PathBuf,
    mc_version: String,
    loader: crate::instances::schema::LoaderKind,
}

fn roots(app: &tauri::AppHandle, instance_id: &str) -> Result<Roots, Error> {
    let instance = crate::instances::read_instance(app, instance_id)?;
    Ok(Roots {
        data_root: crate::paths::app_dir(app).map_err(|e| Error::io("<app_dir>", e))?,
        instance_root: crate::paths::instance_dir(app, instance_id)
            .map_err(|e| Error::io("<instance_dir>", e))?,
        versions: crate::paths::versions_dir(app).map_err(|e| Error::io("<versions_dir>", e))?,
        mc_version: instance.mc_version,
        loader: instance.loader,
    })
}

/// Decide a new instance's settings-sync membership by its origin (spec
/// §B7). The creation stands either way: a failure to decide (an unreadable
/// app.json or source sidecar) is logged, and the instance does not join.
pub(crate) fn join_new_instance(app: &tauri::AppHandle, instance_id: &str, origin: Origin) {
    let paths = crate::paths::app_file(app).and_then(|app_json| {
        crate::paths::instance_dir(app, instance_id).map(|root| (app_json, root))
    });
    match paths {
        Ok((app_json, root)) => {
            if let Err(e) = on_created(&app_json, &root, origin) {
                crate::diag!("settings sync: {instance_id} did not join ({e})");
            }
        }
        Err(e) => crate::diag!("settings sync: {instance_id} did not join ({e})"),
    }
}

/// What the section shows: each kind's toggle, join and availability, and
/// the last failure. Read-only.
#[tauri::command]
#[specta::specta]
pub async fn settings_sync_state(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<SectionState, Error> {
    let r = roots(&app, &instance_id)?;
    tokio::task::spawn_blocking(move || {
        let facts = VersionFacts::read(&r.versions, &r.mc_version, r.loader);
        membership::state(&r.data_root, &r.instance_root, &facts)
    })
    .await
    .map_err(|e| Error::io("<settings sync state>", e))?
}

/// Turn one kind on (joining now when the version allows; `join` is the
/// hotbars choice) or off. Holds the instance's claim: no launch, update or
/// migration runs while its files change. A join first checks for a game
/// Lucerna lost track of, as a launch does: `SettingsSyncAskGameClosed`
/// comes back until the user says it is closed (`game_closed_confirmed`).
#[tauri::command]
#[specta::specta]
pub async fn settings_sync_set_kind(
    app: tauri::AppHandle,
    instance_id: String,
    kind: Kind,
    enabled: bool,
    join: Option<Join>,
    game_closed_confirmed: bool,
) -> Result<Option<KindOutcome>, Error> {
    let claim = crate::instances::maintenance::claim_write(&instance_id)?;
    let r = roots(&app, &instance_id)?;
    let outcome = tokio::task::spawn_blocking(move || {
        let facts = VersionFacts::read(&r.versions, &r.mc_version, r.loader);
        membership::join_or_leave(
            &r.data_root,
            &r.instance_root,
            &facts,
            kind,
            enabled,
            join,
            game_closed_confirmed,
            &|pid| crate::platform::process_image_probe(pid, "java"),
        )
    })
    .await
    .map_err(|e| Error::io("<settings sync>", e));
    drop(claim);
    outcome?
}

/// "Stop syncing <kind> here", from the launch dialog or the section; `kind`
/// is `None` when the sync settings themselves could not be read (they go to
/// the Recycle Bin and start over with nothing on).
///
/// (The repair command below is the way out of a damaged sync file.)
#[tauri::command]
#[specta::specta]
pub async fn settings_sync_stop(
    app: tauri::AppHandle,
    instance_id: String,
    kind: Option<Kind>,
) -> Result<(), Error> {
    let claim = crate::instances::maintenance::claim_write(&instance_id)?;
    let instance_root = crate::paths::instance_dir(&app, &instance_id)
        .map_err(|e| Error::io("<instance_dir>", e))?;
    let done = tokio::task::spawn_blocking(move || {
        membership::stop(
            &instance_root,
            kind,
            &crate::platform::recycle_bin::to_recycle_bin,
        )
    })
    .await
    .map_err(|e| Error::io("<settings sync stop>", e));
    drop(claim);
    done?
}

/// The way out of a sync file that does not parse, for its reason class
/// (spec §15): the shared copy replaced by this instance's, this instance's
/// file replaced by the shared copy, or its records started afresh. Holds the
/// instance's claim; asks about a game Lucerna lost track of as a join does.
#[tauri::command]
#[specta::specta]
pub async fn settings_sync_repair(
    app: tauri::AppHandle,
    instance_id: String,
    kind: Kind,
    repair: crate::settings_sync::repair::Repair,
    game_closed_confirmed: bool,
) -> Result<Option<KindOutcome>, Error> {
    let claim = crate::instances::maintenance::claim_write(&instance_id)?;
    let r = roots(&app, &instance_id)?;
    let outcome = tokio::task::spawn_blocking(move || {
        crate::settings_sync::guard_orphan(&r.instance_root, game_closed_confirmed, &|pid| {
            crate::platform::process_image_probe(pid, "java")
        })?;
        let facts = VersionFacts::read(&r.versions, &r.mc_version, r.loader);
        crate::settings_sync::repair::repair(
            &r.data_root,
            &r.instance_root,
            &facts,
            kind,
            repair,
            &crate::platform::recycle_bin::to_recycle_bin,
        )
    })
    .await
    .map_err(|e| Error::io("<settings sync repair>", e));
    drop(claim);
    outcome?
}
