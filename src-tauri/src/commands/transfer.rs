//! Mod-data transfer between instances (spec 2026-10-15 Part A).

use crate::error::Error;
use crate::instances::transfer::{
    engine, ModDataSummary, OnConflict, TransferOutcome, TransferPreview, TransferProgress,
    TransferSelection,
};

/// What the transfer dialog shows for `source_id` → `target_id`. Read-only.
#[tauri::command]
#[specta::specta]
pub async fn transfer_scan(
    app: tauri::AppHandle,
    source_id: String,
    target_id: String,
) -> Result<TransferPreview, Error> {
    crate::data_root::reject_if_root_unusable(&app)?;
    if source_id == target_id {
        return Err(Error::TransferInvalidSelection { name: source_id });
    }
    crate::instances::read_instance(&app, &source_id)?;
    crate::instances::read_instance(&app, &target_id)?;
    let source_mc = crate::paths::minecraft_dir(&app, &source_id)
        .map_err(|e| Error::io("<minecraft_dir>", e))?;
    let target_mc = crate::paths::minecraft_dir(&app, &target_id)
        .map_err(|e| Error::io("<minecraft_dir>", e))?;
    tokio::task::spawn_blocking(move || {
        let free = crate::platform::free_disk_mb(&target_mc);
        crate::instances::transfer::preview::preview(&source_mc, &target_mc, free)
            .map_err(|e| Error::io(source_mc.display().to_string(), e))
    })
    .await
    .map_err(|e| Error::io("<transfer_scan>", e))?
}

/// Copy the selection from `source_id` into `target_id`. Holds both
/// instances' maintenance claims for the whole run, so neither can be launched
/// or written while files are read from one and placed into the other.
#[tauri::command]
#[specta::specta]
pub async fn transfer_mod_data(
    app: tauri::AppHandle,
    source_id: String,
    target_id: String,
    selection: TransferSelection,
    on_conflict: OnConflict,
    on_progress: tauri::ipc::Channel<TransferProgress>,
) -> Result<TransferOutcome, Error> {
    crate::data_root::reject_if_root_unusable(&app)?;
    if source_id == target_id {
        return Err(Error::TransferInvalidSelection { name: source_id });
    }
    let source = crate::instances::read_instance(&app, &source_id)?;
    let target = crate::instances::read_instance(&app, &target_id)?;
    let (target_claim, source_claim) = crate::instances::transfer::claim_both(
        (&target_id, &target.name),
        (&source_id, &source.name),
    )?;
    let source_mc = crate::paths::minecraft_dir(&app, &source_id)
        .map_err(|e| Error::io("<minecraft_dir>", e))?;
    let target_root =
        crate::paths::instance_dir(&app, &target_id).map_err(|e| Error::io("<instance_dir>", e))?;
    let request = engine::Request {
        source_mc,
        target_root: target_root.clone(),
        source_name: source.name.clone(),
        target_name: target.name.clone(),
        selection,
        on_conflict,
    };
    let outcome = engine::run(
        request,
        engine::Seams::production(),
        std::sync::Arc::new(move |p| {
            // A closed dialog is not an error: the copy goes on and the task
            // row reports its end.
            let _ = on_progress.send(p);
        }),
    )
    .await;
    drop(source_claim);
    drop(target_claim);
    let outcome = outcome?;
    let mut event = crate::journal::content(crate::journal::ContentAction::DataCopied, source.name);
    if let crate::journal::JournalEvent::Content { affected, .. } = &mut event {
        *affected = Some(f64::from(outcome.copied));
    }
    crate::journal::record(&target_root, event);
    Ok(outcome)
}

/// Mod data `id` has, for the clone dialog's "Mod data" row. Read-only.
#[tauri::command]
#[specta::specta]
pub async fn mod_data_scan(app: tauri::AppHandle, id: String) -> Result<ModDataSummary, Error> {
    crate::instances::read_instance(&app, &id)?;
    let mc = crate::paths::minecraft_dir(&app, &id).map_err(|e| Error::io("<minecraft_dir>", e))?;
    tokio::task::spawn_blocking(move || crate::instances::transfer::clone_summary(&mc))
        .await
        .map_err(|e| Error::io("<mod_data_scan>", e))
}
