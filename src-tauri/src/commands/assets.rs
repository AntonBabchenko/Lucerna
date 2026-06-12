use super::*;

// =========================================================================
// Add-on (resource pack / shader) commands — install/list/uninstall/updates
// =========================================================================

/// List installed resource packs or shaders for an instance, filtered by
/// `kind`. Reads the per-instance assets registry; never touches mods.
#[tauri::command]
#[specta::specta]
pub async fn assets_list(
    app: tauri::AppHandle,
    instance_id: String,
    kind: crate::mods::platform::ContentKind,
) -> crate::error::Result<Vec<crate::mods::platform::InstalledAsset>> {
    crate::mods::assets::require_asset_kind(kind)?;
    let inst_root = instance_root(&app, &instance_id)?;
    // Retro-fit instances imported before assets were tracked (no-op once the
    // registry file exists). Best-effort: a backfill error must not block the
    // list, so log and continue.
    if let Err(e) = crate::mods::assets::backfill_from_pack_origin_if_missing(&inst_root).await {
        eprintln!("[assets_list] backfill failed (non-fatal): {e}");
    }
    crate::mods::assets::list(&inst_root, kind).await
}

/// Remove an asset's file from disk (best-effort) and drop its registry
/// entry. The registry is the source of truth, so a missing file is fine.
#[tauri::command]
#[specta::specta]
pub async fn asset_uninstall(
    app: tauri::AppHandle,
    instance_id: String,
    kind: crate::mods::platform::ContentKind,
    filename: String,
) -> crate::error::Result<()> {
    crate::mods::assets::require_asset_kind(kind)?;
    let inst_root = instance_root(&app, &instance_id)?;
    // Guard against path escape before touching the filesystem (defense-in-depth:
    // install_asset validates the same way, so any registry basename always passes).
    let path = crate::mods::install::safe_asset_remove_path(&inst_root, kind, &filename)?;
    let _ = tokio::fs::remove_file(&path).await; // best-effort; registry is source of truth
    crate::mods::assets::remove(&inst_root, kind, &filename).await
}

/// Download + install a resource pack or shader version into an instance,
/// recording it in the assets registry. No progress events yet (no UI
/// callers), so a no-op progress sink is supplied.
#[tauri::command]
#[specta::specta]
pub async fn asset_install(
    app: tauri::AppHandle,
    instance_id: String,
    version: crate::mods::platform::ModVersion,
    kind: crate::mods::platform::ContentKind,
) -> crate::error::Result<()> {
    crate::mods::assets::require_asset_kind(kind)?;
    let inst_root = instance_root(&app, &instance_id)?;
    let dd = data_dir(&app)?;
    let f = &version.primary_file;
    let progress: crate::mods::install::ProgressFn = Box::new(|_, _, _| {});
    crate::mods::install::install_asset_tracked(
        &dd,
        &inst_root,
        kind,
        Some(version.source),
        Some(version.project_id.clone()),
        Some(version.version_id.clone()),
        &version.name,
        Some(version.version_number.clone()),
        &f.filename,
        &f.url,
        f.sha1.as_deref(),
        f.size,
        &progress,
    )
    .await
}

/// Check every installed asset of `kind` that carries platform identity
/// for a newer version on the instance's MC version. A single asset's
/// query failure becomes that asset's `CheckFailed` state. Resource packs
/// and shaders are not loader-filtered, so the loader facet is omitted.
///
/// Hand-dropped assets with no platform identity (no `source`/`project_id`)
/// are silently omitted from the result — there is nothing to query an
/// update against.
#[tauri::command]
#[specta::specta]
pub async fn assets_check_updates(
    app: tauri::AppHandle,
    instance_id: String,
    kind: crate::mods::platform::ContentKind,
) -> crate::error::Result<Vec<crate::mods::platform::AssetUpdateCheck>> {
    use crate::mods::platform::{AssetUpdateCheck, AssetUpdateState};
    crate::mods::assets::require_asset_kind(kind)?;
    let inst_root = instance_root(&app, &instance_id)?;
    let (mc_version, _loader) = read_active_mc_and_loader(&app, &instance_id)?;
    let installed = crate::mods::assets::list(&inst_root, kind).await?;
    let mut out = Vec::with_capacity(installed.len());
    for a in installed {
        let (Some(source), Some(pid)) = (a.source, a.project_id.clone()) else {
            continue;
        };
        let state = match platform_for(source)
            .versions(&pid, Some(&mc_version), None)
            .await
        {
            Ok(versions) => {
                crate::mods::updates::classify_asset_update(a.version_id.as_deref(), &versions)
            }
            Err(e) => AssetUpdateState::CheckFailed {
                reason: e.to_string(),
            },
        };
        out.push(AssetUpdateCheck {
            filename: a.filename,
            name: a.name,
            state,
        });
    }
    Ok(out)
}
