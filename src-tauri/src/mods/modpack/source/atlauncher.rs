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
        let cf_key = crate::mods::curseforge::keyring::get().ok().flatten();
        crate::mods::modpack::source::ftb::resolve_cf_refs(
            &mut summary,
            "https://api.curseforge.com",
            cf_key.as_deref(),
        )
        .await;
        let json = serde_json::to_vec(&summary).map_err(|e| Error::ModsDecode {
            platform: "atlauncher".into(),
            details: e.to_string(),
        })?;
        crate::mods::modpack::source::stage::write_to_temp(app, &json, "atlpack.json").await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::test_env_lock()
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
        let _g = test_lock();
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/packs/full/all"))
            .respond_with(ResponseTemplate::new(200).set_body_json(catalogue()))
            .mount(&s)
            .await;
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let page = search_impl(&s.uri(), "", 0, None, None, 20).await.unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert_eq!(page.total, 2, "private pack excluded");
        assert_eq!(page.hits[0].title, "Alpha Pack", "alphabetical order");
        assert_eq!(page.hits[0].source, ModSource::Atlauncher);
    }

    #[tokio::test]
    async fn search_mc_filter_client_side() {
        let _g = test_lock();
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/packs/full/all"))
            .respond_with(ResponseTemplate::new(200).set_body_json(catalogue()))
            .mount(&s)
            .await;
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let page = search_impl(&s.uri(), "", 0, Some("1.20.1"), None, 20)
            .await
            .unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert_eq!(page.total, 1);
        assert_eq!(page.hits[0].project_id, "BetaPack");
    }
}
