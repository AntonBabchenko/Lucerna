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
        crate::diag!("[assets_list] backfill failed (non-fatal): {e}");
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
    // Resolve the display name before the registry row goes away.
    let name = asset_display_name(&inst_root, kind, &filename).await;
    let _ = tokio::fs::remove_file(&path).await; // best-effort; registry is source of truth
    crate::mods::assets::remove(&inst_root, kind, &filename).await?;
    crate::journal::record(
        &inst_root,
        crate::journal::content(crate::journal::ContentAction::AssetRemoved, name),
    );
    Ok(())
}

/// Display name of an installed asset, falling back to its filename when the
/// registry can't be read or has no row for it. Journal rows should carry the
/// name the user saw in the Installed list.
async fn asset_display_name(
    inst_root: &std::path::Path,
    kind: crate::mods::platform::ContentKind,
    filename: &str,
) -> String {
    crate::mods::assets::list(inst_root, kind)
        .await
        .ok()
        .and_then(|list| {
            list.into_iter()
                .find(|a| a.filename.eq_ignore_ascii_case(filename))
                .map(|a| a.name)
        })
        .unwrap_or_else(|| filename.to_string())
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
    .await?;
    crate::journal::record(
        &inst_root,
        crate::journal::content_versioned(
            crate::journal::ContentAction::AssetInstalled,
            version.name.clone(),
            None,
            Some(version.version_number.clone()),
        ),
    );
    Ok(())
}

/// Install a local resource-pack / shader `.zip` from disk into the instance as
/// a manual asset (`source: None`). Validity-only: rejects a non-zip, or a
/// resource pack missing `pack.mcmeta`. No Tauri event is emitted — the
/// frontend bumps the `assetsChanged` rune to refresh the Installed view.
#[tauri::command]
#[specta::specta]
pub async fn asset_install_local(
    app: tauri::AppHandle,
    instance_id: String,
    kind: crate::mods::platform::ContentKind,
    file_path: String,
) -> crate::error::Result<crate::mods::platform::InstalledAsset> {
    crate::mods::assets::require_asset_kind(kind)?;
    let inst_root = instance_root(&app, &instance_id)?;
    let bytes =
        tokio::fs::read(&file_path)
            .await
            .map_err(|e| crate::error::Error::ModsInstancePath {
                path: file_path.clone(),
                details: format!("{} ({})", e, e.kind()),
            })?;
    let filename = std::path::Path::new(&file_path)
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| crate::error::Error::ModsInstancePath {
            path: file_path.clone(),
            details: "dropped path has no filename".into(),
        })?
        .to_string();
    let installed =
        crate::mods::asset_local::install_asset_local(&inst_root, kind, &filename, &bytes).await?;
    // Hand-dropped zip: no platform version to record.
    crate::journal::record(
        &inst_root,
        crate::journal::content(
            crate::journal::ContentAction::AssetInstalled,
            installed.name.clone(),
        ),
    );
    Ok(installed)
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

/// Apply one asset update with replace semantics: install `target` (fetch →
/// verify → copy → registry add), then — when the filename moved — remove the
/// superseded file (best-effort) and its registry row. A download or
/// verification failure leaves the currently installed asset fully intact.
#[tauri::command]
#[specta::specta]
pub async fn asset_update_one(
    app: tauri::AppHandle,
    instance_id: String,
    kind: crate::mods::platform::ContentKind,
    old_filename: String,
    target: crate::mods::platform::ModVersion,
) -> crate::error::Result<()> {
    crate::network::throttle::with_interactive(async move {
        crate::mods::assets::require_asset_kind(kind)?;
        let inst_root = instance_root(&app, &instance_id)?;
        let dd = data_dir(&app)?;
        let progress: crate::mods::install::ProgressFn = Box::new(|_, _, _| {});
        // The outgoing version, read before the replace drops its registry row.
        let previous = crate::mods::assets::list(&inst_root, kind)
            .await
            .ok()
            .and_then(|list| {
                list.into_iter()
                    .find(|a| a.filename.eq_ignore_ascii_case(&old_filename))
                    .and_then(|a| a.version_number)
            });
        let name = target.name.clone();
        let to_version = target.version_number.clone();
        crate::mods::install::update_asset(&dd, &inst_root, kind, &old_filename, target, &progress)
            .await?;
        crate::journal::record(
            &inst_root,
            crate::journal::content_versioned(
                crate::journal::ContentAction::AssetUpdated,
                name,
                previous,
                Some(to_version),
            ),
        );
        Ok(())
    })
    .await
}
