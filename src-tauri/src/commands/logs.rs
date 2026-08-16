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

/// Union of every instance's allowed log roots. Shared by the commands
/// that accept a caller-supplied path and must confine it (read, open
/// folder, delete, annotate).
fn all_instance_log_roots(
    app: &tauri::AppHandle,
) -> Result<Vec<std::path::PathBuf>, crate::error::Error> {
    let all = crate::instances::list_instances_with_status(app)?;
    let mut roots: Vec<std::path::PathBuf> = Vec::new();
    for inst in &all {
        let mut r = crate::logs::files::allowed_roots(app, &inst.id)?;
        roots.append(&mut r);
    }
    Ok(roots)
}

/// Sanitize the UI-supplied f64 byte cap: non-finite / negative → 0
/// (which `read_with_cap` maps to its 5 MB default).
fn sanitize_cap(max_bytes: f64) -> u64 {
    if !max_bytes.is_finite() || max_bytes < 0.0 {
        0
    } else {
        max_bytes as u64
    }
}

/// Read up to `max_bytes` of a log file. `max_bytes` is clamped to
/// `[64 KB, 100 MB]`; `0` becomes the 5 MB default. `path` must be
/// under one of SOME instance's allowed log roots — anything else is
/// rejected with `Error::Io`. Async + `spawn_blocking` (mirrors
/// `annotate_log_file`): up to 100 MB off disk must not run on the IPC
/// thread.
#[tauri::command]
#[specta::specta]
pub async fn read_log_file(
    app: tauri::AppHandle,
    path: String,
    max_bytes: f64,
) -> Result<String, crate::error::Error> {
    let roots = all_instance_log_roots(&app)?;
    let path = std::path::PathBuf::from(&path);
    crate::logs::files::assert_under_allowed_roots(&path, &roots)?;
    let cap = sanitize_cap(max_bytes);
    tokio::task::spawn_blocking(move || crate::logs::read::read_with_cap(&path, cap))
        .await
        .map_err(|e| crate::error::Error::io("<read_log_file>", format!("join: {e}")))?
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

/// Apply the live "is this still a real problem?" guards to a freshly-matched
/// diagnosis. Returns `None` when the problem has since been resolved by the
/// user (cited mods installed / blocking mods disabled). Patterns without a
/// live check pass through unchanged. Reads installed mods only when needed.
async fn apply_live_suppression(
    app: &tauri::AppHandle,
    instance_id: &str,
    path: &std::path::Path,
    diag: crate::logs::diagnose::Diagnosis,
) -> Result<Option<crate::logs::diagnose::Diagnosis>, crate::error::Error> {
    // Stale-log guard: the missing-mods diagnosis is driven by historical log
    // text that never changes. If the user has since installed the cited mods,
    // suppress the now-resolved warning instead of nagging on the old log.
    if diag.pattern_id == "server-missing-mods" {
        let log = crate::logs::read::read_with_cap(path, 1024 * 1024)?;
        let cited = crate::logs::diagnose::server_mods::parse_server_mod_rejection(&log);
        if !cited.is_empty() {
            let inst_root = instance_root(app, instance_id)?;
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
        let log = crate::logs::read::read_with_cap(path, 1024 * 1024)?;
        let ids = crate::logs::diagnose::server_mods::parse_blocking_client_mods(&log);
        // Only suppress when blocking ids were actually parsed: an empty parse
        // means the log doesn't truly match (e.g. truncated below the terminator),
        // so leave the advisory rather than hide it. `build_repair_plan` returns
        // `None` for an empty ids set, so the Fix button won't appear regardless.
        if !ids.is_empty() {
            let inst_root = instance_root(app, instance_id)?;
            let installed = crate::mods::installed::list(&inst_root).await?;
            // The emptiness check only needs the blocking list, not `breaks`, so
            // an empty deps map is fine here (no jar reads at diagnose time).
            if crate::logs::diagnose::repair::build_blocking_mods(
                &ids,
                &installed,
                &std::collections::HashMap::new(),
            )
            .is_empty()
            {
                return Ok(None);
            }
        }
    }
    Ok(Some(diag))
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
    match apply_live_suppression(&app, &instance_id, &path_buf, diag).await? {
        Some(d) => Ok(Some(d)),
        None => Ok(None),
    }
}

/// Cap for `annotate_log_text` input. The live server console holds at
/// most 500 lines; archives read through `server_read_log` are already
/// capped upstream. This is a defensive backstop, not a tuning knob.
const ANNOTATE_TEXT_CAP: usize = 25 * 1024 * 1024;

/// Truncate to at most `cap` bytes, snapping DOWN to a char boundary.
/// Head-keeping (unlike `read_with_cap`, which keeps the tail) — this is
/// a never-expected backstop, not a windowing policy.
fn truncate_at_char_boundary(text: &str, cap: usize) -> &str {
    if text.len() <= cap {
        return text;
    }
    let mut end = cap;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Per-line inline-hint annotations for a log FILE. Reads the file with
/// the SAME `max_bytes` clamp as `read_log_file` so the line indices
/// align with the content the viewer displays (both reads tail on
/// overflow). Path validation mirrors `read_log_file`.
#[tauri::command]
#[specta::specta]
pub async fn annotate_log_file(
    app: tauri::AppHandle,
    path: String,
    max_bytes: f64,
    side: crate::logs::diagnose::annotate::AnnotateSide,
) -> Result<crate::logs::diagnose::annotate::AnnotateResult, crate::error::Error> {
    let roots = all_instance_log_roots(&app)?;
    let path = std::path::PathBuf::from(&path);
    crate::logs::files::assert_under_allowed_roots(&path, &roots)?;
    let cap = sanitize_cap(max_bytes);
    // Read + scan off the IPC thread: up to 100 MB × ~43 patterns is real CPU.
    tokio::task::spawn_blocking(move || {
        let content = crate::logs::read::read_with_cap(&path, cap)?;
        Ok(crate::logs::diagnose::annotate::annotate_lines(
            &content, side,
        ))
    })
    .await
    .map_err(|e| crate::error::Error::io("<annotate_log_file>", format!("join: {e}")))?
}

/// Per-line inline-hint annotations for TEXT the UI already holds (the
/// live server console buffer / an archive blob). Input is truncated at
/// a defensive cap; annotation output is capped inside `annotate_lines`.
#[tauri::command]
#[specta::specta]
pub async fn annotate_log_text(
    text: String,
    side: crate::logs::diagnose::annotate::AnnotateSide,
) -> Result<crate::logs::diagnose::annotate::AnnotateResult, crate::error::Error> {
    // Scan off the IPC thread (mirrors `annotate_log_file` above): up to
    // ANNOTATE_TEXT_CAP (25 MB) of text × ~43 patterns is real CPU, and a sync
    // command runs it on the main thread with the window frozen.
    tokio::task::spawn_blocking(move || {
        let slice = truncate_at_char_boundary(&text, ANNOTATE_TEXT_CAP);
        crate::logs::diagnose::annotate::annotate_lines(slice, side)
    })
    .await
    .map_err(|e| crate::error::Error::io("<annotate_log_text>", format!("join: {e}")))
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
            let cache = jar_scan_cache_path(&app);
            let compat = crate::mods::local::scan_instance(
                &inst_root,
                cache.as_deref(),
                instance.loader,
                &instance.mc_version,
                instance.loader_version.as_deref(),
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
            // Read each enabled mod's jar-declared dependencies so the card can
            // warn when disabling a blocking mod would break a mod that needs it.
            // (The recorded `requires` registry is too sparse to rely on — it
            // only records install-time pulls; jars carry the real declarations,
            // same as FML reads at load.)
            let mods_dir = crate::mods::installed::mods_dir(&inst_root);
            let mut declared_deps: std::collections::HashMap<String, Vec<String>> =
                std::collections::HashMap::new();
            for m in installed.iter().filter(|m| m.enabled) {
                if let Ok(bytes) = tokio::fs::read(mods_dir.join(&m.filename)).await {
                    if let Ok(deps) = crate::mods::local::read_jar_dependency_ids(&bytes) {
                        if !deps.is_empty() {
                            declared_deps.insert(m.sha1.clone(), deps);
                        }
                    }
                }
            }
            let mods = crate::logs::diagnose::repair::build_blocking_mods(
                &ids,
                &installed,
                &declared_deps,
            );
            if mods.is_empty() {
                return Ok(None);
            }
            Ok(Some(RepairPlan::DisableBlockingMods { mods }))
        }
        RepairKind::InstallFixMod => {
            use crate::logs::diagnose::fixmods::{fix_mod_for, pin_for};
            let Some(fix) = fix_mod_for(&diag.pattern_id) else {
                return Ok(None);
            };
            // Try to pin-resolve for this instance's config. Any failure
            // (no pinned row / no CF key / incompatible / distribution off)
            // degrades to install=None — the card still offers the project link.
            let install = match pin_for(fix, &instance.mc_version, instance.loader) {
                Some(pin) => {
                    let plat = platform_for(fix.source);
                    crate::mods::fix_resolve::resolve_pinned(
                        fix.source,
                        fix.project_id,
                        pin.version_id,
                        &instance.mc_version,
                        instance.loader,
                        plat.as_ref(),
                    )
                    .await
                }
                None => None,
            };
            Ok(Some(RepairPlan::InstallFixMod {
                title: fix.human_title.to_string(),
                source: fix.source,
                slug: fix.slug.to_string(),
                version_label: install.as_ref().map(|p| p.version_label.clone()),
                install: install.map(|p| p.target),
            }))
        }
        // Server-side client-only-mod removal is handled by the dedicated
        // server_remove_mods command, not this instance repair pipeline.
        RepairKind::RemoveClientServerMods => Ok(None),
    }
}

/// Newest diagnosable log for an instance: scan the most-recent files and
/// return the first that matches a pattern, with its meta. Bounded so a noisy
/// log dir can't make this O(all files). `None` when nothing matches.
const LATEST_SCAN_LIMIT: usize = 6;

async fn latest_diagnosed_log(
    app: &tauri::AppHandle,
    instance_id: &str,
) -> Result<
    Option<(
        crate::logs::files::LogFileMeta,
        crate::logs::diagnose::Diagnosis,
    )>,
    crate::error::Error,
> {
    let files = crate::logs::files::list_log_files(app, instance_id)?;
    for meta in files.into_iter().take(LATEST_SCAN_LIMIT) {
        let path = std::path::PathBuf::from(&meta.path);
        if let Some(diag) = crate::logs::diagnose::diagnose(&path).await? {
            return Ok(Some((meta, diag)));
        }
    }
    Ok(None)
}

/// Signature of the latest diagnosable log, or `None` when none matches.
/// Used by `execute_repair` to stamp `handled_log_sig`.
async fn latest_diagnosable_signature(
    app: &tauri::AppHandle,
    instance_id: &str,
) -> Result<Option<String>, crate::error::Error> {
    Ok(latest_diagnosed_log(app, instance_id)
        .await?
        .map(|(meta, _)| crate::logs::files::log_signature(&meta)))
}

/// Diagnose the instance's LATEST log and classify a surface status for the
/// persistent banner + indicators. Independent of any opened file. Applies the
/// live-suppression guards, the OOM heap-vs-recommended rule, and the persisted
/// handled-signature.
#[tauri::command]
#[specta::specta]
pub async fn diagnose_latest(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<crate::logs::diagnose::LatestDiagnosis, crate::error::Error> {
    use crate::logs::diagnose::{classify_status, DiagnosisStatus, LatestDiagnosis};

    let none = LatestDiagnosis {
        status: DiagnosisStatus::None,
        diagnosis: None,
        path: None,
        signature: None,
    };

    let Some((meta, diag)) = latest_diagnosed_log(&app, &instance_id).await? else {
        return Ok(none);
    };
    let path_buf = std::path::PathBuf::from(&meta.path);
    // Live guards: a since-resolved problem reports as None (nothing to show).
    let Some(diag) = apply_live_suppression(&app, &instance_id, &path_buf, diag).await? else {
        return Ok(none);
    };

    let signature = crate::logs::files::log_signature(&meta);
    let instance = crate::instances::read_instance(&app, &instance_id)?;
    let ram = crate::platform::total_system_ram_mb();
    let status = classify_status(
        &diag,
        instance.max_heap_mb,
        ram,
        &signature,
        instance.handled_log_sig.as_deref(),
    );

    Ok(LatestDiagnosis {
        status,
        diagnosis: Some(diag),
        path: Some(meta.path),
        signature: Some(signature),
    })
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

    // gates on ANY running instance: repair/verify touch SHARED libraries/versions dirs, not per-instance files
    if crate::launch::spawn::is_any_running() {
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
        }
        RepairChoice::ReinstallLoader => {
            let effective_id = resolve_instance_effective_id(&app, &instance_id)?;
            crate::versions::install_version(&effective_id, &app).await?;
        }
        RepairChoice::DisableMod { sha1 } => {
            let inst_root = instance_root(&app, &instance_id)?;
            // A diagnosis-driven repair changes content just as much as the
            // manual toggle does, so it gets the same history row.
            let identity = mod_identity(&inst_root, &sha1).await;
            crate::mods::install::disable(&inst_root, &sha1).await?;
            if let Some((name, version)) = identity {
                crate::journal::record(
                    &inst_root,
                    crate::journal::content_versioned(
                        crate::journal::ContentAction::ModDisabled,
                        name,
                        version,
                        None,
                    ),
                );
            }
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
            //
            // Journalling: only the removal is recorded here. The install half
            // goes through `mods_install_with_deps`, which writes its own
            // `ModInstalled` row — recording it again would double-count. For a
            // conflict swap the two rows name different mods, which is exactly
            // the history the user needs.
            let removed = mod_identity(&inst_root, &old_sha1).await;
            crate::mods::install::uninstall(&inst_root, &old_sha1).await?;
            if let Some((name, version)) = removed {
                crate::journal::record(
                    &inst_root,
                    crate::journal::content_versioned(
                        crate::journal::ContentAction::ModRemoved,
                        name,
                        version,
                        None,
                    ),
                );
            }
            mods_install_with_deps(app.clone(), instance_id.clone(), target, vec![]).await?;
        }
        RepairChoice::InstallFixMod {
            source,
            project_id,
            version_id,
        } => {
            // Install via the verified path (SHA-1 verify + required-deps
            // closure). The fix mod's own deps (e.g. the base mod it patches)
            // are already present; install_with_deps no-ops on installed ones.
            // No journal call here — `mods_install_with_deps` records the row.
            let target = crate::mods::platform::VersionRef {
                source,
                project_id,
                version_id,
            };
            mods_install_with_deps(app.clone(), instance_id.clone(), target, vec![]).await?;
        }
    }

    // Remember that this latest log has been acted on so the banner/indicator
    // self-suppresses until the next run rotates the log. Best-effort: a failure
    // to record must not fail the (already-applied) repair.
    if let Ok(Some(sig)) = latest_diagnosable_signature(&app, &instance_id).await {
        let _ = crate::instances::set_instance_handled_log_sig(&app, &instance_id, Some(sig));
    }
    Ok(())
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
    let roots = all_instance_log_roots(&app)?;
    crate::logs::files::assert_under_allowed_roots(&path, &roots)?;
    let dir = path.parent().ok_or_else(|| {
        crate::error::Error::io(path.display().to_string(), "log file has no parent dir")
    })?;
    app.opener()
        .open_path(dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| crate::error::Error::io(dir.display().to_string(), format!("opener: {e}")))?;
    Ok(())
}

/// Open the launcher's own log directory (`<app_data>/logs/`).
///
/// Deliberately NOT routed through [`open_log_folder`]: that command validates
/// its path against `all_instance_log_roots`, which is built by iterating the
/// instance list and is therefore EMPTY on an install with no instances — the
/// exact state a user hitting a startup crash is most likely to be in. This
/// takes no path at all, so there is nothing to validate and nothing to escape.
#[tauri::command]
#[specta::specta]
pub async fn open_launcher_log_folder(app: tauri::AppHandle) -> Result<(), crate::error::Error> {
    use tauri_plugin_opener::OpenerExt;
    let dir = crate::paths::app_logs_dir(&app)
        .map_err(|e| crate::error::Error::io("<app_logs>", e.to_string()))?;
    // The directory may not exist yet if nothing has been logged.
    std::fs::create_dir_all(&dir)
        .map_err(|e| crate::error::Error::io(dir.display().to_string(), e.to_string()))?;
    app.opener()
        .open_path(dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| crate::error::Error::io(dir.display().to_string(), format!("opener: {e}")))?;
    Ok(())
}

/// Delete a single log file. `path` must be under some instance's
/// allowed log roots — anything else is rejected with `Error::Io`. Unlike
/// auto-retention, this WILL delete `latest.log` / `debug.log` if the user
/// explicitly picks them (they regenerate on next launch).
#[tauri::command]
#[specta::specta]
pub fn delete_log_file(app: tauri::AppHandle, path: String) -> Result<(), crate::error::Error> {
    let roots = all_instance_log_roots(&app)?;
    crate::logs::retention::delete_one(&std::path::PathBuf::from(&path), &roots)?;
    Ok(())
}

/// Delete every old log for `instance_id` across the log roots except the
/// protected game logs (`latest.log` / `debug.log`) and the launcher's own
/// `lucerna.log`. Returns how many files / bytes were removed.
#[tauri::command]
#[specta::specta]
pub fn clear_old_logs(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<crate::logs::retention::CleanupResult, crate::error::Error> {
    crate::logs::retention::clear_old(&app, &instance_id)
}

/// Apply the global retention policy to `instance_id` now. Called by the
/// Logs window on open. No-op (returns zeros) when the policy is disabled.
#[tauri::command]
#[specta::specta]
pub fn apply_log_retention(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<crate::logs::retention::CleanupResult, crate::error::Error> {
    crate::logs::retention::apply_from_settings(&app, &instance_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_passes_through_ascii_at_exact_cap() {
        let text = "abcdef";
        assert_eq!(truncate_at_char_boundary(text, 6), "abcdef");
        assert_eq!(truncate_at_char_boundary(text, 4), "abcd");
    }

    #[test]
    fn truncate_snaps_down_at_multibyte_boundary() {
        // "héllo wörld": é = 2 bytes (1..3), ö = 2 bytes (8..10). A cap of
        // 9 lands mid-ö and must snap DOWN to 8 without panicking.
        let text = "héllo wörld";
        assert_eq!(truncate_at_char_boundary(text, 9), "héllo w");
        // Cap of 2 lands mid-é → snaps to 1.
        assert_eq!(truncate_at_char_boundary(text, 2), "h");
    }
}
