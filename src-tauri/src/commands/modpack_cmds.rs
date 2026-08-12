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
) -> Result<crate::mods::modpack::schema::ModpackImportOutcome, crate::error::Error> {
    crate::data_root::reject_if_fallen_back(&app)?;
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
        let outcome = modpack::import::install_resolved_pack(
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
        .await?;
        journal_pack_import(&app, &outcome.instance, outcome.details.clone());
        return Ok(outcome);
    }

    // Archive path (Modrinth `.mrpack` / CurseForge `.zip`).
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|e| crate::error::Error::Io {
            path: path.clone(),
            details: e.to_string(),
        })?;
    let outcome = modpack::import::import(
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
    .await?;
    journal_pack_import(&app, &outcome.instance, outcome.details.clone());
    Ok(outcome)
}

/// Open the freshly-created instance's journal with the import that made it.
/// Shared by the sidecar and archive paths so the two cannot record the pack
/// differently. Silent on a path failure — the import itself succeeded.
///
/// `mint_and_record` persists `details` under a fresh task id BEFORE the
/// journal write below (chained via `with_report_id`), so the row always
/// names a report that already exists on disk.
fn journal_pack_import(
    app: &tauri::AppHandle,
    imported: &crate::instances::schema::InstanceWithStatus,
    details: Vec<crate::tasks::TaskDetail>,
) {
    if let Ok(inst_root) = instance_root(app, &imported.id) {
        let task_id = crate::reports::mint_and_record(&inst_root, details);
        crate::journal::record(
            &inst_root,
            crate::journal::content_versioned(
                crate::journal::ContentAction::ModpackImported,
                imported
                    .mrpack_name
                    .clone()
                    .unwrap_or_else(|| imported.name.clone()),
                None,
                imported.mrpack_version.clone(),
            )
            .with_report_id(task_id),
        );
    }
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

/// What an inbound import link resolved to.
#[derive(Debug, Clone, Serialize, Type)]
pub struct ResolvedImportUrl {
    pub hit: ModpackHit,
    /// `None` when the link named no version, or named one that no longer
    /// exists — the UI then shows the full version list instead of failing the
    /// whole import over a stale link.
    pub version_id: Option<String>,
}

/// Resolve an inbound import link — a `lucerna://…` deeplink the OS handed us,
/// or a platform page URL the user pasted — to the pack it names.
///
/// **Read-only by design.** One metadata GET; no download, no install. The
/// returned hit is handed to the same detail modal and import picker the Browse
/// flow uses, so a link can only ever *open a confirmation*, never install
/// something behind the user's back (design spec §2).
#[tauri::command]
#[specta::specta]
pub async fn modpack_resolve_url(url: String) -> Result<ResolvedImportUrl, crate::error::Error> {
    let target = crate::deeplink::parse_import_url(&url)?;
    let source = modpack::source::modpack_source_for(target.source);
    let hit = source.resolve_project_hit(&target.project).await?;
    // A version reference that no longer resolves must not sink the import:
    // resolve what we can and let the UI show the version list. Matched against
    // the id first, then Modrinth's human version number (page URLs carry that,
    // not the id).
    let version_id = match &target.version {
        None => None,
        Some(want) => source
            .get_versions(&hit.project_id)
            .await
            .ok()
            .and_then(|versions| {
                versions
                    .iter()
                    .find(|v| &v.id == want || &v.version_number == want)
                    .map(|v| v.id.clone())
            }),
    };
    Ok(ResolvedImportUrl { hit, version_id })
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
    let mut st = crate::mods::modpack::import::compute_status(origin, &installed, &asset_present);
    // `compute_status` is pure; reading the completer's manifest needs the
    // instance directory, so it happens here.
    st.pack_completion = crate::mods::pack_completion::read(&inst_root);
    Ok(Some(st))
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
    // This restores exactly one file, so the "N of M" counter is a
    // constant 1 of 1 — no shared `ProgressCount` needed, unlike the
    // multi-item install/update paths.
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
                current: 1,
                total: 1,
            },
            crate::mods::install::ModInstallPhase::Verifying => ModInstallProgress::Verifying {
                instance_id: instance_id_for_progress.clone(),
                project_id: project_id_for_progress.clone(),
                bytes_done: done as f64,
                current: 1,
                total: 1,
            },
            crate::mods::install::ModInstallPhase::Copying => ModInstallProgress::Copying {
                instance_id: instance_id_for_progress.clone(),
                project_id: project_id_for_progress.clone(),
                current: 1,
                total: 1,
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
        crate::mods::install::install_one(&dd, &inst_root, mv, None, &prog).await?;
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
    // Restoring a pack file is an install from the user's point of view; the
    // action reflects WHAT was restored so the history reads consistently with
    // the rest of the mod/asset rows.
    let restored_action = if file.install_path.starts_with("mods/") {
        crate::journal::ContentAction::ModInstalled
    } else {
        crate::journal::ContentAction::AssetInstalled
    };
    crate::journal::record(
        &inst_root,
        crate::journal::content(restored_action, file.name.clone()),
    );
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
) -> crate::error::Result<crate::mods::modpack::schema::ModpackUpdateOutcome> {
    let inst = crate::instances::read_instance(&app, &instance_id)?;
    let inst_root = instance_root(&app, &instance_id)?;
    let dd = data_dir(&app)?;
    let origin = crate::mods::installed::get_pack_origin(&inst_root)
        .await?
        .ok_or_else(|| crate::error::Error::ModsNotFound {
            platform: "pack_origin".into(),
        })?;
    // Captured before `origin` is moved into `with_carried_notes` below — the
    // journal row for the version bump still needs to name where it came from.
    // (Two independently-green PRs, #312 and #315, collided here: one made
    // `with_carried_notes` take ownership, the other added a read after it.)
    let previous_pack_name = origin.project_name.clone();
    let previous_version = origin.version.clone();
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
    // Snapshot BEFORE any file is touched: which updated mods the user had
    // disabled, so the fresh versions can be re-disabled after install.
    let installed_before = crate::mods::installed::list(&inst_root).await?;
    let carry_disabled =
        crate::mods::modpack::import::carry_disabled_shas(&installed_before, &diff);

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
    // Concurrent pre-warm (same 8-way fan-out as fresh import). A major pack
    // bump changes hundreds of files; fetching them one-by-one was the single
    // largest wall-clock cost of an update. The serial loop below stays the
    // source of truth for per-file success/failure — it just hits warm cache.
    crate::mods::modpack::import::prewarm_cache(&dd, &to_fetch, &install_progress).await;
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

    // ---- Phase 2: apply locally (cache is warm). Continues past a
    // per-file failure and records it — see `apply_update_diff`'s doc
    // comment for why aborting here would leave a WORSE half-update than
    // continuing does. ----
    let details: Vec<crate::tasks::TaskDetail> = apply_update_diff(
        &dd,
        &inst_root,
        &diff,
        &summary.game_version,
        summary.loader,
        &carry_disabled,
        &install_progress,
    )
    .await;

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
    // Phase 2 has finished writing the new mod set, so the mods dir can be
    // re-classified for jars built for a loader family this instance cannot
    // load. Recomputing beats carrying the old verdict (stale after a loader
    // change) and beats clearing it (blanks the warning exactly when a loader
    // migration makes it most useful).
    let inert_loader_jars = crate::mods::modpack::import::classify_inert_loader_jars(
        &crate::mods::installed::mods_dir(&inst_root),
        summary.loader,
        &summary.game_version,
    );
    // The carried notes describe state an apply cannot alter — see
    // `with_carried_notes`.
    let new_origin = crate::mods::modpack::import::with_carried_notes(
        new_origin,
        origin,
        inert_loader_jars.clone(),
    );
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
    // One row for the whole version bump, with the file churn as detail —
    // the per-mod installs above are the mechanism, not the user's action.
    // `mint_and_record` persists `details` (phase 2's per-file rows) under a
    // fresh id BEFORE the journal write below, so the row always names a
    // report that already exists on disk.
    let task_id = crate::reports::mint_and_record(&inst_root, details.clone());
    crate::journal::record(
        &inst_root,
        crate::journal::JournalEvent::Content {
            action: crate::journal::ContentAction::ModpackUpdated,
            subject: previous_pack_name,
            from_version: Some(previous_version),
            to_version: Some(summary.version.clone()),
            affected: Some((diff.added.len() + diff.updated.len() + diff.removed.len()) as f64),
            report_id: Some(task_id),
        },
    );
    // Phase marker only — the result rides the return value below.
    let _ = on_progress.send(ModpackProgress::Done);
    Ok(crate::mods::modpack::schema::ModpackUpdateOutcome {
        instance: updated_inst,
        inert_loader_jars,
        details,
    })
}

/// Phase 2 of `modpack_apply_update`: remove the old files, then install the
/// new/changed ones from the already-warm cache. Takes plain
/// `dd`/`inst_root` paths (not an `AppHandle`) so it can be driven directly
/// in tests, mirroring `mods::modpack::import::install_selected_files`.
///
/// Continues past a per-file failure instead of aborting, and returns
/// (never errors) one `TaskDetail` row per install attempt plus a `Failed`
/// row for any removal or carried-disable step that errors. This is
/// deliberate, not an oversight: by the time phase 2 runs, the removals
/// loop has already started mutating the instance (it runs BEFORE the
/// installs loop), so an abort here is strictly worse than continuing —
/// it would leave old files deleted with nothing installed to replace
/// them, and `pack_origin`/the instance's version metadata would still
/// describe the OLD pack version because both are written after this
/// function returns. Continuing at least reaches that write and leaves a
/// coherent, documented record of exactly what landed and what didn't.
///
/// Removals are reported ONLY on failure. `remove_pack_file` swallows its
/// own file-unlink errors internally (`let _ =`) — only its registry
/// `remove` call propagates a real error via `?` — so a success row here
/// would assert an unlink that was never actually verified.
#[allow(clippy::too_many_arguments)]
async fn apply_update_diff(
    dd: &std::path::Path,
    inst_root: &std::path::Path,
    diff: &crate::mods::modpack::schema::ModpackUpdateDiff,
    game_version: &str,
    loader: crate::mods::platform::LoaderKind,
    carry_disabled: &[String],
    install_progress: &crate::mods::install::ProgressFn,
) -> Vec<crate::tasks::TaskDetail> {
    use crate::mods::modpack::import::modpack_file_detail;
    use crate::tasks::{DetailOutcome, TaskDetail};

    let mut details: Vec<TaskDetail> = Vec::new();

    for f in diff
        .removed
        .iter()
        .chain(diff.updated.iter().map(|e| &e.old))
    {
        if let Err(e) = remove_pack_file(inst_root, f).await {
            details.push(removal_failure_detail(f, e.to_string()));
        }
    }

    for f in diff.added.iter().chain(diff.updated.iter().map(|e| &e.new)) {
        if f.install_path.starts_with("mods/") {
            let mv =
                crate::mods::modpack::import::modpack_file_to_mod_version(f, game_version, loader);
            match crate::mods::install::install_one(dd, inst_root, mv, None, install_progress).await
            {
                Ok(installed) => {
                    let outcome = match installed.placement {
                        Some(placement) => DetailOutcome::Installed {
                            fetched: installed.fetched,
                            placement,
                        },
                        None => DetailOutcome::Unchanged,
                    };
                    details.push(modpack_file_detail(f, Some(&installed.sha1), outcome));

                    // Respect the user's choice IMMEDIATELY, not in a
                    // post-loop pass: a mod they disabled stays disabled
                    // across the update. Only reached after THIS file's
                    // install succeeded, so a later file's failure can't
                    // strand this one's disable state — and a failure
                    // here is recorded, not silently dropped.
                    if carry_disabled.contains(&f.sha1.to_ascii_lowercase()) {
                        if let Err(e) =
                            crate::mods::install::disable(inst_root, &f.sha1.to_ascii_lowercase())
                                .await
                        {
                            details.push(modpack_file_detail(
                                f,
                                Some(&f.sha1),
                                DetailOutcome::Failed {
                                    reason: e.to_string(),
                                },
                            ));
                        }
                    }
                }
                Err(e) => {
                    details.push(modpack_file_detail(
                        f,
                        Some(&f.sha1),
                        DetailOutcome::Failed {
                            reason: e.to_string(),
                        },
                    ));
                }
            }
        } else {
            match crate::mods::install::install_asset(
                dd,
                inst_root,
                &f.url,
                &f.sha1,
                f.size,
                &f.install_path,
                install_progress,
            )
            .await
            {
                Ok(asset) => {
                    details.push(modpack_file_detail(
                        f,
                        Some(&f.sha1),
                        DetailOutcome::Installed {
                            fetched: asset.fetched,
                            placement: asset.placement,
                        },
                    ));
                }
                Err(e) => {
                    details.push(modpack_file_detail(
                        f,
                        Some(&f.sha1),
                        DetailOutcome::Failed {
                            reason: e.to_string(),
                        },
                    ));
                }
            }
        }
    }

    details
}

/// Build the `TaskDetail` row for a phase-2 removal failure. Only ever
/// called on `remove_pack_file`'s `Err` arm — see `apply_update_diff`'s
/// doc comment for why no row is ever built for a removal that reports
/// success.
fn removal_failure_detail(
    file: &crate::mods::installed::PackOriginFile,
    reason: String,
) -> crate::tasks::TaskDetail {
    crate::tasks::TaskDetail {
        name: file.name.clone(),
        install_path: file.install_path.clone(),
        origin: file.source.into(),
        host: crate::network::request::host_of(&file.url),
        bytes: Some(file.size),
        sha1: {
            let s = file.sha1.trim();
            if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        },
        outcome: crate::tasks::DetailOutcome::Failed { reason },
    }
}

#[cfg(test)]
mod apply_update_diff_tests {
    use super::*;
    use crate::mods::modpack::schema::{EnvSupport, ModpackFile, ModpackUpdateDiff};
    use crate::mods::platform::ModSource;
    use crate::tasks::DetailOutcome;

    fn added_file(sha: &str, url: String) -> ModpackFile {
        ModpackFile {
            project_id: format!("proj-{sha}"),
            version_id: format!("ver-{sha}"),
            name: format!("Mod {sha}"),
            filename: format!("{sha}.jar"),
            install_path: format!("mods/{sha}.jar"),
            sha1: sha.into(),
            md5: None,
            url,
            size: 42.0,
            env_client: EnvSupport::Required,
            source: ModSource::Modrinth,
        }
    }

    #[tokio::test]
    async fn apply_update_diff_continues_past_one_failure_and_installs_the_rest() {
        use sha1::{Digest, Sha1};
        use tempfile::TempDir;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let body: &[u8] = b"apply-update-good-bytes";
        let good_sha1 = hex::encode(Sha1::digest(body));

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/good.jar"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body.to_vec()))
            .mount(&server)
            .await;

        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let td_data = TempDir::new().unwrap();
        let td_inst = TempDir::new().unwrap();

        let good = added_file(&good_sha1, format!("{}/good.jar", server.uri()));
        let bad = added_file(
            "deadbeefcafe",
            "https://not-on-allowlist.example.invalid/bad.jar".into(),
        );

        let diff = ModpackUpdateDiff {
            added: vec![good.clone(), bad.clone()],
            removed: vec![],
            updated: vec![],
            version_bump: None,
            new_version_number: "2.0.0".into(),
        };

        let noop: crate::mods::install::ProgressFn = Box::new(|_, _, _| {});

        // No `.expect`/`?` here at all — that IS the fix: `apply_update_diff`
        // returns `Vec<TaskDetail>` unconditionally now, it never aborts.
        let details = apply_update_diff(
            td_data.path(),
            td_inst.path(),
            &diff,
            "1.20.1",
            crate::mods::platform::LoaderKind::Fabric,
            &[],
            &noop,
        )
        .await;

        assert_eq!(
            details.len(),
            2,
            "one TaskDetail row per file, success and failure alike"
        );

        let good_detail = details
            .iter()
            .find(|d| d.install_path == good.install_path)
            .expect("the succeeding file must still have a row");
        assert!(
            matches!(good_detail.outcome, DetailOutcome::Installed { .. }),
            "expected Installed, got {:?}",
            good_detail.outcome
        );

        let bad_detail = details
            .iter()
            .find(|d| d.install_path == bad.install_path)
            .expect("the failing file must still have a row");
        match &bad_detail.outcome {
            DetailOutcome::Failed { reason } => assert!(!reason.is_empty()),
            other => panic!("expected Failed, got {other:?}"),
        }

        // The whole point of the change: a failure on one file must not
        // stop the OTHER file from actually landing on disk.
        let installed_jar = td_inst
            .path()
            .join(".minecraft")
            .join("mods")
            .join(&good.filename);
        assert!(
            tokio::fs::try_exists(&installed_jar).await.unwrap(),
            "the non-failing file must still be installed on disk"
        );
    }
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
    // Persist, don't just announce. The drawer's "Bundled files skipped" and
    // "Files that won't load" sections read `pack_origin`, not this event — so
    // reporting the fresh lists only here left the persisted ones stale after a
    // reimport. That staleness used to be cleared by accident on the next
    // version apply (which wiped the notes); now that an apply correctly carries
    // them forward, a stale note would survive indefinitely.
    //
    // Re-extracting `overrides/` can drop bundled jars into `mods/`, so the
    // inert-loader classification is redone here too rather than left alone.
    let inert_loader_jars = crate::mods::modpack::import::classify_inert_loader_jars(
        &crate::mods::installed::mods_dir(&inst_root),
        inst.loader,
        &inst.mc_version,
    );
    if let Some(mut origin) = crate::mods::installed::get_pack_origin(&inst_root).await? {
        origin.skipped_overrides = outcome.skipped.clone();
        origin.inert_loader_jars = inert_loader_jars.clone();
        crate::mods::installed::set_pack_origin(&inst_root, origin).await?;
    }

    // Phase marker only. This path's caller (`ImportedDetailDrawer`'s
    // `reimportPackFiles`) discards the result and re-reads the instance, and
    // the drawer's "skipped"/"won't load" sections read `pack_origin` — which
    // was just refreshed above — so there is nothing for a payload to carry.
    let _ = on_progress.send(ModpackProgress::Done);
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
