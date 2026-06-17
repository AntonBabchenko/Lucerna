use super::*;

// =========================================================================
// Mod browser commands (v0.5.0 sub-feature 3)
// =========================================================================

#[tauri::command]
#[specta::specta]
pub async fn mods_search(query: ModSearchQuery) -> crate::error::Result<ModSearchPage> {
    platform_for(query.source).search(&query).await
}

#[tauri::command]
#[specta::specta]
pub async fn mods_project(
    source: ModSource,
    project_id: String,
) -> crate::error::Result<ModProject> {
    crate::mods::project_cache::get_or_fetch(source, &project_id, || async {
        platform_for(source).project(&project_id).await
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn mods_versions(
    source: ModSource,
    project_id: String,
    mc_version: Option<String>,
    loader: Option<LoaderKind>,
) -> crate::error::Result<Vec<ModVersion>> {
    platform_for(source)
        .versions(&project_id, mc_version.as_deref(), loader)
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn mods_resolve_deps(
    version: ModVersion,
    mc_version: String,
    loader: LoaderKind,
) -> crate::error::Result<ResolvedDeps> {
    platform_for(version.source)
        .resolve_deps(&version, &mc_version, loader)
        .await
}

/// Install `primary` plus the TRANSITIVE required closure of the primary and
/// each chosen optional, deduped, installed deps-first, then primary, then
/// chosen optionals. Emits:
///   - `mod-install-progress` repeatedly during downloads,
///   - `mod-installed` once per mod that lands successfully,
///   - `mod-install-failed` if any single install errors (the run halts
///     after the first failure; previously-installed mods are kept).
///
/// Returns an `InstallSummary` so the UI can show which dependencies were
/// pulled in automatically.
///
/// `primary` is a `VersionRef` (not a full `ModVersion`) so the caller
/// doesn't need to keep a heavy struct around — we re-fetch from the
/// platform here. This also re-validates against the live API.
#[tauri::command]
#[specta::specta]
pub async fn mods_install_with_deps(
    app: tauri::AppHandle,
    instance_id: String,
    primary: VersionRef,
    optional_deps: Vec<VersionRef>,
) -> crate::error::Result<crate::mods::platform::InstallSummary> {
    use crate::mods::deps::{resolve_closure, ProjectKey};
    use std::sync::Arc;

    let inst_root = instance_root(&app, &instance_id)?;
    let dd = data_dir(&app)?;
    let (mc_version, loader) = read_active_mc_and_loader(&app, &instance_id)?;

    // Two handles: Box for find_version calls, Arc for make_fetch closure.
    let mut platform_box = platform_for(primary.source);
    let primary_v = find_version(&mut platform_box, &primary, &mc_version, loader).await?;

    // Build the set of already-installed mods so resolve_closure can prune
    // them. Two views: by source-specific ProjectKey, and by lowercased jar
    // filename so a dependency already satisfied from a *different* source —
    // same jar, different platform id — is also pruned.
    //
    // The filename view is deliberately enabled-only: a disabled mod lives on
    // disk as `<name>.jar.disabled`, so it neither loads at runtime (it cannot
    // satisfy a dependency) nor collides with a fresh `<name>.jar` install.
    // Letting the dependency install a fresh enabled copy is the right call.
    // (The ProjectKey view keeps its pre-existing all-mods behaviour for
    // same-source pruning; only the new cross-source path is enabled-gated.)
    let installed_mods = crate::mods::installed::list(&inst_root).await?;
    let installed: std::collections::HashSet<ProjectKey> = installed_mods
        .iter()
        .filter_map(|m| match (m.source, m.project_id.as_deref()) {
            (Some(ModSource::Modrinth), Some(pid)) => Some(ProjectKey::Modrinth(pid.to_string())),
            (Some(ModSource::Curseforge), Some(pid)) => {
                pid.parse().ok().map(ProjectKey::Curseforge)
            }
            _ => None,
        })
        .collect();
    let installed_filenames: std::collections::HashSet<String> = installed_mods
        .iter()
        .filter(|m| m.enabled)
        .map(|m| m.filename.to_ascii_lowercase())
        .collect();

    // Shared Arc platform + loader-slug cache for the make_fetch factory.
    let platform_arc: Arc<dyn crate::mods::platform::ModPlatform> =
        platform_for(primary.source).into();
    let loader_cache = Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::<
        ProjectKey,
        bool,
    >::new()));

    // Factory: produce a fresh fetch closure that shares the Arc'd platform + cache.
    let make_fetch = || {
        let platform = platform_arc.clone();
        let loader_cache = loader_cache.clone();
        let mc = mc_version.clone();
        move |v: ModVersion| {
            let platform = platform.clone();
            let loader_cache = loader_cache.clone();
            let mc = mc.clone();
            async move { fetch_one_level(platform.as_ref(), &loader_cache, &v, &mc, loader).await }
        }
    };

    // Progress callback closes over a clone of the AppHandle and the
    // primary's project_id (used to tag every progress event so the UI
    // can route the bar to the right card). Dep installs reuse the same
    // project_id tag — the UI shows them as part of the same operation.
    let app_for_progress = app.clone();
    let instance_id_for_progress = instance_id.clone();
    let project_id_for_progress = primary_v.project_id.clone();
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

    // Compute the primary's transitive required closure.
    let primary_required = resolve_closure(
        std::slice::from_ref(&primary_v),
        &installed,
        &installed_filenames,
        make_fetch(),
    )
    .await?
    .required;

    // Project IDs of the primary's transitive required closure — persisted
    // onto the primary's registry entry for offline orphan detection.
    let primary_required_ids: Vec<String> = {
        let mut ids: Vec<String> = primary_required
            .iter()
            .map(|v| v.project_id.clone())
            .collect();
        ids.sort();
        ids.dedup();
        ids
    };

    // For each chosen optional: resolve it to a full version, then compute its
    // transitive sub-closure (excluding installed + already-collected deps).
    let mut dep_versions: Vec<ModVersion> = primary_required;
    let mut chosen_optionals: Vec<ModVersion> = Vec::new();
    // Assumption: chosen optionals share the primary's platform (the dialog only offers same-source optionals). A cross-source optional would resolve against the wrong platform.
    for opt in &optional_deps {
        let ov = find_version(&mut platform_box, opt, &mc_version, loader).await?;
        let mut excl = installed.clone();
        for v in &dep_versions {
            excl.insert(ProjectKey::of_version(v));
        }
        for v in &chosen_optionals {
            excl.insert(ProjectKey::of_version(v));
        }
        excl.insert(ProjectKey::of_version(&ov));
        let sub = resolve_closure(
            std::slice::from_ref(&ov),
            &excl,
            &installed_filenames,
            make_fetch(),
        )
        .await?;
        dep_versions.extend(sub.required);
        chosen_optionals.push(ov);
    }
    let dep_versions = dedup_versions(dep_versions.into_iter());

    // Install sequence: required deps first, then primary, then chosen optionals.
    let mut install_seq = dep_versions.clone();
    install_seq.push(primary_v.clone());
    install_seq.extend(chosen_optionals.iter().cloned());

    let mut installed_dependencies: Vec<String> = Vec::new();
    let mut primary_sha1: Option<String> = None;
    for v in install_seq {
        let is_primary = version_matches(&v, &primary);
        let v_project_id = v.project_id.clone();
        match crate::mods::install::install_one(&dd, &inst_root, v.clone(), &prog).await {
            Ok(inst) => {
                if is_primary {
                    primary_sha1 = Some(inst.sha1.clone());
                } else {
                    installed_dependencies.push(inst.name.clone());
                }
                let _ = ModInstalled {
                    instance_id: instance_id.clone(),
                    sha1: inst.sha1,
                    filename: inst.filename,
                    name: inst.name,
                }
                .emit(&app);
            }
            Err(e) => {
                let _ = ModInstallFailed {
                    instance_id: instance_id.clone(),
                    project_id: v_project_id,
                    error: e.clone(),
                }
                .emit(&app);
                return Err(e);
            }
        }
    }
    if let Some(sha1) = primary_sha1 {
        crate::mods::installed::set_requires(&inst_root, &sha1, primary_required_ids).await?;
    }
    Ok(crate::mods::platform::InstallSummary {
        primary_name: primary_v.name.clone(),
        installed_dependencies,
    })
}

// =========================================================================
// Transitive install-plan command (Task 3)
// =========================================================================

/// Resolve the full `InstallPlan` for `primary`:
/// - `required`: primary's transitive required closure (all must be installed)
/// - `optional`: each direct optional dep + its own transitive required sub-closure
/// - `incompatible` / `unresolvable`: refs from the primary's one-level scan
/// - `loader_requirements`: loader project refs (informational, not installed)
///
/// Already-installed mods are pruned from all lists.
#[tauri::command]
#[specta::specta]
pub async fn mods_resolve_install_plan(
    app: tauri::AppHandle,
    instance_id: String,
    primary: ModVersion,
    mc_version: String,
    loader: LoaderKind,
) -> crate::error::Result<InstallPlan> {
    use crate::mods::deps::{resolve_closure, ProjectKey};
    use std::sync::Arc;

    let root = instance_root(&app, &instance_id)?;
    // Mirror mods_install_with_deps: prune by source-specific ProjectKey AND by
    // lowercased jar filename, so a dependency already satisfied from a
    // different source is not offered for (re)install. The filename view is
    // enabled-only for the same reason as there — a disabled `.jar.disabled`
    // neither loads nor collides, so it should not suppress a fresh install.
    let installed_mods = crate::mods::installed::list(&root).await?;
    let installed: std::collections::HashSet<ProjectKey> = installed_mods
        .iter()
        .filter_map(|m| match (m.source, m.project_id.as_deref()) {
            (Some(ModSource::Modrinth), Some(pid)) => Some(ProjectKey::Modrinth(pid.to_string())),
            (Some(ModSource::Curseforge), Some(pid)) => {
                pid.parse().ok().map(ProjectKey::Curseforge)
            }
            _ => None,
        })
        .collect();
    let installed_filenames: std::collections::HashSet<String> = installed_mods
        .iter()
        .filter(|m| m.enabled)
        .map(|m| m.filename.to_ascii_lowercase())
        .collect();

    // Shared platform + loader-slug cache, cloned into each closure via Arc.
    let platform: Arc<dyn crate::mods::platform::ModPlatform> = platform_for(primary.source).into();
    let loader_cache = Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::<
        ProjectKey,
        bool,
    >::new()));

    // Factory: produce a fresh fetch closure that shares the Arc'd platform + cache.
    let make_fetch = || {
        let platform = platform.clone();
        let loader_cache = loader_cache.clone();
        let mc = mc_version.clone();
        move |v: ModVersion| {
            let platform = platform.clone();
            let loader_cache = loader_cache.clone();
            let mc = mc.clone();
            async move { fetch_one_level(platform.as_ref(), &loader_cache, &v, &mc, loader).await }
        }
    };

    // 1. One-level deps of the primary (required/optional/incompat/unresolvable).
    let top = fetch_one_level(
        platform.as_ref(),
        &loader_cache,
        &primary,
        &mc_version,
        loader,
    )
    .await?;

    // 2. Primary's transitive required closure; prune already-installed.
    //    The closure walker enqueues the root's required deps, so
    //    `primary_closure.required` already contains everything that a
    //    separate `direct_required` collect would have added — a second
    //    independent fetch would also open a theoretical version-skew
    //    window between two network calls to the same endpoint.
    let primary_closure = resolve_closure(
        std::slice::from_ref(&primary),
        &installed,
        &installed_filenames,
        make_fetch(),
    )
    .await?;
    // The transitive closure already includes the primary's direct requireds,
    // is deduplicated, and has installed mods pruned by `resolve_closure`.
    let required = primary_closure.required;

    // 3. Each direct optional + its transitive required sub-closure,
    //    excluding primary's requireds + installed.
    let mut exclude = installed.clone();
    for v in &required {
        exclude.insert(ProjectKey::of_version(v));
    }
    let mut optional = Vec::new();
    // Skip loaders AND optionals already installed in this instance — offering
    // to "install" a mod the user already has is confusing. (The required list
    // is already installed-pruned by resolve_closure; this does the same for
    // the top-level optionals.)
    for opt in top.optional.iter().filter(|n| {
        !n.is_loader
            && !installed.contains(&ProjectKey::of_version(&n.version))
            && !installed_filenames.contains(&n.version.primary_file.filename.to_ascii_lowercase())
    }) {
        let sub = resolve_closure(
            std::slice::from_ref(&opt.version),
            &exclude,
            &installed_filenames,
            make_fetch(),
        )
        .await?;
        optional.push(OptionalDep {
            version: opt.version.clone(),
            requires: dedup_versions(sub.required.into_iter()),
        });
    }

    // 4. Loader refs seen at the primary's top level.
    let loader_requirements = top
        .required
        .iter()
        .chain(top.optional.iter())
        .filter(|n| n.is_loader)
        .map(|n| version_to_ref(&n.version))
        .collect();

    Ok(InstallPlan {
        required,
        optional,
        incompatible: top.incompatible,
        unresolvable: top.unresolvable,
        loader_requirements,
    })
}

/// Reconciled view of `{instance}/.minecraft/mods/`: any jar present is
/// listed (with synthesized metadata if it wasn't installed via the
/// launcher), and stale registry entries with no file on disk are dropped.
#[tauri::command]
#[specta::specta]
pub async fn mods_list_installed(
    app: tauri::AppHandle,
    instance_id: String,
) -> crate::error::Result<Vec<InstalledMod>> {
    let inst_root = instance_root(&app, &instance_id)?;
    crate::mods::installed::list(&inst_root).await
}

/// Rename `<name>.jar` to `<name>.jar.disabled` and flip the registry
/// flag so the next launch skips this mod. Emits `mod-toggle` with
/// `enabled: false`.
#[tauri::command]
#[specta::specta]
pub async fn mods_disable(
    app: tauri::AppHandle,
    instance_id: String,
    sha1: String,
) -> crate::error::Result<()> {
    let inst_root = instance_root(&app, &instance_id)?;
    crate::mods::install::disable(&inst_root, &sha1).await?;
    let _ = ModToggle {
        instance_id,
        sha1,
        enabled: false,
    }
    .emit(&app);
    Ok(())
}

/// Inverse of `mods_disable`. Emits `mod-toggle` with `enabled: true`.
#[tauri::command]
#[specta::specta]
pub async fn mods_enable(
    app: tauri::AppHandle,
    instance_id: String,
    sha1: String,
) -> crate::error::Result<()> {
    let inst_root = instance_root(&app, &instance_id)?;
    crate::mods::install::enable(&inst_root, &sha1).await?;
    let _ = ModToggle {
        instance_id,
        sha1,
        enabled: true,
    }
    .emit(&app);
    Ok(())
}

/// Remove the jar (enabled or disabled flavor) and drop the registry
/// entry. The shared cache copy survives. Emits `mod-uninstalled`.
#[tauri::command]
#[specta::specta]
pub async fn mods_uninstall(
    app: tauri::AppHandle,
    instance_id: String,
    sha1: String,
) -> crate::error::Result<()> {
    let inst_root = instance_root(&app, &instance_id)?;
    crate::mods::install::uninstall(&inst_root, &sha1).await?;
    let _ = ModUninstalled { instance_id, sha1 }.emit(&app);
    Ok(())
}

/// Check every eligible installed user-mod for a newer version. For
/// each mod with platform identity that is not a modpack-origin mod,
/// query its source platform for the versions available on the
/// instance's MC + loader and classify the result. A single mod's
/// query failure becomes that mod's `CheckFailed` state — the command
/// fails wholesale only on a catastrophic error (instance missing,
/// registry unreadable). Modpack-origin and hand-dropped mods are
/// absent from the result.
#[tauri::command]
#[specta::specta]
pub async fn mods_check_updates(
    app: tauri::AppHandle,
    instance_id: String,
) -> crate::error::Result<Vec<crate::mods::updates::ModUpdateCheck>> {
    use crate::mods::updates::{
        classify_update, eligible_identity, ModUpdateCheck, ModUpdateState,
    };
    use futures_util::stream::{self, StreamExt};

    let inst_root = instance_root(&app, &instance_id)?;
    let (mc_version, loader) = read_active_mc_and_loader(&app, &instance_id)?;
    let installed = crate::mods::installed::list(&inst_root).await?;
    let pack_origin = crate::mods::installed::get_pack_origin(&inst_root).await?;

    // Bound platform polling so a large instance doesn't fan out dozens of
    // simultaneous requests (which intermittently trips per-IP rate limits).
    const CHECK_UPDATES_CONCURRENCY: usize = 6;

    // Eligible mods paired with their original index, so output order
    // matches the installed list after the unordered concurrent poll.
    // Each task OWNS its `InstalledMod` (a small clone): borrowing `m`
    // across the `.await` made the `.map` closure fail the higher-ranked
    // lifetime bound `buffer_unordered` requires (`FnOnce` not general
    // enough). Owning the few fields each task needs sidesteps that.
    // Collecting eagerly here (rather than feeding the borrowing
    // `filter_map` straight to `stream::iter`) is what decouples those
    // borrows from the async closure's lifetimes — passing the lazy
    // iterator directly re-triggers the same HRTB failure.
    let eligible: Vec<(
        usize,
        crate::mods::platform::InstalledMod,
        ModSource,
        String,
        String,
    )> = installed
        .iter()
        .enumerate()
        .filter_map(|(i, m)| {
            eligible_identity(m, pack_origin.as_ref()).map(|(source, project_id, version_id)| {
                (i, m.clone(), source, project_id, version_id)
            })
        })
        .collect();

    // Bounded-concurrency platform poll. Identical per-mod semantics to the
    // prior sequential loop: one `ModUpdateCheck` per eligible mod, same
    // `classify_update`, same `CheckFailed`-on-error.
    let mut results: Vec<(usize, ModUpdateCheck)> = stream::iter(eligible)
        .map(|(i, m, source, project_id, version_id)| {
            let mc = mc_version.clone();
            async move {
                let state = match platform_for(source)
                    .versions(&project_id, Some(&mc), Some(loader))
                    .await
                {
                    Ok(versions) => classify_update(&m, &versions),
                    Err(e) => ModUpdateState::CheckFailed {
                        reason: e.to_string(),
                    },
                };
                (
                    i,
                    ModUpdateCheck {
                        sha1: m.sha1.clone(),
                        name: m.name.clone(),
                        source,
                        project_id,
                        current_version_id: version_id,
                        current_version_number: m.version_number.clone(),
                        state,
                    },
                )
            }
        })
        .buffer_unordered(CHECK_UPDATES_CONCURRENCY)
        .collect()
        .await;

    // Restore installed-list order: `buffer_unordered` yields completions
    // out of order, so re-sort by the paired original index.
    results.sort_by_key(|(i, _)| *i);
    let out: Vec<ModUpdateCheck> = results.into_iter().map(|(_, c)| c).collect();
    Ok(out)
}

/// The instance's modpack origin reduced to chip data: the pack name
/// and the SHA-1s of its bundled `mods/` files. `None` for an instance
/// that was not created from a modpack import.
#[tauri::command]
#[specta::specta]
pub async fn mods_pack_origin_summary(
    app: tauri::AppHandle,
    instance_id: String,
) -> crate::error::Result<Option<crate::mods::updates::PackOriginSummary>> {
    let inst_root = instance_root(&app, &instance_id)?;
    let pack_origin = crate::mods::installed::get_pack_origin(&inst_root).await?;
    Ok(pack_origin
        .as_ref()
        .map(crate::mods::updates::pack_origin_summary))
}

/// Identify the instance's modpack override-bundled mods by file hash
/// and backfill their platform identity into the registry. Returns the
/// number of mods newly resolved. Best-effort and idempotent: a no-op
/// (returns 0) for instances that were not modpack-imported, or whose
/// pack mods are all already identified or already attempted.
#[tauri::command]
#[specta::specta]
pub async fn mods_enrich_pack_mods(
    app: tauri::AppHandle,
    instance_id: String,
) -> crate::error::Result<u32> {
    let inst_root = instance_root(&app, &instance_id)?;
    let cf_key = crate::mods::curseforge::keyring::resolve();
    crate::mods::enrich::enrich_instance(
        &inst_root,
        "https://api.modrinth.com",
        "https://api.curseforge.com",
        cf_key.as_deref(),
    )
    .await
}

/// Apply one mod update: resolve `target`'s required dependencies,
/// pre-warm the cache, swap the old jar (`old_sha1`) for `target` plus
/// its required deps, and preserve the old mod's enabled state. Emits
/// `mod-install-progress` during downloads, `mod-uninstalled` for the
/// old jar, `mod-installed` per landed mod, and `mod-install-failed`
/// on error. Optional dependencies are intentionally not installed —
/// see the spec ("Dependencies on update").
#[tauri::command]
#[specta::specta]
pub async fn mods_update_one(
    app: tauri::AppHandle,
    instance_id: String,
    old_sha1: String,
    target: ModVersion,
) -> crate::error::Result<()> {
    let inst_root = instance_root(&app, &instance_id)?;
    let dd = data_dir(&app)?;
    let (mc_version, loader) = read_active_mc_and_loader(&app, &instance_id)?;

    // Required dependencies of the target version (optional deps skipped).
    let platform = platform_for(target.source);
    let resolved = platform.resolve_deps(&target, &mc_version, loader).await?;
    let required_deps: Vec<ModVersion> = resolved.required.into_iter().map(|r| r.version).collect();

    // Progress events tagged with the target's project_id so the UI can
    // route the bar to the right card (same pattern as install).
    let app_for_progress = app.clone();
    let instance_id_for_progress = instance_id.clone();
    let project_id_for_progress = target.project_id.clone();
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

    let target_project_id = target.project_id.clone();
    match crate::mods::install::update_one(&dd, &inst_root, &old_sha1, target, required_deps, &prog)
        .await
    {
        Ok(outcome) => {
            let _ = ModUninstalled {
                instance_id: instance_id.clone(),
                sha1: outcome.removed_sha1,
            }
            .emit(&app);
            for inst in std::iter::once(outcome.primary).chain(outcome.deps) {
                let _ = ModInstalled {
                    instance_id: instance_id.clone(),
                    sha1: inst.sha1,
                    filename: inst.filename,
                    name: inst.name,
                }
                .emit(&app);
            }
            Ok(())
        }
        Err(e) => {
            let _ = ModInstallFailed {
                instance_id: instance_id.clone(),
                project_id: target_project_id,
                error: e.clone(),
            }
            .emit(&app);
            Err(e)
        }
    }
}

/// Inspect a local mod `.jar`: read its descriptor and judge loader/MC
/// compatibility against the target instance. No filesystem writes.
#[tauri::command]
#[specta::specta]
pub async fn mods_inspect_local(
    app: tauri::AppHandle,
    instance_id: String,
    jar_path: String,
) -> crate::error::Result<crate::mods::local::CompatVerdict> {
    let inst = crate::instances::read_instance(&app, &instance_id)?;
    let bytes =
        tokio::fs::read(&jar_path)
            .await
            .map_err(|e| crate::error::Error::ModsInstancePath {
                path: jar_path.clone(),
                details: format!("{} ({})", e, e.kind()),
            })?;
    let meta = crate::mods::local::read_jar_meta(&bytes)?;
    Ok(crate::mods::local::compat_verdict(
        &meta,
        inst.loader,
        &inst.mc_version,
    ))
}

/// Install a local mod `.jar` into the instance as a manual mod. Emits
/// `mod-installed` on success so the Installed view refreshes the same
/// way it does after a platform install.
#[tauri::command]
#[specta::specta]
pub async fn mods_install_local(
    app: tauri::AppHandle,
    instance_id: String,
    jar_path: String,
) -> crate::error::Result<crate::mods::platform::InstalledMod> {
    let inst_root = instance_root(&app, &instance_id)?;
    let bytes =
        tokio::fs::read(&jar_path)
            .await
            .map_err(|e| crate::error::Error::ModsInstancePath {
                path: jar_path.clone(),
                details: format!("{} ({})", e, e.kind()),
            })?;
    let filename = std::path::Path::new(&jar_path)
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| crate::error::Error::ModsInstancePath {
            path: jar_path.clone(),
            details: "dropped path has no filename".into(),
        })?
        .to_string();
    let meta = crate::mods::local::read_jar_meta(&bytes)?;
    let inst = crate::mods::local::install_local(
        &inst_root,
        &filename,
        &bytes,
        meta.display_name.as_deref(),
    )
    .await?;
    let _ = ModInstalled {
        instance_id,
        sha1: inst.sha1.clone(),
        filename: inst.filename.clone(),
        name: inst.name.clone(),
    }
    .emit(&app);
    Ok(inst)
}

/// For each installed mod in `id`, report whether any platform version
/// exists for the given target `mc` + `loader`. Non-destructive — no
/// files are modified.
///
/// Mods with no platform identity (hand-dropped jars) and pack-origin
/// mods report [`ModCompatStatus::Unknown`]. A single mod's query
/// failure becomes [`ModCompatStatus::Unknown`] for that mod — the
/// command fails wholesale only on a catastrophic error (instance
/// missing, registry unreadable).
#[tauri::command]
#[specta::specta]
pub async fn check_instance_mod_compat(
    app: tauri::AppHandle,
    id: String,
    mc: String,
    loader: crate::instances::schema::LoaderKind,
) -> crate::error::Result<Vec<crate::mods::compat::ModCompat>> {
    use crate::mods::updates::eligible_identity;

    let inst_root = instance_root(&app, &id)?;
    let installed = crate::mods::installed::list(&inst_root).await?;
    let pack_origin = crate::mods::installed::get_pack_origin(&inst_root).await?;

    let mut out = Vec::new();
    for m in &installed {
        let status = match eligible_identity(m, pack_origin.as_ref()) {
            None => crate::mods::compat::ModCompatStatus::Unknown,
            Some((source, project_id, _vid)) => crate::mods::compat::classify_compat(
                platform_for(source)
                    .versions(&project_id, Some(&mc), Some(loader))
                    .await,
            ),
        };
        out.push(crate::mods::compat::ModCompat {
            sha1: m.sha1.clone(),
            name: m.name.clone(),
            status,
        });
    }
    Ok(out)
}

/// Offline loader-compatibility scan of an instance's installed mods
/// (Layer 1). Network-free; reads each jar's descriptor. Returns one
/// `ModLocalCompat` per registered mod.
#[tauri::command]
#[specta::specta]
pub async fn scan_instance_mod_compat(
    app: tauri::AppHandle,
    id: String,
    mc: String,
    loader: crate::instances::schema::LoaderKind,
) -> crate::error::Result<Vec<crate::mods::compat::ModLocalCompat>> {
    let inst_root = instance_root(&app, &id)?;
    crate::mods::local::scan_instance(&inst_root, loader, &mc).await
}

#[tauri::command]
#[specta::specta]
pub async fn mods_find_orphans(
    app: tauri::AppHandle,
    instance_id: String,
    removing: Vec<String>,
) -> crate::error::Result<Vec<crate::mods::platform::OrphanRef>> {
    let root = instance_root(&app, &instance_id)?;
    let mods = crate::mods::installed::list(&root).await?;
    Ok(crate::mods::orphans::find_orphans(&mods, &removing))
}

/// Build a full nested dependency graph for all platform-identified mods in
/// `instance_id`. Each installed mod is a root; its required and optional
/// subtrees are walked recursively (cycle-guarded, memoized). Each node is
/// classified as `satisfied / missing_required / optional_present /
/// optional_absent` against the installed set.
///
/// The graph is informational — no files are written. Intended to power the
/// "Dependency Tree" view in the Mods tab.
///
/// `depgraph::build_graph` produces a `Send` future (boxed recursive walk with
/// `+ Send` on the alias), so it can be awaited directly on the Tauri executor.
#[tauri::command]
#[specta::specta]
pub async fn mods_dependency_graph(
    app: tauri::AppHandle,
    instance_id: String,
) -> crate::error::Result<crate::mods::depgraph::DependencyGraph> {
    use crate::mods::depgraph::{build_graph, DepChild, InstalledNode, NodeDeps};
    use std::sync::Arc;

    let root = instance_root(&app, &instance_id)?;
    let (mc_version, loader) = read_active_mc_and_loader(&app, &instance_id)?;
    let installed_mods = crate::mods::installed::list(&root).await?;

    // Roots: platform-identified installed mods only (anonymous local jars have
    // no source metadata and cannot be queried for deps).
    let roots: Vec<InstalledNode> = installed_mods
        .iter()
        .filter_map(|m| match (m.source, m.project_id.as_ref()) {
            (Some(source), Some(pid)) => Some(InstalledNode {
                sha1: m.sha1.clone(),
                source,
                project_id: pid.clone(),
                name: m.name.clone(),
            }),
            _ => None,
        })
        .collect();

    // Per-source platform handles + shared loader-slug cache cloned into each call.
    let mr: Arc<dyn crate::mods::platform::ModPlatform> = platform_for(ModSource::Modrinth).into();
    let cf: Arc<dyn crate::mods::platform::ModPlatform> =
        platform_for(ModSource::Curseforge).into();
    // FTB has no per-mod browser; build the stub once and clone per call (mirrors mr/cf above).
    let ftb: Arc<dyn crate::mods::platform::ModPlatform> =
        Arc::new(crate::mods::unsupported::UnsupportedModPlatform {
            source: ModSource::Ftb,
        });
    // ATLauncher has no per-mod browser; separate stub so error labels name the right source.
    let atl: Arc<dyn crate::mods::platform::ModPlatform> =
        Arc::new(crate::mods::unsupported::UnsupportedModPlatform {
            source: ModSource::Atlauncher,
        });
    let loader_cache = Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::<
        ProjectKey,
        bool,
    >::new()));
    let mc = mc_version.clone();

    // Build the fetch closure. Each invocation clones the lightweight Arcs and
    // drives one project's deps: platform.versions() → latest version →
    // platform.resolve_deps() → filter loaders → enrich display names.
    let make_fetch = move || {
        let mr = mr.clone();
        let cf = cf.clone();
        let ftb = ftb.clone();
        let atl = atl.clone();
        let loader_cache = loader_cache.clone();
        let mc = mc.clone();
        move |source: ModSource, project_id: String| {
            let platform: Arc<dyn crate::mods::platform::ModPlatform> = match source {
                ModSource::Modrinth => mr.clone(),
                ModSource::Curseforge => cf.clone(),
                // FTB: pack-managed, not individually dep-resolvable — treat as leaf.
                ModSource::Ftb => ftb.clone(),
                // ATLauncher: pack-managed, not individually dep-resolvable — treat as leaf.
                ModSource::Atlauncher => atl.clone(),
            };
            let loader_cache = loader_cache.clone();
            let mc = mc.clone();
            async move {
                let mut versions = platform
                    .versions(&project_id, Some(&mc), Some(loader))
                    .await
                    .unwrap_or_default();
                // Pick the newest compatible version explicitly: Modrinth
                // returns newest-first but CurseForge's order is undocumented,
                // so sort by `published_at` (RFC 3339, lexicographically
                // sortable) descending rather than trusting API order. None
                // sorts last.
                versions.sort_by(|a, b| b.published_at.cmp(&a.published_at));
                let Some(v) = versions.into_iter().next() else {
                    // Couldn't resolve (e.g. CF without a key) — treat as leaf.
                    return Ok(NodeDeps::default());
                };
                let rd = match platform.resolve_deps(&v, &mc, loader).await {
                    Ok(rd) => rd,
                    Err(_) => return Ok(NodeDeps::default()),
                };
                // Filter out loader projects; enrich child display names via project summary.
                let mut required = Vec::new();
                for r in rd.required {
                    if is_loader_project(platform.as_ref(), &loader_cache, &r.version).await {
                        continue;
                    }
                    let name = platform
                        .project(&r.version.project_id)
                        .await
                        .map(|p| p.summary.name)
                        .unwrap_or_else(|_| r.version.name.clone());
                    required.push(DepChild {
                        source: r.version.source,
                        project_id: r.version.project_id,
                        name,
                    });
                }
                let mut optional = Vec::new();
                for o in rd.optional {
                    if is_loader_project(platform.as_ref(), &loader_cache, &o.version).await {
                        continue;
                    }
                    let name = platform
                        .project(&o.version.project_id)
                        .await
                        .map(|p| p.summary.name)
                        .unwrap_or_else(|_| o.version.name.clone());
                    optional.push(DepChild {
                        source: o.version.source,
                        project_id: o.version.project_id,
                        name,
                    });
                }
                Ok::<NodeDeps, crate::error::Error>(NodeDeps { required, optional })
            }
        }
    };

    build_graph(&roots, make_fetch()).await
}

// =========================================================================
// Dependency version pre-flight (Task 6)
// =========================================================================
//
// IPC types (ViolationKind, DepViolation, PreflightReport) and the testable
// core (dependency_preflight_for_root) all live in `crate::mods::preflight`
// so integration tests can reach them via the public `mods` module without
// needing the `commands` module to be public.

/// Offline dependency version pre-flight for an instance. Reads every enabled
/// mod jar's descriptor, builds a provider index, and checks that every
/// mandatory dependency is present and within the declared version range.
/// Network-free; pure local jar inspection. An empty `violations` list means
/// no detected problems.
#[tauri::command]
#[specta::specta]
pub async fn instance_dependency_preflight(
    app: tauri::AppHandle,
    instance_id: String,
) -> crate::error::Result<crate::mods::preflight::PreflightReport> {
    let root = instance_root(&app, &instance_id)?;
    crate::mods::preflight::dependency_preflight_for_root(&root).await
}
