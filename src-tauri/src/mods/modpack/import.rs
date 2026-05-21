//! Orchestrates: inspect → resolve name → create_instance → install N mods
//! → extract overrides. Emits typed progress events at each phase.

use crate::error::Error;
use crate::mods::install::{install_asset, install_one, ProgressFn};
use crate::mods::installed::{PackOrigin, PackOriginFile};
use crate::mods::modpack::detect::detect_format;
use crate::mods::modpack::overrides;
use crate::mods::modpack::schema::*;
use crate::mods::modpack::{curseforge as cf_parse, modrinth as mr_parse};
use crate::mods::platform::{ModFile, ModSource, ModVersion};

pub async fn inspect(
    bytes: &[u8],
    cf_base: &str,
) -> Result<ModpackSummary, Error> {
    let fmt = detect_format(bytes)?;
    match fmt {
        ModpackFormat::Modrinth => mr_parse::parse(bytes),
        ModpackFormat::Curseforge => cf_parse::parse(bytes, cf_base).await,
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
                    UnresolvableReason::DistributionDisabled | UnresolvableReason::HostNotAllowed
                )
            })
            .cloned()
            .collect(),
    }
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

/// A `missing_mods` entry counts as installed once the user has added
/// that mod by any route. Matched on three exact, case-insensitive
/// signals — sha1, filename, or display name — so a mod added via the
/// drag-drop local install (which records the descriptor name) is
/// detected even when the user grabbed a different file than the pack
/// pinned. All matches are exact, so there are no false positives.
fn missing_mod_installed(
    m: &ModpackUnresolvable,
    installed: &[crate::mods::platform::InstalledMod],
) -> bool {
    installed.iter().any(|i| {
        m.sha1
            .as_deref()
            .is_some_and(|s| i.sha1.eq_ignore_ascii_case(s))
            || i.filename.eq_ignore_ascii_case(&m.filename)
            || i.name.eq_ignore_ascii_case(&m.mod_name)
    })
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
    let installed_shas: Vec<String> =
        installed.iter().map(|m| m.sha1.to_ascii_lowercase()).collect();
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
            installed: missing_mod_installed(m, installed),
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
            sha1: Some(file.sha1.clone()),
            size: file.size,
            distribution_allowed: true,
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
            sha1: Some(f.sha1.clone()),
            size: f.size,
            distribution_allowed: true,
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

#[allow(clippy::too_many_arguments)]
pub async fn import(
    app: &tauri::AppHandle,
    bytes: &[u8],
    selected_shas: &[String],
    apply_overrides: bool,
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
    on_progress(ModpackProgress::Inspecting);
    let summary = inspect(bytes, cf_base).await?;

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
    };
    let (mrpack_project_id, pack_meta, mrpack_source) = match (
        hint_project_id.as_deref(),
        hint_source,
        summary.format,
    ) {
        (Some(pid), Some(crate::mods::platform::ModSource::Modrinth), ModpackFormat::Modrinth) => {
            let meta = fetch_modrinth_project(pid).await.unwrap_or_default();
            (Some(pid.to_string()), meta, parser_source)
        }
        (Some(pid), Some(crate::mods::platform::ModSource::Curseforge), ModpackFormat::Curseforge) => {
            // Browse-flow CF import: backfill the pack's project name +
            // short summary. Best-effort — failure keeps them None.
            let key = crate::mods::curseforge::keyring::get().ok().flatten();
            let (cf_name, cf_summary) =
                crate::mods::modpack::cf_api::fetch_summary(cf_base, key.as_deref(), pid)
                    .await
                    .unwrap_or((None, None));
            (
                Some(pid.to_string()),
                PackMeta { name: cf_name, description: cf_summary },
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
    };
    let PackMeta { name: platform_name, description: mrpack_summary } = pack_meta;
    let pack_name = resolve_pack_name(platform_name.as_deref(), &summary.name);

    let existing: Vec<String> = crate::instances::list_instances_with_status(app)?
        .into_iter().map(|i| i.name).collect();
    let final_name = resolve_name(&pack_name, &existing)?;

    on_progress(ModpackProgress::CreatingInstance { name: final_name.clone() });
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
    ).map_err(|e| Error::ModpackInstanceCreationFailed { details: e.to_string() })?;

    let instance_root = crate::paths::instance_dir(app, &inst.id)
        .map_err(|e| Error::Io { path: "<instance_dir>".into(), details: e.to_string() })?;
    let data_dir = crate::paths::app_dir(app)
        .map_err(|e| Error::Io { path: "<app_dir>".into(), details: e.to_string() })?;

    let selected: Vec<&ModpackFile> = summary.files.iter()
        .filter(|f| selected_shas.iter().any(|s| s.eq_ignore_ascii_case(&f.sha1))).collect();
    let total = selected.len() as u32;
    let mut failures: Vec<(String, String)> = vec![];

    for (idx, file) in selected.iter().enumerate() {
        on_progress(ModpackProgress::InstallingFile {
            current: idx as u32 + 1,
            total,
            file_name: file.name.clone(),
        });
        let res = if file.install_path.starts_with("mods/") {
            let mv = modpack_file_to_mod_version(file, &summary.game_version, summary.loader);
            install_one(&data_dir, &instance_root, mv, &install_progress)
                .await
                .map(|_| ())
        } else {
            install_asset(
                &data_dir,
                &instance_root,
                &file.url,
                &file.sha1,
                file.size,
                &file.install_path,
                &install_progress,
            )
            .await
        };
        if let Err(e) = res {
            failures.push((file.install_path.clone(), e.to_string()));
        }
    }

    // Bundled assets from overrides/ (mods/*.jar plus top-level
    // resourcepacks/ and shaderpacks/ files) are tracked here so the
    // origin snapshot below captures them. Without this, the drawer's
    // scan-reconcile-driven InstalledMod entries land with source=None
    // and fall into the "manual" badge even though the bytes came
    // straight from the pack archive.
    let mut bundled_assets: Vec<crate::mods::modpack::overrides::ExtractedAsset> = vec![];
    if apply_overrides && (summary.has_overrides || summary.has_client_overrides) {
        let bytes_clone = bytes.to_vec();
        bundled_assets = overrides::extract(&bytes_clone, &instance_root, |c, t| {
            on_progress(ModpackProgress::ExtractingOverrides { current: c, total: t });
        })
        .await?;
    }

    // Persist the origin snapshot. Best-effort — the import itself
    // is already done at this point; a write failure here only loses
    // the modified/restore affordance, not any installed mod or
    // instance. Log and continue.
    let mut origin = build_pack_origin(&summary, &selected, mrpack_project_id_for_origin, &pack_name);
    // Fold bundled-from-overrides jars into the origin so they badge
    // as "pack" in the drawer. `url` stays empty (bundled bytes have
    // no remote source) — the Restore path checks for this and
    // returns a typed error instead of trying to install_one a no-URL
    // entry.
    let bundled_source = origin.source;
    for m in &bundled_assets {
        origin.files.push(crate::mods::installed::PackOriginFile {
            sha1: m.sha1.clone(),
            name: m.filename.trim_end_matches(".jar").trim_end_matches(".disabled").to_string(),
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
    if let Err(e) = crate::mods::installed::set_pack_origin(&instance_root, origin).await {
        eprintln!("[modpack::import] set_pack_origin failed (non-fatal): {e}");
    }

    on_progress(ModpackProgress::Done { instance_id: inst.id.clone() });

    if failures.is_empty() {
        Ok(inst)
    } else {
        Err(Error::ModpackPartialFailure { instance_id: inst.id, failed: failures })
    }
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
async fn fetch_modrinth_metadata(
    version_id: &str,
) -> Result<(Option<String>, PackMeta), Error> {
    #[derive(serde::Deserialize)]
    struct V { project_id: String }

    let v_url = format!("https://api.modrinth.com/v2/version/{version_id}");
    let v_resp = crate::network::request::get(
        &v_url,
        &[("user-agent", "AntonBabchenko/FTlauncher")],
        "modpacks",
    )
    .await
    .map_err(|e| Error::ModsNetwork { url: v_url.clone(), details: e.to_string() })?;
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
async fn fetch_modrinth_project(
    project_id: &str,
) -> Result<PackMeta, Error> {
    #[derive(serde::Deserialize)]
    struct P {
        title: Option<String>,
        description: Option<String>,
    }

    let p_url = format!("https://api.modrinth.com/v2/project/{project_id}");
    let p_resp = crate::network::request::get(
        &p_url,
        &[("user-agent", "AntonBabchenko/FTlauncher")],
        "modpacks",
    )
    .await
    .map_err(|e| Error::ModsNetwork { url: p_url.clone(), details: e.to_string() })?;
    if !(200..300).contains(&p_resp.status) {
        return Ok(PackMeta::default());
    }
    let p: P = serde_json::from_slice(&p_resp.body).map_err(|e| Error::ModsDecode {
        platform: "modrinth".into(),
        details: e.to_string(),
    })?;
    Ok(PackMeta { name: p.title, description: p.description })
}

#[cfg(test)]
mod tests {
    use super::*;

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
            "Pack".to_string(), "Pack (2)".to_string(), "Pack (3)".to_string()
        ];
        assert_eq!(resolve_name("Pack", &existing).unwrap(), "Pack (4)");
    }

    #[test]
    fn resolve_name_errors_when_999_exhausted() {
        let mut existing = vec!["Pack".to_string()];
        for i in 2..=999 { existing.push(format!("Pack ({i})")); }
        assert!(matches!(resolve_name("Pack", &existing), Err(Error::ModpackInstanceCreationFailed { .. })));
    }

    #[test]
    fn resolve_pack_name_prefers_platform_name() {
        assert_eq!(resolve_pack_name(Some("Sodium Plus"), "2.3.7"), "Sodium Plus");
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
        assert_eq!(resolve_pack_name(Some("  Sodium Plus  "), "2.3.7"), "Sodium Plus");
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
            url: format!("https://example.com/{sha}.jar"),
            size: 42.0,
            env_client: EnvSupport::Required,
            source: ModSource::Modrinth,
        }
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
            source: if with_source { Some(ModSource::Modrinth) } else { None },
            project_id: if with_source { Some(format!("p-{sha}")) } else { None },
            version_id: if with_source { Some(format!("v-{sha}")) } else { None },
            name: format!("Mod {sha}"),
            version_number: None,
            installed_at: chrono::Utc::now().to_rfc3339(),
            enabled: true,
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
        };
        let mut m = installed("ABC", true);
        m.sha1 = "abc".into();
        let s = compute_status(origin, &[m], &std::collections::HashSet::new());
        assert!(!s.is_modified);
    }

    #[test]
    fn pack_origin_file_to_mod_version_round_trip() {
        let f = pack_file("aaa");
        let v = pack_origin_file_to_mod_version(&f, "1.20.1", crate::mods::platform::LoaderKind::Fabric);
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
        }
    }

    #[test]
    fn diff_classifies_added_removed_updated_unchanged() {
        let mut a_old = pack_file("aaa"); a_old.project_id = "A".into();
        let mut b_old = pack_file("bbb"); b_old.project_id = "B".into();
        let mut c_old = pack_file("ccc"); c_old.project_id = "C".into();
        let origin = origin_with(vec![a_old, b_old, c_old]);
        let new_files = vec![
            mp_file("A", "aaa"),
            mp_file("B", "bbb-2"),
            mp_file("D", "ddd"),
        ];
        let summary = ModpackSummary { files: new_files, ..sample_summary(ModpackFormat::Modrinth) };
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
        let summary = ModpackSummary { files: vec![], ..sample_summary(ModpackFormat::Modrinth) };
        let diff = compute_update_diff(
            &summary,
            &origin,
            "1.20.1",
            crate::mods::platform::LoaderKind::Fabric,
            &Some("0.15.7".into()),
        );
        assert!(diff.removed.is_empty(), "bundled entry must not be in removed");
    }

    #[test]
    fn diff_detects_version_bump() {
        let origin = origin_with(vec![]);
        let mut summary = ModpackSummary { files: vec![], ..sample_summary(ModpackFormat::Modrinth) };
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
        };
        let mut wanted = origin.files[0].clone();
        wanted.sha1 = "AbCdEf".into();
        let found = origin.files.iter().find(|f| f.sha1.eq_ignore_ascii_case("abcdef"));
        assert!(found.is_some());
        let none = origin.files.iter().find(|f| f.sha1.eq_ignore_ascii_case("missing"));
        assert!(none.is_none());
    }

    #[test]
    fn is_tracked_mod_only_matches_top_level_jar() {
        assert!(is_tracked_mod("mods/sodium.jar"));
        assert!(!is_tracked_mod("mods/Emis_Rlcraft.zip"));
        assert!(!is_tracked_mod("mods/memory_repo/com/x/llibrary/llibrary.jar"));
        assert!(!is_tracked_mod("resourcepacks/RLHats.zip"));
        assert!(!is_tracked_mod("mods/"));
    }

    #[test]
    fn compute_status_zip_in_mods_dir_checked_on_disk() {
        let mut zip = pack_file("z");
        zip.install_path = "mods/Emis_Rlcraft.zip".into();
        let origin = PackOrigin {
            project_id: None, source: ModSource::Curseforge,
            project_name: "P".into(), version: "1".into(),
            files: vec![pack_file("a"), zip],
            missing_mods: vec![],
        };
        let installed = vec![installed("a", true)];
        let present: std::collections::HashSet<String> =
            ["mods/Emis_Rlcraft.zip".to_string()].into_iter().collect();
        let s = compute_status(origin, &installed, &present);
        assert!(!s.is_modified, "a .zip present on disk must not be 'removed'");
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
            },
            ModpackUnresolvable {
                reason: UnresolvableReason::HostNotAllowed,
                mod_name: "mods/x.jar".into(),
                manual_action_url: "https://github.com/x.jar".into(),
                filename: "x.jar".into(),
                size: 2.0,
                sha1: Some("ab".into()),
            },
            ModpackUnresolvable {
                reason: UnresolvableReason::UnsafePath,
                mod_name: "../escape.jar".into(),
                manual_action_url: String::new(),
                filename: "escape.jar".into(),
                size: 3.0,
                sha1: None,
            },
        ];
        let origin = build_pack_origin(&summary, &[], None, "Test Pack");
        assert_eq!(origin.missing_mods.len(), 2);
        assert!(origin
            .missing_mods
            .iter()
            .all(|m| !matches!(m.reason, UnresolvableReason::UnsafePath)));
    }

    #[test]
    fn compute_status_nested_mods_jar_checked_on_disk() {
        let mut nested = pack_file("n");
        nested.install_path = "mods/memory_repo/com/x/llibrary.jar".into();
        let origin = PackOrigin {
            project_id: None, source: ModSource::Curseforge,
            project_name: "P".into(), version: "1".into(),
            files: vec![pack_file("a"), nested],
            missing_mods: vec![],
        };
        let installed = vec![installed("a", true)];
        let present: std::collections::HashSet<String> =
            ["mods/memory_repo/com/x/llibrary.jar".to_string()].into_iter().collect();
        let s = compute_status(origin, &installed, &present);
        assert!(!s.is_modified, "a nested mods/ jar present on disk must not be 'removed'");
    }

    fn missing_entry(sha1: Option<&str>, filename: &str, name: &str) -> ModpackUnresolvable {
        ModpackUnresolvable {
            reason: UnresolvableReason::DistributionDisabled,
            mod_name: name.to_string(),
            manual_action_url: "https://www.curseforge.com/projects/1".into(),
            filename: filename.to_string(),
            size: 1.0,
            sha1: sha1.map(|s| s.to_string()),
        }
    }

    fn installed_mod(sha1: &str, filename: &str, name: &str, enabled: bool)
        -> crate::mods::platform::InstalledMod
    {
        crate::mods::platform::InstalledMod {
            filename: filename.to_string(),
            sha1: sha1.to_string(),
            source: None,
            project_id: None,
            version_id: None,
            name: name.to_string(),
            version_number: None,
            installed_at: chrono::Utc::now().to_rfc3339(),
            enabled,
        }
    }

    fn origin_with_missing(missing: Vec<ModpackUnresolvable>) -> PackOrigin {
        let mut o = origin_with(vec![]);
        o.missing_mods = missing;
        o
    }

    #[test]
    fn missing_mod_detected_by_sha1() {
        let origin = origin_with_missing(vec![missing_entry(Some("aa"), "srp.jar", "SRP")]);
        let installed = vec![installed_mod("AA", "whatever.jar", "Whatever", true)];
        let st = compute_status(origin, &installed, &Default::default());
        assert_eq!(st.missing_mods.len(), 1);
        assert!(st.missing_mods[0].installed);
    }

    #[test]
    fn missing_mod_detected_by_filename() {
        let origin = origin_with_missing(vec![missing_entry(Some("aa"), "srp.jar", "SRP")]);
        let installed = vec![installed_mod("zz", "SRP.JAR", "Whatever", true)];
        let st = compute_status(origin, &installed, &Default::default());
        assert!(st.missing_mods[0].installed);
    }

    #[test]
    fn missing_mod_detected_by_name() {
        let origin = origin_with_missing(vec![missing_entry(None, "srp.jar", "Scape and Run")]);
        let installed = vec![installed_mod("zz", "other.jar", "scape and run", true)];
        let st = compute_status(origin, &installed, &Default::default());
        assert!(st.missing_mods[0].installed);
    }

    #[test]
    fn missing_mod_not_detected_when_nothing_matches() {
        let origin = origin_with_missing(vec![missing_entry(Some("aa"), "srp.jar", "SRP")]);
        let installed = vec![installed_mod("zz", "other.jar", "Other", true)];
        let st = compute_status(origin, &installed, &Default::default());
        assert!(!st.missing_mods[0].installed);
    }

    #[test]
    fn disabled_installed_mod_still_resolves_missing_entry() {
        let origin = origin_with_missing(vec![missing_entry(Some("aa"), "srp.jar", "SRP")]);
        let installed = vec![installed_mod("aa", "srp.jar", "SRP", false)];
        let st = compute_status(origin, &installed, &Default::default());
        assert!(st.missing_mods[0].installed);
    }
}
