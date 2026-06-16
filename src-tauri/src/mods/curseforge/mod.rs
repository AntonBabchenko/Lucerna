//! CurseForge Eternal API client (v1).

pub mod keyring;
mod types;

use async_trait::async_trait;

use crate::error::Error;
use crate::mods::platform::*;

const BASE_DEFAULT: &str = "https://api.curseforge.com";

/// CurseForge's `/v1/mods/search` caps `pageSize` at 50 and returns HTTP 400
/// for anything larger. The shared browser UI offers up to 100 results/page
/// (Modrinth allows it), so a larger request is fanned out into back-to-back
/// windows of this size — see `search()`.
const CF_MAX_PAGE_SIZE: u32 = 50;

pub struct CurseForgeClient {
    base: String,
    api_key: Option<String>,
}

impl CurseForgeClient {
    pub fn new() -> Self {
        Self {
            base: BASE_DEFAULT.into(),
            api_key: keyring::get().ok().flatten(),
        }
    }

    pub fn with_base_and_key(base: impl Into<String>, key: Option<String>) -> Self {
        Self {
            base: base.into(),
            api_key: key,
        }
    }

    /// The API key, validated as header-safe. `Missing` when no key is
    /// stored; `Invalid` when the stored key contains control characters
    /// (a paste error — a real key is an opaque printable token).
    fn auth(&self) -> Result<&str, Error> {
        let k = self.api_key.as_deref().ok_or(Error::ModsPlatformAuth {
            kind: crate::error::ModsAuthKind::Missing,
        })?;
        if k.chars().any(|c| c.is_control()) {
            return Err(Error::ModsPlatformAuth {
                kind: crate::error::ModsAuthKind::Invalid,
            });
        }
        Ok(k)
    }

    fn map_status<T: serde::de::DeserializeOwned>(
        &self,
        resp: crate::network::request::HttpResponse,
        url: String,
    ) -> Result<T, Error> {
        if resp.status == 401 || resp.status == 403 {
            keyring::clear().ok();
            return Err(Error::ModsPlatformAuth {
                kind: crate::error::ModsAuthKind::Invalid,
            });
        }
        if resp.status == 404 {
            return Err(Error::ModsNotFound {
                platform: "curseforge".into(),
            });
        }
        if !(200..300).contains(&resp.status) {
            return Err(Error::ModsNetwork {
                url,
                details: format!("HTTP {}", resp.status),
            });
        }
        serde_json::from_slice(&resp.body).map_err(|e| Error::ModsDecode {
            platform: "curseforge".into(),
            details: e.to_string(),
        })
    }
}

impl Default for CurseForgeClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ModPlatform for CurseForgeClient {
    async fn search(&self, q: &ModSearchQuery) -> Result<ModSearchPage, Error> {
        // Pre-validate auth so the missing-key path doesn't bother hitting
        // the network.
        let auth = self.auth()?;

        // CurseForge caps pageSize at CF_MAX_PAGE_SIZE; a UI request for more
        // (the shared browser offers up to 100, which Modrinth allows) is
        // served by stitching back-to-back windows that tile [offset, offset +
        // page_size). page_size <= the cap stays a single request, exactly as
        // before. Windows stop early once one underfills (end of results).
        //
        // CF also enforces index + pageSize <= 10_000; a deep-page request
        // (roughly page ~200 at 50/page) then returns HTTP 400, surfacing as a
        // ModsNetwork error — acceptable, as CF's own catalogue stops paging
        // there too.
        let want = q.page_size.max(1);
        let mut hits: Vec<ModSummary> = Vec::with_capacity(want as usize);
        let mut total = 0u32;
        let mut fetched = 0u32;
        while fetched < want {
            let chunk = (want - fetched).min(CF_MAX_PAGE_SIZE);
            let index = q.offset + fetched;
            let mut params: Vec<(&str, String)> = vec![
                ("gameId", types::GAME_MINECRAFT.to_string()),
                ("searchFilter", q.query.clone()),
                ("pageSize", chunk.to_string()),
                ("index", index.to_string()),
            ];
            params.push(("classId", types::class_id(q.kind).to_string()));
            if let Some(mc) = &q.mc_version {
                params.push(("gameVersion", mc.clone()));
            }
            if q.kind == crate::mods::platform::ContentKind::Mod {
                if let Some(l) = q.loader {
                    params.push(("modLoaderType", types::loader_type(l).to_string()));
                }
            }
            match q.sort {
                ModSort::Downloads => {
                    params.push(("sortField", "6".into()));
                    params.push(("sortOrder", "desc".into()));
                }
                ModSort::Updated => {
                    params.push(("sortField", "3".into()));
                    params.push(("sortOrder", "desc".into()));
                }
                ModSort::Relevance => {}
            }
            let url = format!("{}/v1/mods/search?{}", self.base, encode_pairs(&params));
            let resp = crate::network::request::get(&url, &[("x-api-key", auth)], "mods")
                .await
                .map_err(|e| Error::ModsNetwork {
                    url: url.clone(),
                    details: e.to_string(),
                })?;
            let env: types::ListEnvelope<types::Mod> = self.map_status(resp, url)?;
            let got = env.data.len() as u32;
            total = env
                .pagination
                .as_ref()
                .map(|p| p.total_count)
                .unwrap_or(fetched + got);
            hits.extend(env.data.into_iter().map(convert_mod_summary));
            fetched += got;
            // A short window means the catalogue is exhausted — stop rather
            // than firing a guaranteed-empty follow-up request.
            if got < chunk {
                break;
            }
        }
        Ok(ModSearchPage {
            hits,
            total,
            offset: q.offset,
            page_size: q.page_size,
        })
    }

    async fn project(&self, project_id: &str) -> Result<ModProject, Error> {
        let auth = self.auth()?;
        let url = format!("{}/v1/mods/{}", self.base, project_id);
        let resp = crate::network::request::get(&url, &[("x-api-key", auth)], "mods")
            .await
            .map_err(|e| Error::ModsNetwork {
                url: url.clone(),
                details: e.to_string(),
            })?;
        let env: types::Envelope<types::Mod> = self.map_status(resp, url)?;
        let mut m = env.data;
        let screenshots = std::mem::take(&mut m.screenshots);
        let gallery = screenshots
            .into_iter()
            .filter(|s| crate::mods::render::is_safe_image_url(&s.url))
            .map(|s| crate::mods::platform::GalleryImage {
                url: s.url,
                title: s.title,
            })
            .collect();
        // CF mod summary discards `links.websiteUrl` during conversion; the
        // detail modal falls back to constructing the canonical CurseForge URL
        // from `slug`. Keep `website_url` None for v1.
        let summary = convert_mod_summary(m);

        // Second hop: CurseForge serves the long description as HTML from a
        // dedicated endpoint. A failure here degrades to an empty body (the
        // modal falls back to the short summary) rather than failing the
        // whole detail load.
        let desc_url = format!("{}/v1/mods/{}/description", self.base, project_id);
        let body_html =
            match crate::network::request::get(&desc_url, &[("x-api-key", auth)], "mods").await {
                Ok(r) if (200..300).contains(&r.status) => {
                    match serde_json::from_slice::<types::Envelope<String>>(&r.body) {
                        Ok(e) => crate::mods::render::sanitize_html(&e.data),
                        Err(_) => String::new(),
                    }
                }
                _ => String::new(),
            };

        Ok(ModProject {
            summary,
            body_html,
            gallery,
            website_url: None,
        })
    }

    async fn versions(
        &self,
        project_id: &str,
        mc: Option<&str>,
        loader: Option<LoaderKind>,
    ) -> Result<Vec<ModVersion>, Error> {
        let auth = self.auth()?;
        let mut params: Vec<(&str, String)> = vec![("pageSize", "50".to_string())];
        if let Some(v) = mc {
            params.push(("gameVersion", v.to_string()));
        }
        if let Some(l) = loader {
            if l != LoaderKind::Vanilla {
                params.push(("modLoaderType", types::loader_type(l).to_string()));
            }
        }
        let url = format!(
            "{}/v1/mods/{}/files?{}",
            self.base,
            project_id,
            encode_pairs(&params)
        );
        let resp = crate::network::request::get(&url, &[("x-api-key", auth)], "mods")
            .await
            .map_err(|e| Error::ModsNetwork {
                url: url.clone(),
                details: e.to_string(),
            })?;
        let env: types::ListEnvelope<types::File> = self.map_status(resp, url)?;
        let versions: Vec<ModVersion> = env
            .data
            .into_iter()
            .filter_map(|f| convert_version(f, project_id))
            // CF's `modLoaderType` query filter is unreliable and leaks files
            // for other loaders (e.g. a NeoForge build returned for a Forge
            // query). Post-filter on the loader tags we parsed from
            // `gameVersions`: drop any version that names loaders but not the
            // requested one. A version with no detectable loader tag is
            // ambiguous and kept. Forge and NeoForge are distinct here.
            .filter(|v| match loader {
                Some(want) if !v.loaders.is_empty() => v.loaders.contains(&want),
                _ => true,
            })
            .collect();
        // Second, filename-based guard against loader mis-tagging (shared with
        // the Modrinth client).
        Ok(crate::mods::platform::drop_filename_loader_mismatches(
            versions, loader,
        ))
    }

    async fn resolve_deps(
        &self,
        version: &ModVersion,
        mc: &str,
        loader: LoaderKind,
    ) -> Result<ResolvedDeps, Error> {
        let mut required = Vec::new();
        let mut optional = Vec::new();
        let mut incompatible = Vec::new();
        let mut unresolvable = Vec::new();
        for dep in &version.deps {
            // Incompatible / embedded deps don't need a version lookup.
            match dep.kind {
                DepKind::Incompatible => {
                    incompatible.push(dep.project_ref.clone());
                    continue;
                }
                DepKind::Embedded => continue,
                _ => {}
            }
            let pid = match &dep.project_ref {
                DepProjectRef::Curseforge { mod_id, .. } => mod_id.to_string(),
                DepProjectRef::Modrinth { .. } => {
                    // Cross-source dep we can't resolve here — only flag if required.
                    if dep.kind == DepKind::Required {
                        unresolvable.push(dep.project_ref.clone());
                    }
                    continue;
                }
            };
            let vs = self.versions(&pid, Some(mc), Some(loader)).await?;
            if let Some(v) = vs.into_iter().next() {
                let resolved = ResolvedDep {
                    project_ref: dep.project_ref.clone(),
                    version: v,
                };
                match dep.kind {
                    DepKind::Required => required.push(resolved),
                    DepKind::Optional => optional.push(resolved),
                    _ => {}
                }
            } else if dep.kind == DepKind::Required {
                // Only a missing *required* dep is worth surfacing; an optional
                // one that has no compatible build is skipped silently.
                unresolvable.push(dep.project_ref.clone());
            }
        }
        Ok(ResolvedDeps {
            required,
            optional,
            incompatible,
            unresolvable,
        })
    }
}

fn convert_mod_summary(m: types::Mod) -> ModSummary {
    ModSummary {
        source: ModSource::Curseforge,
        project_id: m.id.to_string(),
        slug: Some(m.slug),
        name: m.name,
        summary: m.summary,
        icon_url: m.logo.and_then(|l| l.url),
        downloads: m.download_count as f64,
        author: m
            .authors
            .into_iter()
            .map(|a| a.name)
            .next()
            .unwrap_or_default(),
        updated_at: m.date_modified,
    }
}

fn convert_version(f: types::File, project_id: &str) -> Option<ModVersion> {
    let sha1 = f
        .hashes
        .iter()
        .find(|h| h.algo == 1)
        .map(|h| h.value.clone());
    let url = f.download_url.unwrap_or_default();
    let distribution_allowed = !url.is_empty() && f.is_available;
    // CF mixes Minecraft versions and loader names in `gameVersions`
    // (e.g. ["1.20.1", "NeoForge"]). Split them: recognized loader tags
    // populate `loaders`; everything else stays in `mc_versions`. This is
    // what lets `versions()` post-filter by loader, since CF's own
    // `modLoaderType` query filter is unreliable.
    let mut loaders = Vec::new();
    let mut mc_versions = Vec::new();
    for gv in f.game_versions {
        match types::loader_from_tag(&gv) {
            Some(l) if !loaders.contains(&l) => loaders.push(l),
            Some(_) => {} // duplicate loader tag
            // Drop "Client"/"Server" markers; keep everything else as an MC version.
            None if types::is_environment_marker(&gv) => {}
            None => mc_versions.push(gv),
        }
    }
    Some(ModVersion {
        source: ModSource::Curseforge,
        project_id: project_id.to_string(),
        version_id: f.id.to_string(),
        name: f.display_name,
        version_number: f.file_name.clone(),
        mc_versions,
        loaders,
        primary_file: ModFile {
            filename: f.file_name,
            url,
            sha1,
            size: f.file_length as f64,
            distribution_allowed,
        },
        deps: f
            .dependencies
            .into_iter()
            .filter_map(|d| {
                let kind = match d.relation_type {
                    3 => DepKind::Required,
                    2 => DepKind::Optional,
                    5 => DepKind::Incompatible,
                    1 => DepKind::Embedded,
                    _ => return None,
                };
                Some(ModDepLink {
                    kind,
                    project_ref: DepProjectRef::Curseforge {
                        mod_id: d.mod_id,
                        file_id: None,
                    },
                })
            })
            .collect(),
        published_at: f.file_date,
    })
}

/// Minimal percent-encoder for query-string values. Encodes anything outside
/// the URL-safe unreserved set [A-Za-z0-9-_.~] using uppercase %HH form.
/// We use this instead of `RequestBuilder::query()` to avoid pulling
/// `serde_urlencoded` features that are disabled by our `reqwest` flags.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        let safe = b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~');
        if safe {
            out.push(b as char);
        } else {
            out.push('%');
            out.push_str(&format!("{:02X}", b));
        }
    }
    out
}

fn encode_pairs(pairs: &[(&str, String)]) -> String {
    let mut out = String::new();
    for (i, (k, v)) in pairs.iter().enumerate() {
        if i > 0 {
            out.push('&');
        }
        out.push_str(&urlencode(k));
        out.push('=');
        out.push_str(&urlencode(v));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::test_env_lock()
    }

    fn client(uri: String) -> CurseForgeClient {
        CurseForgeClient::with_base_and_key(uri, Some("test-key".into()))
    }

    #[tokio::test]
    async fn missing_key_returns_platform_auth_missing() {
        let _g = test_lock();
        let s = MockServer::start().await;
        let c = CurseForgeClient::with_base_and_key(s.uri(), None);
        let q = ModSearchQuery {
            source: ModSource::Curseforge,
            kind: ContentKind::Mod,
            query: "x".into(),
            mc_version: None,
            loader: None,
            sort: ModSort::Relevance,
            page_size: 20,
            offset: 0,
        };
        let err = c.search(&q).await.unwrap_err();
        match err {
            Error::ModsPlatformAuth { kind } => {
                assert_eq!(kind, crate::error::ModsAuthKind::Missing)
            }
            _ => panic!("expected ModsPlatformAuth::Missing, got {err:?}"),
        }
    }

    #[tokio::test]
    async fn unauthorized_clears_key_and_returns_invalid() {
        let _g = test_lock();
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/search"))
            .and(header("x-api-key", "test-key"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&s)
            .await;
        let c = client(s.uri());
        let q = ModSearchQuery {
            source: ModSource::Curseforge,
            kind: ContentKind::Mod,
            query: "x".into(),
            mc_version: None,
            loader: None,
            sort: ModSort::Relevance,
            page_size: 20,
            offset: 0,
        };
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let err = c.search(&q).await.unwrap_err();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        match err {
            Error::ModsPlatformAuth { kind } => {
                assert_eq!(kind, crate::error::ModsAuthKind::Invalid)
            }
            _ => panic!("expected ModsPlatformAuth::Invalid, got {err:?}"),
        }
    }

    #[tokio::test]
    async fn project_maps_screenshots_and_sanitizes_description() {
        let _g = test_lock();
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/77"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {
                    "id": 77, "slug": "jei", "name": "JEI", "summary": "items",
                    "downloadCount": 5, "authors": [{"name":"m"}],
                    "logo": {"url":"https://example/icon.png"},
                    "links": {"websiteUrl": null},
                    "screenshots": [{"url":"https://media.forgecdn.net/s.png","title":"Shot"}]
                }
            })))
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/77/description"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": "<p>Hi</p><script>alert(1)</script>"
            })))
            .mount(&s)
            .await;
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let c = client(s.uri());
        let p = c.project("77").await.unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert_eq!(p.gallery.len(), 1);
        assert_eq!(p.gallery[0].url, "https://media.forgecdn.net/s.png");
        assert!(p.body_html.contains("<p>Hi</p>"));
        assert!(!p.body_html.contains("<script"));
    }

    #[tokio::test]
    async fn search_parses_envelope() {
        let _g = test_lock();
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/search"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{
                "data":[{"id":12345,"slug":"jei","name":"JEI","summary":"Items",
                         "downloadCount":1234,"authors":[{"name":"mezz"}],
                         "logo":{"url":"https://example/icon.png"},
                         "dateModified":"2026-05-01T00:00:00Z",
                         "links":{"websiteUrl":"https://www.curseforge.com/minecraft/mc-mods/jei"}}],
                "pagination":{"index":0,"pageSize":20,"resultCount":1,"totalCount":1}
            }"#,
            ))
            .mount(&s)
            .await;
        let q = ModSearchQuery {
            source: ModSource::Curseforge,
            kind: ContentKind::Mod,
            query: "jei".into(),
            mc_version: Some("1.20.1".into()),
            loader: Some(LoaderKind::Fabric),
            sort: ModSort::Downloads,
            page_size: 20,
            offset: 0,
        };
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let page = client(s.uri()).search(&q).await.unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert_eq!(page.total, 1);
        assert_eq!(page.hits[0].name, "JEI");
        assert_eq!(page.hits[0].project_id, "12345");
    }

    #[tokio::test]
    async fn versions_marks_distribution_disabled_when_url_absent() {
        let _g = test_lock();
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/12345/files"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{
                "data":[{"id":99,"modId":12345,"displayName":"v1.0","fileName":"x.jar",
                         "fileLength":100,"hashes":[{"value":"abc","algo":1}],
                         "gameVersions":["1.20.1"],"downloadUrl":null,
                         "fileDate":"2026-05-01T00:00:00Z","isAvailable":true,
                         "releaseType":1,"dependencies":[]}],
                "pagination":null}"#,
            ))
            .mount(&s)
            .await;
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let v = client(s.uri())
            .versions("12345", Some("1.20.1"), Some(LoaderKind::Fabric))
            .await
            .unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert_eq!(v.len(), 1);
        assert!(!v[0].primary_file.distribution_allowed);
    }

    #[test]
    fn convert_version_splits_loaders_from_mc_versions() {
        let f = types::File {
            id: 99,
            mod_id: 12345,
            display_name: "Xaero 1.20.1 NeoForge".into(),
            file_name: "xaerominimap.jar".into(),
            file_length: 100,
            hashes: vec![types::Hash {
                value: "abc".into(),
                algo: 1,
            }],
            game_versions: vec!["1.20.1".into(), "NeoForge".into(), "Client".into()],
            download_url: Some("https://example/x.jar".into()),
            file_date: None,
            is_available: true,
            release_type: 1,
            dependencies: vec![],
        };
        let v = convert_version(f, "12345").unwrap();
        assert_eq!(v.loaders, vec![LoaderKind::NeoForge]);
        // Loader tags and environment markers ("Client"/"Server") are stripped;
        // only real MC versions remain in mc_versions.
        assert_eq!(v.mc_versions, vec!["1.20.1".to_string()]);
    }

    #[tokio::test]
    async fn versions_filters_out_mismatched_loader() {
        // CF leaks a NeoForge file even when modLoaderType=Forge is requested.
        // The post-filter must drop it and keep only the Forge file.
        let _g = test_lock();
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/12345/files"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{
                "data":[
                  {"id":1,"modId":12345,"displayName":"neo","fileName":"x-neoforge.jar",
                   "fileLength":100,"hashes":[{"value":"a","algo":1}],
                   "gameVersions":["1.20.1","NeoForge"],"downloadUrl":"https://e/n.jar",
                   "fileDate":null,"isAvailable":true,"releaseType":1,"dependencies":[]},
                  {"id":2,"modId":12345,"displayName":"forge","fileName":"x-forge.jar",
                   "fileLength":100,"hashes":[{"value":"b","algo":1}],
                   "gameVersions":["1.20.1","Forge"],"downloadUrl":"https://e/f.jar",
                   "fileDate":null,"isAvailable":true,"releaseType":1,"dependencies":[]}
                ],
                "pagination":null}"#,
            ))
            .mount(&s)
            .await;
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let v = client(s.uri())
            .versions("12345", Some("1.20.1"), Some(LoaderKind::Forge))
            .await
            .unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert_eq!(v.len(), 1, "only the Forge file should survive");
        assert_eq!(v[0].loaders, vec![LoaderKind::Forge]);
        assert_eq!(v[0].version_number, "x-forge.jar");
    }

    /// Build `count` minimal CF mod JSON entries with ids `start..start+count`.
    fn mod_entries(start: u32, count: u32) -> Vec<serde_json::Value> {
        (start..start + count)
            .map(|i| {
                serde_json::json!({
                    "id": i, "slug": format!("m{i}"), "name": format!("Mod {i}"),
                    "summary": "s", "downloadCount": 1, "authors": [{"name": "a"}],
                    "logo": {"url": "https://example/i.png"},
                    "links": {"websiteUrl": null}
                })
            })
            .collect()
    }

    #[tokio::test]
    async fn search_chunks_page_size_above_cf_max() {
        // CurseForge rejects pageSize > 50 with HTTP 400. A UI request for 100
        // results/page must fan out into two ≤50 windows (index 0 + 50) and
        // stitch them — never sending pageSize=100. The mocks only match
        // pageSize=50, so a single pageSize=100 request would 404 and fail.
        let _g = test_lock();
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/search"))
            .and(query_param("pageSize", "50"))
            .and(query_param("index", "0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": mod_entries(0, 50),
                "pagination": {"index": 0, "pageSize": 50, "resultCount": 50, "totalCount": 120}
            })))
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/search"))
            .and(query_param("pageSize", "50"))
            .and(query_param("index", "50"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": mod_entries(50, 50),
                "pagination": {"index": 50, "pageSize": 50, "resultCount": 50, "totalCount": 120}
            })))
            .mount(&s)
            .await;
        let q = ModSearchQuery {
            source: ModSource::Curseforge,
            kind: ContentKind::Mod,
            query: String::new(),
            mc_version: None,
            loader: None,
            sort: ModSort::Downloads,
            page_size: 100,
            offset: 0,
        };
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let page = client(s.uri()).search(&q).await.unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert_eq!(page.hits.len(), 100, "two 50-windows should stitch to 100");
        assert_eq!(page.total, 120);
        assert_eq!(page.page_size, 100, "returns the requested page size");
        assert_eq!(page.offset, 0);
        assert_eq!(page.hits[0].project_id, "0");
        assert_eq!(page.hits[99].project_id, "99");
    }

    #[tokio::test]
    async fn search_stops_early_when_window_underfills() {
        // Only 30 results exist though 100 were requested: the first window
        // returns < pageSize, so no second request is made.
        let _g = test_lock();
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/search"))
            .and(query_param("index", "0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": mod_entries(0, 30),
                "pagination": {"index": 0, "pageSize": 50, "resultCount": 30, "totalCount": 30}
            })))
            .mount(&s)
            .await;
        let q = ModSearchQuery {
            source: ModSource::Curseforge,
            kind: ContentKind::Mod,
            query: String::new(),
            mc_version: None,
            loader: None,
            sort: ModSort::Relevance,
            page_size: 100,
            offset: 0,
        };
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let page = client(s.uri()).search(&q).await.unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert_eq!(page.hits.len(), 30);
        assert_eq!(page.total, 30);
    }

    #[tokio::test]
    async fn search_sends_class_id_for_shader_kind() {
        let _g = test_lock();
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/search"))
            .and(header("x-api-key", "test-key"))
            .and(query_param("classId", "6552"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [],
                "pagination": {"index": 0, "pageSize": 20, "resultCount": 0, "totalCount": 0}
            })))
            .mount(&s)
            .await;
        let q = ModSearchQuery {
            source: ModSource::Curseforge,
            kind: ContentKind::Shader,
            query: "complementary".into(),
            mc_version: None,
            loader: None,
            sort: ModSort::Relevance,
            page_size: 20,
            offset: 0,
        };
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let page = client(s.uri()).search(&q).await.unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert_eq!(page.total, 0);
        assert!(page.hits.is_empty());
    }
}

/// Outcome of validating a candidate CurseForge API key against the
/// `/v1/games/432` ping. Pure classification of an already-received
/// response — transport failures are handled by the caller before this.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum KeyCheckOutcome {
    /// 2xx — key accepted.
    Ok,
    /// The API genuinely rejected the key (non-2xx with a real API body).
    Invalid,
    /// Reached the edge but Cloudflare / a region block intercepted the
    /// request (protective status, or a 403 carrying an HTML/CF body).
    /// The key's validity is unknown.
    Unreachable,
}

/// Cloudflare / region-block status codes that are never the key's fault.
const EDGE_BLOCK_STATUSES: &[u16] = &[429, 503, 520, 521, 522, 523, 524, 525, 526];

/// Markers that a non-2xx body came from Cloudflare / an HTML interstitial
/// rather than the CurseForge JSON API.
const CF_BODY_MARKERS: &[&str] = &[
    "cloudflare",
    "cf-ray",
    "<!doctype html",
    "<html",
    "just a moment",
    "attention required",
    "access denied",
];

/// Classify a key-validation response. See `KeyCheckOutcome`.
pub(crate) fn classify_key_check(status: u16, body: &[u8]) -> KeyCheckOutcome {
    if (200..=299).contains(&status) {
        return KeyCheckOutcome::Ok;
    }
    if EDGE_BLOCK_STATUSES.contains(&status) {
        return KeyCheckOutcome::Unreachable;
    }
    if status == 403 {
        let hay = String::from_utf8_lossy(body).to_ascii_lowercase();
        if CF_BODY_MARKERS.iter().any(|m| hay.contains(m)) {
            return KeyCheckOutcome::Unreachable;
        }
    }
    KeyCheckOutcome::Invalid
}

#[cfg(test)]
mod key_check_tests {
    use super::{classify_key_check, KeyCheckOutcome};

    #[test]
    fn success_status_is_ok() {
        assert_eq!(classify_key_check(200, b"{}"), KeyCheckOutcome::Ok);
        assert_eq!(classify_key_check(204, b""), KeyCheckOutcome::Ok);
    }

    #[test]
    fn genuine_api_rejection_is_invalid() {
        // CurseForge returns JSON on a bad/missing key.
        let body = br#"{"error":"API key invalid"}"#;
        assert_eq!(classify_key_check(403, body), KeyCheckOutcome::Invalid);
        assert_eq!(classify_key_check(401, body), KeyCheckOutcome::Invalid);
        assert_eq!(classify_key_check(404, b"{}"), KeyCheckOutcome::Invalid);
    }

    #[test]
    fn cloudflare_html_403_is_unreachable() {
        let body = b"<!DOCTYPE html><html><head><title>Just a moment...</title>\
                     </head><body>cf-ray: 1234</body></html>";
        assert_eq!(classify_key_check(403, body), KeyCheckOutcome::Unreachable);
    }

    #[test]
    fn attention_required_403_is_unreachable() {
        let body = b"<html><body>Attention Required! | Cloudflare</body></html>";
        assert_eq!(classify_key_check(403, body), KeyCheckOutcome::Unreachable);
    }

    #[test]
    fn protective_statuses_are_unreachable() {
        for s in [429u16, 503, 520, 521, 522, 523, 524, 525, 526] {
            assert_eq!(
                classify_key_check(s, b"whatever"),
                KeyCheckOutcome::Unreachable,
                "status {s} should be Unreachable"
            );
        }
    }

    #[test]
    fn case_insensitive_body_match() {
        assert_eq!(
            classify_key_check(403, b"CLOUDFLARE blocked this request"),
            KeyCheckOutcome::Unreachable
        );
    }
}
