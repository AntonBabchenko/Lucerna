use async_trait::async_trait;

use crate::error::Error;
use crate::mods::modpack::ftb_api::FtbTarget;
use crate::mods::modpack::schema::{
    ModpackProject, ModpackSearchPage, ModpackSort, ModpackSummary, ModpackUnresolvable,
    ModpackVersionEntry, UnresolvableReason,
};
use crate::mods::modpack::source::{ModpackSource, SourceCaps};
use crate::mods::platform::{GalleryImage, LoaderKind, ModSource};

pub struct FtbModpackSource;

const FTB_BASE: &str = "https://api.modpacks.ch";

/// Upper bound on ids fetched per FTB search/popular call. Chosen to exceed the
/// entire FTB-native catalogue (~90 packs as of 2026) so a single fetch returns
/// the complete result set, letting us report an exact total and page through it
/// client-side. If FTB's catalogue ever grows past this, results beyond the cap
/// are simply not browsable — the same deep-paging cap Modrinth/CurseForge apply.
const FTB_FETCH_LIMIT: u32 = 200;

// ── helpers ───────────────────────────────────────────────────────────────────

/// Map an FTB modloader target name to a `LoaderKind`, if recognised.
fn loader_kind_from_name(name: &str) -> Option<LoaderKind> {
    match name {
        "forge" => Some(LoaderKind::Forge),
        "fabric" => Some(LoaderKind::Fabric),
        "neoforge" => Some(LoaderKind::NeoForge),
        "quilt" => Some(LoaderKind::Quilt),
        _ => None,
    }
}

/// Find the minecraft-game target version for a slice of `FtbTarget`s.
fn mc_version_from_targets(targets: &[FtbTarget]) -> Option<String> {
    targets
        .iter()
        .find(|t| t.target_type == "game" && t.name == "minecraft")
        .map(|t| t.version.clone())
}

/// Search FTB modpacks, page, filter, and return a `ModpackSearchPage`.
///
/// FTB search returns ALL matching ids with no server-side offset paging;
/// filters are applied client-side after fetching pack details.
pub(crate) async fn search_impl(
    base: &str,
    query: &str,
    page: u32,
    mc: Option<&str>,
    loader: Option<LoaderKind>,
    page_size: u32,
) -> Result<ModpackSearchPage, Error> {
    use crate::mods::modpack::ftb_api;

    // FTB has no server-side offset paging, and its response `total` field is
    // unusable for paging: the search endpoint mixes FTB + CurseForge counts
    // into it, and the popular endpoint returns `limit - 1` ids for some limits
    // (e.g. asking 21 yields 20), which silently defeats any per-page "n+1
    // sentinel" probe. The redeeming fact is that the FTB-native catalogue is
    // tiny (~90 packs total), so a single generously-capped id fetch returns the
    // COMPLETE result set. We treat the returned id count as the exact total and
    // slice the requested page client-side — giving Modrinth-style exact page
    // counts and a reachable last page. Detail is fetched only for the page slice.
    let all_ids = if query.trim().is_empty() {
        // Default browse (empty query): FTB's search endpoint rejects empty/short
        // terms ("Search term too short."), so fall back to the most-installed
        // list — matching how Modrinth/CurseForge show popular packs first.
        ftb_api::popular_ids(base, FTB_FETCH_LIMIT).await?
    } else {
        ftb_api::search_ids(base, query, FTB_FETCH_LIMIT).await?
    };
    // Exact total of FTB-native results (the cap exceeds the whole catalogue, so
    // nothing is truncated in practice). NOTE: when a client-side mc/loader
    // filter is active the displayed page may hold fewer than page_size hits and
    // this total over-counts — FTB cannot server-filter (supports_server_filter
    // = false), so that remains best-effort. Unfiltered browse paging is exact.
    let total = all_ids.len() as u32;
    let page_ids: Vec<u64> = all_ids
        .into_iter()
        .skip((page * page_size) as usize)
        .take(page_size as usize)
        .collect();

    // N+1 bounded by page_size: fetch detail for each id in the page slice
    // sequentially. A sequential loop is acceptable and avoids the `futures` crate.
    let mut hits = Vec::with_capacity(page_ids.len());
    for id in page_ids {
        match ftb_api::pack_detail(base, id).await {
            Err(e) => {
                // One bad pack shouldn't kill the page — skip and log.
                crate::diag!("[ftb] skipping pack {id}: {e}");
                continue;
            }
            Ok(detail) => {
                // icon_url: prefer art_type == "square" (FTB icon), else first art.
                let icon_url = detail
                    .art
                    .iter()
                    .find(|a| a.art_type == "square")
                    .or_else(|| detail.art.first())
                    .map(|a| a.url.clone());

                // latest_mc_version: from the NEWEST version (max by updated).
                let latest_mc_version = detail
                    .versions
                    .iter()
                    .max_by_key(|v| v.updated)
                    .and_then(|v| mc_version_from_targets(&v.targets));

                // supported_loaders: union of modloader target names across all
                // versions, mapped to LoaderKind, deduped.
                let mut supported_loaders: Vec<LoaderKind> = Vec::new();
                for ver in &detail.versions {
                    for t in &ver.targets {
                        if t.target_type == "modloader" {
                            if let Some(kind) = loader_kind_from_name(&t.name) {
                                if !supported_loaders.contains(&kind) {
                                    supported_loaders.push(kind);
                                }
                            }
                        }
                    }
                }

                hits.push(crate::mods::modpack::schema::ModpackHit {
                    project_id: id.to_string(),
                    slug: id.to_string(),
                    title: detail.name,
                    description: detail.synopsis,
                    icon_url,
                    downloads: detail.installs as f64,
                    latest_mc_version,
                    supported_loaders,
                    source: ModSource::Ftb,
                    distribution_allowed: None,
                    author: detail.authors.first().map(|a| a.name.clone()),
                });
            }
        }
    }

    // caps.supports_server_filter=false — FTB filters are best-effort client-side.
    // TODO(ftb): mc filter keys on latest_mc_version only — a pack with an older
    // version matching the filter but a newer latest is under-selected. Acceptable
    // for v1; scan all version game-targets if this proves too coarse.
    if let Some(mc_ver) = mc {
        hits.retain(|h| {
            h.latest_mc_version
                .as_deref()
                .map(|v| v == mc_ver)
                .unwrap_or(false)
        });
    }
    if let Some(want_loader) = loader {
        hits.retain(|h| h.supported_loaders.contains(&want_loader));
    }

    Ok(ModpackSearchPage {
        hits,
        total,
        offset: page * page_size,
        limit: page_size,
    })
}

/// Return a version list for the given FTB pack id.
pub(crate) async fn get_versions_impl(
    base: &str,
    project_id: &str,
) -> Result<Vec<ModpackVersionEntry>, Error> {
    use crate::mods::modpack::ftb_api;

    let id: u64 = project_id
        .parse()
        .map_err(|_| Error::ModpackManifestInvalid {
            format: "ftb".into(),
            details: "non-numeric pack id".into(),
        })?;
    let detail = ftb_api::pack_detail(base, id).await?;

    let entries = detail
        .versions
        .iter()
        .map(|v| {
            let game_versions: Vec<String> = v
                .targets
                .iter()
                .filter(|t| t.target_type == "game" && t.name == "minecraft")
                .map(|t| t.version.clone())
                .collect();

            let loaders: Vec<String> = v
                .targets
                .iter()
                .filter(|t| t.target_type == "modloader")
                .map(|t| t.name.clone())
                .collect();

            let date_published = chrono::DateTime::from_timestamp(v.updated, 0)
                .map(|d| d.to_rfc3339())
                .unwrap_or_default();

            ModpackVersionEntry {
                id: v.id.to_string(),
                name: v.name.clone(),
                version_number: v.name.clone(),
                game_versions,
                loaders,
                date_published,
            }
        })
        .collect();

    Ok(entries)
}

/// Return project detail (body HTML + gallery) for the given FTB pack id.
pub(crate) async fn get_project_impl(
    base: &str,
    project_id: &str,
) -> Result<ModpackProject, Error> {
    use crate::mods::modpack::ftb_api;
    use crate::mods::render;

    let id: u64 = project_id
        .parse()
        .map_err(|_| Error::ModpackManifestInvalid {
            format: "ftb".into(),
            details: "non-numeric pack id".into(),
        })?;
    let detail = ftb_api::pack_detail(base, id).await?;

    let md_src = if !detail.description.is_empty() {
        &detail.description
    } else {
        &detail.synopsis
    };
    let body_html = render::markdown_to_safe_html(md_src);

    let gallery: Vec<GalleryImage> = detail
        .art
        .iter()
        .filter(|a| render::is_safe_image_url(&a.url))
        .map(|a| GalleryImage {
            url: a.url.clone(),
            title: None,
        })
        .collect();

    // Use the first link entry with type "website" and a valid https:// URL.
    let website_url = detail
        .links
        .iter()
        .find(|l| l.link_type == "website" && l.link.starts_with("https://"))
        .map(|l| l.link.clone());

    Ok(ModpackProject {
        body_html,
        gallery,
        website_url,
    })
}

/// CurseForge API base for CF-ref resolution in FTB packs.
const CF_BASE: &str = "https://api.curseforge.com";

/// Bulk-resolve any `ModpackFile` entries in `summary` that have
/// `source == Curseforge` and an empty url (i.e. FTB CF-ref placeholders
/// emitted by `ftb_map::map_version`).
///
/// - If a CF API key is present: POST to the CF bulk-files endpoint and fill
///   each file's url. Files whose `downloadUrl` is `null` (distribution
///   disabled) are moved to `summary.unresolvable`.
/// - If no CF API key is available: all CF-ref placeholder files are moved to
///   `summary.unresolvable` with `reason: HostNotAllowed` (closest existing
///   reason for "couldn't fetch") and a manual CurseForge project URL so the
///   import picker can show them with an "Open on CurseForge" link.
///
/// FTB cf-ref files need a CurseForge API key to resolve; without one they
/// degrade to manual.
pub(crate) async fn resolve_cf_refs(
    summary: &mut ModpackSummary,
    cf_base: &str,
    key: Option<&str>,
) {
    use crate::mods::modpack::cf_api;

    // Collect indices of CF-ref placeholder files (source=Curseforge, empty url).
    let cf_indices: Vec<usize> = summary
        .files
        .iter()
        .enumerate()
        .filter(|(_, f)| f.source == ModSource::Curseforge && f.url.is_empty())
        .map(|(i, _)| i)
        .collect();

    if cf_indices.is_empty() {
        return;
    }

    // Collect CF file ids in the same order as cf_indices.
    let file_ids: Vec<u64> = cf_indices
        .iter()
        .filter_map(|&i| summary.files[i].version_id.parse::<u64>().ok())
        .collect();

    if key.is_none() {
        // No key — move all CF-ref placeholders to unresolvable (manual download).
        // Process in reverse index order so removals don't shift earlier indices.
        for &idx in cf_indices.iter().rev() {
            let f = summary.files.remove(idx);
            summary.unresolvable.push(ModpackUnresolvable {
                reason: UnresolvableReason::HostNotAllowed,
                mod_name: f.name,
                manual_action_url: format!("https://www.curseforge.com/projects/{}", f.project_id),
                filename: f.filename,
                size: f.size,
                sha1: if f.sha1.is_empty() {
                    None
                } else {
                    Some(f.sha1)
                },
                project_id: Some(f.project_id),
            });
        }
        return;
    }

    // Attempt bulk resolution.
    let resolved = match cf_api::resolve_files(cf_base, key, &file_ids).await {
        Ok(map) => map,
        Err(_) => {
            // Network/decode failure — degrade all CF-ref files to unresolvable.
            for &idx in cf_indices.iter().rev() {
                let f = summary.files.remove(idx);
                summary.unresolvable.push(ModpackUnresolvable {
                    reason: UnresolvableReason::HostNotAllowed,
                    mod_name: f.name,
                    manual_action_url: format!(
                        "https://www.curseforge.com/projects/{}",
                        f.project_id
                    ),
                    filename: f.filename,
                    size: f.size,
                    sha1: if f.sha1.is_empty() {
                        None
                    } else {
                        Some(f.sha1)
                    },
                    project_id: Some(f.project_id),
                });
            }
            return;
        }
    };

    // Apply resolutions. Collect indices to remove (distribution-disabled) first,
    // then remove in reverse order.
    let mut to_remove: Vec<usize> = Vec::new();

    for &idx in &cf_indices {
        let f = &mut summary.files[idx];
        let file_id: u64 = match f.version_id.parse() {
            Ok(id) => id,
            Err(_) => {
                to_remove.push(idx);
                continue;
            }
        };
        match resolved.get(&file_id) {
            Some(r) => {
                match &r.download_url {
                    Some(url) => {
                        // Fill the placeholder url.
                        f.url = url.clone();
                        // Backfill sha1 from CF if FTB didn't provide one.
                        if f.sha1.is_empty() {
                            if let Some(ref h) = r.sha1 {
                                f.sha1 = h.clone();
                            }
                        }
                        // no-TOFU (B.6): CF returned a URL but no checksum and FTB
                        // provided none — refuse to install an unverifiable file.
                        if f.sha1.trim().is_empty() {
                            to_remove.push(idx);
                        }
                    }
                    // distribution_disabled (None download_url).
                    None => {
                        to_remove.push(idx);
                    }
                }
            }
            // Absent from response — treat as distribution-disabled.
            None => {
                to_remove.push(idx);
            }
        }
    }

    // Move distribution-disabled (and no-TOFU-rejected) files to unresolvable
    // (reverse order to preserve earlier indices).
    to_remove.sort_unstable();
    for &idx in to_remove.iter().rev() {
        let f = summary.files.remove(idx);
        // Determine whether this is a no-TOFU reject (url set but sha1 missing)
        // or a genuine distribution-disabled / absent case.
        let (reason, cf_sha1) = if !f.url.is_empty() {
            // URL was filled but sha1 is missing after all backfill attempts —
            // no-TOFU rejection: we refuse to install an unverifiable file.
            (UnresolvableReason::MissingChecksum, None)
        } else {
            // distribution_disabled or absent from CF response — try to recover
            // sha1 from the resolved map for the unresolvable entry.
            let sha1 = f
                .version_id
                .parse::<u64>()
                .ok()
                .and_then(|id| resolved.get(&id))
                .and_then(|r| r.sha1.clone());
            (UnresolvableReason::DistributionDisabled, sha1)
        };
        let sha1 = if !f.sha1.is_empty() {
            Some(f.sha1)
        } else {
            cf_sha1
        };
        summary.unresolvable.push(ModpackUnresolvable {
            reason,
            mod_name: f.name,
            manual_action_url: format!("https://www.curseforge.com/projects/{}", f.project_id),
            filename: f.filename,
            size: f.size,
            sha1,
            project_id: Some(f.project_id),
        });
    }
}

/// Download the FTB version manifest, map it to `ModpackSummary`, serialise to
/// a `.ftbpack.json` sidecar in the OS temp dir, and return the path.
pub(crate) async fn stage_impl(
    app: &tauri::AppHandle,
    base: &str,
    project_id: &str,
    version_id: &str,
) -> Result<String, Error> {
    use crate::mods::modpack::{ftb_api, ftb_map};

    let id: u64 = project_id
        .parse()
        .map_err(|_| Error::ModpackManifestInvalid {
            format: "ftb".into(),
            details: "non-numeric pack id".into(),
        })?;
    let vid: u64 = version_id
        .parse()
        .map_err(|_| Error::ModpackManifestInvalid {
            format: "ftb".into(),
            details: "non-numeric version id".into(),
        })?;

    let detail = ftb_api::pack_detail(base, id).await?;

    // Find the version ref to get its display name; fall back to version_id string.
    let version_name = detail
        .versions
        .iter()
        .find(|v| v.id == vid)
        .map(|v| v.name.clone())
        .unwrap_or_else(|| version_id.to_string());

    let manifest = ftb_api::version_manifest(base, id, vid).await?;
    let mut summary = ftb_map::map_version(&detail.name, &version_name, &manifest);

    // Resolve any CurseForge-ref files (empty-url placeholders with source=Curseforge).
    let cf_key = crate::mods::curseforge::keyring::resolve();
    resolve_cf_refs(&mut summary, CF_BASE, cf_key.as_deref()).await;

    // Serialise ModpackSummary and write to temp sidecar.
    // Filename MUST end with .ftbpack.json (Task 9 detects FTB by this extension).
    // write_to_temp produces <uuid>.ftbpack.json when ext = "ftbpack.json".
    let json = serde_json::to_vec(&summary).map_err(|e| Error::ModsDecode {
        platform: "ftb".into(),
        details: e.to_string(),
    })?;

    crate::mods::modpack::source::stage::write_to_temp(app, &json, "ftbpack.json").await
}

// ── trait impl ────────────────────────────────────────────────────────────────

#[async_trait]
impl ModpackSource for FtbModpackSource {
    fn caps(&self) -> SourceCaps {
        SourceCaps {
            needs_api_key: false,
            supports_server_filter: false,
            can_export: false,
        }
    }

    async fn search(
        &self,
        query: &str,
        page: u32,
        mc_version: Option<&str>,
        loader: Option<LoaderKind>,
        _sort: ModpackSort,
        page_size: u32,
    ) -> Result<ModpackSearchPage, Error> {
        search_impl(FTB_BASE, query, page, mc_version, loader, page_size).await
    }

    async fn get_versions(&self, project_id: &str) -> Result<Vec<ModpackVersionEntry>, Error> {
        get_versions_impl(FTB_BASE, project_id).await
    }

    async fn get_project(&self, project_id: &str) -> Result<ModpackProject, Error> {
        get_project_impl(FTB_BASE, project_id).await
    }

    async fn stage_version_to_temp(
        &self,
        app: &tauri::AppHandle,
        project_id: &str,
        version_id: &str,
    ) -> Result<String, Error> {
        stage_impl(app, FTB_BASE, project_id, version_id).await
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::modpack::schema::{EnvSupport, ModpackFile, ModpackFormat, ModpackSummary};
    use crate::mods::platform::{LoaderKind, ModSource};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::test_env_lock()
    }

    /// Build a minimal `ModpackSummary` with one CF-ref placeholder file.
    fn summary_with_cf_placeholder(project_id: &str, file_id: &str, sha1: &str) -> ModpackSummary {
        ModpackSummary {
            format: ModpackFormat::Ftb,
            name: "Test Pack".into(),
            version: "1.0".into(),
            game_version: "1.20.1".into(),
            loader: LoaderKind::Forge,
            loader_version: Some("47.2.0".into()),
            files: vec![ModpackFile {
                project_id: project_id.into(),
                version_id: file_id.into(),
                name: "ae2.jar".into(),
                filename: "ae2.jar".into(),
                install_path: "mods/ae2.jar".into(),
                sha1: sha1.into(),
                md5: None,
                url: String::new(), // placeholder
                size: 1024.0,
                env_client: EnvSupport::Required,
                source: ModSource::Curseforge,
            }],
            unresolvable: vec![],
            has_overrides: false,
            has_client_overrides: false,
            has_saves_in_overrides: false,
        }
    }

    /// resolve_cf_refs fills the url and backfills sha1 when the CF API returns
    /// a download URL.
    #[tokio::test]
    async fn resolve_cf_refs_fills_url_and_backfills_sha1() {
        let s = MockServer::start().await;
        let body = serde_json::json!({
            "data": [{
                "id": 4499899u64,
                "downloadUrl": "https://edge.forgecdn.net/files/4/4/ae2.jar",
                "hashes": [{ "value": "aabbccdd", "algo": 1 }]
            }]
        });
        Mock::given(method("POST"))
            .and(path("/v1/mods/files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&s)
            .await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        // sha1 empty — CF should backfill it.
        let mut summary = summary_with_cf_placeholder("238222", "4499899", "");
        resolve_cf_refs(&mut summary, &s.uri(), Some("test-key")).await;

        assert_eq!(summary.files.len(), 1);
        assert!(summary.unresolvable.is_empty());
        assert_eq!(
            summary.files[0].url,
            "https://edge.forgecdn.net/files/4/4/ae2.jar"
        );
        assert_eq!(
            summary.files[0].sha1, "aabbccdd",
            "sha1 must be backfilled from CF"
        );
    }

    /// resolve_cf_refs preserves an existing FTB sha1 even when CF also returns one.
    #[tokio::test]
    async fn resolve_cf_refs_preserves_existing_sha1() {
        let s = MockServer::start().await;
        let body = serde_json::json!({
            "data": [{
                "id": 4499899u64,
                "downloadUrl": "https://edge.forgecdn.net/files/4/4/ae2.jar",
                "hashes": [{ "value": "cf-sha1", "algo": 1 }]
            }]
        });
        Mock::given(method("POST"))
            .and(path("/v1/mods/files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&s)
            .await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        // FTB sha1 already present — must be kept.
        let mut summary = summary_with_cf_placeholder("238222", "4499899", "ftb-sha1");
        resolve_cf_refs(&mut summary, &s.uri(), Some("test-key")).await;

        assert_eq!(
            summary.files[0].sha1, "ftb-sha1",
            "FTB sha1 must not be overwritten"
        );
    }

    /// resolve_cf_refs moves a distribution-disabled file to unresolvable.
    #[tokio::test]
    async fn resolve_cf_refs_distribution_disabled_moves_to_unresolvable() {
        let s = MockServer::start().await;
        let body = serde_json::json!({
            "data": [{
                "id": 4499899u64,
                "downloadUrl": null,
                "hashes": [{ "value": "deadbeef", "algo": 1 }]
            }]
        });
        Mock::given(method("POST"))
            .and(path("/v1/mods/files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&s)
            .await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let mut summary = summary_with_cf_placeholder("238222", "4499899", "");
        resolve_cf_refs(&mut summary, &s.uri(), Some("test-key")).await;

        assert!(
            summary.files.is_empty(),
            "distribution-disabled file must leave files"
        );
        assert_eq!(summary.unresolvable.len(), 1);
        let u = &summary.unresolvable[0];
        assert!(matches!(u.reason, UnresolvableReason::DistributionDisabled));
        assert!(u.manual_action_url.contains("238222"));
        assert_eq!(u.project_id.as_deref(), Some("238222"));
        // sha1 backfilled from CF response
        assert_eq!(u.sha1.as_deref(), Some("deadbeef"));
    }

    /// resolve_cf_refs moves a file to unresolvable when CF returns a downloadUrl
    /// but provides no sha1 hash and FTB had none either (no-TOFU, B.6).
    #[tokio::test]
    async fn resolve_cf_refs_url_without_sha1_is_unresolvable() {
        let s = MockServer::start().await;
        // CF returns a downloadUrl but an empty hashes array — no sha1.
        let body = serde_json::json!({
            "data": [{
                "id": 4499899u64,
                "downloadUrl": "https://edge.forgecdn.net/files/4/4/ae2.jar",
                "hashes": []
            }]
        });
        Mock::given(method("POST"))
            .and(path("/v1/mods/files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&s)
            .await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        // FTB also has no sha1.
        let mut summary = summary_with_cf_placeholder("238222", "4499899", "");
        resolve_cf_refs(&mut summary, &s.uri(), Some("test-key")).await;

        assert!(
            summary.files.is_empty(),
            "url-without-sha1 file must not stay in files (no-TOFU)"
        );
        assert_eq!(summary.unresolvable.len(), 1);
        let u = &summary.unresolvable[0];
        assert!(
            matches!(u.reason, UnresolvableReason::MissingChecksum),
            "expected MissingChecksum for no-TOFU rejection (url present, sha1 absent), got {:?}",
            u.reason
        );
        assert!(u.manual_action_url.contains("238222"));
        assert_eq!(u.project_id.as_deref(), Some("238222"));
    }

    /// resolve_cf_refs with no CF key moves all CF-ref files to unresolvable
    /// with HostNotAllowed reason.
    #[tokio::test]
    async fn resolve_cf_refs_no_key_degrades_to_unresolvable() {
        let mut summary = summary_with_cf_placeholder("238222", "4499899", "abc");
        resolve_cf_refs(&mut summary, "http://127.0.0.1:1", None).await;
        assert!(
            summary.files.is_empty(),
            "no-key path must move CF-ref files out"
        );
        assert_eq!(summary.unresolvable.len(), 1);
        let u = &summary.unresolvable[0];
        assert!(matches!(u.reason, UnresolvableReason::HostNotAllowed));
        assert!(u.manual_action_url.contains("238222"));
        assert_eq!(u.sha1.as_deref(), Some("abc"));
        assert_eq!(u.project_id.as_deref(), Some("238222"));
    }

    /// resolve_cf_refs is a no-op when there are no CF-ref placeholders.
    #[tokio::test]
    async fn resolve_cf_refs_noop_when_no_cf_placeholders() {
        let mut summary = ModpackSummary {
            format: ModpackFormat::Ftb,
            name: "P".into(),
            version: "1".into(),
            game_version: "1.20.1".into(),
            loader: LoaderKind::Fabric,
            loader_version: None,
            files: vec![ModpackFile {
                project_id: "1001".into(),
                version_id: "abc".into(),
                name: "sodium.jar".into(),
                filename: "sodium.jar".into(),
                install_path: "mods/sodium.jar".into(),
                sha1: "abc".into(),
                md5: None,
                url: "https://dist.modpacks.ch/x/sodium.jar".into(),
                size: 100.0,
                env_client: EnvSupport::Required,
                source: ModSource::Ftb,
            }],
            unresolvable: vec![],
            has_overrides: false,
            has_client_overrides: false,
            has_saves_in_overrides: false,
        };
        resolve_cf_refs(&mut summary, "http://127.0.0.1:1", None).await;
        assert_eq!(
            summary.files.len(),
            1,
            "non-CF-ref files must not be touched"
        );
        assert!(summary.unresolvable.is_empty());
    }

    fn pack_detail_json(id: u64, mc: &str, loader: &str, installs: u64) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "name": format!("Pack {id}"),
            "synopsis": format!("Synopsis for pack {id}"),
            "description": format!("Full description for pack {id}"),
            "art": [
                {
                    "url": format!("https://dist.modpacks.ch/packs/{id}/art/square.png"),
                    "type": "square"
                },
                {
                    "url": format!("https://dist.modpacks.ch/packs/{id}/art/wide.png"),
                    "type": "wide"
                }
            ],
            "authors": [{ "name": "Test Author" }],
            "installs": installs,
            "versions": [
                {
                    "id": 6495u64,
                    "name": "1.7.0",
                    "type": "Release",
                    "targets": [
                        {
                            "version": "36.2.39",
                            "id": 736u64,
                            "name": loader,
                            "type": "modloader",
                            "updated": 1700000002i64
                        },
                        {
                            "version": mc,
                            "id": 100u64,
                            "name": "minecraft",
                            "type": "game",
                            "updated": 1700000003i64
                        }
                    ],
                    "updated": 1700000010i64
                }
            ]
        })
    }

    /// Search fetches pack detail for each id, builds a ModpackHit per
    /// detail, and tags hits with source=Ftb.
    #[tokio::test]
    async fn ftb_search_fetches_detail_per_id() {
        let s = MockServer::start().await;

        // Single fixed-cap fetch covering the whole catalogue → path "200".
        Mock::given(method("GET"))
            .and(path("/public/modpack/search/200"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({ "packs": [91u64], "curseforge": [], "total": 1 }),
            ))
            .mount(&s)
            .await;

        Mock::given(method("GET"))
            .and(path("/public/modpack/91"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(pack_detail_json(91, "1.16.5", "forge", 500_000)),
            )
            .mount(&s)
            .await;

        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let page = search_impl(&s.uri(), "test", 0, None, None, 20)
            .await
            .unwrap();

        assert_eq!(page.hits.len(), 1);
        let hit = &page.hits[0];
        assert_eq!(hit.source, ModSource::Ftb);
        assert_eq!(hit.project_id, "91");
        assert_eq!(hit.title, "Pack 91");
        assert_eq!(hit.author.as_deref(), Some("Test Author"));
        assert_eq!(hit.latest_mc_version, Some("1.16.5".to_string()));
        assert!(
            hit.supported_loaders.contains(&LoaderKind::Forge),
            "expected Forge in supported_loaders, got {:?}",
            hit.supported_loaders
        );
        // square art is preferred as icon
        assert!(
            hit.icon_url.as_deref().unwrap_or("").contains("square.png"),
            "expected square art URL, got {:?}",
            hit.icon_url
        );
    }

    /// Client-side MC version filter: two packs targeting different MC
    /// versions — only the matching one is returned when mc is set.
    #[tokio::test]
    async fn ftb_search_filters_mc_client_side() {
        let s = MockServer::start().await;

        // Single fixed-cap fetch covering the whole catalogue → path "200".
        Mock::given(method("GET"))
            .and(path("/public/modpack/search/200"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({ "packs": [91u64, 92u64], "curseforge": [], "total": 2 }),
            ))
            .mount(&s)
            .await;

        Mock::given(method("GET"))
            .and(path("/public/modpack/91"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(pack_detail_json(91, "1.16.5", "forge", 100)),
            )
            .mount(&s)
            .await;

        Mock::given(method("GET"))
            .and(path("/public/modpack/92"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(pack_detail_json(92, "1.20.1", "fabric", 200)),
            )
            .mount(&s)
            .await;

        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let page = search_impl(&s.uri(), "test", 0, Some("1.20.1"), None, 20)
            .await
            .unwrap();

        assert_eq!(
            page.hits.len(),
            1,
            "only the 1.20.1 pack should survive the client-side filter"
        );
        assert_eq!(page.hits[0].project_id, "92");
    }

    /// get_versions maps FtbVersionRef fields into ModpackVersionEntry correctly.
    #[tokio::test]
    async fn ftb_get_versions_maps_fields() {
        let s = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/public/modpack/91"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(pack_detail_json(91, "1.16.5", "forge", 500_000)),
            )
            .mount(&s)
            .await;

        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let versions = get_versions_impl(&s.uri(), "91").await.unwrap();

        assert_eq!(versions.len(), 1);
        let v = &versions[0];
        assert_eq!(v.id, "6495");
        assert_eq!(v.name, "1.7.0");
        assert_eq!(v.version_number, "1.7.0");
        assert_eq!(v.game_versions, vec!["1.16.5"]);
        assert_eq!(v.loaders, vec!["forge"]);
        assert!(!v.date_published.is_empty(), "date_published must be set");
    }

    /// get_project maps body_html from description + gallery from art.
    #[tokio::test]
    async fn ftb_get_project_maps_detail() {
        let s = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/public/modpack/91"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(pack_detail_json(91, "1.16.5", "forge", 500_000)),
            )
            .mount(&s)
            .await;

        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let proj = get_project_impl(&s.uri(), "91").await.unwrap();

        assert!(!proj.body_html.is_empty(), "body_html must be non-empty");
        // Both art entries are https:// — both should appear in gallery.
        assert_eq!(
            proj.gallery.len(),
            2,
            "expected 2 gallery images (both https art entries)"
        );
        assert!(proj.gallery.iter().all(|g| g.url.starts_with("https://")));
    }

    /// get_project sets website_url from the first link with type "website"
    /// and an https:// URL.
    #[tokio::test]
    async fn ftb_get_project_sets_website_from_links() {
        let s = MockServer::start().await;

        let resp = serde_json::json!({
            "id": 91,
            "name": "FTB Presents Direwolf20 1.16",
            "synopsis": "Synopsis",
            "description": "Full description.",
            "art": [],
            "authors": [{ "name": "Direwolf20" }],
            "installs": 100,
            "versions": [],
            "links": [
                { "name": "Website", "link": "https://feed-the-beast.com/modpack/91", "type": "website" },
                { "name": "Discord", "link": "https://discord.gg/ftb", "type": "discord" }
            ]
        });
        Mock::given(method("GET"))
            .and(path("/public/modpack/91"))
            .respond_with(ResponseTemplate::new(200).set_body_json(resp))
            .mount(&s)
            .await;

        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let proj = get_project_impl(&s.uri(), "91").await.unwrap();

        assert_eq!(
            proj.website_url,
            Some("https://feed-the-beast.com/modpack/91".to_string()),
            "website_url must be populated from the website link"
        );
    }

    /// get_project returns website_url = None when there is no website link.
    #[tokio::test]
    async fn ftb_get_project_no_website_link_yields_none() {
        let s = MockServer::start().await;

        let resp = serde_json::json!({
            "id": 92,
            "name": "Some Pack",
            "synopsis": "Synopsis",
            "description": "",
            "art": [],
            "authors": [],
            "installs": 0,
            "versions": [],
            "links": [
                { "name": "Discord", "link": "https://discord.gg/some", "type": "discord" }
            ]
        });
        Mock::given(method("GET"))
            .and(path("/public/modpack/92"))
            .respond_with(ResponseTemplate::new(200).set_body_json(resp))
            .mount(&s)
            .await;

        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let proj = get_project_impl(&s.uri(), "92").await.unwrap();

        assert!(
            proj.website_url.is_none(),
            "website_url must be None when no website link is present"
        );
    }

    /// Non-numeric project_id returns ModpackManifestInvalid.
    #[tokio::test]
    async fn ftb_get_versions_non_numeric_id_errors() {
        let _g = test_lock();
        let result = get_versions_impl("http://unused", "not-a-number").await;
        assert!(
            matches!(result, Err(Error::ModpackManifestInvalid { .. })),
            "expected ModpackManifestInvalid, got: {result:?}"
        );
    }

    /// Page 1 (0-indexed) with page_size=2: the full id list is fetched once
    /// (fixed cap) and sliced skip(2).take(2) client-side.
    ///
    /// With search returning ids [1, 2, 3], page 1 should have 1 hit (id 3),
    /// offset=2, total=3.
    #[tokio::test]
    async fn ftb_search_page_one_returns_second_window() {
        let s = MockServer::start().await;

        // Single fixed-cap fetch covering the whole catalogue → path "200".
        Mock::given(method("GET"))
            .and(path("/public/modpack/search/200"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({ "packs": [1u64, 2u64, 3u64], "curseforge": [], "total": 3 }),
            ))
            .mount(&s)
            .await;

        // Only id 3 is on page 1 after skip(2).take(2)
        Mock::given(method("GET"))
            .and(path("/public/modpack/3"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(pack_detail_json(3, "1.20.1", "fabric", 42)),
            )
            .mount(&s)
            .await;

        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let page = search_impl(&s.uri(), "test", 1, None, None, 2)
            .await
            .unwrap();

        assert_eq!(
            page.hits.len(),
            1,
            "page 1 with ids [1,2,3] and page_size=2 should yield 1 hit"
        );
        assert_eq!(page.hits[0].project_id, "3");
        assert_eq!(page.offset, 2, "offset should be page * page_size = 2");
        assert_eq!(page.total, 3, "total should be unfiltered id count = 3");
    }

    /// Regression: when more packs exist than fit on the current page, `total`
    /// must exceed `(page + 1) * page_size` so the UI pager keeps "Next" enabled.
    /// Earlier the fetched window matched the page size exactly (and FTB's popular
    /// endpoint even returns `limit - 1`), so `total` never signalled more pages
    /// and the pager could not advance past page 0 for FTB.
    #[tokio::test]
    async fn ftb_search_total_signals_more_pages_exist() {
        let s = MockServer::start().await;

        // Single fixed-cap fetch returns the whole result set [1,2,3]; with
        // page_size 2 that is an exact total of 3 → 2 pages, Next reachable.
        Mock::given(method("GET"))
            .and(path("/public/modpack/search/200"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({ "packs": [1u64, 2u64, 3u64], "curseforge": [], "total": 3 }),
            ))
            .mount(&s)
            .await;
        // Only the first 2 ids are on page 0 (take(page_size)); detail is fetched
        // for those, never for the off-page id 3.
        for id in [1u64, 2u64] {
            Mock::given(method("GET"))
                .and(path(format!("/public/modpack/{id}")))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(pack_detail_json(id, "1.20.1", "fabric", 10)),
                )
                .mount(&s)
                .await;
        }

        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let page = search_impl(&s.uri(), "test", 0, None, None, 2)
            .await
            .unwrap();

        assert_eq!(page.hits.len(), 2, "page 0 shows page_size hits");
        assert!(
            page.total > 2,
            "total ({}) must exceed page_size so the pager enables Next",
            page.total
        );
        // The exact gating the UI applies: Next is disabled when
        // (page + 1) * page_size >= total. Here page=0, page_size=2, so the
        // threshold is 2; 2 >= 3 is false → Next is enabled.
        let page_num: u32 = 0;
        let page_size: u32 = 2;
        assert!(
            (page_num + 1) * page_size < page.total,
            "(page+1)*page_size must be < total for Next to be reachable"
        );
    }

    /// Regression (manual-test gap): an empty query must populate the default
    /// browse from the FTB *popular* endpoint, NOT call /search with an empty
    /// term (which FTB rejects with "Search term too short."). Only the popular
    /// mock is mounted — if search_impl wrongly hit /search, no detail would be
    /// fetched and the page would be empty, failing this test.
    #[tokio::test]
    async fn ftb_empty_query_uses_popular_endpoint() {
        let s = MockServer::start().await;

        // Single fixed-cap popular fetch → path "200".
        Mock::given(method("GET"))
            .and(path("/public/modpack/popular/installs/200"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({ "packs": [91u64], "total": 1, "status": "success" }),
            ))
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/public/modpack/91"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(pack_detail_json(91, "1.16.5", "forge", 500_000)),
            )
            .mount(&s)
            .await;

        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let page = search_impl(&s.uri(), "", 0, None, None, 20).await.unwrap();

        assert_eq!(page.hits.len(), 1, "empty query should show popular packs");
        assert_eq!(page.hits[0].project_id, "91");
    }

    /// A too-short term makes FTB return HTTP 200 with an error body
    /// (`{"status":"error","message":"Search term too short."}`) that has no
    /// `packs` field. This must degrade to an empty page, never an error.
    #[tokio::test]
    async fn ftb_too_short_term_yields_empty_page_not_error() {
        let s = MockServer::start().await;

        // Single fixed-cap fetch → path "200".
        Mock::given(method("GET"))
            .and(path("/public/modpack/search/200"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({ "status": "error", "message": "Search term too short." }),
            ))
            .mount(&s)
            .await;

        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let page = search_impl(&s.uri(), "ab", 0, None, None, 20)
            .await
            .unwrap();

        assert_eq!(
            page.hits.len(),
            0,
            "too-short term should yield empty, not error"
        );
    }
}
