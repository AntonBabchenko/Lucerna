use async_trait::async_trait;

use crate::error::Error;
use crate::mods::modpack::atl_api::{self, AtlPack};
use crate::mods::modpack::schema::{
    ModpackHit, ModpackProject, ModpackSearchPage, ModpackSort, ModpackVersionEntry,
};
use crate::mods::modpack::source::{ModpackSource, SourceCaps};
use crate::mods::platform::{LoaderKind, ModSource};

pub struct AtlauncherModpackSource;

const ATL_API_BASE: &str = "https://api.atlauncher.com";
const CF_BASE: &str = "https://api.curseforge.com";

/// Resolve sha1 for ATLauncher `direct` mods that point at a CurseForge CDN
/// but carried no md5 in the pack manifest. The forgecdn URL already encodes
/// the CF file id (stored in `version_id`); we fetch the file's sha1 from CF
/// and verify against the existing forgecdn URL at install. Files we cannot
/// get a checksum for are moved to `unresolvable` (no-TOFU) with the working
/// forgecdn URL as the manual action — never installed unverified.
///
/// Predicate for forgecdn-direct files awaiting resolution:
///   `source == Atlauncher && sha1.is_empty() && md5.is_none()`
async fn resolve_forgecdn_sha1(
    summary: &mut crate::mods::modpack::schema::ModpackSummary,
    cf_base: &str,
    key: Option<&str>,
) {
    use crate::mods::modpack::cf_api;
    use crate::mods::modpack::schema::{ModpackUnresolvable, UnresolvableReason};
    use crate::mods::platform::ModSource;

    // Indices of forgecdn-direct files awaiting sha1 resolution.
    let idxs: Vec<usize> = summary
        .files
        .iter()
        .enumerate()
        .filter(|(_, f)| f.source == ModSource::Atlauncher && f.sha1.is_empty() && f.md5.is_none())
        .map(|(i, _)| i)
        .collect();

    if idxs.is_empty() {
        return;
    }

    let file_ids: Vec<u64> = idxs
        .iter()
        .filter_map(|&i| summary.files[i].version_id.parse::<u64>().ok())
        .collect();

    // No key (or empty id set): degrade all to unresolvable, preserving the
    // forgecdn url as the manual action.
    let resolved = match key {
        Some(_) if !file_ids.is_empty() => {
            cf_api::resolve_files(cf_base, key, &file_ids).await.ok()
        }
        _ => None,
    };

    let mut to_remove: Vec<usize> = Vec::new();
    for &i in &idxs {
        let file_id: u64 = match summary.files[i].version_id.parse() {
            Ok(v) => v,
            Err(_) => {
                to_remove.push(i);
                continue;
            }
        };
        match resolved
            .as_ref()
            .and_then(|m| m.get(&file_id))
            .and_then(|r| r.sha1.as_deref())
        {
            Some(sha1) if !sha1.trim().is_empty() => {
                summary.files[i].sha1 = sha1.to_ascii_lowercase();
            }
            _ => to_remove.push(i),
        }
    }

    to_remove.sort_unstable();
    for &i in to_remove.iter().rev() {
        let f = summary.files.remove(i);
        summary.unresolvable.push(ModpackUnresolvable {
            reason: UnresolvableReason::MissingChecksum,
            mod_name: f.name,
            manual_action_url: f.url, // the working forgecdn direct URL
            filename: f.filename,
            size: f.size,
            sha1: None,
            project_id: None,
        });
    }
}

/// Build a ModpackHit from a public AtlPack.
fn pack_to_hit(p: &AtlPack) -> ModpackHit {
    let latest_mc_version = p.versions.first().map(|v| v.minecraft.clone());
    ModpackHit {
        project_id: p.safe_name.clone(),
        slug: p.safe_name.clone(),
        title: p.name.clone(),
        description: p.description.clone(),
        icon_url: None, // ATLauncher API exposes no icon
        downloads: 0.0, // not exposed by the catalogue
        latest_mc_version,
        supported_loaders: Vec::new(), // not derivable without per-version Configs
        source: ModSource::Atlauncher,
        distribution_allowed: None,
        author: None, // ATLauncher public catalogue exposes no author
    }
}

pub(crate) async fn search_impl(
    base: &str,
    query: &str,
    page: u32,
    mc: Option<&str>,
    _loader: Option<LoaderKind>,
    page_size: u32,
) -> Result<ModpackSearchPage, Error> {
    let all = atl_api::all_packs(base).await?;
    let q = query.trim().to_ascii_lowercase();
    let mut hits: Vec<ModpackHit> = all
        .iter()
        .filter(|p| p.pack_type == "public" && !p.versions.is_empty())
        .filter(|p| q.is_empty() || p.name.to_ascii_lowercase().contains(&q))
        .filter(|p| match mc {
            Some(v) => p.versions.iter().any(|ver| ver.minecraft == v),
            None => true,
        })
        .map(pack_to_hit)
        .collect();
    hits.sort_by(|a, b| {
        a.title
            .to_ascii_lowercase()
            .cmp(&b.title.to_ascii_lowercase())
    });
    let total = hits.len() as u32;
    let page_hits: Vec<ModpackHit> = hits
        .into_iter()
        .skip((page * page_size) as usize)
        .take(page_size as usize)
        .collect();
    Ok(ModpackSearchPage {
        hits: page_hits,
        total,
        offset: page * page_size,
        limit: page_size,
    })
}

async fn find_pack(base: &str, safe_name: &str) -> Result<AtlPack, Error> {
    atl_api::all_packs(base)
        .await?
        .into_iter()
        .find(|p| p.safe_name == safe_name)
        .ok_or_else(|| Error::ModpackManifestInvalid {
            format: "atlauncher".into(),
            details: format!("pack not found: {safe_name}"),
        })
}

#[async_trait]
impl ModpackSource for AtlauncherModpackSource {
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
        search_impl(ATL_API_BASE, query, page, mc_version, loader, page_size).await
    }

    async fn get_versions(&self, project_id: &str) -> Result<Vec<ModpackVersionEntry>, Error> {
        let pack = find_pack(ATL_API_BASE, project_id).await?;
        Ok(pack
            .versions
            .iter()
            .map(|v| ModpackVersionEntry {
                id: v.version.clone(),
                name: v.version.clone(),
                version_number: v.version.clone(),
                game_versions: vec![v.minecraft.clone()],
                loaders: Vec::new(),
                date_published: chrono::DateTime::from_timestamp(v.published, 0)
                    .map(|d| d.to_rfc3339())
                    .unwrap_or_default(),
            })
            .collect())
    }

    async fn get_project(&self, project_id: &str) -> Result<ModpackProject, Error> {
        let pack = find_pack(ATL_API_BASE, project_id).await?;
        let body_html = crate::mods::render::markdown_to_safe_html(&pack.description);
        Ok(ModpackProject {
            body_html,
            gallery: Vec::new(),
            website_url: pack.website_url,
        })
    }

    async fn stage_version_to_temp(
        &self,
        app: &tauri::AppHandle,
        project_id: &str,
        version_id: &str,
    ) -> Result<String, Error> {
        let pack = find_pack(ATL_API_BASE, project_id).await?;
        let configs = atl_api::configs(atl_api::nodecdn_base(), project_id, version_id).await?;
        let mut summary = crate::mods::modpack::atl_map::map_configs(
            &pack.name,
            version_id,
            atl_api::nodecdn_base(),
            &configs,
        );
        let cf_key = crate::mods::curseforge::keyring::resolve();
        crate::mods::modpack::source::ftb::resolve_cf_refs(
            &mut summary,
            CF_BASE,
            cf_key.as_deref(),
        )
        .await;
        resolve_forgecdn_sha1(&mut summary, CF_BASE, cf_key.as_deref()).await;
        let json = serde_json::to_vec(&summary).map_err(|e| Error::ModsDecode {
            platform: "atlauncher".into(),
            details: e.to_string(),
        })?;
        crate::mods::modpack::source::stage::write_to_temp(app, &json, "atlpack.json").await
    }

    async fn resolve_project_hit(
        &self,
        _project_ref: &str,
    ) -> Result<crate::mods::modpack::schema::ModpackHit, Error> {
        // ATLauncher packs are keyed by `safeName`, which its pack-page URLs do
        // not carry verbatim; guessing the mapping would produce confident wrong
        // answers. v1 of import-by-link ships Modrinth + CurseForge.
        Err(Error::ImportUrlUnsupportedSource {
            platform: "ATLauncher".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::modpack::schema::{EnvSupport, ModpackFile, ModpackFormat, ModpackSummary};
    use crate::mods::platform::{LoaderKind, ModSource};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Build a minimal `ModpackSummary` with one ATLauncher forgecdn-direct
    /// placeholder file (the state emitted by `map_configs` for a forgecdn mod
    /// with no md5 — sha1 empty, md5 None, source=Atlauncher).
    fn summary_with_forgecdn_placeholder(file_id: &str, forgecdn_url: &str) -> ModpackSummary {
        ModpackSummary {
            format: ModpackFormat::Atlauncher,
            name: "Test Pack".into(),
            version: "1.0".into(),
            game_version: "1.18.2".into(),
            loader: LoaderKind::Forge,
            loader_version: Some("40.2.0".into()),
            files: vec![ModpackFile {
                project_id: "journeymap.jar".into(),
                version_id: file_id.into(),
                name: "JourneyMap".into(),
                filename: "journeymap.jar".into(),
                install_path: "mods/journeymap.jar".into(),
                sha1: String::new(), // awaiting CF resolution
                md5: None,           // not an md5-verified file
                url: forgecdn_url.into(),
                size: 500_000.0,
                env_client: EnvSupport::Required,
                source: ModSource::Atlauncher,
            }],
            unresolvable: vec![],
            has_overrides: false,
            has_client_overrides: false,
            has_saves_in_overrides: false,
        }
    }

    // ---- resolve_forgecdn_sha1 tests ----------------------------------------

    /// CF returns the sha1 for the file id → file stays in `files` with sha1
    /// filled in (lowercased), unresolvable stays empty.
    #[tokio::test]
    async fn resolve_forgecdn_sha1_fills_sha1_on_success() {
        let s = MockServer::start().await;
        let body = serde_json::json!({
            "data": [{
                "id": 4499899u64,
                "downloadUrl": "https://edge.forgecdn.net/files/4499/899/ae2.jar",
                "hashes": [{ "value": "AABBCCDD", "algo": 1 }]
            }]
        });
        Mock::given(method("POST"))
            .and(path("/v1/mods/files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&s)
            .await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let mut summary = summary_with_forgecdn_placeholder(
            "4499899",
            "https://edge.forgecdn.net/files/4499/899/ae2.jar",
        );
        resolve_forgecdn_sha1(&mut summary, &s.uri(), Some("test-key")).await;

        assert_eq!(
            summary.files.len(),
            1,
            "file must stay in files after sha1 is resolved"
        );
        assert!(
            summary.unresolvable.is_empty(),
            "nothing should be unresolvable"
        );
        assert_eq!(summary.files[0].sha1, "aabbccdd", "sha1 must be lowercased");
    }

    /// No CF key → all forgecdn-direct files are moved to unresolvable with
    /// `MissingChecksum` and the forgecdn URL preserved as `manual_action_url`
    /// (NOT a curseforge.com link).
    #[tokio::test]
    async fn resolve_forgecdn_sha1_no_key_degrades_preserving_url() {
        let forgecdn_url =
            "https://edge.forgecdn.net/files/3820/040/journeymap-1.18.2-5.8.5-forge.jar";
        let mut summary = summary_with_forgecdn_placeholder("3820040", forgecdn_url);
        resolve_forgecdn_sha1(&mut summary, "http://127.0.0.1:1", None).await;

        assert!(
            summary.files.is_empty(),
            "no-key path must move forgecdn files out of files"
        );
        assert_eq!(summary.unresolvable.len(), 1);
        let u = &summary.unresolvable[0];
        assert!(
            matches!(
                u.reason,
                crate::mods::modpack::schema::UnresolvableReason::MissingChecksum
            ),
            "expected MissingChecksum, got {:?}",
            u.reason
        );
        assert_eq!(
            u.manual_action_url, forgecdn_url,
            "manual_action_url must be the forgecdn URL, not a curseforge.com link"
        );
    }

    /// CF returns the file id but with no sha1 in hashes → file moved to
    /// unresolvable (no-TOFU), forgecdn URL preserved.
    #[tokio::test]
    async fn resolve_forgecdn_sha1_missing_sha1_degrades() {
        let s = MockServer::start().await;
        let forgecdn_url = "https://edge.forgecdn.net/files/4499/899/ae2.jar";
        let body = serde_json::json!({
            "data": [{
                "id": 4499899u64,
                "downloadUrl": "https://edge.forgecdn.net/files/4499/899/ae2.jar",
                "hashes": []   // no sha1
            }]
        });
        Mock::given(method("POST"))
            .and(path("/v1/mods/files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&s)
            .await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let mut summary = summary_with_forgecdn_placeholder("4499899", forgecdn_url);
        resolve_forgecdn_sha1(&mut summary, &s.uri(), Some("test-key")).await;

        assert!(
            summary.files.is_empty(),
            "file without sha1 must be moved to unresolvable (no-TOFU)"
        );
        assert_eq!(summary.unresolvable.len(), 1);
        let u = &summary.unresolvable[0];
        assert!(
            matches!(
                u.reason,
                crate::mods::modpack::schema::UnresolvableReason::MissingChecksum
            ),
            "expected MissingChecksum, got {:?}",
            u.reason
        );
        assert_eq!(
            u.manual_action_url, forgecdn_url,
            "manual_action_url must be the forgecdn URL"
        );
    }

    fn catalogue() -> serde_json::Value {
        serde_json::json!({
            "data": [
                { "id": 1, "name": "Beta Pack", "safeName": "BetaPack", "type": "public",
                  "description": "b", "versions": [{ "version": "1.0", "minecraft": "1.20.1", "published": 1 }] },
                { "id": 2, "name": "Alpha Pack", "safeName": "AlphaPack", "type": "public",
                  "description": "a", "versions": [{ "version": "2.0", "minecraft": "1.12.2", "published": 2 }] },
                { "id": 3, "name": "Hidden", "safeName": "Hidden", "type": "private", "versions": [] }
            ]
        })
    }

    #[tokio::test]
    async fn search_filters_public_sorts_and_paginates() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/packs/full/all"))
            .respond_with(ResponseTemplate::new(200).set_body_json(catalogue()))
            .mount(&s)
            .await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let page = search_impl(&s.uri(), "", 0, None, None, 20).await.unwrap();
        assert_eq!(page.total, 2, "private pack excluded");
        assert_eq!(page.hits[0].title, "Alpha Pack", "alphabetical order");
        assert_eq!(page.hits[0].source, ModSource::Atlauncher);
        assert_eq!(
            page.hits[0].author, None,
            "ATLauncher catalogue exposes no author"
        );
    }

    #[tokio::test]
    async fn search_mc_filter_client_side() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/packs/full/all"))
            .respond_with(ResponseTemplate::new(200).set_body_json(catalogue()))
            .mount(&s)
            .await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let page = search_impl(&s.uri(), "", 0, Some("1.20.1"), None, 20)
            .await
            .unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.hits[0].project_id, "BetaPack");
    }
}
