use super::*;

/// List every log file under `instance_id`'s three documented roots.
/// Sorted by mtime descending.
#[tauri::command]
#[specta::specta]
pub fn list_log_files(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<Vec<crate::logs::files::LogFileMeta>, crate::error::Error> {
    crate::logs::files::list_log_files(&app, &instance_id)
}

/// Read up to `max_bytes` of a log file. `max_bytes` is clamped to
/// `[64 KB, 100 MB]`; `0` becomes the 5 MB default. `path` must be
/// under one of SOME instance's allowed log roots — anything else is
/// rejected with `Error::Io`.
#[tauri::command]
#[specta::specta]
pub fn read_log_file(
    app: tauri::AppHandle,
    path: String,
    max_bytes: f64,
) -> Result<String, crate::error::Error> {
    let all = crate::instances::list_instances_with_status(&app)?;
    let mut roots: Vec<std::path::PathBuf> = Vec::new();
    for inst in &all {
        let mut r = crate::logs::files::allowed_roots(&app, &inst.id)?;
        roots.append(&mut r);
    }
    let path = std::path::PathBuf::from(&path);
    crate::logs::files::assert_under_allowed_roots(&path, &roots)?;
    let cap = if !max_bytes.is_finite() || max_bytes < 0.0 {
        0
    } else {
        max_bytes as u64
    };
    crate::logs::read::read_with_cap(&path, cap)
}

/// Newest crash report (if any) for `instance_id`. Used by the UI to
/// show a banner on non-zero MC exit.
#[tauri::command]
#[specta::specta]
pub fn latest_crash(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<Option<crate::logs::files::CrashReport>, crate::error::Error> {
    crate::logs::files::latest_crash(&app, &instance_id)
}

/// Run the diagnoser over `path`. Returns `Ok(None)` when no known
/// pattern matches or the file is empty/too short. Path must be
/// under one of `instance_id`'s allowed log roots — anything else
/// is rejected with `Error::Io` (mirrors `read_log_file` semantics
/// but scoped to a single instance rather than all instances).
#[tauri::command]
#[specta::specta]
pub async fn diagnose_log(
    app: tauri::AppHandle,
    instance_id: String,
    path: String,
) -> Result<Option<crate::logs::diagnose::Diagnosis>, crate::error::Error> {
    let roots = crate::logs::files::allowed_roots(&app, &instance_id)?;
    let path_buf = std::path::PathBuf::from(&path);
    crate::logs::files::assert_under_allowed_roots(&path_buf, &roots)?;
    let Some(diag) = crate::logs::diagnose::diagnose(&path_buf).await? else {
        return Ok(None);
    };
    // Stale-log guard: the missing-mods diagnosis is driven by historical log
    // text that never changes. If the user has since installed the cited mods,
    // suppress the now-resolved warning instead of nagging on the old log.
    if diag.pattern_id == "server-missing-mods" {
        let log = crate::logs::read::read_with_cap(&path_buf, 1024 * 1024)?;
        let cited = crate::logs::diagnose::server_mods::parse_server_mod_rejection(&log);
        if !cited.is_empty() {
            let inst_root = instance_root(&app, &instance_id)?;
            let installed = crate::mods::installed::list(&inst_root).await?;
            if crate::mods::cited_resolve::unsatisfied_cited(&cited, &installed).is_empty() {
                return Ok(None);
            }
        }
    }
    // Symmetric guard for the inverse case: once the blocking mods are disabled
    // (or removed), there's nothing left to act on — suppress the stale warning
    // instead of nagging on the historical log.
    if diag.pattern_id == "client-extra-mods" {
        let log = crate::logs::read::read_with_cap(&path_buf, 1024 * 1024)?;
        let ids = crate::logs::diagnose::server_mods::parse_blocking_client_mods(&log);
        // Only suppress when blocking ids were actually parsed: an empty parse
        // means the log doesn't truly match (e.g. truncated below the terminator),
        // so leave the advisory rather than hide it. `build_repair_plan` returns
        // `None` for an empty ids set, so the Fix button won't appear regardless.
        if !ids.is_empty() {
            let inst_root = instance_root(&app, &instance_id)?;
            let installed = crate::mods::installed::list(&inst_root).await?;
            if crate::logs::diagnose::repair::build_blocking_mods(&ids, &installed).is_empty() {
                return Ok(None);
            }
        }
    }
    Ok(Some(diag))
}

/// Build a concrete, confirmable repair plan for a diagnosed log, or
/// `None` when no safe fix can be constructed (the UI then keeps the
/// advisory recommendation text). Lazy: called only when the user
/// clicks "Fix this", so the network swap-lookup for conflicts runs
/// only on intent.
#[tauri::command]
#[specta::specta]
pub async fn build_repair_plan(
    app: tauri::AppHandle,
    instance_id: String,
    path: String,
) -> Result<Option<crate::logs::diagnose::repair::RepairPlan>, crate::error::Error> {
    use crate::logs::diagnose::repair::{
        build_conflict_candidates, extract_conflict_mods, extract_corrupt_jar, suggest_heap_mb,
        RepairKind, RepairPlan,
    };

    let path_buf = std::path::PathBuf::from(&path);
    // Reject paths outside this instance's log roots before reading anything
    // — same guard `diagnose_log`/`read_log_file` apply (the caller supplies
    // `path`, so an arbitrary filesystem path must not be readable here).
    let roots = crate::logs::files::allowed_roots(&app, &instance_id)?;
    crate::logs::files::assert_under_allowed_roots(&path_buf, &roots)?;
    // Re-run the diagnoser as the single source of truth for the pattern.
    let Some(diag) = crate::logs::diagnose::diagnose(&path_buf).await? else {
        return Ok(None);
    };
    let Some(kind) = diag.repair else {
        return Ok(None);
    };

    // The matched log body, capped (same cap the diagnoser uses).
    let log = crate::logs::read::read_with_cap(&path_buf, 1024 * 1024)?;
    let instance = crate::instances::read_instance(&app, &instance_id)?;

    match kind {
        RepairKind::RaiseHeap => {
            let ram = crate::platform::total_system_ram_mb();
            match suggest_heap_mb(instance.max_heap_mb, ram) {
                Some(to) => Ok(Some(RepairPlan::RaiseHeap {
                    from_mb: instance.max_heap_mb,
                    to_mb: to,
                })),
                None => Ok(None),
            }
        }
        RepairKind::ReinstallLoader => {
            if instance.loader == crate::instances::schema::LoaderKind::Vanilla {
                Ok(None)
            } else {
                Ok(Some(RepairPlan::ReinstallLoader {
                    loader: instance.loader,
                }))
            }
        }
        RepairKind::RedownloadMod => {
            let Some(jar) = extract_corrupt_jar(&log) else {
                return Ok(None);
            };
            let inst_root = instance_root(&app, &instance_id)?;
            let installed = crate::mods::installed::list(&inst_root).await?;
            let hit = installed.iter().find(|m| {
                m.filename.eq_ignore_ascii_case(&jar)
                    || m.filename.eq_ignore_ascii_case(&format!("{jar}.disabled"))
            });
            // Only platform mods (source+project+version all present) can be
            // re-downloaded. A hand-dropped jar lacks identity → advisory.
            if let Some(m) = hit {
                if let (Some(source), Some(project_id), Some(version_id)) =
                    (m.source, m.project_id.clone(), m.version_id.clone())
                {
                    return Ok(Some(RepairPlan::RedownloadMod {
                        old_sha1: m.sha1.clone(),
                        filename: m.filename.clone(),
                        target: crate::mods::platform::VersionRef {
                            source,
                            project_id,
                            version_id,
                        },
                    }));
                }
            }
            Ok(None)
        }
        RepairKind::ResolveConflict => {
            let named = extract_conflict_mods(&log);
            if named.is_empty() {
                return Ok(None);
            }
            let inst_root = instance_root(&app, &instance_id)?;
            let installed = crate::mods::installed::list(&inst_root).await?;
            let compat = crate::mods::local::scan_instance(
                &inst_root,
                instance.loader,
                &instance.mc_version,
            )
            .await
            .unwrap_or_default();
            let flagged: Vec<&str> = compat
                .iter()
                .filter(|c| c.loader_mismatch)
                .map(|c| c.sha1.as_str())
                .collect();
            let mut candidates = build_conflict_candidates(&named, &installed, &flagged);
            if candidates.is_empty() {
                return Ok(None);
            }
            enrich_swap_targets(&mut candidates, &installed, &instance).await;
            Ok(Some(RepairPlan::ResolveConflict { candidates }))
        }
        RepairKind::InstallMissingMods => {
            use crate::logs::diagnose::server_mods::parse_server_mod_rejection;
            let cited = parse_server_mod_rejection(&log);
            if cited.is_empty() {
                return Ok(None);
            }
            let inst_root = instance_root(&app, &instance_id)?;
            let installed = crate::mods::installed::list(&inst_root).await?;
            // Drop mods the user has since installed — the reject log is
            // historical and never changes, so without this the card keeps
            // re-offering a mod that's already present. All satisfied → no card.
            let cited = crate::mods::cited_resolve::unsatisfied_cited(&cited, &installed);
            if cited.is_empty() {
                return Ok(None);
            }
            let modrinth = platform_for(crate::mods::platform::ModSource::Modrinth);
            let curseforge = platform_for(crate::mods::platform::ModSource::Curseforge);
            let mods = crate::mods::cited_resolve::resolve(
                &cited,
                &instance.mc_version,
                instance.loader,
                modrinth.as_ref(),
                curseforge.as_ref(),
                &installed,
            )
            .await;
            Ok(Some(RepairPlan::InstallMissingMods { mods }))
        }
        RepairKind::DisableBlockingMods => {
            use crate::logs::diagnose::server_mods::parse_blocking_client_mods;
            let ids = parse_blocking_client_mods(&log);
            if ids.is_empty() {
                return Ok(None);
            }
            let inst_root = instance_root(&app, &instance_id)?;
            let installed = crate::mods::installed::list(&inst_root).await?;
            let mods = crate::logs::diagnose::repair::build_blocking_mods(&ids, &installed);
            if mods.is_empty() {
                return Ok(None);
            }
            Ok(Some(RepairPlan::DisableBlockingMods { mods }))
        }
    }
}

/// Apply a user-confirmed repair choice by dispatching to the existing
/// mutation commands. No resolution happens here — `build_repair_plan`
/// already produced fully-formed parameters. Re-running a now-stale fix
/// surfaces as a normal error toast in the caller.
#[tauri::command]
#[specta::specta]
pub async fn execute_repair(
    app: tauri::AppHandle,
    instance_id: String,
    choice: crate::logs::diagnose::repair::RepairChoice,
) -> Result<(), crate::error::Error> {
    use crate::logs::diagnose::repair::RepairChoice;

    // Reject while a game is running — can't mutate an instance whose files
    // are in use.
    if crate::launch::spawn::is_running() {
        return Err(crate::error::Error::InstanceBusy);
    }
    // Hold the repair guard for the whole rewrite. ReinstallLoader/Reinstall
    // write into the instance's shared library/jar dirs, so a concurrent
    // integrity repair must be excluded, and `launch_instance` reads this
    // flag to block a launch mid-rewrite. Also rejects a second concurrent
    // repair on any instance. Dropped automatically on return.
    let _repair_guard =
        crate::verify::RepairGuard::acquire().ok_or(crate::error::Error::InstanceBusy)?;

    match choice {
        RepairChoice::RaiseHeap { to_mb } => {
            crate::instances::set_instance_memory(&app, &instance_id, to_mb)?;
            Ok(())
        }
        RepairChoice::ReinstallLoader => {
            let effective_id = resolve_instance_effective_id(&app, &instance_id)?;
            crate::versions::install_version(&effective_id, &app).await
        }
        RepairChoice::DisableMod { sha1 } => {
            let inst_root = instance_root(&app, &instance_id)?;
            crate::mods::install::disable(&inst_root, &sha1).await
        }
        RepairChoice::Reinstall { old_sha1, target } => {
            let inst_root = instance_root(&app, &instance_id)?;
            // Uninstall the old jar first, then install the target (with its
            // required deps). install_with_deps re-fetches with SHA verification,
            // so this self-heals a corrupt download.
            //
            // NOT atomic: if the download fails the old jar is already gone. For
            // the corrupt-jar case that's fine (the jar was unusable anyway); for
            // a conflict-swap a network failure leaves the mod uninstalled and the
            // user must reinstall it. The error propagates to a failure toast.
            // Uninstall-first is required because the corrupt-redownload target
            // has the same filename as the broken jar — installing first would
            // collide on disk.
            crate::mods::install::uninstall(&inst_root, &old_sha1).await?;
            mods_install_with_deps(app.clone(), instance_id.clone(), target, vec![]).await?;
            Ok(())
        }
    }
}

/// Anonymise a log body and upload it to mclo.gs. Returns the
/// shareable URL. Frontend caller is the Logs popover "Share"
/// button. Anonymisation runs server-side so the frontend can't
/// accidentally bypass it.
#[tauri::command]
#[specta::specta]
pub async fn share_log_to_mclogs(content: String) -> crate::error::Result<String> {
    let anon = crate::logs::share::anonymise(&content);
    crate::logs::share::upload_to_mclogs(&anon).await
}

/// Open the parent directory of `path` (a log file path the popover is
/// currently viewing) in the OS file manager. The path is validated
/// against every instance's allowed log roots (mirrors `read_log_file`
/// semantics) so a crafted path cannot escape the log directories. This
/// lets a user click "Open folder" on either an MC game log
/// (`.minecraft/logs/`), a crash report (`.minecraft/crash-reports/`),
/// or a launcher capture (`<instance>/logs/`) and land in the exact dir
/// that contains the file they're looking at.
#[tauri::command]
#[specta::specta]
pub async fn open_log_folder(
    app: tauri::AppHandle,
    path: String,
) -> Result<(), crate::error::Error> {
    use tauri_plugin_opener::OpenerExt;
    let path = std::path::PathBuf::from(&path);
    let all = crate::instances::list_instances_with_status(&app)?;
    let mut roots: Vec<std::path::PathBuf> = Vec::new();
    for inst in &all {
        let mut r = crate::logs::files::allowed_roots(&app, &inst.id)?;
        roots.append(&mut r);
    }
    crate::logs::files::assert_under_allowed_roots(&path, &roots)?;
    let dir = path.parent().ok_or_else(|| {
        crate::error::Error::io(path.display().to_string(), "log file has no parent dir")
    })?;
    app.opener()
        .open_path(dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| crate::error::Error::io(dir.display().to_string(), format!("opener: {e}")))?;
    Ok(())
}
