//! Finish installing a pack (spec 2026-11-05 §4): check files the user
//! downloaded and place the pack's own. Writes under the instance's shared
//! maintenance claim, taken only around the writes — reading and hashing a
//! folder never holds it.

use std::path::PathBuf;

use tauri::Manager;
use tauri_specta::Event;

use super::{instance_root, ModInstalled};
use crate::error::{Error, Result};
use crate::mods::manual_downloads::supply::{self, Ctx, PlacedFile};
use crate::mods::manual_downloads::types::{ReplaceOutcome, ScanReport, SupplyReport, SupplyRow};
use crate::mods::manual_downloads::{self as md, state};

async fn loaded(root: &std::path::Path) -> Result<md::Loaded> {
    md::load(root).await?.ok_or(Error::ModsNotFound {
        platform: "pack_origin".into(),
    })
}

fn announce(
    app: &tauri::AppHandle,
    instance_id: &str,
    root: &std::path::Path,
    placed: &[PlacedFile],
) {
    for f in placed {
        let action = if f.is_mod {
            crate::journal::ContentAction::ModInstalled
        } else {
            crate::journal::ContentAction::AssetInstalled
        };
        crate::journal::record(root, crate::journal::content(action, f.name.clone()));
        if f.is_mod {
            // The page and the drawer refresh on it; a failed emit only delays that.
            if let Err(e) = (ModInstalled {
                instance_id: instance_id.to_string(),
                sha1: f.sha1.clone(),
                filename: f.filename.clone(),
                name: f.name.clone(),
            })
            .emit(app)
            {
                crate::diag!("[manual-downloads] mod-installed not emitted: {e}");
            }
        }
    }
}

/// Check the dropped / picked files and place the pack's own.
#[tauri::command]
#[specta::specta]
pub async fn manual_downloads_supply(
    app: tauri::AppHandle,
    instance_id: String,
    paths: Vec<String>,
) -> Result<SupplyReport> {
    let root = instance_root(&app, &instance_id)?;
    let l = loaded(&root).await?;
    let ctx = Ctx {
        instance_root: &root,
        origin: &l.origin,
        installed: &l.installed,
        statuses: &l.statuses,
    };
    let cf = crate::mods::curseforge::CurseForgeClient::new();
    let planned = supply::plan(&ctx, &paths, Some(&cf)).await;
    let write = crate::instances::maintenance::claim_shared_write(&instance_id)?;
    let applied = supply::apply(&ctx, planned).await;
    drop(write);
    announce(&app, &instance_id, &root, &applied.placed);
    Ok(SupplyReport { rows: applied.rows })
}

/// Look in a folder (default: the OS downloads folder) for the pack's files.
#[tauri::command]
#[specta::specta]
pub async fn manual_downloads_scan(
    app: tauri::AppHandle,
    instance_id: String,
    folder: Option<String>,
) -> Result<ScanReport> {
    let folder = match folder {
        Some(f) => PathBuf::from(f),
        None => app
            .path()
            .download_dir()
            .map_err(|e| Error::ManualDownloadsFolderUnreadable {
                path: String::new(),
                details: e.to_string(),
            })?,
    };
    let shown = folder.display().to_string();
    let root = instance_root(&app, &instance_id)?;
    let l = loaded(&root).await?;
    let sizes: std::collections::HashSet<u64> = l
        .statuses
        .iter()
        .filter(|s| {
            matches!(
                s.state,
                crate::mods::modpack::schema::MissingModState::Missing
                    | crate::mods::modpack::schema::MissingModState::DifferentVersion
            ) && state::is_actionable(&s.entry)
                && state::expected(&s.entry) != state::Expected::Nothing
                && s.entry.size > 0.0
        })
        .map(|s| s.entry.size as u64)
        .collect();
    let (found, mut skipped) = md::scan::candidates(&folder, &sizes).await.map_err(|e| {
        Error::ManualDownloadsFolderUnreadable {
            path: shown.clone(),
            details: e.to_string(),
        }
    })?;
    let ctx = Ctx {
        instance_root: &root,
        origin: &l.origin,
        installed: &l.installed,
        statuses: &l.statuses,
    };
    let planned = supply::plan(&ctx, &found, None).await;
    let mut kept = Vec::new();
    for p in planned {
        match p {
            supply::Planned::Row(SupplyRow::Unreadable { .. }) => skipped += 1,
            supply::Planned::Row(SupplyRow::NotInPack { .. }) => {}
            other => kept.push(other),
        }
    }
    let rows = if kept.iter().any(|p| matches!(p, supply::Planned::Place(_))) {
        let write = crate::instances::maintenance::claim_shared_write(&instance_id)?;
        let applied = supply::apply(&ctx, kept).await;
        drop(write);
        announce(&app, &instance_id, &root, &applied.placed);
        applied.rows
    } else {
        supply::apply(&ctx, kept).await.rows
    };
    let mut out = Vec::new();
    for r in rows {
        match r {
            SupplyRow::Unreadable { .. } => skipped += 1,
            other => out.push(other),
        }
    }
    Ok(ScanReport {
        folder: shown,
        rows: out,
        skipped_unreadable: skipped,
    })
}

/// Use the user's own file for an entry with nothing to check against (D8).
#[tauri::command]
#[specta::specta]
pub async fn manual_downloads_supply_unverified(
    app: tauri::AppHandle,
    instance_id: String,
    key: String,
    path: String,
) -> Result<SupplyRow> {
    let root = instance_root(&app, &instance_id)?;
    let l = loaded(&root).await?;
    let ctx = Ctx {
        instance_root: &root,
        origin: &l.origin,
        installed: &l.installed,
        statuses: &l.statuses,
    };
    let write = crate::instances::maintenance::claim_shared_write(&instance_id)?;
    let mut applied = supply::supply_unverified(&ctx, &key, &path).await;
    drop(write);
    announce(&app, &instance_id, &root, &applied.placed);
    // `supply_unverified` answers for exactly the one path it was handed.
    Ok(applied.rows.pop().unwrap_or(SupplyRow::CantPlace { path }))
}

/// The confirmed Replace (D9).
#[tauri::command]
#[specta::specta]
pub async fn manual_downloads_replace(
    app: tauri::AppHandle,
    instance_id: String,
    key: String,
    path: String,
) -> Result<ReplaceOutcome> {
    let root = instance_root(&app, &instance_id)?;
    let l = loaded(&root).await?;
    let ctx = Ctx {
        instance_root: &root,
        origin: &l.origin,
        installed: &l.installed,
        statuses: &l.statuses,
    };
    let write = crate::instances::maintenance::claim_shared_write(&instance_id)?;
    let done = supply::replace(&ctx, &key, &path).await;
    drop(write);
    let receipt = done
        .trashed
        .map(|t| super::mods::announce_trashed(&app, &instance_id, &root, t));
    announce(&app, &instance_id, &root, &done.placed);
    Ok(ReplaceOutcome {
        row: done.row,
        receipt,
        recycled: done.recycled,
    })
}

/// The OS downloads folder, for the dialog's folder line and the picker; `None`
/// when the OS does not name one.
#[tauri::command]
#[specta::specta]
pub async fn manual_downloads_default_folder(app: tauri::AppHandle) -> Result<Option<String>> {
    Ok(app
        .path()
        .download_dir()
        .ok()
        .map(|p| p.display().to_string()))
}
