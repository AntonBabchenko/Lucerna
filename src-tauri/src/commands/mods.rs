use super::*;

// =========================================================================
// TTL-cached versions helper (used by background sweeps only)
// =========================================================================

/// `platform.versions(...)` with a TTL cache in front. Used by the background
/// sweeps (compat check, update check, dependency graph) so repeat scans don't
/// re-hit the network. NOT used by the per-install resolution (installs want a
/// fresh canonical version).
async fn cached_versions(
    platform: &dyn crate::mods::platform::ModPlatform,
    source: crate::mods::platform::ModSource,
    project_id: &str,
    mc: &str,
    loader: LoaderKind,
) -> crate::error::Result<Vec<crate::mods::platform::ModVersion>> {
    if let Some(hit) = crate::mods::version_cache::get(source, project_id, mc, loader) {
        return Ok(hit);
    }
    let v = platform
        .versions(project_id, Some(mc), Some(loader))
        .await?;
    crate::mods::version_cache::put(source, project_id, mc, loader, v.clone());
    Ok(v)
}

// =========================================================================
// Mod browser commands (v0.5.0 sub-feature 3)
// =========================================================================

#[tauri::command]
#[specta::specta]
pub async fn mods_search(query: ModSearchQuery) -> crate::error::Result<ModSearchPage> {
    crate::network::throttle::with_interactive(async move {
        platform_for(query.source).search(&query).await
    })
    .await
}

/// Pure (no network): given version-number strings and a required range +
/// family, return the indices that satisfy it (input order preserved, so the
/// first index is the newest satisfying version). The frontend already fetched
/// the versions via `mods_versions`; this avoids a second round-trip and powers
/// both smart-Update and the picker's satisfies badges.
#[tauri::command]
#[specta::specta]
pub fn mods_filter_satisfying(
    versions: Vec<String>,
    needed: String,
    family: crate::mods::version_range::RangeFamily,
) -> Vec<u32> {
    let refs: Vec<&str> = versions.iter().map(String::as_str).collect();
    crate::mods::version_range::satisfying_indices(&refs, &needed, family)
        .into_iter()
        // safe: a Vec<String> of version strings cannot approach 2^32 entries.
        .map(|i| i as u32)
        .collect()
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

/// The configured mod-metadata cache TTL (days). Read from `app.json`; defaults
/// to 7 when unset. `0` = never expire.
fn mod_metadata_ttl_days(app: &tauri::AppHandle) -> crate::error::Result<u32> {
    let path = crate::paths::app_file(app).map_err(|e| crate::error::Error::io("<app_file>", e))?;
    Ok(crate::instances::store::read_app_json(&path)?
        .general
        .mod_metadata_ttl_days)
}

/// Batch-fetch project summaries (name / slug / icon) for the installed list.
/// Serves fresh entries from the shared disk cache and batch-fetches the
/// missing/stale set in one request via `ModPlatform::summaries`, collapsing
/// the old per-mod `mods_project` fan-out (a 429 source on large instances)
/// into a handful of requests. Unknown ids are simply omitted — the caller
/// degrades that row.
#[tauri::command]
#[specta::specta]
pub async fn mods_projects(
    app: tauri::AppHandle,
    source: ModSource,
    project_ids: Vec<String>,
) -> crate::error::Result<Vec<ModSummary>> {
    let ttl = mod_metadata_ttl_days(&app)?;
    let path = crate::paths::mods_cache_file(&app)
        .map_err(|e| crate::error::Error::io("<mods_cache_file>", e))?;
    let platform = platform_for(source);
    let out = crate::mods::summary_cache::get_many(
        &path,
        source,
        &project_ids,
        ttl,
        move |ids: Vec<String>| async move {
            let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
            platform.summaries(&refs).await
        },
    )
    .await;
    Ok(out)
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
    crate::network::throttle::with_interactive(async move {
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

    // Compute the primary's transitive required closure. The executor only needs
    // the versions to download — collapse PlannedDep to ModVersion here (the
    // install-plan path keeps the reason; this one does not surface it).
    let primary_required: Vec<ModVersion> = resolve_closure(
        std::slice::from_ref(&primary_v),
        &installed,
        &installed_filenames,
        make_fetch(),
    )
    .await?
    .required
    .into_iter()
    .map(|p| p.version)
    .collect();

    // Best-effort: read the primary jar's manifest and fold in required
    // libraries the platform metadata omitted (e.g. Waystones requires Balm,
    // but CF metadata doesn't list it). Unlike the dialog, each extra candidate
    // is provides-verified over its DOWNLOADED jar before being committed:
    // verify-fail → skip + log, so we never install a wrong mod. Reading the
    // primary jar is best-effort — any failure yields no extras and preserves
    // the prior behaviour. Computed BEFORE `primary_required_ids` and
    // `install_seq` because both fold these extras in.
    let extras_raw = manifest_extra_root_versions(&dd, &primary_v, &mc_version, loader).await;
    let extras = dedup_extra_candidates(
        extras_raw,
        &installed,
        &installed_filenames,
        &primary_required,
    );
    let mut extra_install: Vec<ModVersion> = Vec::new();
    {
        use crate::mods::dep_resolve::jar_provides;
        let mut excl: std::collections::HashSet<ProjectKey> = installed.clone();
        for v in &primary_required {
            excl.insert(ProjectKey::of_version(v));
        }
        // Executor path only needs the candidate to download — the
        // `SelectionReason` is surfaced on the install-plan path, not here.
        for (needed_id, cand, _reason) in extras {
            if excl.contains(&ProjectKey::of_version(&cand)) {
                continue;
            }
            let sha = match cand.primary_file.sha1.as_deref() {
                Some(s) if !s.trim().is_empty() => s.to_ascii_lowercase(),
                _ => continue,
            };
            let Ok(cached) = crate::mods::install::fetch_to_cache(
                &dd,
                &cand.primary_file.url,
                &sha,
                cand.primary_file.size,
                "mods",
                &prog,
            )
            .await
            else {
                continue;
            };
            let Ok(bytes) = tokio::fs::read(&cached).await else {
                continue;
            };
            if !jar_provides(&bytes, &needed_id) {
                crate::diag!("dep_resolve: skipping '{needed_id}' — candidate did not provide it");
                continue;
            }
            excl.insert(ProjectKey::of_version(&cand));
            let sub = resolve_closure(
                std::slice::from_ref(&cand),
                &excl,
                &installed_filenames,
                make_fetch(),
            )
            .await?;
            for p in &sub.required {
                excl.insert(ProjectKey::of_version(&p.version));
            }
            extra_install.extend(sub.required.into_iter().map(|p| p.version));
            extra_install.push(cand);
        }
    }

    // Project IDs of the primary's transitive required closure plus any
    // manifest-discovered extras — persisted onto the primary's registry entry
    // for offline orphan detection.
    let primary_required_ids: Vec<String> = {
        let mut ids: Vec<String> = primary_required
            .iter()
            .chain(extra_install.iter())
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
        for v in &extra_install {
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
        dep_versions.extend(sub.required.into_iter().map(|p| p.version));
        chosen_optionals.push(ov);
    }
    let dep_versions = dedup_versions(dep_versions.into_iter());

    // Install sequence: required deps + manifest-discovered extras first (both
    // before the primary, so the libs are present when the primary loads), then
    // primary, then chosen optionals.
    let mut install_seq = dep_versions.clone();
    install_seq.extend(extra_install.iter().cloned());
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
    })
    .await
}

// =========================================================================
// Server browse-and-install kernel (S2 #3)
// =========================================================================

/// Lowercased filenames of the *enabled* `.jar`s already in `dir` (a server's
/// `mods/`). Used to prune dependencies that are already satisfied. A
/// `.jar.disabled` neither loads nor collides with a fresh `.jar`, so it is
/// deliberately excluded — matching the instance installer's enabled-only view.
/// Returns an empty set when `dir` is missing or unreadable (e.g. a fresh server
/// with no `mods/` yet) — the install then re-resolves the full closure, which
/// is wrong-but-safe (over-installs rather than under-installs).
fn enabled_jar_filenames(dir: &std::path::Path) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let low = e.file_name().to_string_lossy().to_ascii_lowercase();
            if low.ends_with(".jar") {
                out.insert(low);
            }
        }
    }
    out
}

/// Download `v` through `network::` and copy its jar into `dest` (created if
/// absent), returning the written filename. The platform-supplied filename is
/// guarded BEFORE any join. No registry, no events — the server has neither.
async fn copy_version_into_dir(
    data_dir: &std::path::Path,
    dest: &std::path::Path,
    v: &ModVersion,
    progress: &crate::mods::install::ProgressFn,
) -> crate::error::Result<String> {
    let Some(sha) = v
        .primary_file
        .sha1
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase())
    else {
        return Err(crate::error::Error::ModsSha1Unavailable);
    };
    if !crate::mods::modpack::path_safety::is_safe_filename(&v.primary_file.filename) {
        return Err(crate::error::Error::ModsUnsafeFilename {
            filename: v.primary_file.filename.clone(),
        });
    }
    let cached = crate::mods::install::fetch_to_cache(
        data_dir,
        &v.primary_file.url,
        &sha,
        v.primary_file.size,
        "servers",
        progress,
    )
    .await?;
    tokio::fs::create_dir_all(dest)
        .await
        .map_err(|e| crate::error::Error::io(dest.display().to_string(), e))?;
    let out = dest.join(&v.primary_file.filename);
    // Defense-in-depth: `is_safe_filename` above already rejects traversal, but
    // re-assert containment after the join (the repo's two-layer pattern).
    if !out.starts_with(dest) {
        return Err(crate::error::Error::ModsUnsafeFilename {
            filename: v.primary_file.filename.clone(),
        });
    }
    tokio::fs::copy(&cached, &out)
        .await
        .map_err(|e| crate::error::Error::io(out.display().to_string(), e))?;
    Ok(v.primary_file.filename.clone())
}

/// Install a chosen mod version + its transitive REQUIRED closure (plus any
/// manifest-discovered required libraries) into an arbitrary `dest` mods dir.
///
/// The server-side counterpart of [`mods_install_with_deps`]: it reuses the
/// exact resolution path (`resolve_closure` + `fetch_one_level` +
/// `manifest_extra_root_versions`) but writes jars straight into `dest` with NO
/// instance registry, NO emitted events, and NO optional deps. Required deps are
/// installed faithfully — never dropped on a "client-only" flag, which would be
/// the libraryferret footgun (a mis-signalled lib a real server mod needs).
/// `dest`'s existing enabled jars prune deps already present.
///
/// Best-effort per dependency: a dep that fails to resolve/download is recorded
/// in `unresolved` and the rest still install. The chosen primary is installed
/// last; a hard failure there propagates (the user explicitly picked it).
pub(crate) async fn install_version_into_dir(
    data_dir: &std::path::Path,
    dest: &std::path::Path,
    source: ModSource,
    project_id: &str,
    version_id: &str,
    mc_version: &str,
    loader: LoaderKind,
) -> crate::error::Result<crate::mods::dep_resolve::InstallMissingReport> {
    // Own the borrowed inputs before the async block so the future does not hold
    // references into the caller's frame (robust if this is ever spawned).
    let data_dir = data_dir.to_path_buf();
    let dest = dest.to_path_buf();
    let project_id = project_id.to_string();
    let version_id = version_id.to_string();
    let mc_version = mc_version.to_string();
    crate::network::throttle::with_interactive(async move {
        use crate::mods::deps::{resolve_closure, ProjectKey};
        use crate::mods::dep_resolve::{jar_provides, InstallMissingReport};
        use std::sync::Arc;

        let data_dir = data_dir.as_path();
        let dest = dest.as_path();
        let mc_version = mc_version.as_str();
        let mut report = InstallMissingReport::default();
        let nop: crate::mods::install::ProgressFn = Box::new(|_, _, _| {});

        // 1. Resolve the chosen version against the live API (mc + loader filtered).
        let vr = VersionRef {
            source,
            project_id,
            version_id,
        };
        let mut platform_box = platform_for(source);
        let primary_v = find_version(&mut platform_box, &vr, mc_version, loader).await?;

        // 2. Prune deps already present in `dest` (by lowercased filename only —
        //    servers keep no installed-mods registry, so the ProjectKey set is empty).
        let installed: std::collections::HashSet<ProjectKey> = std::collections::HashSet::new();
        let installed_filenames = enabled_jar_filenames(dest);

        // 3. Shared platform + loader cache + fetch factory (mirrors the instance path).
        let platform_arc: Arc<dyn crate::mods::platform::ModPlatform> = platform_for(source).into();
        let loader_cache = Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::<
            ProjectKey,
            bool,
        >::new()));
        let make_fetch = || {
            let platform = platform_arc.clone();
            let loader_cache = loader_cache.clone();
            let mc = mc_version.to_string();
            move |v: ModVersion| {
                let platform = platform.clone();
                let loader_cache = loader_cache.clone();
                let mc = mc.clone();
                async move {
                    fetch_one_level(platform.as_ref(), &loader_cache, &v, &mc, loader).await
                }
            }
        };

        // 4. Primary's transitive required closure. Collapse PlannedDep to
        //    ModVersion — the server path only needs versions to download.
        let primary_required: Vec<ModVersion> = resolve_closure(
            std::slice::from_ref(&primary_v),
            &installed,
            &installed_filenames,
            make_fetch(),
        )
        .await?
        .required
        .into_iter()
        .map(|p| p.version)
        .collect();

        // 5. Manifest-discovered required libs the platform metadata omits, each
        //    provides-verified over its DOWNLOADED jar before being committed.
        let extras_raw =
            manifest_extra_root_versions(data_dir, &primary_v, mc_version, loader).await;
        let extras =
            dedup_extra_candidates(extras_raw, &installed, &installed_filenames, &primary_required);
        let mut extra_install: Vec<ModVersion> = Vec::new();
        {
            let mut excl = installed.clone();
            for v in &primary_required {
                excl.insert(ProjectKey::of_version(v));
            }
            // Executor path only needs the candidate to download — the
            // `SelectionReason` is surfaced on the install-plan path, not here.
            for (needed_id, cand, _reason) in extras {
                if excl.contains(&ProjectKey::of_version(&cand)) {
                    continue;
                }
                let sha = match cand.primary_file.sha1.as_deref() {
                    Some(s) if !s.trim().is_empty() => s.to_ascii_lowercase(),
                    _ => continue,
                };
                let Ok(cached) = crate::mods::install::fetch_to_cache(
                    data_dir,
                    &cand.primary_file.url,
                    &sha,
                    cand.primary_file.size,
                    "servers",
                    &nop,
                )
                .await
                else {
                    continue;
                };
                let Ok(bytes) = tokio::fs::read(&cached).await else {
                    continue;
                };
                if !jar_provides(&bytes, &needed_id) {
                    continue;
                }
                excl.insert(ProjectKey::of_version(&cand));
                let sub = resolve_closure(
                    std::slice::from_ref(&cand),
                    &excl,
                    &installed_filenames,
                    make_fetch(),
                )
                .await?;
                for p in &sub.required {
                    excl.insert(ProjectKey::of_version(&p.version));
                }
                extra_install.extend(sub.required.into_iter().map(|p| p.version));
                extra_install.push(cand);
            }
        }

        // 6. Install dependencies first (best-effort), then the chosen primary.
        let deps = dedup_versions(primary_required.into_iter().chain(extra_install));
        for v in &deps {
            match copy_version_into_dir(data_dir, dest, v, &nop).await {
                Ok(filename) => report.installed.push(filename),
                Err(_) => report.unresolved.push(v.name.clone()),
            }
        }
        let primary_filename = copy_version_into_dir(data_dir, dest, &primary_v, &nop).await?;
        report.installed.push(primary_filename);
        Ok(report)
    })
    .await
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
    crate::network::throttle::with_interactive(async move {
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
    let mut required = primary_closure.required;

    // 2b. Best-effort: read the primary jar's manifest and fold in required
    //     libraries the platform metadata omitted (e.g. Waystones requires Balm,
    //     but CF metadata doesn't list it). Each resolved candidate brings its
    //     own transitive required sub-closure. Unresolved manifest ids are
    //     intentionally NOT surfaced here (InstallPlan.unresolvable carries
    //     DepProjectRefs, which a bare loader-id can't populate) — the preflight
    //     panel catches those. Reading the jar is best-effort: any failure
    //     yields no extras and preserves the prior behaviour.
    let extras_raw =
        manifest_extra_root_versions(&data_dir(&app)?, &primary, &mc_version, loader).await;
    // Only project `required` into owned versions and run the dedup pass when the
    // manifest actually yielded extras — the common case (no manifest extras)
    // skips the full clone + dedup entirely.
    let extras = if extras_raw.is_empty() {
        Vec::new()
    } else {
        let required_versions: Vec<ModVersion> =
            required.iter().map(|p| p.version.clone()).collect();
        dedup_extra_candidates(extras_raw, &installed, &installed_filenames, &required_versions)
    };
    if !extras.is_empty() {
        let mut excl = installed.clone();
        for p in &required {
            excl.insert(ProjectKey::of_version(&p.version));
        }
        for (_needed_id, cand, reason) in extras {
            if excl.contains(&ProjectKey::of_version(&cand)) {
                continue;
            }
            excl.insert(ProjectKey::of_version(&cand));
            let sub = resolve_closure(
                std::slice::from_ref(&cand),
                &excl,
                &installed_filenames,
                make_fetch(),
            )
            .await?;
            for p in sub.required {
                excl.insert(ProjectKey::of_version(&p.version));
                required.push(p);
            }
            // Carry the real range-aware reason threaded by
            // `manifest_extra_root_versions` (Task 7) for the candidate itself.
            required.push(PlannedDep {
                version: cand,
                selection_reason: reason,
            });
        }
    }

    // 3. Each direct optional + its transitive required sub-closure,
    //    excluding primary's requireds + installed.
    let mut exclude = installed.clone();
    for p in &required {
        exclude.insert(ProjectKey::of_version(&p.version));
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
            requires: dedup_planned(sub.required.into_iter()),
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
    })
    .await
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
    let mut results: Vec<(usize, ModUpdateCheck)> =
        stream::iter(eligible)
            .map(|(i, m, source, project_id, version_id)| {
                let mc = mc_version.clone();
                async move {
                    let platform = platform_for(source);
                    let state =
                        match cached_versions(platform.as_ref(), source, &project_id, &mc, loader)
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
    crate::network::throttle::with_interactive(async move {
        let inst_root = instance_root(&app, &instance_id)?;
        let dd = data_dir(&app)?;
        let (mc_version, loader) = read_active_mc_and_loader(&app, &instance_id)?;

        // Required dependencies of the target version (optional deps skipped).
        let platform = platform_for(target.source);
        let resolved = platform.resolve_deps(&target, &mc_version, loader).await?;
        let required_deps: Vec<ModVersion> =
            resolved.required.into_iter().map(|r| r.version).collect();

        // Progress events tagged with the target's project_id so the UI can
        // route the bar to the right card (same pattern as install).
        let app_for_progress = app.clone();
        let instance_id_for_progress = instance_id.clone();
        let project_id_for_progress = target.project_id.clone();
        let prog: crate::mods::install::ProgressFn = Box::new(move |phase, done, total| {
            let payload = match phase {
                crate::mods::install::ModInstallPhase::Downloading => {
                    ModInstallProgress::Downloading {
                        instance_id: instance_id_for_progress.clone(),
                        project_id: project_id_for_progress.clone(),
                        bytes_done: done as f64,
                        bytes_total: total.map(|t| t as f64),
                    }
                }
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
        match crate::mods::install::update_one(
            &dd,
            &inst_root,
            &old_sha1,
            target,
            required_deps,
            &prog,
        )
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
    })
    .await
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
            Some((source, project_id, _vid)) => {
                let platform = platform_for(source);
                crate::mods::compat::classify_compat(
                    cached_versions(platform.as_ref(), source, &project_id, &mc, loader).await,
                )
            }
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

/// Best-effort: read `primary`'s jar manifest and resolve required libraries
/// the platform metadata omitted. Returns `(needed_id, candidate,
/// selection_reason)` triples — the reason is the range-aware provenance the
/// install plan surfaces. Any error (no sha1, download failure, unreadable jar)
/// yields an empty vec so the install/plan proceeds exactly as before.
async fn manifest_extra_root_versions(
    dd: &std::path::Path,
    primary: &ModVersion,
    mc: &str,
    loader: LoaderKind,
) -> Vec<(String, ModVersion, crate::mods::platform::SelectionReason)> {
    let Some(sha) = primary
        .primary_file
        .sha1
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.to_ascii_lowercase())
    else {
        return Vec::new();
    };
    let nop: crate::mods::install::ProgressFn = Box::new(|_, _, _| {});
    let Ok(cached) = crate::mods::install::fetch_to_cache(
        dd,
        &primary.primary_file.url,
        &sha,
        primary.primary_file.size,
        "mods",
        &nop,
    )
    .await
    else {
        return Vec::new();
    };
    let Ok(bytes) = tokio::fs::read(&cached).await else {
        return Vec::new();
    };

    // Concrete clients; `CurseForgeClient::new()` resolves the keyring/embedded
    // key internally. Both route through `network::` — no raw HTTP added.
    let mr = crate::mods::modrinth::ModrinthClient::new();
    let cf = crate::mods::curseforge::CurseForgeClient::new();
    let mc_owned = mc.to_string();
    let (extras, unresolved) = crate::mods::dep_resolve::manifest_extra_roots(&bytes, |dep| {
        let mr = &mr;
        let cf = &cf;
        let mc = mc_owned.clone();
        async move {
            // Thread the manifest dep's declared range + family into selection;
            // an empty range string means "no constraint" → None.
            let range_owned = dep.range.trim().to_string();
            let range = if range_owned.is_empty() {
                None
            } else {
                Some((range_owned.as_str(), dep.family))
            };
            // Separate owned MC clones per lookup closure — each is captured by
            // its own `async move` future, so they cannot share one binding.
            let mr_mc = mc.clone();
            let cf_mc = mc;
            crate::mods::dep_resolve::resolve_missing_dep(
                &dep.dep_id,
                range,
                move |id| {
                    let mc = mr_mc.clone();
                    async move {
                        Ok(crate::mods::dep_resolve::modrinth_lookup(mr, &id, &mc, loader).await)
                    }
                },
                move |id| {
                    let mc = cf_mc.clone();
                    async move {
                        Ok(crate::mods::dep_resolve::curseforge_lookup(cf, &id, &mc, loader).await)
                    }
                },
            )
            .await
        }
    })
    .await;
    // Transparency: these bare loader/dep ids can't populate `InstallPlan.unresolvable`
    // (which holds `DepProjectRef`s) — the preflight panel is the user-facing backstop.
    // Leave a trace so the launcher log records what the resolver could not auto-resolve.
    if !unresolved.is_empty() {
        crate::diag!(
            "manifest_extra_roots: {} dep(s) could not be auto-resolved for '{}' ({}): {}",
            unresolved.len(),
            primary.name,
            primary.version_id,
            unresolved.join(", ")
        );
    }
    extras
        .into_iter()
        .map(|e| (e.needed_id, e.candidate, e.selection_reason))
        .collect()
}

/// Drop extra candidates already installed or already in the required set
/// (by source-specific ProjectKey or by lowercased jar filename). Pure/testable.
/// The `SelectionReason` rides through untouched — dedup keys off the candidate
/// only.
fn dedup_extra_candidates(
    extras: Vec<(String, ModVersion, crate::mods::platform::SelectionReason)>,
    installed: &std::collections::HashSet<ProjectKey>,
    installed_filenames: &std::collections::HashSet<String>,
    already_required: &[ModVersion],
) -> Vec<(String, ModVersion, crate::mods::platform::SelectionReason)> {
    let mut excl = installed.clone();
    for v in already_required {
        excl.insert(ProjectKey::of_version(v));
    }
    extras
        .into_iter()
        .filter(|(_id, c, _reason)| {
            let fresh = excl.insert(ProjectKey::of_version(c));
            fresh && !installed_filenames.contains(&c.primary_file.filename.to_ascii_lowercase())
        })
        .collect()
}

/// One-click install of a missing required dependency identified only by its
/// loader mod-id (e.g. `balm`). Resolves it (Modrinth-slug-first + name-search
/// fallback -> CF, the latter loader/MC-decoupled), verifies the downloaded jar
/// actually provides that id, then installs it. No manifest range context on
/// this bare-id path → `range = None`. On any resolution/verification miss
/// returns `OpenSearch` so the UI can offer a pre-filled search instead of
/// guessing.
#[tauri::command]
#[specta::specta]
pub async fn mods_install_missing_required(
    app: tauri::AppHandle,
    instance_id: String,
    dep_id: String,
) -> crate::error::Result<crate::mods::platform::InstallMissingOutcome> {
    use crate::mods::dep_resolve::{jar_provides, DepResolution};
    use crate::mods::platform::InstallMissingOutcome;

    let inst_root = instance_root(&app, &instance_id)?;
    let dd = data_dir(&app)?;
    let (mc_version, loader) = read_active_mc_and_loader(&app, &instance_id)?;

    // Concrete clients; `CurseForgeClient::new()` resolves the key internally.
    let mr = crate::mods::modrinth::ModrinthClient::new();
    let cf = crate::mods::curseforge::CurseForgeClient::new();
    let resolution = crate::mods::dep_resolve::resolve_missing_dep(
        &dep_id,
        None,
        |id| {
            let mr = &mr;
            let mc = mc_version.clone();
            async move { Ok(crate::mods::dep_resolve::modrinth_lookup(mr, &id, &mc, loader).await) }
        },
        |id| {
            let cf = &cf;
            let mc = mc_version.clone();
            async move {
                Ok(crate::mods::dep_resolve::curseforge_lookup(cf, &id, &mc, loader).await)
            }
        },
    )
    .await;
    let DepResolution::Resolved {
        candidate,
        needed_id,
        selection_reason,
    } = resolution
    else {
        return Ok(InstallMissingOutcome::OpenSearch { query: dep_id });
    };
    crate::diag!(
        "dep_resolve: {dep_id} -> {} ({selection_reason:?})",
        candidate.version_id
    );

    let nop: crate::mods::install::ProgressFn = Box::new(|_, _, _| {});
    let sha = match candidate.primary_file.sha1.as_deref() {
        Some(s) if !s.trim().is_empty() => s.to_ascii_lowercase(),
        _ => return Ok(InstallMissingOutcome::OpenSearch { query: dep_id }),
    };
    let cached = crate::mods::install::fetch_to_cache(
        &dd,
        &candidate.primary_file.url,
        &sha,
        candidate.primary_file.size,
        "mods",
        &nop,
    )
    .await?;
    let bytes = tokio::fs::read(&cached)
        .await
        .map_err(|e| crate::error::Error::io("<dep-candidate-cache>", e))?;
    if !jar_provides(&bytes, &needed_id) {
        crate::diag!(
            "dep_resolve: candidate for '{needed_id}' did not provide it; degrading to search"
        );
        return Ok(InstallMissingOutcome::OpenSearch { query: dep_id });
    }

    let inst = crate::mods::install::install_one(&dd, &inst_root, candidate, &nop).await?;
    let _ = ModInstalled {
        instance_id: instance_id.clone(),
        sha1: inst.sha1,
        filename: inst.filename,
        name: inst.name.clone(),
    }
    .emit(&app);
    Ok(InstallMissingOutcome::Installed { name: inst.name })
}

/// Build a full nested dependency graph for all platform-identified mods in
/// `instance_id`. Each installed mod is a root; its required and optional
/// subtrees are walked recursively (cycle-guarded, memoized) and classified as
/// `satisfied / missing_required / optional_present / optional_absent` against
/// the installed set.
///
/// Network-frugal by construction. The old approach queried each mod's newest
/// version and resolved every dependency one project at a time — ~1000+
/// individual requests on a large instance, a 429 rate-limit storm. Instead
/// this:
///   1. batch-fetches each installed mod's *installed* version by id (the
///      version object carries its declared deps), and
///   2. batch-fetches every referenced project's summary into the shared cache
///      for display names + loader-slug detection,
/// then runs the recursion over that in-memory data: an installed project
/// contributes its version's deps; a non-installed project is a leaf (no
/// recursion, no network). Informational only — no files are written.
#[tauri::command]
#[specta::specta]
pub async fn mods_dependency_graph(
    app: tauri::AppHandle,
    instance_id: String,
) -> crate::error::Result<crate::mods::depgraph::DependencyGraph> {
    use crate::mods::depgraph::{build_graph, DependencyGraph, InstalledNode, NodeDeps};
    use std::collections::{HashMap, HashSet};
    use std::sync::Arc;

    let root = instance_root(&app, &instance_id)?;
    // The instance loader scopes the graph: a declaring mod whose loader family
    // this instance cannot load is inert here, so its declared deps must not be
    // shown as required (the depgraph analogue of preflight's #154 scoping).
    let loader = crate::instances::read_instance(&app, &instance_id)?.loader;
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
    if roots.is_empty() {
        return Ok(DependencyGraph { roots: Vec::new() });
    }

    // 1. Batch-fetch each installed mod's *installed* version by id, per source.
    //    The version object carries its declared deps. A mod with no stored
    //    version_id contributes no entry → it becomes a root with no children.
    let mut version_ids: HashMap<ModSource, Vec<String>> = HashMap::new();
    for m in &installed_mods {
        if let (Some(source), Some(_), Some(vid)) =
            (m.source, m.project_id.as_ref(), m.version_id.as_ref())
        {
            version_ids.entry(source).or_default().push(vid.clone());
        }
    }
    // (source, project_id) -> that mod's installed-version dependency links.
    let mut deps_by_project: HashMap<(ModSource, String), DepNodeMeta> = HashMap::new();
    for (source, vids) in &version_ids {
        let refs: Vec<&str> = vids.iter().map(String::as_str).collect();
        let versions = match platform_for(*source).versions_by_ids(&refs).await {
            Ok(v) => v,
            Err(e) => {
                // Degrade to roots-with-no-children for this source, but record
                // why (missing CF key, transient 429) so it's diagnosable.
                crate::diag!("[depgraph] versions_by_ids failed for {source:?}: {e}");
                Vec::new()
            }
        };
        for v in versions {
            deps_by_project.insert(
                (*source, v.project_id.clone()),
                DepNodeMeta {
                    loaders: v.loaders,
                    deps: v.deps,
                },
            );
        }
    }

    // 2. Batch every referenced project_id (roots + dep children) into the
    //    shared summary cache, per source, for display names + loader slugs.
    let ttl = mod_metadata_ttl_days(&app)?;
    let cache_path = crate::paths::mods_cache_file(&app)
        .map_err(|e| crate::error::Error::io("<mods_cache_file>", e))?;
    let mut ids_by_source: HashMap<ModSource, HashSet<String>> = HashMap::new();
    for n in &roots {
        ids_by_source
            .entry(n.source)
            .or_default()
            .insert(n.project_id.clone());
    }
    for node in deps_by_project.values() {
        for d in &node.deps {
            let (src, pid) = dep_ref_key(&d.project_ref);
            ids_by_source.entry(src).or_default().insert(pid);
        }
    }
    let mut summaries: HashMap<(ModSource, String), ModSummary> = HashMap::new();
    for (source, set) in ids_by_source {
        let ids: Vec<String> = set.into_iter().collect();
        let platform = platform_for(source);
        let got = crate::mods::summary_cache::get_many(
            &cache_path,
            source,
            &ids,
            ttl,
            move |q: Vec<String>| async move {
                let refs: Vec<&str> = q.iter().map(String::as_str).collect();
                platform.summaries(&refs).await
            },
        )
        .await;
        for s in got {
            summaries.insert((source, s.project_id.clone()), s);
        }
    }

    // 3. Build the graph over in-memory data. `fetch` is a synchronous lookup:
    //    an installed project yields its version's required/optional children
    //    (loader-only deps dropped via cached slug; names from the cache,
    //    falling back to the project id); a non-installed project yields
    //    nothing → emitted as a leaf, no recursion, no network.
    let deps_by_project = Arc::new(deps_by_project);
    let summaries = Arc::new(summaries);
    let fetch = move |source: ModSource, project_id: String| {
        let deps_by_project = deps_by_project.clone();
        let summaries = summaries.clone();
        async move {
            let result = match deps_by_project.get(&(source, project_id)) {
                Some(node) => node_deps_scoped(node, loader, &summaries),
                None => NodeDeps::default(),
            };
            Ok::<NodeDeps, crate::error::Error>(result)
        }
    };

    build_graph(&roots, fetch).await
}

/// Map a dependency reference to the `(source, project_id)` key used by the
/// installed set and the summary cache. CurseForge ids are numeric mod ids,
/// stringified to match the installed registry.
fn dep_ref_key(r: &DepProjectRef) -> (ModSource, String) {
    match r {
        DepProjectRef::Modrinth { project_id, .. } => (ModSource::Modrinth, project_id.clone()),
        DepProjectRef::Curseforge { mod_id, .. } => (ModSource::Curseforge, mod_id.to_string()),
    }
}

/// One installed mod's installed-version metadata needed to build its graph
/// children: the platform-declared `loaders` (for instance-loader scoping) and
/// the declared dependency links.
struct DepNodeMeta {
    loaders: Vec<crate::mods::platform::LoaderKind>,
    deps: Vec<ModDepLink>,
}

/// Project a declaring node's required/optional graph children from its platform
/// metadata, scoped to the instance loader. A node inert on this instance (its
/// declared loaders are family-disjoint from `loader`) yields no children — this
/// is what stops a Forge instance from showing an inert Fabric jar's `fabric-api`
/// requirement (the dependency-graph analogue of preflight's loader scoping,
/// PR #154). Synchronous, pure in-memory projection.
fn node_deps_scoped(
    node: &DepNodeMeta,
    loader: crate::mods::platform::LoaderKind,
    summaries: &std::collections::HashMap<(ModSource, String), ModSummary>,
) -> crate::mods::depgraph::NodeDeps {
    use crate::mods::depgraph::{DepChild, NodeDeps};
    if crate::mods::local::loaders_disjoint_from_instance(&node.loaders, loader) {
        return NodeDeps::default();
    }
    let mut required = Vec::new();
    let mut optional = Vec::new();
    for link in &node.deps {
        if matches!(link.kind, DepKind::Incompatible | DepKind::Embedded) {
            continue;
        }
        let (child_src, child_pid) = dep_ref_key(&link.project_ref);
        let summary = summaries.get(&(child_src, child_pid.clone()));
        // Loader projects (fabric/forge/…) are instance-managed and never
        // shown as a mod dependency — drop them by their cached slug.
        let is_loader = summary
            .and_then(|s| s.slug.as_deref())
            .map(|slug| LOADER_SLUGS.contains(&slug.to_ascii_lowercase().as_str()))
            .unwrap_or(false);
        if is_loader {
            continue;
        }
        let name = summary
            .map(|s| s.name.clone())
            .unwrap_or_else(|| child_pid.clone());
        let child = DepChild {
            source: child_src,
            project_id: child_pid,
            name,
        };
        match link.kind {
            DepKind::Required => required.push(child),
            DepKind::Optional => optional.push(child),
            _ => {}
        }
    }
    NodeDeps { required, optional }
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
    let loader = crate::instances::read_instance(&app, &instance_id)?.loader;
    crate::mods::preflight::dependency_preflight_for_root(&root, loader).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::deps::ProjectKey;
    use crate::mods::platform::{
        DepKind, DepProjectRef, LoaderKind, ModDepLink, ModFile, ModSource, ModVersion,
    };
    use std::collections::HashSet;

    #[test]
    fn node_deps_scoped_suppresses_inert_fabric_node_on_forge() {
        // A Fabric-only declaring node whose only child is the (phantom) Fabric API.
        let node = DepNodeMeta {
            loaders: vec![LoaderKind::Fabric],
            deps: vec![ModDepLink {
                kind: DepKind::Required,
                project_ref: DepProjectRef::Modrinth {
                    project_id: "P7dR8mSH".into(),
                    version_id: None,
                },
            }],
        };
        let out = node_deps_scoped(&node, LoaderKind::Forge, &std::collections::HashMap::new());
        assert!(
            out.required.is_empty(),
            "inert Fabric node must yield no required deps on Forge"
        );
        assert!(out.optional.is_empty());
    }

    #[test]
    fn node_deps_scoped_keeps_deps_for_matching_loader() {
        let node = DepNodeMeta {
            loaders: vec![LoaderKind::Forge],
            deps: vec![ModDepLink {
                kind: DepKind::Required,
                project_ref: DepProjectRef::Modrinth {
                    project_id: "abc".into(),
                    version_id: None,
                },
            }],
        };
        let out = node_deps_scoped(&node, LoaderKind::Forge, &std::collections::HashMap::new());
        assert_eq!(out.required.len(), 1);
        assert_eq!(out.required[0].project_id, "abc");
    }

    #[test]
    fn node_deps_scoped_keeps_deps_when_loaders_unknown() {
        // Platform reported no loader tags → conservative: do not suppress.
        let node = DepNodeMeta {
            loaders: vec![],
            deps: vec![ModDepLink {
                kind: DepKind::Required,
                project_ref: DepProjectRef::Modrinth {
                    project_id: "abc".into(),
                    version_id: None,
                },
            }],
        };
        let out = node_deps_scoped(&node, LoaderKind::Forge, &std::collections::HashMap::new());
        assert_eq!(out.required.len(), 1, "unknown loaders must not suppress");
    }

    /// Mirror the `mv` helper from `dep_resolve.rs` tests: a minimal Modrinth
    /// `ModVersion` whose `project_id` and jar filename derive from `slug`.
    fn mv(slug: &str) -> ModVersion {
        ModVersion {
            source: ModSource::Modrinth,
            project_id: slug.into(),
            version_id: format!("{slug}-v"),
            name: slug.into(),
            version_number: "1.0".into(),
            mc_versions: vec!["1.20.4".into()],
            loaders: vec![LoaderKind::NeoForge],
            primary_file: ModFile {
                filename: format!("{slug}.jar"),
                url: format!("https://cdn/{slug}.jar"),
                sha1: Some("aa".into()),
                size: 1.0,
                distribution_allowed: true,
            },
            deps: vec![],
            published_at: None,
        }
    }

    #[test]
    fn dedup_prunes_already_installed_and_required_extras() {
        let balm = mv("balm");
        let curios = mv("curios");

        // `balm` is already installed; a duplicate `curios` is in the input.
        let mut installed: HashSet<ProjectKey> = HashSet::new();
        installed.insert(ProjectKey::of_version(&balm));
        let installed_filenames: HashSet<String> = HashSet::new();
        let already_required: Vec<ModVersion> = Vec::new();

        let r = crate::mods::platform::SelectionReason::NewestNoPin;
        let extras = vec![
            ("balm".to_string(), balm.clone(), r),
            ("curios".to_string(), curios.clone(), r),
            ("curios".to_string(), curios.clone(), r),
        ];

        let kept =
            dedup_extra_candidates(extras, &installed, &installed_filenames, &already_required);

        // `balm` pruned (installed); `curios` kept exactly once (deduped).
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert_eq!(kept[0].0, "curios");
        assert_eq!(kept[0].1.project_id, "curios");
    }

    #[test]
    fn dedup_prunes_candidate_already_in_required_set() {
        let balm = mv("balm");

        let installed: HashSet<ProjectKey> = HashSet::new();
        let installed_filenames: HashSet<String> = HashSet::new();
        // `balm` is already in the primary's required closure.
        let already_required = vec![balm.clone()];

        let kept = dedup_extra_candidates(
            vec![(
                "balm".to_string(),
                balm.clone(),
                crate::mods::platform::SelectionReason::NewestNoPin,
            )],
            &installed,
            &installed_filenames,
            &already_required,
        );
        assert!(kept.is_empty(), "{kept:?}");
    }

    #[test]
    fn dedup_prunes_candidate_matching_installed_filename() {
        let balm = mv("balm");

        let installed: HashSet<ProjectKey> = HashSet::new();
        // A copy of balm.jar is already on disk under a *different* source id.
        let mut installed_filenames: HashSet<String> = HashSet::new();
        installed_filenames.insert("balm.jar".to_string());
        let already_required: Vec<ModVersion> = Vec::new();

        let kept = dedup_extra_candidates(
            vec![(
                "balm".to_string(),
                balm,
                crate::mods::platform::SelectionReason::NewestNoPin,
            )],
            &installed,
            &installed_filenames,
            &already_required,
        );
        assert!(kept.is_empty(), "{kept:?}");
    }
}
