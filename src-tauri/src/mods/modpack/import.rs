//! Orchestrates: inspect → resolve name → create_instance → install N mods
//! → extract overrides. Emits typed progress events at each phase.

use reqwest::Client;

use crate::error::Error;
use crate::mods::install::{install_one, ProgressFn};
use crate::mods::installed::{PackOrigin, PackOriginFile};
use crate::mods::modpack::detect::detect_format;
use crate::mods::modpack::overrides;
use crate::mods::modpack::schema::*;
use crate::mods::modpack::{curseforge as cf_parse, modrinth as mr_parse};
use crate::mods::platform::{ModFile, ModSource, ModVersion};

pub async fn inspect(
    bytes: &[u8],
    http: &Client,
    cf_base: &str,
) -> Result<ModpackSummary, Error> {
    let fmt = detect_format(bytes)?;
    match fmt {
        ModpackFormat::Modrinth => mr_parse::parse(bytes),
        ModpackFormat::Curseforge => cf_parse::parse(bytes, http, cf_base).await,
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
        project_name: summary.name.clone(),
        version: summary.version.clone(),
        files,
    }
}

/// Diff the immutable pack-origin snapshot against the live installed
/// list to compute what's been removed since import and how many user
/// additions exist. SHA-1 comparison is case-insensitive (lowercased
/// once on each side and compared as lowercase). Pure — used by the
/// `modpack_status` IPC command and unit-tested independently.
pub fn compute_status(
    mut origin: PackOrigin,
    installed: &[crate::mods::platform::InstalledMod],
) -> crate::mods::modpack::schema::ModpackStatus {
    // Defensive cleanup for packs imported before the parser-side
    // mods/-only filter landed: drop non-`mods/` entries from the origin
    // before computing the diff. Otherwise resourcepacks/shaderpacks
    // declared in the .mrpack `files[]` show as false-positive "Removed
    // from pack" rows (they were never actually installed — install_one
    // is hardcoded to mods_dir, and the .jar-only scan reconciliation
    // scrubbed their installed-mods.json entries on the next list call).
    // New imports never see this thanks to the parser fix; this branch
    // covers pre-fix on-disk state without forcing a re-import.
    origin.files.retain(|f| f.install_path.starts_with("mods/"));

    let origin_shas: std::collections::HashSet<String> =
        origin.files.iter().map(|f| f.sha1.to_ascii_lowercase()).collect();
    let installed_shas: Vec<String> =
        installed.iter().map(|m| m.sha1.to_ascii_lowercase()).collect();
    let installed_set: std::collections::HashSet<&String> = installed_shas.iter().collect();

    let removed_files: Vec<PackOriginFile> = origin
        .files
        .iter()
        .filter(|f| !installed_set.contains(&f.sha1.to_ascii_lowercase()))
        .cloned()
        .collect();

    let added_count: u32 = installed_shas
        .iter()
        .filter(|s| !origin_shas.contains(*s))
        .count() as u32;

    let is_modified = !removed_files.is_empty() || added_count > 0;

    crate::mods::modpack::schema::ModpackStatus {
        origin,
        installed_shas,
        removed_files,
        added_count,
        is_modified,
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
    http: &Client,
    cf_base: &str,
    // Browse-flow hints. When the import was kicked off from the
    // Modpacks → Browse sub-tab the UI already knows the source
    // platform and the pack's project_id (it came from a `ModpackHit`),
    // so we pass them through and skip the version→project lookup.
    // Drag-drop imports pass `None` here and we auto-look-up below.
    hint_project_id: Option<String>,
    hint_source: Option<crate::mods::platform::ModSource>,
    // `Send + Sync` so the resulting future is `Send` — required by
    // the Tauri command boundary in `commands::modpack_import`. The
    // wrapped channel closure already satisfies these bounds.
    on_progress: &(dyn Fn(ModpackProgress) + Send + Sync),
    install_progress: ProgressFn,
) -> Result<crate::instances::schema::InstanceWithStatus, Error> {
    on_progress(ModpackProgress::Inspecting);
    let summary = inspect(bytes, http, cf_base).await?;

    if selected_shas.is_empty() {
        return Err(Error::ModpackNoFilesSelected);
    }

    let existing: Vec<String> = crate::instances::list_instances_with_status(app)?
        .into_iter().map(|i| i.name).collect();
    let final_name = resolve_name(&summary.name, &existing)?;

    // Best-effort Modrinth metadata fetch. We need the pack's own
    // project_id + a short description for the Imported drawer. The
    // .mrpack itself carries name + versionId, but no project_id and no
    // description, so we look them up via the Modrinth API. Failures
    // are silent — fields stay None.
    //
    // Two paths:
    //   * Browse-flow gave us the project_id already (`hint_project_id`)
    //     — skip the /v2/version/{id} round-trip and fetch only the
    //     description.
    //   * Drag-drop or no hints — do the full version→project lookup.
    //
    // CurseForge packs: source tag is set; project_id and summary stay
    // None (no CF backfill path yet).
    let parser_source = match summary.format {
        ModpackFormat::Modrinth => Some(crate::mods::platform::ModSource::Modrinth),
        ModpackFormat::Curseforge => Some(crate::mods::platform::ModSource::Curseforge),
    };
    let (mrpack_project_id, mrpack_summary, mrpack_source) = match (
        hint_project_id.as_deref(),
        hint_source,
        summary.format,
    ) {
        // Browse-flow hint matches the parser-detected source: trust it.
        (Some(pid), Some(crate::mods::platform::ModSource::Modrinth), ModpackFormat::Modrinth) => {
            let desc = fetch_modrinth_description(http, pid).await.unwrap_or(None);
            (Some(pid.to_string()), desc, parser_source)
        }
        (Some(pid), Some(crate::mods::platform::ModSource::Curseforge), ModpackFormat::Curseforge) => {
            // CF: no description fetch path yet — keep summary None.
            (Some(pid.to_string()), None, parser_source)
        }
        // Modrinth pack with no/mismatched hint: do the version→project
        // lookup via the API.
        (_, _, ModpackFormat::Modrinth) => {
            let (pid, desc) = fetch_modrinth_metadata(http, &summary.version)
                .await
                .unwrap_or((None, None));
            (pid, desc, parser_source)
        }
        // CF pack with no hint: nothing more to do.
        (_, _, ModpackFormat::Curseforge) => (None, None, parser_source),
    };

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
        Some((summary.name.clone(), summary.version.clone())),
        mrpack_project_id,
        mrpack_source,
        mrpack_summary,
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
        on_progress(ModpackProgress::InstallingMod {
            current: idx as u32 + 1,
            total,
            mod_name: file.name.clone(),
        });
        let mv = ModVersion {
            source: file.source,
            project_id: file.project_id.clone(),
            version_id: file.version_id.clone(),
            name: file.name.clone(),
            version_number: String::new(),
            mc_versions: vec![summary.game_version.clone()],
            loaders: vec![summary.loader],
            primary_file: ModFile {
                filename: file.filename.clone(),
                url: file.url.clone(),
                sha1: Some(file.sha1.clone()),
                size: file.size,
                distribution_allowed: true,
            },
            deps: vec![],
            published_at: None,
        };
        if let Err(e) = install_one(http, &data_dir, &instance_root, mv, &install_progress).await {
            failures.push((file.install_path.clone(), e.to_string()));
        }
    }

    // Bundled mods from overrides/mods/*.jar are tracked here so the
    // origin snapshot below captures them. Without this, the drawer's
    // scan-reconcile-driven InstalledMod entries land with source=None
    // and fall into the "manual" badge even though the bytes came
    // straight from the pack archive.
    let mut bundled_mods: Vec<crate::mods::modpack::overrides::ExtractedMod> = vec![];
    if apply_overrides && (summary.has_overrides || summary.has_client_overrides) {
        let bytes_clone = bytes.to_vec();
        bundled_mods = overrides::extract(&bytes_clone, &instance_root, |c, t| {
            on_progress(ModpackProgress::ExtractingOverrides { current: c, total: t });
        })
        .await?;
    }

    // Persist the origin snapshot. Best-effort — the import itself
    // is already done at this point; a write failure here only loses
    // the modified/restore affordance, not any installed mod or
    // instance. Log and continue.
    let mut origin = build_pack_origin(&summary, &selected, mrpack_project_id_for_origin);
    // Fold bundled-from-overrides jars into the origin so they badge
    // as "pack" in the drawer. `url` stays empty (bundled bytes have
    // no remote source) — the Restore path checks for this and
    // returns a typed error instead of trying to install_one a no-URL
    // entry.
    let bundled_source = origin.source;
    for m in &bundled_mods {
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

/// Modrinth `version_id` → `(project_id, description)`. Two hops:
///   1. `GET /v2/version/{version_id}` → `project_id`.
///   2. `GET /v2/project/{project_id}` → `description`.
///
/// The eat-the-error-and-return-`None` pattern (rather than propagating)
/// is intentional — metadata is nice-to-have, not blocking. Callers use
/// `.unwrap_or((None, None))` so a 404 or transient network failure can
/// never abort an otherwise-successful import.
async fn fetch_modrinth_metadata(
    http: &Client,
    version_id: &str,
) -> Result<(Option<String>, Option<String>), Error> {
    #[derive(serde::Deserialize)]
    struct V { project_id: String }

    let v_url = format!("https://api.modrinth.com/v2/version/{version_id}");
    let v_resp = http.get(&v_url)
        .header(reqwest::header::USER_AGENT, "AntonBabchenko/FTlauncher")
        .send().await
        .map_err(|e| Error::ModsNetwork { url: v_url.clone(), details: e.to_string() })?;
    if !v_resp.status().is_success() {
        return Ok((None, None));
    }
    let v: V = v_resp.json().await.map_err(|e| Error::ModsDecode {
        platform: "modrinth".into(), details: e.to_string()
    })?;
    let project_id = v.project_id;
    let desc = fetch_modrinth_description(http, &project_id).await?;
    Ok((Some(project_id), desc))
}

/// Modrinth `project_id` → `description`. Used directly when the
/// project_id is already known (browse-flow hint), and as the second
/// hop of `fetch_modrinth_metadata`. Same silent-on-failure semantics.
async fn fetch_modrinth_description(
    http: &Client,
    project_id: &str,
) -> Result<Option<String>, Error> {
    #[derive(serde::Deserialize)]
    struct P { description: Option<String> }

    let p_url = format!("https://api.modrinth.com/v2/project/{project_id}");
    let p_resp = http.get(&p_url)
        .header(reqwest::header::USER_AGENT, "AntonBabchenko/FTlauncher")
        .send().await
        .map_err(|e| Error::ModsNetwork { url: p_url.clone(), details: e.to_string() })?;
    if !p_resp.status().is_success() {
        return Ok(None);
    }
    let p: P = p_resp.json().await.map_err(|e| Error::ModsDecode {
        platform: "modrinth".into(), details: e.to_string()
    })?;
    Ok(p.description)
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
        let origin = build_pack_origin(&summary, &selected, Some("ABC123".into()));
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
        let origin = build_pack_origin(&summary, &[&f], None);
        assert_eq!(origin.source, ModSource::Curseforge);
        assert!(origin.project_id.is_none());
    }

    #[test]
    fn build_pack_origin_with_no_selected_files_yields_empty_files_vec() {
        let summary = sample_summary(ModpackFormat::Modrinth);
        let origin = build_pack_origin(&summary, &[], None);
        assert!(origin.files.is_empty());
        assert_eq!(origin.project_name, "Cool Pack");
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
        };
        let installed = vec![installed("a", true), installed("b", true)];
        let s = compute_status(origin, &installed);
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
        };
        // "b" no longer installed.
        let installed = vec![installed("a", true)];
        let s = compute_status(origin, &installed);
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
        };
        // User added "z" manually.
        let installed = vec![installed("a", true), installed("z", false)];
        let s = compute_status(origin, &installed);
        assert!(s.is_modified);
        assert!(s.removed_files.is_empty());
        assert_eq!(s.added_count, 1);
    }

    #[test]
    fn compute_status_drops_non_mods_path_entries_from_origin() {
        // Pre-bundle-2 parser allowed resourcepacks/.zip into pack_origin.
        // They never actually installed (install_one is mods/-only and
        // scan reconciliation drops .zip). Defensive filter in
        // compute_status hides them from the Removed-from-pack section.
        let mut rp = pack_file("rp1");
        rp.install_path = "resourcepacks/RP.zip".into();
        let origin = PackOrigin {
            project_id: None,
            source: ModSource::Modrinth,
            project_name: "P".into(),
            version: "1".into(),
            files: vec![pack_file("a"), rp],
        };
        let installed = vec![installed("a", true)];
        let s = compute_status(origin, &installed);
        // "a" is present, the resourcepack should be filtered out (not
        // counted as removed) — so the pack is "clean".
        assert!(!s.is_modified);
        assert!(s.removed_files.is_empty());
        assert_eq!(s.origin.files.len(), 1);
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
        };
        let mut m = installed("ABC", true);
        m.sha1 = "abc".into();
        let s = compute_status(origin, &[m]);
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

    #[test]
    fn pack_origin_lookup_finds_file_by_lowercase_sha() {
        let origin = PackOrigin {
            project_id: None,
            source: ModSource::Modrinth,
            project_name: "P".into(),
            version: "1".into(),
            files: vec![pack_file("AbCdEf"), pack_file("zzz")],
        };
        let mut wanted = origin.files[0].clone();
        wanted.sha1 = "AbCdEf".into();
        let found = origin.files.iter().find(|f| f.sha1.eq_ignore_ascii_case("abcdef"));
        assert!(found.is_some());
        let none = origin.files.iter().find(|f| f.sha1.eq_ignore_ascii_case("missing"));
        assert!(none.is_none());
    }
}
