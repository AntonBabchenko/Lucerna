//! Orchestrates: inspect → resolve name → create_instance → install N mods
//! → extract overrides. Emits typed progress events at each phase.

use crate::error::Error;
use crate::mods::install::{fetch_to_cache, install_asset, install_one, ProgressFn};
use crate::mods::installed::{PackOrigin, PackOriginFile};
use crate::mods::modpack::detect::detect_format;
use crate::mods::modpack::overrides;
use crate::mods::modpack::schema::*;
use crate::mods::modpack::{curseforge as cf_parse, modrinth as mr_parse};
use crate::mods::platform::{ModFile, ModSource, ModVersion};
use futures_util::stream::{self, StreamExt};

/// Concurrent download fan-out width for the modpack pre-warm pass. Matches
/// `assets`/`jre`/`libraries` so the network saturates the same way.
const MODPACK_PREWARM_CONCURRENCY: usize = 8;

pub async fn inspect(bytes: &[u8], cf_base: &str) -> Result<ModpackSummary, Error> {
    let fmt = detect_format(bytes)?;
    match fmt {
        ModpackFormat::Modrinth => mr_parse::parse(bytes),
        ModpackFormat::Curseforge => cf_parse::parse(bytes, cf_base).await,
        // FTB: pack-managed source — FTB packs are imported via the API
        // sidecar path (not a local archive). This arm is unreachable for
        // drag-drop inspect but required for exhaustiveness.
        ModpackFormat::Ftb => Err(Error::ModpackFormatUnknown),
        // ATLauncher: pack-managed source — imported via the API path.
        // Unreachable for drag-drop inspect but required for exhaustiveness.
        ModpackFormat::Atlauncher => Err(Error::ModpackFormatUnknown),
    }
}

/// Best-effort: if `install_path` lands in `resourcepacks/` or `shaderpacks/`,
/// record the file in the per-instance assets registry so it shows in the
/// Add-ons → Installed view. Non-asset paths (mods/, config/, …) are ignored —
/// they have no Installed tab. A registry-write failure is logged, never fatal:
/// the file is already on disk and tracked in `pack_origin`.
#[allow(clippy::too_many_arguments)]
async fn register_asset_if_applicable(
    instance_root: &std::path::Path,
    install_path: &str,
    filename: &str,
    sha1: &str,
    name: &str,
    source: Option<ModSource>,
    project_id: Option<String>,
    version_id: Option<String>,
) {
    let Some(kind) = crate::mods::install::content_kind_for_install_path(install_path) else {
        return;
    };
    let asset = crate::mods::assets::make_asset(
        kind,
        filename,
        sha1,
        source,
        project_id,
        version_id,
        name,
        chrono::Utc::now().to_rfc3339(),
    );
    if let Err(e) = crate::mods::assets::add(instance_root, asset).await {
        crate::diag!("[modpack::import] asset registry add failed (non-fatal): {e}");
    }
}

/// Build the immutable origin snapshot from the resolved summary and
/// the user-selected file slice. Pure: no I/O, easy to unit-test. Called
/// once at the end of a successful import; the snapshot lives in
/// `installed-mods.json` so the launcher can later diff against the live
/// installed-mods list without re-parsing the .mrpack/.zip.
pub fn build_pack_origin(
    summary: &ModpackSummary,
    selected: &[&ModpackFile],
    mrpack_project_id: Option<String>,
    pack_name: &str,
) -> PackOrigin {
    let source = match summary.format {
        ModpackFormat::Modrinth => ModSource::Modrinth,
        ModpackFormat::Curseforge => ModSource::Curseforge,
        // FTB: pack-managed source — provenance is Ftb.
        ModpackFormat::Ftb => ModSource::Ftb,
        // ATLauncher: pack-managed source — provenance is Atlauncher.
        ModpackFormat::Atlauncher => ModSource::Atlauncher,
    };
    let files = selected
        .iter()
        .map(|f| PackOriginFile {
            sha1: f.sha1.clone(),
            name: f.name.clone(),
            filename: f.filename.clone(),
            install_path: f.install_path.clone(),
            url: f.url.clone(),
            size: f.size,
            project_id: f.project_id.clone(),
            version_id: f.version_id.clone(),
            env_client: f.env_client,
            source: f.source,
        })
        .collect();
    PackOrigin {
        project_id: mrpack_project_id,
        source,
        project_name: pack_name.to_string(),
        version: summary.version.clone(),
        files,
        missing_mods: summary
            .unresolvable
            .iter()
            .filter(|u| {
                matches!(
                    u.reason,
                    UnresolvableReason::DistributionDisabled
                        | UnresolvableReason::HostNotAllowed
                        | UnresolvableReason::MissingChecksum
                )
            })
            .cloned()
            .collect(),
        // Populated by the orchestrator after override extraction reports
        // which oversized bundled files it skipped. build_pack_origin is
        // pure (no archive access), so it always starts empty here.
        skipped_overrides: vec![],
        resolved_missing: Vec::new(),
        // Populated by the orchestrator after it scans the installed mods for
        // jars built for a loader family this instance cannot load.
        // build_pack_origin is pure (no disk access), so it starts empty.
        inert_loader_jars: vec![],
    }
}

/// Scan jars in `mods_dir`, returning those built for a loader family the
/// `instance_loader` cannot load (inert here). Best-effort: an unreadable jar
/// is skipped, never fatal. Deduped by filename. Loader-FAMILY only (delegates
/// to `compat_verdict`; MC-version mismatch is deliberately ignored — it
/// false-positives on bundled jars). A Vanilla instance has no loader family,
/// so nothing is ever flagged.
fn classify_inert_loader_jars(
    mods_dir: &std::path::Path,
    instance_loader: crate::instances::schema::LoaderKind,
    instance_mc: &str,
) -> Vec<InertLoaderJar> {
    let Ok(entries) = std::fs::read_dir(mods_dir) else {
        return Vec::new();
    };
    let mut out: Vec<InertLoaderJar> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jar") {
            continue;
        }
        let Some(filename) = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_string)
        else {
            continue;
        };
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Ok(meta) = crate::mods::local::read_jar_meta(&bytes) else {
            continue;
        };
        let verdict = crate::mods::local::compat_verdict(&meta, instance_loader, instance_mc);
        if verdict.loader_mismatch {
            out.push(InertLoaderJar {
                filename,
                // loader_mismatch is only set when the jar declares a non-empty
                // family set, so compat_verdict always carries a detected_loader
                // label here — the unwrap_or_default "" fallback is unreachable.
                detected_loader: verdict.detected_loader.unwrap_or_default(),
            });
        }
    }
    dedupe_inert(out)
}

/// Collapse duplicate `InertLoaderJar` entries by filename, keeping first seen.
fn dedupe_inert(mut jars: Vec<InertLoaderJar>) -> Vec<InertLoaderJar> {
    let mut seen = std::collections::HashSet::new();
    jars.retain(|j| seen.insert(j.filename.clone()));
    jars
}

/// A pack-origin file is a *tracked mod* iff it lands as a direct child
/// of `mods/` with a `.jar` extension — exactly what the installed-mods
/// scan (and Forge's loader) recognises. Anything else under `mods/` —
/// a `.zip` resource pack, or a nested `mods/<repo>/…/x.jar` bundled
/// library — is an asset whose presence is verified on disk, not via
/// the mod registry.
pub(crate) fn is_tracked_mod(install_path: &str) -> bool {
    install_path
        .strip_prefix("mods/")
        .is_some_and(|rest| !rest.is_empty() && !rest.contains('/') && rest.ends_with(".jar"))
}

/// The leading, non-version part of a jar filename — a fuzzy,
/// version-independent mod identity. CurseForge filenames are
/// `{modid-ish}-{mc}-{modver}.jar`, so the part before the first
/// version-looking segment identifies the mod across versions.
/// `srparasites-1.12.2-2.7.1.jar` → `srparasites`. Splits on `-` and
/// `_`, keeps leading segments up to the first one starting with an
/// ASCII digit, lowercased. `None` when nothing remains (a filename
/// that starts with a digit) — a `None` stem never matches.
fn filename_stem(filename: &str) -> Option<String> {
    let base = filename.strip_suffix(".disabled").unwrap_or(filename);
    let base = base.strip_suffix(".jar").unwrap_or(base);
    let mut kept: Vec<&str> = Vec::new();
    for seg in base.split(['-', '_']) {
        if seg.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            break;
        }
        if !seg.is_empty() {
            kept.push(seg);
        }
    }
    if kept.is_empty() {
        None
    } else {
        Some(kept.join("-").to_ascii_lowercase())
    }
}

/// Classify a `missing_mods` entry against the installed jars.
/// `Substituted` = a user-chosen substitute is installed (recorded in `resolved`); it takes priority over all other signals.
/// `Installed` = the exact pinned file (sha1 or filename match).
/// `DifferentVersion` = the mod is present but not the pinned file —
/// matched by any of three signals: platform project id (reliable, but
/// recorded only for mods installed via the launcher's mod browser),
/// descriptor name (case-insensitive), or version-independent filename
/// stem (fuzzy; catches a hand-dropped different version, which carries
/// no project id). `Missing` otherwise.
fn missing_mod_state(
    m: &ModpackUnresolvable,
    installed: &[crate::mods::platform::InstalledMod],
    resolved: &[crate::mods::installed::ResolvedMissing],
) -> MissingModState {
    // A user-chosen substitute closes the entry as long as its jar is still
    // installed. Self-healing: if the sha1 left the registry, fall through to
    // the normal pinned/different/missing classification below.
    let substituted = resolved.iter().any(|r| {
        r.filename.eq_ignore_ascii_case(&m.filename)
            // Exact (not case-insensitive) by design: the overlay records the
            // identical `mod_name` string at resolution time, so they match
            // byte-for-byte. Do not widen this to eq_ignore_ascii_case.
            && r.mod_name == m.mod_name
            && installed
                .iter()
                .any(|i| i.sha1.eq_ignore_ascii_case(&r.sha1))
    });
    if substituted {
        return MissingModState::Substituted;
    }

    let pinned = installed.iter().any(|i| {
        m.sha1
            .as_deref()
            .is_some_and(|s| i.sha1.eq_ignore_ascii_case(s))
            || i.filename.eq_ignore_ascii_case(&m.filename)
    });
    if pinned {
        return MissingModState::Installed;
    }
    let m_stem = filename_stem(&m.filename);
    let different = installed.iter().any(|i| {
        // `is_some()` guard: without it, two project-id-less mods would
        // match on `None == None`.
        (m.project_id.is_some() && i.project_id.as_deref() == m.project_id.as_deref())
            || i.name.eq_ignore_ascii_case(&m.mod_name)
            // Filename stem — fuzzy but version-independent; catches a
            // hand-dropped different version (project_id is absent then).
            || (m_stem.is_some() && m_stem == filename_stem(&i.filename))
    });
    if different {
        MissingModState::DifferentVersion
    } else {
        MissingModState::Missing
    }
}

/// Diff the immutable pack-origin snapshot against live instance state.
/// A `mods/*` origin file is "present" iff its SHA-1 is in the installed
/// mod registry; a non-`mods/` (asset) origin file is "present" iff its
/// `install_path` is in `asset_present` (computed by the caller via a
/// filesystem stat). Pure — unit-tested independently.
pub fn compute_status(
    origin: PackOrigin,
    installed: &[crate::mods::platform::InstalledMod],
    asset_present: &std::collections::HashSet<String>,
) -> crate::mods::modpack::schema::ModpackStatus {
    let installed_shas: Vec<String> = installed
        .iter()
        .map(|m| m.sha1.to_ascii_lowercase())
        .collect();
    let installed_set: std::collections::HashSet<&String> = installed_shas.iter().collect();

    // Mod SHAs the pack declared — used for the user-additions count.
    let origin_mod_shas: std::collections::HashSet<String> = origin
        .files
        .iter()
        .filter(|f| is_tracked_mod(&f.install_path))
        .map(|f| f.sha1.to_ascii_lowercase())
        .collect();

    let removed_files: Vec<PackOriginFile> = origin
        .files
        .iter()
        .filter(|f| {
            if is_tracked_mod(&f.install_path) {
                !installed_set.contains(&f.sha1.to_ascii_lowercase())
            } else {
                !asset_present.contains(&f.install_path)
            }
        })
        .cloned()
        .collect();

    let added_count: u32 = installed_shas
        .iter()
        .filter(|s| !origin_mod_shas.contains(*s))
        .count() as u32;

    let is_modified = !removed_files.is_empty() || added_count > 0;

    // Reconcile missing mods against what the user has since installed.
    // Read origin.missing_mods before the struct literal moves `origin`.
    let missing_mods: Vec<MissingModStatus> = origin
        .missing_mods
        .iter()
        .map(|m| MissingModStatus {
            entry: m.clone(),
            state: missing_mod_state(m, installed, &origin.resolved_missing),
        })
        .collect();

    crate::mods::modpack::schema::ModpackStatus {
        origin,
        installed_shas,
        removed_files,
        added_count,
        is_modified,
        missing_mods,
    }
}

/// Diff a new pack version's `files[]` against the installed
/// `pack_origin`. Only URL-bearing entries are considered on both sides
/// — `overrides/`-bundled entries (empty `url`) are excluded; a normal
/// update never touches `overrides/`. Files are matched by `project_id`;
/// equal SHA-1 = unchanged, different = updated, unmatched = added/removed.
/// Pure — unit-tested independently.
pub fn compute_update_diff(
    new_summary: &crate::mods::modpack::schema::ModpackSummary,
    current_origin: &PackOrigin,
    instance_mc_version: &str,
    instance_loader: crate::mods::platform::LoaderKind,
    instance_loader_version: &Option<String>,
) -> crate::mods::modpack::schema::ModpackUpdateDiff {
    use crate::mods::modpack::schema::{ModpackUpdateDiff, ModpackUpdateEntry, ModpackVersionBump};
    use std::collections::HashMap;

    let old_by_project: HashMap<&str, &PackOriginFile> = current_origin
        .files
        .iter()
        .filter(|f| !f.url.is_empty())
        .map(|f| (f.project_id.as_str(), f))
        .collect();
    let new_by_project: HashMap<&str, &ModpackFile> = new_summary
        .files
        .iter()
        .filter(|f| !f.url.is_empty())
        .map(|f| (f.project_id.as_str(), f))
        .collect();

    let mut added = vec![];
    let mut updated = vec![];
    for (pid, nf) in &new_by_project {
        match old_by_project.get(pid) {
            None => added.push((*nf).clone()),
            Some(of) => {
                if !of.sha1.eq_ignore_ascii_case(&nf.sha1) {
                    updated.push(ModpackUpdateEntry {
                        old: (*of).clone(),
                        new: (*nf).clone(),
                    });
                }
            }
        }
    }
    let removed: Vec<PackOriginFile> = old_by_project
        .iter()
        .filter(|(pid, _)| !new_by_project.contains_key(*pid))
        .map(|(_, of)| (*of).clone())
        .collect();

    let version_bump = if new_summary.game_version != instance_mc_version
        || new_summary.loader != instance_loader
        || &new_summary.loader_version != instance_loader_version
    {
        Some(ModpackVersionBump {
            old_game_version: instance_mc_version.to_string(),
            new_game_version: new_summary.game_version.clone(),
            old_loader_version: instance_loader_version.clone(),
            new_loader_version: new_summary.loader_version.clone(),
        })
    } else {
        None
    };

    ModpackUpdateDiff {
        added,
        removed,
        updated,
        version_bump,
        new_version_number: new_summary.version.clone(),
    }
}

/// Synthesise a `ModVersion` from a live `ModpackFile` so both the
/// import orchestrator and the update orchestrator can feed it to
/// `install_one`. Shared helper — extract once, use everywhere.
pub fn modpack_file_to_mod_version(
    file: &ModpackFile,
    game_version: &str,
    loader: crate::mods::platform::LoaderKind,
) -> ModVersion {
    // Emit None for an empty/whitespace sha1 so install_one's sha-guard
    // (`ok_or(Error::ModsSha1Unavailable)`) rejects the file rather than
    // silently installing an unverifiable archive (no-TOFU, Principle B.6).
    let sha1 = {
        let s = file.sha1.trim();
        if s.is_empty() {
            None
        } else {
            Some(s.to_string())
        }
    };
    ModVersion {
        source: file.source,
        project_id: file.project_id.clone(),
        version_id: file.version_id.clone(),
        name: file.name.clone(),
        version_number: String::new(),
        mc_versions: vec![game_version.to_string()],
        loaders: vec![loader],
        primary_file: ModFile {
            filename: file.filename.clone(),
            url: file.url.clone(),
            sha1,
            size: file.size,
            distribution_allowed: true,
            sha256: None,
        },
        deps: vec![],
        published_at: None,
    }
}

/// Synthesise a `ModVersion` from a frozen `PackOriginFile` so the
/// restore path can reuse `install_one`. The pack-origin snapshot
/// carries every field needed (sha, url, project_id, version_id,
/// env_client) — the only fields we have to fabricate are
/// `mc_versions`, `loaders`, and `version_number` (the snapshot never
/// recorded a display version_number since the .mrpack/.zip parsers
/// drop it; we leave it blank like the original import did).
/// `distribution_allowed` is set to `true` because the origin file is
/// only ever written for files we successfully installed via the import
/// pipeline, which already enforces the distribution-allowed check.
pub fn pack_origin_file_to_mod_version(
    f: &PackOriginFile,
    mc_version: &str,
    loader: crate::mods::platform::LoaderKind,
) -> ModVersion {
    // Emit None for an empty/whitespace sha1 so install_one's sha-guard
    // rejects the file rather than installing an unverifiable archive
    // (no-TOFU, Principle B.6). The pack-origin snapshot is only written
    // for successfully installed files, so an empty sha here is a
    // defensive guard against corrupt snapshot data.
    let sha1 = {
        let s = f.sha1.trim();
        if s.is_empty() {
            None
        } else {
            Some(s.to_string())
        }
    };
    ModVersion {
        source: f.source,
        project_id: f.project_id.clone(),
        version_id: f.version_id.clone(),
        name: f.name.clone(),
        version_number: String::new(),
        mc_versions: vec![mc_version.to_string()],
        loaders: vec![loader],
        primary_file: ModFile {
            filename: f.filename.clone(),
            url: f.url.clone(),
            sha1,
            size: f.size,
            distribution_allowed: true,
            sha256: None,
        },
        deps: vec![],
        published_at: None,
    }
}

/// Pick the pack's display name. The platform project name (Modrinth
/// `title` / CurseForge `name`) is authoritative; the archive's internal
/// `name` field is author-controlled free text — some authors set it to
/// the version string — so it is used only as a fallback when no platform
/// metadata could be fetched. An empty / whitespace-only platform name is
/// treated as "not available".
fn resolve_pack_name(platform_name: Option<&str>, archive_name: &str) -> String {
    match platform_name.map(str::trim) {
        Some(n) if !n.is_empty() => n.to_string(),
        _ => archive_name.to_string(),
    }
}

pub fn resolve_name(desired: &str, existing: &[String]) -> Result<String, Error> {
    if !existing.iter().any(|e| e == desired) {
        return Ok(desired.into());
    }
    for n in 2..=999u32 {
        let candidate = format!("{desired} ({n})");
        if !existing.iter().any(|e| e == &candidate) {
            return Ok(candidate);
        }
    }
    Err(Error::ModpackInstanceCreationFailed {
        details: format!("name collision: 999 suffix attempts exhausted for '{desired}'"),
    })
}

/// Selected files eligible for the concurrent pre-warm pass: sha1-keyed
/// (no `md5`), non-empty sha1, deduplicated by lowercased sha1. Returns
/// `(url, sha1_lower, size)` ready to feed straight to `fetch_to_cache`.
///
/// - **md5 files are excluded** — ATLauncher's `fetch_to_cache_md5` keys the
///   cache on the *computed* sha1 (unknown up front), so it cannot cache-hit
///   and pre-warming would double-download. They stay on the serial path.
/// - **empty-sha files are excluded** — the serial path rejects them anyway
///   (no-TOFU), so there is nothing to warm.
/// - **deduplicated by sha1** so no two concurrent tasks race on the same
///   `<sha>.tmp` (the cache miss-path writes a sha-keyed temp file).
///
/// The lowercasing mirrors `install_one`/`install_asset`, which both
/// `sha.to_ascii_lowercase()` before `fetch_to_cache` — matching the key
/// guarantees the serial loop's fetch is an instant cache hit.
/// Concurrent cache pre-warm for a set of pack files — see the HIGH-5 comment
/// in `install_resolved_pack` for the rationale. Dedups by sha1; md5-keyed and
/// empty-sha files are excluded; errors are swallowed (the caller's serial
/// apply loop stays the single source of truth for per-file success/failure).
/// Shared by fresh import and `modpack_apply_update`.
pub async fn prewarm_cache(
    data_dir: &std::path::Path,
    files: &[&ModpackFile],
    progress: &crate::mods::install::ProgressFn,
) {
    let targets = prewarm_targets(files);
    if targets.is_empty() {
        return;
    }
    stream::iter(targets)
        .map(|(url, sha, size)| async move {
            let _ = fetch_to_cache(data_dir, &url, &sha, size, "modpacks", progress).await;
        })
        .buffer_unordered(MODPACK_PREWARM_CONCURRENCY)
        .collect::<Vec<()>>()
        .await;
}

fn prewarm_targets(selected: &[&ModpackFile]) -> Vec<(String, String, f64)> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for f in selected {
        if f.md5.is_some() {
            continue;
        }
        let sha = f.sha1.trim().to_ascii_lowercase();
        if sha.is_empty() {
            continue;
        }
        if seen.insert(sha.clone()) {
            out.push((f.url.clone(), sha, f.size));
        }
    }
    out
}

/// Shared install pipeline for a pack whose manifest has already been
/// resolved into a `ModpackSummary`. Called by `import()` (Modrinth/CF
/// archive path, passes `archive_bytes = Some(bytes)`) and by the FTB
/// sidecar path in `commands.rs` (passes `archive_bytes = None`).
///
/// `archive_bytes` is `None` for FTB packs — they have no local archive
/// and therefore no overrides to extract; the overrides block is guarded
/// by this option. All other behaviour is identical for every format.
#[allow(clippy::too_many_arguments)]
pub async fn install_resolved_pack(
    app: &tauri::AppHandle,
    summary: ModpackSummary,
    selected_shas: &[String],
    apply_overrides: bool,
    archive_bytes: Option<&[u8]>, // None for FTB — no overrides
    cf_base: &str,
    // Browse-flow hints. When the import was kicked off from the
    // Modpacks → Browse sub-tab the UI already knows the source
    // platform and the pack's project_id (it came from a `ModpackHit`),
    // so we pass them through and skip the version→project lookup.
    // Drag-drop imports pass `None` here and we auto-look-up below.
    hint_project_id: Option<String>,
    hint_source: Option<crate::mods::platform::ModSource>,
    hint_version_id: Option<String>,
    // `Send + Sync` so the resulting future is `Send` — required by
    // the Tauri command boundary in `commands::modpack_import`. The
    // wrapped channel closure already satisfies these bounds.
    on_progress: &(dyn Fn(ModpackProgress) + Send + Sync),
    install_progress: ProgressFn,
) -> Result<crate::instances::schema::InstanceWithStatus, Error> {
    if selected_shas.is_empty() {
        return Err(Error::ModpackNoFilesSelected);
    }

    // Best-effort project-metadata fetch. The .mrpack / .zip archive
    // carries an author-controlled `name` field — some authors set it to
    // a version string — so we prefer the platform's canonical project
    // name. We also need the pack's project_id + a short description for
    // the Imported drawer. Failures are silent: fields stay None and we
    // fall back to the archive name.
    //
    // Two paths:
    //   * Browse-flow gave us the project_id already (`hint_project_id`)
    //     — fetch the project record directly.
    //   * Drag-drop or no hints — do the version→project lookup first.
    //
    // CurseForge drag-drop: no resolvable project_id, so no backfill —
    // `pack_meta` stays default and the archive name is used.
    let parser_source = match summary.format {
        ModpackFormat::Modrinth => Some(crate::mods::platform::ModSource::Modrinth),
        ModpackFormat::Curseforge => Some(crate::mods::platform::ModSource::Curseforge),
        // FTB: pack-managed source — provenance is Ftb.
        ModpackFormat::Ftb => Some(crate::mods::platform::ModSource::Ftb),
        // ATLauncher: pack-managed source — provenance is Atlauncher.
        ModpackFormat::Atlauncher => Some(crate::mods::platform::ModSource::Atlauncher),
    };
    let (mrpack_project_id, pack_meta, mrpack_source) =
        match (hint_project_id.as_deref(), hint_source, summary.format) {
            (
                Some(pid),
                Some(crate::mods::platform::ModSource::Modrinth),
                ModpackFormat::Modrinth,
            ) => {
                let meta = fetch_modrinth_project(pid).await.unwrap_or_default();
                (Some(pid.to_string()), meta, parser_source)
            }
            (
                Some(pid),
                Some(crate::mods::platform::ModSource::Curseforge),
                ModpackFormat::Curseforge,
            ) => {
                // Browse-flow CF import: backfill the pack's project name +
                // short summary. Best-effort — failure keeps them None.
                let key = crate::mods::curseforge::keyring::resolve();
                let (cf_name, cf_summary) =
                    crate::mods::modpack::cf_api::fetch_summary(cf_base, key.as_deref(), pid)
                        .await
                        .unwrap_or((None, None));
                (
                    Some(pid.to_string()),
                    PackMeta {
                        name: cf_name,
                        description: cf_summary,
                    },
                    parser_source,
                )
            }
            (_, _, ModpackFormat::Modrinth) => {
                let (pid, meta) = fetch_modrinth_metadata(&summary.version)
                    .await
                    .unwrap_or((None, PackMeta::default()));
                (pid, meta, parser_source)
            }
            (_, _, ModpackFormat::Curseforge) => (None, PackMeta::default(), parser_source),
            // FTB: pack-managed source — project metadata is fetched by the
            // FtbModpackSource adapter before import; nothing to back-fill here.
            (_, _, ModpackFormat::Ftb) => (None, PackMeta::default(), parser_source),
            // ATLauncher: pack-managed source — project metadata is fetched by the
            // ATLauncher adapter before import; nothing to back-fill here.
            (_, _, ModpackFormat::Atlauncher) => (None, PackMeta::default(), parser_source),
        };
    let PackMeta {
        name: platform_name,
        description: mrpack_summary,
    } = pack_meta;
    let pack_name = resolve_pack_name(platform_name.as_deref(), &summary.name);

    let existing: Vec<String> = crate::instances::list_instances_with_status(app)?
        .into_iter()
        .map(|i| i.name)
        .collect();
    let final_name = resolve_name(&pack_name, &existing)?;

    on_progress(ModpackProgress::CreatingInstance {
        name: final_name.clone(),
    });
    // Keep a clone for the pack_origin snapshot we write later — the
    // value gets moved into `create_instance` below.
    let mrpack_project_id_for_origin = mrpack_project_id.clone();
    let inst = crate::instances::create_instance(
        app,
        final_name,
        summary.game_version.clone(),
        summary.loader,
        summary.loader_version.clone(),
        Some((pack_name.clone(), summary.version.clone())),
        mrpack_project_id,
        mrpack_source,
        mrpack_summary,
        hint_version_id,
        None,
        None,
    )
    .map_err(|e| Error::ModpackInstanceCreationFailed {
        details: e.to_string(),
    })?;

    let instance_root = crate::paths::instance_dir(app, &inst.id).map_err(|e| Error::Io {
        path: "<instance_dir>".into(),
        details: e.to_string(),
    })?;
    let data_dir = crate::paths::app_dir(app).map_err(|e| Error::Io {
        path: "<app_dir>".into(),
        details: e.to_string(),
    })?;

    let selected: Vec<&ModpackFile> = summary
        .files
        .iter()
        .filter(|f| {
            selected_shas
                .iter()
                .any(|s| s.eq_ignore_ascii_case(&f.sha1))
        })
        .collect();
    let total = selected.len() as u32;
    let mut failures: Vec<(String, String)> = vec![];
    // Accumulate files with their REAL sha1s for the origin snapshot.
    // ATLauncher md5 files carry md5-in-sha1 as a transient selection token;
    // the pre-resolve step below replaces it with the real computed sha1.
    // Non-md5 files are pushed as-is (the selection token IS the real sha1).
    let mut resolved_for_origin: Vec<ModpackFile> = Vec::with_capacity(selected.len());

    // Concurrent pre-warm (HIGH-5): download every sha1-keyed selected file
    // into the shared content cache, up to MODPACK_PREWARM_CONCURRENCY at a
    // time, BEFORE the serial install loop below. The loop is otherwise
    // unchanged — each per-file `fetch_to_cache` then becomes an instant cache
    // hit, so total wall-clock collapses from sum(per-file network latency) to
    // roughly one concurrent batch of network time plus the sum of local
    // copies. Errors are deliberately swallowed here: the serial loop stays the
    // single source of truth for per-file success/failure, ordering, and the
    // origin snapshot — a pre-warm miss just means that file's serial fetch
    // re-attempts (and fails identically). The install-time guards (filename
    // safety, distribution, path-escape) also stay in the serial loop;
    // pre-warm only populates the SHA-verified, content-addressed cache, never
    // the instance. (ATLauncher md5 files are excluded — see `prewarm_targets`.)
    prewarm_cache(&data_dir, &selected, &install_progress).await;

    for (idx, file) in selected.iter().enumerate() {
        on_progress(ModpackProgress::InstallingFile {
            current: idx as u32 + 1,
            total,
            file_name: file.name.clone(),
        });

        // ATLauncher md5 files: the summary's sha1 holds the md5 (selection token).
        // Pre-resolve the real sha1 by downloading + md5-verifying into the
        // sha1-keyed cache, then run the normal install path on a clone whose
        // sha1 is the real one (guaranteed cache hit — no second download).
        let resolved_owned;
        let file: &ModpackFile = if let Some(md5) = &file.md5 {
            match crate::mods::install::fetch_to_cache_md5(
                &data_dir,
                &file.url,
                md5,
                file.size,
                "modpacks",
                &install_progress,
            )
            .await
            {
                Ok((_, real_sha1)) => {
                    let mut c = (*file).clone();
                    c.sha1 = real_sha1;
                    c.md5 = None;
                    resolved_owned = c;
                    &resolved_owned
                }
                Err(e) => {
                    failures.push((file.install_path.clone(), e.to_string()));
                    continue;
                }
            }
        } else {
            file
        };

        let res = if file.install_path.starts_with("mods/") {
            let mv = modpack_file_to_mod_version(file, &summary.game_version, summary.loader);
            install_one(&data_dir, &instance_root, mv, &install_progress)
                .await
                .map(|_| ())
        } else {
            let r = install_asset(
                &data_dir,
                &instance_root,
                &file.url,
                &file.sha1,
                file.size,
                &file.install_path,
                &install_progress,
            )
            .await;
            if r.is_ok() {
                register_asset_if_applicable(
                    &instance_root,
                    &file.install_path,
                    &file.filename,
                    &file.sha1,
                    &file.name,
                    Some(file.source),
                    (!file.project_id.is_empty()).then(|| file.project_id.clone()),
                    (!file.version_id.is_empty()).then(|| file.version_id.clone()),
                )
                .await;
            }
            r
        };
        if let Err(e) = &res {
            failures.push((file.install_path.clone(), e.to_string()));
        }
        // Record in the pack origin regardless of install outcome: a file that
        // failed to install must still appear in the snapshot so it surfaces as
        // `removed_files` (with a Restore affordance), matching pre-97e2bd3
        // behaviour. (ATL md5 files whose md5-fetch failed already `continue`d
        // above and are intentionally excluded — we never persist an md5 token.)
        resolved_for_origin.push((*file).clone());
    }

    // md5-in-sha1 selection tokens must never reach the persisted PackOrigin.
    debug_assert!(
        resolved_for_origin.iter().all(|f| f.md5.is_none()),
        "md5-in-sha1 selection token must be resolved to a real sha1 before PackOrigin"
    );

    // Bundled assets from overrides/ (mods/*.jar plus top-level
    // resourcepacks/ and shaderpacks/ files) are tracked here so the
    // origin snapshot below captures them. Without this, the drawer's
    // scan-reconcile-driven InstalledMod entries land with source=None
    // and fall into the "manual" badge even though the bytes came
    // straight from the pack archive.
    // FTB packs have no archive and therefore no overrides — `archive_bytes`
    // is `None` for that path and the block is skipped entirely.
    let mut bundled_assets: Vec<crate::mods::modpack::overrides::ExtractedAsset> = vec![];
    // Oversized `overrides/` blobs the extractor skipped (e.g. a `.rar` left
    // in `mods/`). Surfaced informationally — the import still succeeds.
    let mut skipped_overrides: Vec<crate::mods::modpack::schema::SkippedOverride> = vec![];
    if apply_overrides && (summary.has_overrides || summary.has_client_overrides) {
        if let Some(bytes) = archive_bytes {
            let bytes_clone = bytes.to_vec();
            let outcome = overrides::extract(&bytes_clone, &instance_root, |c, t| {
                on_progress(ModpackProgress::ExtractingOverrides {
                    current: c,
                    total: t,
                });
            })
            .await?;
            bundled_assets = outcome.extracted;
            skipped_overrides = outcome.skipped;
        }
    }

    // Persist the origin snapshot. Best-effort — the import itself
    // is already done at this point; a write failure here only loses
    // the modified/restore affordance, not any installed mod or
    // instance. Log and continue.
    // Use `resolved_for_origin` (real sha1s, md5 cleared) instead of `selected`
    // so ATLauncher md5 files are recorded with their real sha1, not the
    // transient md5-as-selection-token.
    let resolved_refs: Vec<&ModpackFile> = resolved_for_origin.iter().collect();
    let mut origin = build_pack_origin(
        &summary,
        &resolved_refs,
        mrpack_project_id_for_origin,
        &pack_name,
    );
    // Fold bundled-from-overrides jars into the origin so they badge
    // as "pack" in the drawer. `url` stays empty (bundled bytes have
    // no remote source) — the Restore path checks for this and
    // returns a typed error instead of trying to install_one a no-URL
    // entry.
    let bundled_source = origin.source;
    for m in &bundled_assets {
        origin.files.push(crate::mods::installed::PackOriginFile {
            sha1: m.sha1.clone(),
            name: m
                .filename
                .trim_end_matches(".jar")
                .trim_end_matches(".disabled")
                .to_string(),
            filename: m.filename.clone(),
            install_path: m.install_path.clone(),
            url: String::new(),
            size: m.size as f64,
            project_id: String::new(),
            version_id: String::new(),
            env_client: crate::mods::modpack::schema::EnvSupport::Required,
            source: bundled_source,
        });
    }
    // Bundled resource packs / shaders from overrides/ also belong in the
    // assets registry so they show under Add-ons → Installed. Bundled mods and
    // other files are skipped (content_kind_for_install_path returns None).
    for m in &bundled_assets {
        register_asset_if_applicable(
            &instance_root,
            &m.install_path,
            &m.filename,
            &m.sha1,
            &m.filename,
            None,
            None,
            None,
        )
        .await;
    }
    // Scan the freshly-installed mods folder for jars built for a loader
    // family this instance cannot load (inert — e.g. a Fabric jar on a Forge
    // instance). Best-effort and non-fatal: the import already succeeded; this
    // is surfaced for transparency only. Loader-family only, so it never
    // false-positives on a bundled multi-loader or descriptor-less jar.
    let mods_dir = instance_root.join(".minecraft").join("mods");
    // Reads + zip-parses every jar in `mods/` — a full pass over the pack, so
    // run it on a blocking thread instead of the async runtime. Join failure
    // degrades to "no inert jars found" (the scan is best-effort anyway).
    let inert_loader_jars = {
        let mods_dir = mods_dir.clone();
        let loader = summary.loader;
        let game_version = summary.game_version.clone();
        tokio::task::spawn_blocking(move || {
            classify_inert_loader_jars(&mods_dir, loader, &game_version)
        })
        .await
        .unwrap_or_default()
    };

    // Record the skipped oversized overrides so the Imported drawer can
    // show the informational "skipped" note after a restart (the Done
    // event below only reaches the live import toast).
    origin.skipped_overrides = skipped_overrides.clone();
    origin.inert_loader_jars = inert_loader_jars.clone();
    if let Err(e) = crate::mods::installed::set_pack_origin(&instance_root, origin).await {
        crate::diag!("[modpack::import] set_pack_origin failed (non-fatal): {e}");
    }

    // Final phase: hash-enrich the override-bundled mods so the
    // Installed view shows their icons on first open. Best-effort — the
    // import (instance + mods on disk) is already complete; a failure
    // here only delays enrichment to the Installed-view backfill.
    on_progress(ModpackProgress::Enriching);
    let cf_key = crate::mods::curseforge::keyring::resolve();
    if let Err(e) = crate::mods::enrich::enrich_instance(
        &instance_root,
        "https://api.modrinth.com",
        cf_base,
        cf_key.as_deref(),
    )
    .await
    {
        crate::diag!("[modpack::import] enrich_instance failed (non-fatal): {e}");
    }

    on_progress(ModpackProgress::Done {
        instance_id: inst.id.clone(),
        skipped_overrides,
        inert_loader_jars,
    });

    if failures.is_empty() {
        Ok(inst)
    } else {
        Err(Error::ModpackPartialFailure {
            instance_id: inst.id,
            failed: failures,
        })
    }
}

/// Thin wrapper: inspect the archive bytes into a `ModpackSummary`, then
/// delegate to `install_resolved_pack`. Exists for the Modrinth / CurseForge
/// archive (`.mrpack` / `.zip`) import path. FTB imports bypass this and
/// call `install_resolved_pack` directly via the sidecar branch in
/// `commands::modpack_import`.
#[allow(clippy::too_many_arguments)]
pub async fn import(
    app: &tauri::AppHandle,
    bytes: &[u8],
    selected_shas: &[String],
    apply_overrides: bool,
    cf_base: &str,
    hint_project_id: Option<String>,
    hint_source: Option<crate::mods::platform::ModSource>,
    hint_version_id: Option<String>,
    on_progress: &(dyn Fn(ModpackProgress) + Send + Sync),
    install_progress: ProgressFn,
) -> Result<crate::instances::schema::InstanceWithStatus, Error> {
    on_progress(ModpackProgress::Inspecting);
    let summary = inspect(bytes, cf_base).await?;
    install_resolved_pack(
        app,
        summary,
        selected_shas,
        apply_overrides,
        Some(bytes),
        cf_base,
        hint_project_id,
        hint_source,
        hint_version_id,
        on_progress,
        install_progress,
    )
    .await
}

/// Best-effort project metadata backfilled from the mod platform.
/// `name` is the platform's canonical project name (Modrinth `title` /
/// CurseForge `name`) — used to name the instance. `description` is the
/// short blurb shown in the Imported drawer. Both are `Option` because
/// each fetch is best-effort: a network failure or non-2xx response
/// leaves them `None`.
#[derive(Default)]
struct PackMeta {
    name: Option<String>,
    description: Option<String>,
}

/// Modrinth `version_id` → `(project_id, PackMeta)`. Two hops:
///   1. `GET /v2/version/{version_id}` → `project_id`.
///   2. `GET /v2/project/{project_id}` → `(title, description)`.
///
/// The eat-the-error-and-return-default pattern (rather than propagating)
/// is intentional — metadata is nice-to-have, not blocking. Callers use
/// `.unwrap_or((None, PackMeta::default()))` so a 404 or transient network
/// failure can never abort an otherwise-successful import.
async fn fetch_modrinth_metadata(version_id: &str) -> Result<(Option<String>, PackMeta), Error> {
    #[derive(serde::Deserialize)]
    struct V {
        project_id: String,
    }

    let v_url = format!("https://api.modrinth.com/v2/version/{version_id}");
    let v_resp = crate::network::request::get(
        &v_url,
        &[("user-agent", "AntonBabchenko/Lucerna")],
        "modpacks",
    )
    .await
    .map_err(|e| Error::mods_network(v_url.clone(), e))?;
    if !(200..300).contains(&v_resp.status) {
        return Ok((None, PackMeta::default()));
    }
    let v: V = serde_json::from_slice(&v_resp.body).map_err(|e| Error::ModsDecode {
        platform: "modrinth".into(),
        details: e.to_string(),
    })?;
    let project_id = v.project_id;
    let meta = fetch_modrinth_project(&project_id).await?;
    Ok((Some(project_id), meta))
}

/// Modrinth `project_id` → project `(title, description)` as a `PackMeta`.
/// Used directly when the project_id is already known (browse-flow hint),
/// and as the second hop of `fetch_modrinth_metadata`. Same
/// silent-on-failure semantics — a non-2xx response yields a default
/// (all-`None`) `PackMeta`.
async fn fetch_modrinth_project(project_id: &str) -> Result<PackMeta, Error> {
    #[derive(serde::Deserialize)]
    struct P {
        title: Option<String>,
        description: Option<String>,
    }

    let p_url = format!("https://api.modrinth.com/v2/project/{project_id}");
    let p_resp = crate::network::request::get(
        &p_url,
        &[("user-agent", "AntonBabchenko/Lucerna")],
        "modpacks",
    )
    .await
    .map_err(|e| Error::mods_network(p_url.clone(), e))?;
    if !(200..300).contains(&p_resp.status) {
        return Ok(PackMeta::default());
    }
    let p: P = serde_json::from_slice(&p_resp.body).map_err(|e| Error::ModsDecode {
        platform: "modrinth".into(),
        details: e.to_string(),
    })?;
    Ok(PackMeta {
        name: p.title,
        description: p.description,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn register_asset_tracks_resourcepacks_skips_mods() {
        use crate::mods::platform::{ContentKind, ModSource};
        let td = tempfile::tempdir().unwrap();
        let root = td.path();

        // A resource pack → tracked.
        register_asset_if_applicable(
            root,
            "resourcepacks/Faithful.zip",
            "Faithful.zip",
            "AABB",
            "Faithful",
            Some(ModSource::Modrinth),
            Some("pid".into()),
            Some("vid".into()),
        )
        .await;
        // A mod → ignored.
        register_asset_if_applicable(
            root,
            "mods/sodium.jar",
            "sodium.jar",
            "CCDD",
            "Sodium",
            Some(ModSource::Modrinth),
            None,
            None,
        )
        .await;

        let rps = crate::mods::assets::list(root, ContentKind::ResourcePack)
            .await
            .unwrap();
        assert_eq!(rps.len(), 1);
        assert_eq!(rps[0].filename, "Faithful.zip");
        assert_eq!(rps[0].sha1, "aabb");
        assert_eq!(rps[0].project_id.as_deref(), Some("pid"));
        // No mod leaked into the asset registry.
        assert!(crate::mods::assets::list(root, ContentKind::Shader)
            .await
            .unwrap()
            .is_empty());
    }

    #[test]
    fn resolve_name_returns_desired_when_free() {
        assert_eq!(resolve_name("Pack", &[]).unwrap(), "Pack");
    }

    #[test]
    fn resolve_name_suffixes_on_collision() {
        let existing = vec!["Pack".to_string()];
        assert_eq!(resolve_name("Pack", &existing).unwrap(), "Pack (2)");
    }

    #[test]
    fn resolve_name_walks_until_free() {
        let existing = vec![
            "Pack".to_string(),
            "Pack (2)".to_string(),
            "Pack (3)".to_string(),
        ];
        assert_eq!(resolve_name("Pack", &existing).unwrap(), "Pack (4)");
    }

    #[test]
    fn resolve_name_errors_when_999_exhausted() {
        let mut existing = vec!["Pack".to_string()];
        for i in 2..=999 {
            existing.push(format!("Pack ({i})"));
        }
        assert!(matches!(
            resolve_name("Pack", &existing),
            Err(Error::ModpackInstanceCreationFailed { .. })
        ));
    }

    #[test]
    fn resolve_pack_name_prefers_platform_name() {
        assert_eq!(
            resolve_pack_name(Some("Sodium Plus"), "2.3.7"),
            "Sodium Plus"
        );
    }

    #[test]
    fn resolve_pack_name_falls_back_when_platform_name_absent() {
        assert_eq!(resolve_pack_name(None, "2.3.7"), "2.3.7");
    }

    #[test]
    fn resolve_pack_name_falls_back_when_platform_name_blank() {
        assert_eq!(resolve_pack_name(Some(""), "Cool Pack"), "Cool Pack");
        assert_eq!(resolve_pack_name(Some("   "), "Cool Pack"), "Cool Pack");
    }

    #[test]
    fn resolve_pack_name_trims_platform_name() {
        assert_eq!(
            resolve_pack_name(Some("  Sodium Plus  "), "2.3.7"),
            "Sodium Plus"
        );
    }

    fn sample_summary(format: ModpackFormat) -> ModpackSummary {
        ModpackSummary {
            format,
            name: "Cool Pack".into(),
            version: "1.2.3".into(),
            game_version: "1.20.1".into(),
            loader: crate::mods::platform::LoaderKind::Fabric,
            loader_version: Some("0.15.7".into()),
            files: vec![],
            unresolvable: vec![],
            has_overrides: false,
            has_client_overrides: false,
            has_saves_in_overrides: false,
        }
    }

    fn sample_file(sha: &str) -> ModpackFile {
        ModpackFile {
            project_id: "proj-".to_string() + sha,
            version_id: "ver-".to_string() + sha,
            name: "Mod ".to_string() + sha,
            filename: format!("{sha}.jar"),
            install_path: format!("mods/{sha}.jar"),
            sha1: sha.into(),
            md5: None,
            url: format!("https://example.com/{sha}.jar"),
            size: 42.0,
            env_client: EnvSupport::Required,
            source: ModSource::Modrinth,
        }
    }

    #[test]
    fn prewarm_targets_dedups_lowercases_and_excludes_md5_and_empty() {
        let a = sample_file("aaa");
        let a_dup = sample_file("aaa"); // same sha1 — must dedup to one entry
        let a_upper = sample_file("AAA"); // same content id after lowercasing — dedup
        let b = sample_file("bbb");
        let mut md5_file = sample_file("ccc");
        md5_file.md5 = Some("deadbeef".into()); // ATLauncher md5 — excluded
        let mut empty = sample_file("ddd");
        empty.sha1 = "   ".into(); // whitespace-only — excluded (no-TOFU)

        let selected: Vec<&ModpackFile> = vec![&a, &a_dup, &a_upper, &b, &md5_file, &empty];
        let targets = prewarm_targets(&selected);

        // Surviving content ids: "aaa" (once, lowercased) + "bbb".
        let shas: Vec<&str> = targets.iter().map(|(_, s, _)| s.as_str()).collect();
        assert_eq!(targets.len(), 2, "got {shas:?}");
        assert!(shas.contains(&"aaa"));
        assert!(shas.contains(&"bbb"));
        assert!(!shas.contains(&"ccc"), "md5 files must be excluded");
        assert!(
            !shas.contains(&"AAA"),
            "sha must be lowercased before dedup"
        );

        // url + size are carried through for a surviving target.
        let (url, _, size) = targets.iter().find(|(_, s, _)| s == "bbb").unwrap();
        assert_eq!(url, "https://example.com/bbb.jar");
        assert_eq!(*size, 42.0);
    }

    #[test]
    fn build_pack_origin_marks_ftb_source() {
        let summary = sample_summary(ModpackFormat::Ftb);
        let f = sample_file("ddd");
        let origin = build_pack_origin(&summary, &[&f], Some("91".into()), &summary.name);
        assert_eq!(origin.source, ModSource::Ftb);
    }

    #[test]
    fn build_pack_origin_captures_selected_files() {
        let summary = sample_summary(ModpackFormat::Modrinth);
        let f1 = sample_file("aaa");
        let f2 = sample_file("bbb");
        let selected = vec![&f1, &f2];
        let origin = build_pack_origin(&summary, &selected, Some("ABC123".into()), &summary.name);
        assert_eq!(origin.project_name, "Cool Pack");
        assert_eq!(origin.version, "1.2.3");
        assert_eq!(origin.project_id.as_deref(), Some("ABC123"));
        assert_eq!(origin.source, ModSource::Modrinth);
        assert_eq!(origin.files.len(), 2);
        assert_eq!(origin.files[0].sha1, "aaa");
        assert_eq!(origin.files[0].name, "Mod aaa");
        assert_eq!(origin.files[0].url, "https://example.com/aaa.jar");
        assert_eq!(origin.files[1].sha1, "bbb");
    }

    #[test]
    fn build_pack_origin_marks_curseforge_source() {
        let summary = sample_summary(ModpackFormat::Curseforge);
        let f = sample_file("ccc");
        let origin = build_pack_origin(&summary, &[&f], None, &summary.name);
        assert_eq!(origin.source, ModSource::Curseforge);
        assert!(origin.project_id.is_none());
    }

    #[test]
    fn build_pack_origin_with_no_selected_files_yields_empty_files_vec() {
        let summary = sample_summary(ModpackFormat::Modrinth);
        let origin = build_pack_origin(&summary, &[], None, &summary.name);
        assert!(origin.files.is_empty());
        assert_eq!(origin.project_name, "Cool Pack");
    }

    #[test]
    fn build_pack_origin_uses_pack_name_not_summary_name() {
        // summary.name is "Cool Pack" (see sample_summary); the explicit
        // pack_name argument must win.
        let summary = sample_summary(ModpackFormat::Modrinth);
        let f = sample_file("aaa");
        let origin = build_pack_origin(&summary, &[&f], None, "Sodium Plus");
        assert_eq!(origin.project_name, "Sodium Plus");
    }

    fn pack_file(sha: &str) -> PackOriginFile {
        PackOriginFile {
            sha1: sha.into(),
            name: format!("Mod {sha}"),
            filename: format!("{sha}.jar"),
            install_path: format!("mods/{sha}.jar"),
            url: format!("https://example.com/{sha}.jar"),
            size: 1.0,
            project_id: format!("p-{sha}"),
            version_id: format!("v-{sha}"),
            env_client: EnvSupport::Required,
            source: ModSource::Modrinth,
        }
    }

    fn installed(sha: &str, with_source: bool) -> crate::mods::platform::InstalledMod {
        crate::mods::platform::InstalledMod {
            filename: format!("{sha}.jar"),
            sha1: sha.into(),
            source: if with_source {
                Some(ModSource::Modrinth)
            } else {
                None
            },
            project_id: if with_source {
                Some(format!("p-{sha}"))
            } else {
                None
            },
            version_id: if with_source {
                Some(format!("v-{sha}"))
            } else {
                None
            },
            name: format!("Mod {sha}"),
            version_number: None,
            installed_at: chrono::Utc::now().to_rfc3339(),
            enabled: true,
            enrich_attempted: false,
            requires: Vec::new(),
        }
    }

    #[test]
    fn compute_status_clean_when_origin_matches_installed_exactly() {
        let origin = PackOrigin {
            project_id: None,
            source: ModSource::Modrinth,
            project_name: "P".into(),
            version: "1".into(),
            files: vec![pack_file("a"), pack_file("b")],
            missing_mods: vec![],
            skipped_overrides: vec![],
            resolved_missing: Vec::new(),
            inert_loader_jars: vec![],
        };
        let installed = vec![installed("a", true), installed("b", true)];
        let s = compute_status(origin, &installed, &std::collections::HashSet::new());
        assert!(!s.is_modified);
        assert_eq!(s.added_count, 0);
        assert!(s.removed_files.is_empty());
        assert_eq!(s.installed_shas.len(), 2);
    }

    #[test]
    fn compute_status_flags_removed_file_when_pack_sha_missing() {
        let origin = PackOrigin {
            project_id: None,
            source: ModSource::Modrinth,
            project_name: "P".into(),
            version: "1".into(),
            files: vec![pack_file("a"), pack_file("b")],
            missing_mods: vec![],
            skipped_overrides: vec![],
            resolved_missing: Vec::new(),
            inert_loader_jars: vec![],
        };
        // "b" no longer installed.
        let installed = vec![installed("a", true)];
        let s = compute_status(origin, &installed, &std::collections::HashSet::new());
        assert!(s.is_modified);
        assert_eq!(s.removed_files.len(), 1);
        assert_eq!(s.removed_files[0].sha1, "b");
        assert_eq!(s.added_count, 0);
    }

    #[test]
    fn compute_status_flags_added_count_when_installed_sha_not_in_origin() {
        let origin = PackOrigin {
            project_id: None,
            source: ModSource::Modrinth,
            project_name: "P".into(),
            version: "1".into(),
            files: vec![pack_file("a")],
            missing_mods: vec![],
            skipped_overrides: vec![],
            resolved_missing: Vec::new(),
            inert_loader_jars: vec![],
        };
        // User added "z" manually.
        let installed = vec![installed("a", true), installed("z", false)];
        let s = compute_status(origin, &installed, &std::collections::HashSet::new());
        assert!(s.is_modified);
        assert!(s.removed_files.is_empty());
        assert_eq!(s.added_count, 1);
    }

    #[test]
    fn compute_status_asset_present_when_on_disk() {
        let mut rp = pack_file("rp1");
        rp.install_path = "resourcepacks/RP.zip".into();
        let origin = PackOrigin {
            project_id: None,
            source: ModSource::Modrinth,
            project_name: "P".into(),
            version: "1".into(),
            files: vec![pack_file("a"), rp],
            missing_mods: vec![],
            skipped_overrides: vec![],
            resolved_missing: Vec::new(),
            inert_loader_jars: vec![],
        };
        let installed = vec![installed("a", true)];
        let present: std::collections::HashSet<String> =
            ["resourcepacks/RP.zip".to_string()].into_iter().collect();
        let s = compute_status(origin, &installed, &present);
        assert!(!s.is_modified);
        assert!(s.removed_files.is_empty());
    }

    #[test]
    fn compute_status_asset_removed_when_absent_from_disk() {
        let mut rp = pack_file("rp1");
        rp.install_path = "resourcepacks/RP.zip".into();
        let origin = PackOrigin {
            project_id: None,
            source: ModSource::Modrinth,
            project_name: "P".into(),
            version: "1".into(),
            files: vec![pack_file("a"), rp],
            missing_mods: vec![],
            skipped_overrides: vec![],
            resolved_missing: Vec::new(),
            inert_loader_jars: vec![],
        };
        let installed = vec![installed("a", true)];
        let s = compute_status(origin, &installed, &std::collections::HashSet::new());
        assert!(s.is_modified);
        assert_eq!(s.removed_files.len(), 1);
        assert_eq!(s.removed_files[0].install_path, "resourcepacks/RP.zip");
    }

    #[test]
    fn compute_status_sha_comparison_is_case_insensitive() {
        let mut f = pack_file("ABC");
        f.sha1 = "ABC".into();
        let origin = PackOrigin {
            project_id: None,
            source: ModSource::Modrinth,
            project_name: "P".into(),
            version: "1".into(),
            files: vec![f],
            missing_mods: vec![],
            skipped_overrides: vec![],
            resolved_missing: Vec::new(),
            inert_loader_jars: vec![],
        };
        let mut m = installed("ABC", true);
        m.sha1 = "abc".into();
        let s = compute_status(origin, &[m], &std::collections::HashSet::new());
        assert!(!s.is_modified);
    }

    #[test]
    fn modpack_file_to_mod_version_empty_sha1_yields_none() {
        // An empty sha1 in a ModpackFile must produce None in primary_file.sha1
        // so that install_one's sha guard (ok_or(ModsSha1Unavailable)) rejects it.
        let mut f = mp_file("proj", "");
        f.sha1 = String::new();
        let v =
            modpack_file_to_mod_version(&f, "1.20.1", crate::mods::platform::LoaderKind::Fabric);
        assert!(
            v.primary_file.sha1.is_none(),
            "empty sha1 must become None, not Some(\"\")"
        );
    }

    #[test]
    fn modpack_file_to_mod_version_whitespace_sha1_yields_none() {
        let mut f = mp_file("proj", "   ");
        f.sha1 = "   ".to_string();
        let v =
            modpack_file_to_mod_version(&f, "1.20.1", crate::mods::platform::LoaderKind::Fabric);
        assert!(
            v.primary_file.sha1.is_none(),
            "whitespace sha1 must become None"
        );
    }

    #[test]
    fn pack_origin_file_to_mod_version_empty_sha1_yields_none() {
        // Same guard for the restore path.
        let mut f = pack_file("aaa");
        f.sha1 = String::new();
        let v = pack_origin_file_to_mod_version(
            &f,
            "1.20.1",
            crate::mods::platform::LoaderKind::Fabric,
        );
        assert!(
            v.primary_file.sha1.is_none(),
            "empty sha1 in PackOriginFile must produce None"
        );
    }

    #[test]
    fn pack_origin_file_to_mod_version_round_trip() {
        let f = pack_file("aaa");
        let v = pack_origin_file_to_mod_version(
            &f,
            "1.20.1",
            crate::mods::platform::LoaderKind::Fabric,
        );
        assert_eq!(v.source, ModSource::Modrinth);
        assert_eq!(v.project_id, "p-aaa");
        assert_eq!(v.version_id, "v-aaa");
        assert_eq!(v.primary_file.filename, "aaa.jar");
        assert_eq!(v.primary_file.url, "https://example.com/aaa.jar");
        assert_eq!(v.primary_file.sha1.as_deref(), Some("aaa"));
        assert!(v.primary_file.distribution_allowed); // see synthesis comment
        assert_eq!(v.mc_versions, vec!["1.20.1"]);
        assert_eq!(v.loaders, vec![crate::mods::platform::LoaderKind::Fabric]);
    }

    fn mp_file(project: &str, sha: &str) -> ModpackFile {
        ModpackFile {
            project_id: project.into(),
            version_id: format!("v-{sha}"),
            name: format!("Mod {project}"),
            filename: format!("{project}.jar"),
            install_path: format!("mods/{project}.jar"),
            sha1: sha.into(),
            md5: None,
            url: format!("https://cdn.modrinth.com/data/{project}/x/{project}.jar"),
            size: 1.0,
            env_client: EnvSupport::Required,
            source: ModSource::Modrinth,
        }
    }

    fn origin_with(files: Vec<PackOriginFile>) -> PackOrigin {
        PackOrigin {
            project_id: Some("PACK".into()),
            source: ModSource::Modrinth,
            project_name: "P".into(),
            version: "1.0".into(),
            files,
            missing_mods: vec![],
            skipped_overrides: vec![],
            resolved_missing: Vec::new(),
            inert_loader_jars: vec![],
        }
    }

    #[test]
    fn diff_classifies_added_removed_updated_unchanged() {
        let mut a_old = pack_file("aaa");
        a_old.project_id = "A".into();
        let mut b_old = pack_file("bbb");
        b_old.project_id = "B".into();
        let mut c_old = pack_file("ccc");
        c_old.project_id = "C".into();
        let origin = origin_with(vec![a_old, b_old, c_old]);
        let new_files = vec![
            mp_file("A", "aaa"),
            mp_file("B", "bbb-2"),
            mp_file("D", "ddd"),
        ];
        let summary = ModpackSummary {
            files: new_files,
            ..sample_summary(ModpackFormat::Modrinth)
        };
        let diff = compute_update_diff(
            &summary,
            &origin,
            "1.20.1",
            crate::mods::platform::LoaderKind::Fabric,
            &Some("0.15.7".into()),
        );
        assert_eq!(diff.added.len(), 1);
        assert_eq!(diff.added[0].project_id, "D");
        assert_eq!(diff.removed.len(), 1);
        assert_eq!(diff.removed[0].project_id, "C");
        assert_eq!(diff.updated.len(), 1);
        assert_eq!(diff.updated[0].new.project_id, "B");
        assert!(diff.version_bump.is_none());
    }

    #[test]
    fn diff_excludes_bundled_no_url_entries() {
        let mut bundled = pack_file("bun");
        bundled.project_id = String::new();
        bundled.url = String::new();
        let origin = origin_with(vec![bundled]);
        let summary = ModpackSummary {
            files: vec![],
            ..sample_summary(ModpackFormat::Modrinth)
        };
        let diff = compute_update_diff(
            &summary,
            &origin,
            "1.20.1",
            crate::mods::platform::LoaderKind::Fabric,
            &Some("0.15.7".into()),
        );
        assert!(
            diff.removed.is_empty(),
            "bundled entry must not be in removed"
        );
    }

    #[test]
    fn diff_detects_version_bump() {
        let origin = origin_with(vec![]);
        let mut summary = ModpackSummary {
            files: vec![],
            ..sample_summary(ModpackFormat::Modrinth)
        };
        summary.game_version = "1.20.4".into();
        let diff = compute_update_diff(
            &summary,
            &origin,
            "1.20.1",
            crate::mods::platform::LoaderKind::Fabric,
            &Some("0.15.7".into()),
        );
        let bump = diff.version_bump.expect("expected a version bump");
        assert_eq!(bump.old_game_version, "1.20.1");
        assert_eq!(bump.new_game_version, "1.20.4");
    }

    #[test]
    fn pack_origin_lookup_finds_file_by_lowercase_sha() {
        let origin = PackOrigin {
            project_id: None,
            source: ModSource::Modrinth,
            project_name: "P".into(),
            version: "1".into(),
            files: vec![pack_file("AbCdEf"), pack_file("zzz")],
            missing_mods: vec![],
            skipped_overrides: vec![],
            resolved_missing: Vec::new(),
            inert_loader_jars: vec![],
        };
        let mut wanted = origin.files[0].clone();
        wanted.sha1 = "AbCdEf".into();
        let found = origin
            .files
            .iter()
            .find(|f| f.sha1.eq_ignore_ascii_case("abcdef"));
        assert!(found.is_some());
        let none = origin
            .files
            .iter()
            .find(|f| f.sha1.eq_ignore_ascii_case("missing"));
        assert!(none.is_none());
    }

    #[test]
    fn is_tracked_mod_only_matches_top_level_jar() {
        assert!(is_tracked_mod("mods/sodium.jar"));
        assert!(!is_tracked_mod("mods/Emis_Rlcraft.zip"));
        assert!(!is_tracked_mod(
            "mods/memory_repo/com/x/llibrary/llibrary.jar"
        ));
        assert!(!is_tracked_mod("resourcepacks/RLHats.zip"));
        assert!(!is_tracked_mod("mods/"));
    }

    #[test]
    fn compute_status_zip_in_mods_dir_checked_on_disk() {
        let mut zip = pack_file("z");
        zip.install_path = "mods/Emis_Rlcraft.zip".into();
        let origin = PackOrigin {
            project_id: None,
            source: ModSource::Curseforge,
            project_name: "P".into(),
            version: "1".into(),
            files: vec![pack_file("a"), zip],
            missing_mods: vec![],
            skipped_overrides: vec![],
            resolved_missing: Vec::new(),
            inert_loader_jars: vec![],
        };
        let installed = vec![installed("a", true)];
        let present: std::collections::HashSet<String> =
            ["mods/Emis_Rlcraft.zip".to_string()].into_iter().collect();
        let s = compute_status(origin, &installed, &present);
        assert!(
            !s.is_modified,
            "a .zip present on disk must not be 'removed'"
        );
    }

    #[test]
    fn build_pack_origin_keeps_only_manually_installable_unresolvable() {
        use crate::mods::modpack::schema::{ModpackUnresolvable, UnresolvableReason};
        let mut summary = sample_summary(ModpackFormat::Modrinth);
        summary.unresolvable = vec![
            ModpackUnresolvable {
                reason: UnresolvableReason::DistributionDisabled,
                mod_name: "SRP".into(),
                manual_action_url: "https://www.curseforge.com/projects/1".into(),
                filename: "srp.jar".into(),
                size: 1.0,
                sha1: None,
                project_id: None,
            },
            ModpackUnresolvable {
                reason: UnresolvableReason::HostNotAllowed,
                mod_name: "mods/x.jar".into(),
                manual_action_url: "https://github.com/x.jar".into(),
                filename: "x.jar".into(),
                size: 2.0,
                sha1: Some("ab".into()),
                project_id: None,
            },
            ModpackUnresolvable {
                reason: UnresolvableReason::UnsafePath,
                mod_name: "../escape.jar".into(),
                manual_action_url: String::new(),
                filename: "escape.jar".into(),
                size: 3.0,
                sha1: None,
                project_id: None,
            },
            ModpackUnresolvable {
                reason: UnresolvableReason::MissingChecksum,
                mod_name: "nohash.jar".into(),
                manual_action_url: String::new(),
                filename: "nohash.jar".into(),
                size: 4.0,
                sha1: None,
                project_id: None,
            },
        ];
        let origin = build_pack_origin(&summary, &[], None, "Test Pack");
        // DistributionDisabled + HostNotAllowed + MissingChecksum are kept (3 total).
        // UnsafePath is excluded.
        assert_eq!(origin.missing_mods.len(), 3);
        assert!(origin
            .missing_mods
            .iter()
            .all(|m| !matches!(m.reason, UnresolvableReason::UnsafePath)));
        assert!(origin
            .missing_mods
            .iter()
            .any(|m| matches!(m.reason, UnresolvableReason::MissingChecksum)));
    }

    #[test]
    fn compute_status_nested_mods_jar_checked_on_disk() {
        let mut nested = pack_file("n");
        nested.install_path = "mods/memory_repo/com/x/llibrary.jar".into();
        let origin = PackOrigin {
            project_id: None,
            source: ModSource::Curseforge,
            project_name: "P".into(),
            version: "1".into(),
            files: vec![pack_file("a"), nested],
            missing_mods: vec![],
            skipped_overrides: vec![],
            resolved_missing: Vec::new(),
            inert_loader_jars: vec![],
        };
        let installed = vec![installed("a", true)];
        let present: std::collections::HashSet<String> =
            ["mods/memory_repo/com/x/llibrary.jar".to_string()]
                .into_iter()
                .collect();
        let s = compute_status(origin, &installed, &present);
        assert!(
            !s.is_modified,
            "a nested mods/ jar present on disk must not be 'removed'"
        );
    }

    fn missing_entry(
        sha1: Option<&str>,
        filename: &str,
        name: &str,
        project_id: Option<&str>,
    ) -> ModpackUnresolvable {
        ModpackUnresolvable {
            reason: UnresolvableReason::DistributionDisabled,
            mod_name: name.to_string(),
            manual_action_url: "https://www.curseforge.com/projects/1".into(),
            filename: filename.to_string(),
            size: 1.0,
            sha1: sha1.map(|s| s.to_string()),
            project_id: project_id.map(|s| s.to_string()),
        }
    }

    fn installed_mod(
        sha1: &str,
        filename: &str,
        name: &str,
        enabled: bool,
        project_id: Option<&str>,
    ) -> crate::mods::platform::InstalledMod {
        crate::mods::platform::InstalledMod {
            filename: filename.to_string(),
            sha1: sha1.to_string(),
            source: None,
            project_id: project_id.map(|s| s.to_string()),
            version_id: None,
            name: name.to_string(),
            version_number: None,
            installed_at: chrono::Utc::now().to_rfc3339(),
            enabled,
            enrich_attempted: false,
            requires: Vec::new(),
        }
    }

    fn origin_with_missing(missing: Vec<ModpackUnresolvable>) -> PackOrigin {
        let mut o = origin_with(vec![]);
        o.missing_mods = missing;
        o
    }

    #[test]
    fn missing_state_installed_by_sha1() {
        let origin = origin_with_missing(vec![missing_entry(Some("aa"), "srp.jar", "SRP", None)]);
        let installed = vec![installed_mod("AA", "whatever.jar", "Whatever", true, None)];
        let st = compute_status(origin, &installed, &Default::default());
        assert_eq!(st.missing_mods[0].state, MissingModState::Installed);
    }

    #[test]
    fn missing_state_installed_by_filename() {
        let origin = origin_with_missing(vec![missing_entry(Some("aa"), "srp.jar", "SRP", None)]);
        let installed = vec![installed_mod("zz", "SRP.JAR", "Whatever", true, None)];
        let st = compute_status(origin, &installed, &Default::default());
        assert_eq!(st.missing_mods[0].state, MissingModState::Installed);
    }

    #[test]
    fn missing_state_different_version_by_project_id() {
        let origin = origin_with_missing(vec![missing_entry(
            Some("aa"),
            "srp.jar",
            "SRP",
            Some("p1"),
        )]);
        let installed = vec![installed_mod(
            "zz",
            "srp-2.8.jar",
            "Whatever",
            true,
            Some("p1"),
        )];
        let st = compute_status(origin, &installed, &Default::default());
        assert_eq!(st.missing_mods[0].state, MissingModState::DifferentVersion);
    }

    #[test]
    fn missing_state_different_version_by_name() {
        let origin =
            origin_with_missing(vec![missing_entry(None, "srp.jar", "Scape and Run", None)]);
        let installed = vec![installed_mod(
            "zz",
            "other.jar",
            "scape and run",
            true,
            None,
        )];
        let st = compute_status(origin, &installed, &Default::default());
        assert_eq!(st.missing_mods[0].state, MissingModState::DifferentVersion);
    }

    #[test]
    fn missing_state_missing_when_nothing_matches() {
        let origin = origin_with_missing(vec![missing_entry(
            Some("aa"),
            "srp.jar",
            "SRP",
            Some("p1"),
        )]);
        let installed = vec![installed_mod("zz", "other.jar", "Other", true, Some("p2"))];
        let st = compute_status(origin, &installed, &Default::default());
        assert_eq!(st.missing_mods[0].state, MissingModState::Missing);
    }

    #[test]
    fn missing_state_pinned_file_wins_over_project_id() {
        let origin = origin_with_missing(vec![missing_entry(
            Some("aa"),
            "srp.jar",
            "SRP",
            Some("p1"),
        )]);
        let installed = vec![installed_mod("aa", "srp.jar", "SRP", true, Some("p1"))];
        let st = compute_status(origin, &installed, &Default::default());
        assert_eq!(st.missing_mods[0].state, MissingModState::Installed);
    }

    #[test]
    fn missing_state_different_version_for_a_disabled_jar() {
        // A disabled jar still counts: `missing_mod_state` reconciles
        // against every installed mod regardless of `enabled`, so a
        // different version the user toggled off still classifies as
        // `different_version`, not `missing`.
        let origin = origin_with_missing(vec![missing_entry(
            Some("aa"),
            "srp.jar",
            "SRP",
            Some("p1"),
        )]);
        let installed = vec![installed_mod(
            "zz",
            "srp-2.8.jar",
            "Whatever",
            false,
            Some("p1"),
        )];
        let st = compute_status(origin, &installed, &Default::default());
        assert_eq!(st.missing_mods[0].state, MissingModState::DifferentVersion);
    }

    #[test]
    fn filename_stem_strips_the_version_tail() {
        assert_eq!(
            filename_stem("srparasites-1.12.2-2.7.1.jar").as_deref(),
            Some("srparasites")
        );
        assert_eq!(
            filename_stem("jei_1.12.2-4.16.1.302.jar").as_deref(),
            Some("jei")
        );
        assert_eq!(
            filename_stem("RTG-1.12.2-6.1.0.0-snapshot.1.jar").as_deref(),
            Some("rtg")
        );
        // Multi-word leading name is kept whole.
        assert_eq!(
            filename_stem("sodium-extra-0.5.4.jar").as_deref(),
            Some("sodium-extra")
        );
        // A filename starting with a digit yields no stem.
        assert_eq!(filename_stem("2019-mod-1.0.jar"), None);
        // Disabled jars: the .disabled suffix is stripped too.
        assert_eq!(
            filename_stem("srparasites-1.12.2-2.7.1.jar.disabled").as_deref(),
            Some("srparasites")
        );
    }

    #[test]
    fn missing_state_different_version_by_filename_stem() {
        // The pack pinned srparasites 2.7.1; the user hand-dropped 2.8.0 —
        // different sha1, different exact filename, no project_id, and the
        // descriptor name ("Scape and Run") does not match the pack's
        // file display name. Only the filename stem connects them.
        let origin = origin_with_missing(vec![missing_entry(
            Some("aa"),
            "srparasites-1.12.2-2.7.1.jar",
            "SRP v 2.7.1",
            None,
        )]);
        let installed = vec![installed_mod(
            "zz",
            "srparasites-1.12.2-2.8.0.jar",
            "Scape and Run: Parasites",
            true,
            None,
        )];
        let st = compute_status(origin, &installed, &Default::default());
        assert_eq!(st.missing_mods[0].state, MissingModState::DifferentVersion);
    }

    #[test]
    fn missing_state_substituted_when_resolved_sha_installed() {
        let mut origin = origin_with_missing(vec![missing_entry(
            None,
            "ctp.jar",
            "Create Train Parts",
            Some("123"),
        )]);
        origin.resolved_missing = vec![crate::mods::installed::ResolvedMissing {
            filename: "ctp.jar".into(),
            mod_name: "Create Train Parts".into(),
            sha1: "deadbeef".into(),
        }];
        // The substitute jar has a DISTINCT name/filename/id from the entry, so
        // it matches ONLY via the recorded overlay sha1 (DEADBEEF == deadbeef).
        let installed = vec![installed_mod(
            "DEADBEEF",
            "create-train-parts-fabric.jar",
            "Create: Trains & Parts (Modrinth)",
            true,
            Some("modrinth-abc"),
        )];
        let st = compute_status(origin, &installed, &Default::default());
        assert_eq!(st.missing_mods[0].state, MissingModState::Substituted);
    }

    #[test]
    fn missing_state_reverts_to_missing_when_substitute_removed() {
        let mut origin = origin_with_missing(vec![missing_entry(
            None,
            "ctp.jar",
            "Create Train Parts",
            Some("123"),
        )]);
        origin.resolved_missing = vec![crate::mods::installed::ResolvedMissing {
            filename: "ctp.jar".into(),
            mod_name: "Create Train Parts".into(),
            sha1: "deadbeef".into(),
        }];
        // Substitute jar no longer present -> overlay must not falsely close it.
        let installed: Vec<crate::mods::platform::InstalledMod> = vec![];
        let st = compute_status(origin, &installed, &Default::default());
        assert_eq!(st.missing_mods[0].state, MissingModState::Missing);
    }

    #[test]
    fn missing_state_falls_through_to_different_version_when_substitute_gone() {
        let mut origin = origin_with_missing(vec![missing_entry(
            None,
            "ctp.jar",
            "Create Train Parts",
            Some("123"),
        )]);
        origin.resolved_missing = vec![crate::mods::installed::ResolvedMissing {
            filename: "ctp.jar".into(),
            mod_name: "Create Train Parts".into(),
            sha1: "deadbeef".into(),
        }];
        // The recorded substitute (deadbeef) is NOT installed, but a different
        // version of the same mod IS present (matches the entry by display name).
        let installed = vec![installed_mod(
            "0000",
            "create-train-parts-1.2.3.jar",
            "Create Train Parts",
            true,
            Some("123"),
        )];
        let st = compute_status(origin, &installed, &Default::default());
        assert_eq!(st.missing_mods[0].state, MissingModState::DifferentVersion);
    }

    /// ATLauncher md5 files use the md5 as a transient selection token in sha1.
    /// After pre-resolution, the PackOrigin must carry the REAL computed sha1,
    /// not the md5 token. This test drives the fetch_to_cache_md5 + resolve +
    /// build_pack_origin pipeline that the install loop executes for such files.
    #[tokio::test]
    async fn atl_md5_origin_records_real_sha1_not_md5() {
        use sha1::Sha1;
        use tempfile::TempDir;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let body: &[u8] = b"atl-mod-body-bytes";
        // Compute expected checksums for the test body.
        let md5_hex = {
            use md5::Digest as _;
            hex::encode(md5::Md5::digest(body))
        };
        let real_sha1_hex = {
            use sha1::Digest as _;
            hex::encode(Sha1::digest(body))
        };

        // Serve the file from a mock server.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/m.jar"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body.to_vec()))
            .mount(&server)
            .await;

        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let data_dir = TempDir::new().unwrap();
        let noop: crate::mods::install::ProgressFn = Box::new(|_, _, _| {});

        // Build an ATLauncher-style ModpackFile: sha1 holds the md5 (selection token),
        // md5 field is Some (signals ATL md5 pre-resolve).
        let atl_file = ModpackFile {
            project_id: "m.jar".into(),
            version_id: md5_hex.clone(),
            name: "TestMod".into(),
            filename: "m.jar".into(),
            install_path: "mods/m.jar".into(),
            sha1: md5_hex.clone(), // transient selection token
            md5: Some(md5_hex.clone()),
            url: format!("{}/m.jar", server.uri()),
            size: body.len() as f64,
            env_client: EnvSupport::Required,
            source: crate::mods::platform::ModSource::Atlauncher,
        };

        // Simulate what the install loop now does for md5 files:
        // call fetch_to_cache_md5, get real sha1, clone file with real sha1.
        let (_, got_real_sha1) = crate::mods::install::fetch_to_cache_md5(
            data_dir.path(),
            &atl_file.url,
            &md5_hex,
            atl_file.size,
            "modpacks",
            &noop,
        )
        .await
        .expect("fetch_to_cache_md5 must succeed");

        assert_eq!(
            got_real_sha1, real_sha1_hex,
            "fetch_to_cache_md5 must return the real sha1, not the md5"
        );

        // Build the resolved file (md5 cleared, real sha1 in place) as the loop does.
        let mut resolved = atl_file.clone();
        resolved.sha1 = got_real_sha1.clone();
        resolved.md5 = None;

        // build_pack_origin must record the real sha1, not the md5 token.
        let summary = sample_summary(ModpackFormat::Atlauncher);
        let origin = build_pack_origin(&summary, &[&resolved], None, "TestPack");

        assert_eq!(origin.files.len(), 1);
        assert_eq!(
            origin.files[0].sha1, real_sha1_hex,
            "PackOrigin must carry the real sha1, not the md5 selection token"
        );
        assert_ne!(
            origin.files[0].sha1, md5_hex,
            "PackOrigin must NOT carry the md5 as sha1"
        );
    }

    /// Regression test for the bug introduced in 97e2bd3: a non-md5 file whose
    /// install attempt fails must still be recorded in `resolved_for_origin` so
    /// that `build_pack_origin` captures it, and `compute_status` later surfaces
    /// it as a `removed_files` entry (with a Restore affordance). Before 97e2bd3
    /// `build_pack_origin` received `&selected` = ALL user-selected files; after
    /// that commit only successfully-installed files were pushed, so a failed
    /// install silently vanished from the origin.
    ///
    /// This test exercises the accumulation logic directly: it calls the same
    /// `build_pack_origin` helper that the install loop uses, passing a slice
    /// that includes a "failed" file (a file that would not have been pushed
    /// under the regressed code). It then asserts that `compute_status` flags
    /// that file's sha1 as removed — i.e. in the origin but not installed.
    #[test]
    fn failed_install_still_recorded_in_origin() {
        // Two files were selected for install.
        let good_file = sample_file("aaaa1111");
        let failed_file = sample_file("bbbb2222"); // this one "failed" to install

        // Pre-97e2bd3 semantics: both files go into resolved_for_origin,
        // regardless of install success/failure (only ATL md5-fetch failures
        // skip via `continue` — those files never reach this point). After the
        // fix, the install loop pushes both; we simulate that here.
        let resolved_refs: Vec<&ModpackFile> = vec![&good_file, &failed_file];

        let summary = sample_summary(ModpackFormat::Modrinth);
        let origin = build_pack_origin(&summary, &resolved_refs, None, "Test Pack");

        // Origin must record both files.
        assert_eq!(origin.files.len(), 2, "both files must be in the origin");
        let shas: Vec<&str> = origin.files.iter().map(|f| f.sha1.as_str()).collect();
        assert!(
            shas.contains(&"aaaa1111"),
            "good file sha must be in origin"
        );
        assert!(
            shas.contains(&"bbbb2222"),
            "failed file sha must be in origin even though install failed"
        );

        // Now simulate a world where only the good file ended up installed on disk.
        // compute_status must report the failed file as `removed_files` (Restore affordance).
        let installed_on_disk = vec![installed("aaaa1111", true)];
        let status = compute_status(
            origin,
            &installed_on_disk,
            &std::collections::HashSet::new(),
        );

        assert!(
            status.is_modified,
            "pack must be flagged as modified when a file is missing from disk"
        );
        assert_eq!(
            status.removed_files.len(),
            1,
            "exactly one file must appear in removed_files"
        );
        assert_eq!(
            status.removed_files[0].sha1, "bbbb2222",
            "the failed-install file must surface as removed_files so Restore is available"
        );
    }

    // ── classify_inert_loader_jars (import-time wrong-loader detection) ────────

    /// Build an in-memory `.jar` (zip) from (entry-name, body) pairs.
    fn inert_jar(entries: &[(&str, &str)]) -> Vec<u8> {
        use std::io::Write;
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            for (name, body) in entries {
                w.start_file(*name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                w.write_all(body.as_bytes()).unwrap();
            }
            w.finish().unwrap();
        }
        buf
    }

    /// A Fabric-only descriptor (read_jar_meta detects family Fabric).
    const FABRIC_DESCRIPTOR: (&str, &str) =
        ("fabric.mod.json", r#"{"name":"Sodium","id":"sodium"}"#);
    /// A Forge descriptor (read_jar_meta detects family Forge).
    const FORGE_DESCRIPTOR: (&str, &str) = ("META-INF/mods.toml", "modLoader=\"javafml\"\n");

    use crate::instances::schema::LoaderKind;

    async fn write_jar(dir: &std::path::Path, name: &str, bytes: &[u8]) {
        tokio::fs::create_dir_all(dir).await.unwrap();
        tokio::fs::write(dir.join(name), bytes).await.unwrap();
    }

    #[tokio::test]
    async fn classify_flags_fabric_jar_on_forge_instance() {
        let td = tempfile::tempdir().unwrap();
        let dir = td.path().join("mods");
        write_jar(&dir, "sodium.jar", &inert_jar(&[FABRIC_DESCRIPTOR])).await;
        let out = classify_inert_loader_jars(&dir, LoaderKind::Forge, "1.20.1");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].filename, "sodium.jar");
        assert_eq!(out[0].detected_loader, "Fabric");
    }

    #[tokio::test]
    async fn classify_does_not_flag_forge_jar_on_forge_instance() {
        let td = tempfile::tempdir().unwrap();
        let dir = td.path().join("mods");
        write_jar(&dir, "jei.jar", &inert_jar(&[FORGE_DESCRIPTOR])).await;
        let out = classify_inert_loader_jars(&dir, LoaderKind::Forge, "1.20.1");
        assert!(out.is_empty(), "same-family jar is not inert: {out:?}");
    }

    #[tokio::test]
    async fn classify_does_not_flag_descriptorless_jar() {
        let td = tempfile::tempdir().unwrap();
        let dir = td.path().join("mods");
        write_jar(
            &dir,
            "lib.jar",
            &inert_jar(&[("META-INF/MANIFEST.MF", "Manifest-Version: 1.0\n")]),
        )
        .await;
        let out = classify_inert_loader_jars(&dir, LoaderKind::Forge, "1.20.1");
        assert!(out.is_empty(), "no descriptor → never flagged: {out:?}");
    }

    #[tokio::test]
    async fn classify_does_not_flag_fabric_jar_on_quilt_instance() {
        // Quilt loads Fabric mods (same family) — never inert.
        let td = tempfile::tempdir().unwrap();
        let dir = td.path().join("mods");
        write_jar(&dir, "sodium.jar", &inert_jar(&[FABRIC_DESCRIPTOR])).await;
        let out = classify_inert_loader_jars(&dir, LoaderKind::Quilt, "1.20.1");
        assert!(out.is_empty(), "Fabric on Quilt is loadable: {out:?}");
    }

    #[tokio::test]
    async fn classify_vanilla_instance_flags_nothing() {
        // Vanilla has no loader family — compat_verdict never reports a mismatch.
        let td = tempfile::tempdir().unwrap();
        let dir = td.path().join("mods");
        write_jar(&dir, "sodium.jar", &inert_jar(&[FABRIC_DESCRIPTOR])).await;
        write_jar(&dir, "jei.jar", &inert_jar(&[FORGE_DESCRIPTOR])).await;
        let out = classify_inert_loader_jars(&dir, LoaderKind::Vanilla, "1.20.1");
        assert!(out.is_empty(), "Vanilla instance flags nothing: {out:?}");
    }

    #[tokio::test]
    async fn classify_missing_dir_returns_empty() {
        let td = tempfile::tempdir().unwrap();
        let dir = td.path().join("does-not-exist");
        let out = classify_inert_loader_jars(&dir, LoaderKind::Forge, "1.20.1");
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn classify_skips_unreadable_jar_but_keeps_flagging_real_one() {
        let td = tempfile::tempdir().unwrap();
        let dir = td.path().join("mods");
        // A "jar" that is not a valid zip — read_jar_meta errors → skipped.
        write_jar(&dir, "broken.jar", b"not a zip at all").await;
        // A genuine Fabric jar alongside it — still flagged.
        write_jar(&dir, "sodium.jar", &inert_jar(&[FABRIC_DESCRIPTOR])).await;
        let out = classify_inert_loader_jars(&dir, LoaderKind::Forge, "1.20.1");
        assert_eq!(out.len(), 1, "broken jar skipped, real one kept: {out:?}");
        assert_eq!(out[0].filename, "sodium.jar");
    }

    #[tokio::test]
    async fn classify_ignores_non_jar_files() {
        let td = tempfile::tempdir().unwrap();
        let dir = td.path().join("mods");
        // A Fabric descriptor inside a .txt — not a .jar, never read.
        write_jar(&dir, "readme.txt", &inert_jar(&[FABRIC_DESCRIPTOR])).await;
        let out = classify_inert_loader_jars(&dir, LoaderKind::Forge, "1.20.1");
        assert!(out.is_empty(), "non-jar files are ignored: {out:?}");
    }

    #[test]
    fn dedupe_inert_collapses_duplicate_filenames() {
        let jars = vec![
            InertLoaderJar {
                filename: "x.jar".into(),
                detected_loader: "Fabric".into(),
            },
            InertLoaderJar {
                filename: "x.jar".into(),
                detected_loader: "Fabric".into(),
            },
            InertLoaderJar {
                filename: "y.jar".into(),
                detected_loader: "Forge".into(),
            },
        ];
        let out = dedupe_inert(jars);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].filename, "x.jar");
        assert_eq!(out[1].filename, "y.jar");
    }
}
