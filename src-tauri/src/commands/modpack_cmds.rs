use super::*;

// =========================================================================
// Modpack import (v0.5.0 sub-feature 4)
// =========================================================================

/// Read a `.mrpack` / `.zip` from disk and return a parsed summary
/// (resolved mod files, overrides count, loader, mc version). The UI
/// uses this for the picker dialog before the user commits to import.
/// For `.ftbpack.json` / `.atlpack.json` sidecar files the summary is
/// deserialised directly — no archive parsing needed.
#[tauri::command]
#[specta::specta]
pub async fn modpack_inspect(path: String) -> Result<ModpackSummary, crate::error::Error> {
    if is_staged_summary_sidecar(&path) {
        return read_staged_sidecar(&path).await;
    }
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|e| crate::error::Error::Io {
            path: path.clone(),
            details: e.to_string(),
        })?;
    modpack::import::inspect(&bytes, "https://api.curseforge.com").await
}

/// Run the full import: create an instance, download every selected
/// mod (subject to license / distribution allowance), then optionally
/// extract overrides into the instance's `.minecraft/`. Streams two
/// kinds of progress over typed channels:
/// - `on_progress`: coarse-grained `ModpackProgress` phases.
/// - `on_install_progress`: per-mod `ProgressTick` (download / verify
///   / copy bytes). The per-mod stream is keyed by phase only, not by
///   `project_id` — the UI correlates it with the `InstallingFile`
///   phase emitted on `on_progress`.
///
/// For `.ftbpack.json` / `.atlpack.json` sidecar files the summary is
/// deserialised directly and the archive path is skipped entirely (no bytes
/// to read, no overrides).
#[tauri::command]
#[specta::specta]
#[allow(clippy::too_many_arguments)]
pub async fn modpack_import(
    app: tauri::AppHandle,
    path: String,
    selected_shas: Vec<String>,
    apply_overrides: bool,
    // Optional provenance hints from the Browse sub-tab. When the user
    // imports straight off a `ModpackHit` the UI already has these and
    // can pass them through, letting the orchestrator skip a Modrinth
    // /v2/version round-trip. Drag-drop imports pass `null` and the
    // orchestrator auto-looks-up.
    hint_project_id: Option<String>,
    hint_source: Option<crate::mods::platform::ModSource>,
    hint_version_id: Option<String>,
    on_progress: Channel<ModpackProgress>,
    on_install_progress: Channel<crate::mods::install::ProgressTick>,
) -> Result<crate::instances::schema::InstanceWithStatus, crate::error::Error> {
    let install_progress: crate::mods::install::ProgressFn =
        Box::new(move |phase, current, total| {
            let _ = on_install_progress.send(crate::mods::install::ProgressTick {
                phase,
                current: current as f64,
                total: total.map(|t| t as f64),
            });
        });

    // Sidecar path: the `.ftbpack.json` / `.atlpack.json` file holds a
    // pre-resolved `ModpackSummary` serialised by the source's
    // `stage_version_to_temp`.  No archive bytes exist, so overrides
    // extraction is skipped (`archive_bytes = None`).
    if is_staged_summary_sidecar(&path) {
        let summary = read_staged_sidecar(&path).await?;
        on_progress.send(ModpackProgress::Inspecting).ok();
        return modpack::import::install_resolved_pack(
            &app,
            summary,
            &selected_shas,
            apply_overrides,
            None, // no archive bytes, no overrides
            "https://api.curseforge.com",
            hint_project_id,
            hint_source,
            hint_version_id,
            &|p| {
                let _ = on_progress.send(p);
            },
            install_progress,
        )
        .await;
    }

    // Archive path (Modrinth `.mrpack` / CurseForge `.zip`).
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|e| crate::error::Error::Io {
            path: path.clone(),
            details: e.to_string(),
        })?;
    modpack::import::import(
        &app,
        &bytes,
        &selected_shas,
        apply_overrides,
        "https://api.curseforge.com",
        hint_project_id,
        hint_source,
        hint_version_id,
        &|p| {
            let _ = on_progress.send(p);
        },
        install_progress,
    )
    .await
}

/// Search a modpack catalogue. `source` selects Modrinth (anonymous)
/// or CurseForge (requires a stored API key — a missing key surfaces
/// as `ModsPlatformAuth`, which the UI maps to the key banner).
#[tauri::command]
#[specta::specta]
pub async fn modpack_search(
    source: crate::mods::platform::ModSource,
    query: String,
    page: u32,
    mc_version: Option<String>,
    loader: Option<crate::mods::platform::LoaderKind>,
    sort: ModpackSort,
    page_size: u32,
) -> Result<ModpackSearchPage, crate::error::Error> {
    modpack::source::modpack_source_for(source)
        .search(&query, page, mc_version.as_deref(), loader, sort, page_size)
        .await
}

/// Pull a modpack version's archive to a temp path under the OS temp
/// dir, and return the absolute path so the UI can hand it to
/// `modpack_inspect` / `modpack_import`. Modrinth versions resolve to a
/// primary `.mrpack`; CurseForge versions resolve a file's
/// `downloadUrl` to a `.zip`. The temp file is left in place after
/// import — a successful import has already copied every byte that
/// matters into the instance.
#[tauri::command]
#[specta::specta]
pub async fn modpack_fetch_to_temp(
    app: tauri::AppHandle,
    source: crate::mods::platform::ModSource,
    project_id: String,
    version_id: String,
) -> Result<String, crate::error::Error> {
    modpack::source::modpack_source_for(source)
        .stage_version_to_temp(&app, &project_id, &version_id)
        .await
}

/// Return the pack-origin snapshot + a live diff for a pack-imported
/// instance. Returns `None` for instances that were manually created
/// (no .mrpack/.zip ever ran through the import pipeline) and for
/// pre-bundle-2 imports that pre-date the `pack_origin` field. Single
/// IPC round-trip combines `get_pack_origin` + the modified-check so
/// the UI doesn't have to make two calls per card.
#[tauri::command]
#[specta::specta]
pub async fn modpack_status(
    app: tauri::AppHandle,
    instance_id: String,
) -> crate::error::Result<Option<ModpackStatus>> {
    let inst_root = instance_root(&app, &instance_id)?;
    let origin = match crate::mods::installed::get_pack_origin(&inst_root).await? {
        Some(o) => o,
        None => return Ok(None),
    };
    let installed = crate::mods::installed::list(&inst_root).await?;
    // Asset (non-mods/) origin files: an asset is "present" iff its file
    // exists at the declared path under the instance's .minecraft/.
    let mc_dir = inst_root.join(".minecraft");
    let mut asset_present: std::collections::HashSet<String> = Default::default();
    for f in &origin.files {
        if !crate::mods::modpack::import::is_tracked_mod(&f.install_path)
            && tokio::fs::try_exists(mc_dir.join(&f.install_path))
                .await
                .unwrap_or(false)
        {
            asset_present.insert(f.install_path.clone());
        }
    }
    Ok(Some(crate::mods::modpack::import::compute_status(
        origin,
        &installed,
        &asset_present,
    )))
}

/// Record that the user installed a substitute (from `substitute_source` /
/// `substitute_project_id`, typically Modrinth) for a blocked `missing_mods`
/// entry identified by `(entry_filename, entry_mod_name)`. Looks the installed
/// substitute's SHA-1 up in the registry, then upserts a `ResolvedMissing`
/// onto the instance's `PackOrigin` so `modpack_status` reports the entry as
/// `Substituted`. Idempotent. Errors `ModsNotFound` when the substitute jar is
/// not in the registry yet, or the instance has no pack origin.
#[tauri::command]
#[specta::specta]
pub async fn modpack_resolve_missing_with(
    app: tauri::AppHandle,
    instance_id: String,
    entry_filename: String,
    entry_mod_name: String,
    substitute_source: ModSource,
    substitute_project_id: String,
) -> crate::error::Result<()> {
    let inst_root = instance_root(&app, &instance_id)?;
    let installed = crate::mods::installed::list(&inst_root).await?;
    let sha1 = installed
        .iter()
        .find(|m| {
            m.source == Some(substitute_source)
                && m.project_id.as_deref() == Some(substitute_project_id.as_str())
        })
        .map(|m| m.sha1.to_ascii_lowercase())
        .ok_or_else(|| crate::error::Error::ModsNotFound {
            platform: "substitute".into(),
        })?;

    let mut origin = crate::mods::installed::get_pack_origin(&inst_root)
        .await?
        .ok_or_else(|| crate::error::Error::ModsNotFound {
            platform: "pack_origin".into(),
        })?;

    // Upsert: one overlay row per (filename, mod_name).
    origin.resolved_missing.retain(|r| {
        !(r.filename.eq_ignore_ascii_case(&entry_filename) && r.mod_name == entry_mod_name)
    });
    origin
        .resolved_missing
        .push(crate::mods::installed::ResolvedMissing {
            filename: entry_filename,
            mod_name: entry_mod_name,
            sha1,
        });

    crate::mods::installed::set_pack_origin(&inst_root, origin).await?;
    Ok(())
}

/// Re-install a single file that was part of the original pack but is
/// no longer in the instance (= it shows up in `ModpackStatus.removed_files`).
/// Looks the file up by `sha1` in the frozen origin snapshot,
/// synthesises a `ModVersion` from the snapshot fields, and calls
/// `install_one`. Errors `ModsNotFound { source: "pack_origin" }` if
/// `sha1` is not in the origin (= caller has stale data, or the
/// instance has no origin at all).
#[tauri::command]
#[specta::specta]
pub async fn modpack_restore_file(
    app: tauri::AppHandle,
    instance_id: String,
    sha1: String,
) -> crate::error::Result<()> {
    let inst_root = instance_root(&app, &instance_id)?;
    let dd = data_dir(&app)?;
    let origin = crate::mods::installed::get_pack_origin(&inst_root)
        .await?
        .ok_or_else(|| crate::error::Error::ModsNotFound {
            platform: "pack_origin".into(),
        })?;
    let file = origin
        .files
        .iter()
        .find(|f| f.sha1.eq_ignore_ascii_case(&sha1))
        .cloned()
        .ok_or_else(|| crate::error::Error::ModsNotFound {
            platform: "pack_origin".into(),
        })?;
    // Bundled-from-overrides entries carry no URL — the bytes lived
    // inside the .mrpack archive and we don't keep that archive after
    // import. Tell the UI to gray out / disable the Restore button via
    // a typed error instead of trying to install_one a no-URL file.
    if file.url.is_empty() {
        return Err(crate::error::Error::ModpackBundledNoUrl {
            mod_name: file.name.clone(),
        });
    }
    // Re-use the same per-mod progress wiring as `mods_install_with_deps`
    // so the UI surfaces the same Downloading/Verifying/Copying states.
    let app_for_progress = app.clone();
    let instance_id_for_progress = instance_id.clone();
    let project_id_for_progress = file.project_id.clone();
    let prog: crate::mods::install::ProgressFn = Box::new(move |phase, done, total| {
        let payload = match phase {
            crate::mods::install::ModInstallPhase::Downloading => ModInstallProgress::Downloading {
                instance_id: instance_id_for_progress.clone(),
                project_id: project_id_for_progress.clone(),
                bytes_done: done as f64,
                bytes_total: total.map(|t| t as f64),
            },
            crate::mods::install::ModInstallPhase::Verifying => ModInstallProgress::Verifying {
                instance_id: instance_id_for_progress.clone(),
                project_id: project_id_for_progress.clone(),
                bytes_done: done as f64,
            },
            crate::mods::install::ModInstallPhase::Copying => ModInstallProgress::Copying {
                instance_id: instance_id_for_progress.clone(),
                project_id: project_id_for_progress.clone(),
            },
        };
        let _ = payload.emit(&app_for_progress);
    });

    if file.install_path.starts_with("mods/") {
        let (mc_version, loader) = read_active_mc_and_loader(&app, &instance_id)?;
        let mv = crate::mods::modpack::import::pack_origin_file_to_mod_version(
            &file,
            &mc_version,
            loader,
        );
        crate::mods::install::install_one(&dd, &inst_root, mv, &prog).await?;
    } else {
        crate::mods::install::install_asset(
            &dd,
            &inst_root,
            &file.url,
            &file.sha1,
            file.size,
            &file.install_path,
            &prog,
        )
        .await?;
    }
    let _ = ModInstalled {
        instance_id: instance_id.clone(),
        sha1: file.sha1.clone(),
        filename: file.filename.clone(),
        name: file.name.clone(),
    }
    .emit(&app);
    Ok(())
}

/// List the published versions of a modpack project. Modrinth versions
/// come from `/v2/project/{id}/version`; CurseForge versions are the
/// project's files (`/v1/mods/{id}/files`).
#[tauri::command]
#[specta::specta]
pub async fn modpack_get_versions(
    source: crate::mods::platform::ModSource,
    project_id: String,
) -> crate::error::Result<Vec<crate::mods::modpack::schema::ModpackVersionEntry>> {
    modpack::source::modpack_source_for(source)
        .get_versions(&project_id)
        .await
}

/// Fetch a modpack project's description + gallery for the detail modal's
/// Overview tab. Modrinth: `/v2/project/{id}`. CurseForge: `/v1/mods/{id}`
/// + the description endpoint.
#[tauri::command]
#[specta::specta]
pub async fn modpack_project(
    source: crate::mods::platform::ModSource,
    project_id: String,
) -> crate::error::Result<crate::mods::modpack::schema::ModpackProject> {
    modpack::source::modpack_source_for(source)
        .get_project(&project_id)
        .await
}

/// Capability descriptor for a modpack source — read by the UI to drive
/// source-specific affordances (hide the API-key prompt, grey out server
/// filters, hide export) without hardcoding per-source branches.
#[tauri::command]
#[specta::specta]
pub async fn modpack_source_caps(
    source: crate::mods::platform::ModSource,
) -> Result<crate::mods::modpack::source::SourceCaps, crate::error::Error> {
    Ok(modpack::source::modpack_source_for(source).caps())
}

/// Resolve the update status of one pack instance for all sources. Pure
/// preconditions live in `update_status::precheck`; the network list comes
/// from the per-source `get_versions`; a transient error becomes
/// `CheckFailed` so a single offline pack never poisons a batch sweep.
pub(crate) async fn compute_modpack_update_status(
    app: &tauri::AppHandle,
    instance_id: &str,
) -> crate::error::Result<crate::mods::modpack::update_status::ModpackUpdateStatus> {
    use crate::mods::modpack::update_status::{precheck, status_from_versions};

    let inst = crate::instances::read_instance(app, instance_id)?;
    let cf_key_present = crate::mods::curseforge::keyring::resolve().is_some();
    let (source, project_id, version_id) = match precheck(
        inst.mrpack_source,
        inst.mrpack_project_id.as_deref(),
        inst.mrpack_version_id.as_deref(),
        cf_key_present,
    ) {
        Ok(provenance) => provenance,
        // precheck's Err is Box<ModpackUpdateStatus>; deref to return the status.
        Err(status) => return Ok(*status),
    };

    match modpack::source::modpack_source_for(source)
        .get_versions(&project_id)
        .await
    {
        Ok(versions) => Ok(status_from_versions(versions, &version_id)),
        Err(e) => Ok(
            crate::mods::modpack::update_status::ModpackUpdateStatus::CheckFailed {
                message: e.to_string(),
            },
        ),
    }
}

/// Per-instance modpack update check across all four sources. Returns an
/// explicit status that distinguishes "up to date" from "not checkable"
/// (the former Modrinth-only `modpack_check_update` has been removed).
#[tauri::command]
#[specta::specta]
pub async fn modpack_update_status(
    app: tauri::AppHandle,
    instance_id: String,
) -> crate::error::Result<crate::mods::modpack::update_status::ModpackUpdateStatus> {
    compute_modpack_update_status(&app, &instance_id).await
}

/// Batch update-check for many pack instances at once. Bounds concurrency so
/// a large library doesn't fan out dozens of simultaneous requests (per-IP
/// rate-limit safety). A per-instance failure is captured as `CheckFailed`
/// in that entry's status — it never aborts the whole sweep.
#[tauri::command]
#[specta::specta]
pub async fn modpacks_check_updates(
    app: tauri::AppHandle,
    instance_ids: Vec<String>,
) -> crate::error::Result<Vec<crate::mods::modpack::update_status::ModpackInstanceUpdate>> {
    use crate::mods::modpack::update_status::{ModpackInstanceUpdate, ModpackUpdateStatus};
    use futures_util::stream::{self, StreamExt};

    const CHECK_UPDATES_CONCURRENCY: usize = 6;

    let out: Vec<ModpackInstanceUpdate> = stream::iter(instance_ids)
        .map(|instance_id| {
            let app = app.clone();
            async move {
                let status = compute_modpack_update_status(&app, &instance_id)
                    .await
                    .unwrap_or_else(|e| ModpackUpdateStatus::CheckFailed {
                        message: e.to_string(),
                    });
                ModpackInstanceUpdate {
                    instance_id,
                    status,
                }
            }
        })
        .buffer_unordered(CHECK_UPDATES_CONCURRENCY)
        .collect()
        .await;
    Ok(out)
}

/// Diff a downloaded new-version `.mrpack` (already fetched to
/// `mrpack_path` via `modpack_fetch_to_temp`) against the instance's
/// current `pack_origin`. Returns the diff for the confirm dialog.
#[tauri::command]
#[specta::specta]
pub async fn modpack_compute_update(
    app: tauri::AppHandle,
    instance_id: String,
    mrpack_path: String,
) -> crate::error::Result<crate::mods::modpack::schema::ModpackUpdateDiff> {
    let inst = crate::instances::read_instance(&app, &instance_id)?;
    let inst_root = instance_root(&app, &instance_id)?;
    let origin = crate::mods::installed::get_pack_origin(&inst_root)
        .await?
        .ok_or_else(|| crate::error::Error::ModsNotFound {
            platform: "pack_origin".into(),
        })?;
    let bytes = tokio::fs::read(&mrpack_path)
        .await
        .map_err(|e| crate::error::Error::Io {
            path: mrpack_path.clone(),
            details: e.to_string(),
        })?;
    let summary =
        crate::mods::modpack::import::inspect(&bytes, "https://api.curseforge.com").await?;
    Ok(crate::mods::modpack::import::compute_update_diff(
        &summary,
        &origin,
        &inst.mc_version,
        inst.loader,
        &inst.loader_version,
    ))
}

/// Apply a modpack update in place. Phase 1 downloads every new/changed
/// file into the shared cache (the instance is NOT touched — a failure
/// here aborts cleanly). Phase 2 removes the old files, installs the new
/// ones from the warm cache, and rewrites `pack_origin` + the instance's
/// version metadata. `overrides/`-bundled content is not touched.
#[tauri::command]
#[specta::specta]
pub async fn modpack_apply_update(
    app: tauri::AppHandle,
    instance_id: String,
    mrpack_path: String,
    new_version_id: String,
    on_progress: Channel<ModpackProgress>,
    on_install_progress: Channel<crate::mods::install::ProgressTick>,
) -> crate::error::Result<crate::instances::schema::InstanceWithStatus> {
    let inst = crate::instances::read_instance(&app, &instance_id)?;
    let inst_root = instance_root(&app, &instance_id)?;
    let dd = data_dir(&app)?;
    let origin = crate::mods::installed::get_pack_origin(&inst_root)
        .await?
        .ok_or_else(|| crate::error::Error::ModsNotFound {
            platform: "pack_origin".into(),
        })?;
    let bytes = tokio::fs::read(&mrpack_path)
        .await
        .map_err(|e| crate::error::Error::Io {
            path: mrpack_path.clone(),
            details: e.to_string(),
        })?;
    let summary =
        crate::mods::modpack::import::inspect(&bytes, "https://api.curseforge.com").await?;
    let diff = crate::mods::modpack::import::compute_update_diff(
        &summary,
        &origin,
        &inst.mc_version,
        inst.loader,
        &inst.loader_version,
    );

    let install_progress: crate::mods::install::ProgressFn = {
        let ch = on_install_progress.clone();
        Box::new(move |phase, current, total| {
            let _ = ch.send(crate::mods::install::ProgressTick {
                phase,
                current: current as f64,
                total: total.map(|t| t as f64),
            });
        })
    };

    // ---- Phase 1: download every new/changed file into the cache. ----
    let to_fetch: Vec<&crate::mods::modpack::schema::ModpackFile> = diff
        .added
        .iter()
        .chain(diff.updated.iter().map(|e| &e.new))
        .collect();
    let total = to_fetch.len() as u32;
    for (idx, f) in to_fetch.iter().enumerate() {
        let _ = on_progress.send(ModpackProgress::InstallingFile {
            current: idx as u32 + 1,
            total,
            file_name: f.name.clone(),
        });
        crate::mods::install::fetch_to_cache(
            &dd,
            &f.url,
            &f.sha1.to_ascii_lowercase(),
            f.size,
            "modpacks",
            &install_progress,
        )
        .await?;
    }

    // ---- Phase 2: apply locally (cache is warm). ----
    for f in diff
        .removed
        .iter()
        .chain(diff.updated.iter().map(|e| &e.old))
    {
        remove_pack_file(&inst_root, f).await?;
    }
    for f in diff.added.iter().chain(diff.updated.iter().map(|e| &e.new)) {
        if f.install_path.starts_with("mods/") {
            let mv = crate::mods::modpack::import::modpack_file_to_mod_version(
                f,
                &summary.game_version,
                summary.loader,
            );
            crate::mods::install::install_one(&dd, &inst_root, mv, &install_progress).await?;
        } else {
            crate::mods::install::install_asset(
                &dd,
                &inst_root,
                &f.url,
                &f.sha1,
                f.size,
                &f.install_path,
                &install_progress,
            )
            .await?;
        }
    }

    // Rewrite pack_origin: new files[] entries + carried-over bundled.
    let bundled: Vec<crate::mods::installed::PackOriginFile> = origin
        .files
        .iter()
        .filter(|f| f.url.is_empty())
        .cloned()
        .collect();
    let selected: Vec<&crate::mods::modpack::schema::ModpackFile> =
        summary.files.iter().filter(|f| !f.url.is_empty()).collect();
    let mut new_origin = crate::mods::modpack::import::build_pack_origin(
        &summary,
        &selected,
        origin.project_id.clone(),
        &origin.project_name,
    );
    new_origin.files.extend(bundled);
    crate::mods::installed::set_pack_origin(&inst_root, new_origin).await?;

    let updated_inst = crate::instances::set_instance_pack_update(
        &app,
        &instance_id,
        summary.version.clone(),
        summary.game_version.clone(),
        summary.loader,
        summary.loader_version.clone(),
        new_version_id,
    )?;
    let _ = on_progress.send(ModpackProgress::Done {
        instance_id: instance_id.clone(),
        // A version update never touches `overrides/`, so nothing is skipped.
        skipped_overrides: vec![],
    });
    Ok(updated_inst)
}

/// Re-fetch the instance's current modpack version and re-extract its
/// `overrides/` — recovers bundled mods/files that a per-file Restore
/// cannot. Modrinth pack instances only.
#[tauri::command]
#[specta::specta]
pub async fn modpack_reimport_overrides(
    app: tauri::AppHandle,
    instance_id: String,
    on_progress: Channel<ModpackProgress>,
) -> crate::error::Result<()> {
    let inst = crate::instances::read_instance(&app, &instance_id)?;
    let inst_root = instance_root(&app, &instance_id)?;
    let (project_id, version_id) = match (
        inst.mrpack_source,
        inst.mrpack_project_id.as_deref(),
        inst.mrpack_version_id.as_deref(),
    ) {
        (Some(crate::mods::platform::ModSource::Modrinth), Some(pid), Some(vid)) => {
            (pid.to_string(), vid.to_string())
        }
        _ => {
            return Err(crate::error::Error::ModsNotFound {
                platform: "modrinth".into(),
            })
        }
    };

    let temp_path = modpack_fetch_to_temp(
        app.clone(),
        crate::mods::platform::ModSource::Modrinth,
        project_id,
        version_id,
    )
    .await?;
    let bytes = tokio::fs::read(&temp_path)
        .await
        .map_err(|e| crate::error::Error::Io {
            path: temp_path.clone(),
            details: e.to_string(),
        })?;
    let outcome = crate::mods::modpack::overrides::extract(&bytes, &inst_root, |c, t| {
        let _ = on_progress.send(ModpackProgress::ExtractingOverrides {
            current: c,
            total: t,
        });
    })
    .await?;
    let _ = on_progress.send(ModpackProgress::Done {
        instance_id: instance_id.clone(),
        skipped_overrides: outcome.skipped,
    });
    Ok(())
}

// Modpack export (v0.6.0):

/// Return a preview of what will be included in a modpack export for the
/// given instance: enabled mods (with their resolution metadata), which
/// optional content directories exist, and the cumulative saves size for
/// the privacy/size warning in the export dialog.
#[tauri::command]
#[specta::specta]
pub async fn export_preview(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<crate::mods::modpack::export::ExportPreview, crate::error::Error> {
    use crate::mods::modpack::export::{ExportModInfo, ExportPreview};
    let root = instance_root(&app, &instance_id)?;
    let mc = root.join(".minecraft");

    let mods = crate::mods::installed::list(&root).await?;
    let mod_infos: Vec<ExportModInfo> = mods
        .iter()
        .filter(|m| m.enabled)
        .map(|m| ExportModInfo {
            sha1: m.sha1.clone(),
            name: m.name.clone(),
            filename: m.filename.clone(),
            source: m.source,
            has_ids: m.project_id.is_some() && m.version_id.is_some(),
        })
        .collect();

    let mc2 = mc.clone();
    let (has_config, has_resourcepacks, has_shaderpacks, has_saves, saves_size) =
        tokio::task::spawn_blocking(move || {
            let has = |n: &str| mc2.join(n).is_dir();
            (
                has("config"),
                has("resourcepacks"),
                has("shaderpacks"),
                has("saves"),
                dir_size_bytes(&mc2.join("saves")),
            )
        })
        .await
        .map_err(|e| crate::error::Error::io("<export_preview scan>", e))?;

    Ok(ExportPreview {
        mods: mod_infos,
        has_config,
        has_resourcepacks,
        has_shaderpacks,
        has_saves,
        saves_size_bytes: saves_size as f64,
    })
}

/// Run a full modpack export for `instance_id`, writing a `.mrpack` or
/// CurseForge `.zip` to `dest_path`. Progress events (Resolving, Bundling,
/// Writing, Done) are delivered over `on_progress`. Returns `Ok(())` on
/// success; the `Done` event carries the resolved output path.
#[tauri::command]
#[specta::specta]
pub async fn export_modpack(
    app: tauri::AppHandle,
    instance_id: String,
    options: crate::mods::modpack::export::ExportOptions,
    dest_path: String,
    on_progress: tauri::ipc::Channel<crate::mods::modpack::export::ModpackExportProgress>,
) -> Result<(), crate::error::Error> {
    let root = instance_root(&app, &instance_id)?;
    let inst = crate::instances::read_instance(&app, &instance_id)?;
    let mods = crate::mods::installed::list(&root).await?;
    let enabled: Vec<crate::mods::platform::InstalledMod> =
        mods.into_iter().filter(|m| m.enabled).collect();

    let sink = move |p: crate::mods::modpack::export::ModpackExportProgress| {
        let _ = on_progress.send(p);
    };

    crate::mods::modpack::export::run_export(
        &root,
        &inst.mc_version,
        inst.loader,
        inst.loader_version.as_deref(),
        &enabled,
        &options,
        std::path::Path::new(&dest_path),
        &sink,
    )
    .await
}
