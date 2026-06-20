//! Modrinth v2 API client.

mod types;

use async_trait::async_trait;

use crate::error::Error;
use crate::mods::platform::*;
use std::collections::HashMap;

const BASE_DEFAULT: &str = "https://api.modrinth.com";
const UA: &str = "AntonBabchenko/Lucerna (github.com/AntonBabchenko/Lucerna)";

pub struct ModrinthClient {
    base: String,
}

impl ModrinthClient {
    pub fn new() -> Self {
        Self {
            base: BASE_DEFAULT.into(),
        }
    }

    /// Tests inject a wiremock URL here.
    pub fn with_base(base: impl Into<String>) -> Self {
        Self { base: base.into() }
    }

    fn loader_facet(loader: LoaderKind) -> &'static str {
        loader.modrinth_slug()
    }

    fn sort_key(sort: ModSort) -> &'static str {
        match sort {
            ModSort::Relevance => "relevance",
            ModSort::Downloads => "downloads",
            ModSort::Updated => "updated",
        }
    }

    /// Fetch each project's `server_side` support in bulk — mirrors `summaries`
    /// (`GET /v2/projects?ids=[...]`) but extracts only `id` + `server_side`.
    /// Unknown / unrecognized ids are simply absent from the returned map.
    /// Powers the new-server client-mod quarantine (Phase 1).
    pub async fn server_side_bulk(
        &self,
        ids: &[&str],
    ) -> Result<HashMap<String, ServerSideSupport>, Error> {
        #[derive(serde::Deserialize)]
        struct Sides {
            id: String,
            server_side: Option<String>,
        }
        let mut out = HashMap::new();
        for chunk in ids.chunks(BATCH_CHUNK) {
            if chunk.is_empty() {
                continue;
            }
            let ids_json = serde_json::to_string(chunk)
                .expect("a slice of ids always serializes to a JSON array");
            let url = format!("{}/v2/projects?ids={}", self.base, urlencode(&ids_json));
            let resp = crate::network::request::get(&url, &[("user-agent", UA)], "mods")
                .await
                .map_err(|e| Error::ModsNetwork {
                    url: url.clone(),
                    details: e.to_string(),
                })?;
            if !(200..300).contains(&resp.status) {
                return Err(Error::ModsNetwork {
                    url,
                    details: format!("HTTP {}", resp.status),
                });
            }
            let projects: Vec<Sides> =
                serde_json::from_slice(&resp.body).map_err(|e| Error::ModsDecode {
                    platform: "modrinth".into(),
                    details: e.to_string(),
                })?;
            for p in projects {
                out.insert(
                    p.id,
                    ServerSideSupport::from_modrinth(p.server_side.as_deref()),
                );
            }
        }
        Ok(out)
    }

    /// Resolve SHA-1 hashes to Modrinth project ids in bulk via
    /// `POST /v2/version_files`. Returns `sha1 (lowercased) -> project_id` for
    /// every hash Modrinth knows; unknown hashes are absent. No API key needed.
    /// Powers the existing-server client-mod quarantine (Phase 1).
    pub async fn project_ids_by_hash(
        &self,
        shas: &[&str],
    ) -> Result<HashMap<String, String>, Error> {
        #[derive(serde::Deserialize)]
        struct VersionLite {
            project_id: String,
        }
        let mut out = HashMap::new();
        for chunk in shas.chunks(BATCH_CHUNK) {
            if chunk.is_empty() {
                continue;
            }
            let url = format!("{}/v2/version_files", self.base);
            let body = serde_json::to_vec(&serde_json::json!({
                "hashes": chunk,
                "algorithm": "sha1",
            }))
            .expect("a fixed-shape JSON object always serializes");
            let resp = crate::network::request::post(
                &url,
                &[("user-agent", UA), ("content-type", "application/json")],
                &body,
                "mods",
            )
            .await
            .map_err(|e| Error::ModsNetwork {
                url: url.clone(),
                details: e.to_string(),
            })?;
            if !(200..300).contains(&resp.status) {
                return Err(Error::ModsNetwork {
                    url,
                    details: format!("HTTP {}", resp.status),
                });
            }
            let map: HashMap<String, VersionLite> =
                serde_json::from_slice(&resp.body).map_err(|e| Error::ModsDecode {
                    platform: "modrinth".into(),
                    details: e.to_string(),
                })?;
            for (sha, v) in map {
                out.insert(sha.to_ascii_lowercase(), v.project_id);
            }
        }
        Ok(out)
    }
}

impl Default for ModrinthClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ModPlatform for ModrinthClient {
    async fn search(&self, q: &ModSearchQuery) -> Result<ModSearchPage, Error> {
        let facets = build_facets(q.kind, q.mc_version.as_deref(), q.loader);
        let facets_json = serde_json::to_string(&facets).unwrap();
        let url = format!(
            "{}/v2/search?query={}&limit={}&offset={}&index={}&facets={}",
            self.base,
            urlencode(&q.query),
            q.page_size,
            q.offset,
            Self::sort_key(q.sort),
            urlencode(&facets_json),
        );
        let resp = crate::network::request::get(&url, &[("user-agent", UA)], "mods")
            .await
            .map_err(|e| Error::ModsNetwork {
                url: url.clone(),
                details: e.to_string(),
            })?;
        if resp.status == 404 {
            return Err(Error::ModsNotFound {
                platform: "modrinth".into(),
            });
        }
        if !(200..300).contains(&resp.status) {
            return Err(Error::ModsNetwork {
                url,
                details: format!("HTTP {}", resp.status),
            });
        }
        let body: types::SearchResponse =
            serde_json::from_slice(&resp.body).map_err(|e| Error::ModsDecode {
                platform: "modrinth".into(),
                details: e.to_string(),
            })?;
        Ok(ModSearchPage {
            hits: body
                .hits
                .into_iter()
                .map(|h| ModSummary {
                    source: ModSource::Modrinth,
                    project_id: h.project_id,
                    slug: Some(h.slug),
                    name: h.title,
                    summary: h.description,
                    icon_url: h.icon_url,
                    downloads: h.downloads as f64,
                    author: h.author,
                    updated_at: h.date_modified,
                })
                .collect(),
            total: body.total_hits,
            offset: body.offset,
            page_size: body.limit,
        })
    }

    async fn project(&self, project_id: &str) -> Result<ModProject, Error> {
        let url = format!("{}/v2/project/{}", self.base, project_id);
        let resp = crate::network::request::get(&url, &[("user-agent", UA)], "mods")
            .await
            .map_err(|e| Error::ModsNetwork {
                url: url.clone(),
                details: e.to_string(),
            })?;
        if resp.status == 404 {
            return Err(Error::ModsNotFound {
                platform: "modrinth".into(),
            });
        }
        if !(200..300).contains(&resp.status) {
            return Err(Error::ModsNetwork {
                url,
                details: format!("HTTP {}", resp.status),
            });
        }
        let p: types::Project =
            serde_json::from_slice(&resp.body).map_err(|e| Error::ModsDecode {
                platform: "modrinth".into(),
                details: e.to_string(),
            })?;
        let summary = summary_from_project(&p);
        let body_html = crate::mods::render::markdown_to_safe_html(&p.body);
        let website_url = p.source_url.or(p.wiki_url);
        let mut gallery_entries = p.gallery;
        // Featured first, then by the platform's `ordering` (None last).
        gallery_entries.sort_by(|a, b| {
            b.featured.cmp(&a.featured).then(
                a.ordering
                    .unwrap_or(i64::MAX)
                    .cmp(&b.ordering.unwrap_or(i64::MAX)),
            )
        });
        let gallery = gallery_entries
            .into_iter()
            .filter(|e| crate::mods::render::is_safe_image_url(&e.url))
            .map(|e| crate::mods::platform::GalleryImage {
                url: e.url,
                title: e.title,
            })
            .collect();
        Ok(ModProject {
            summary,
            body_html,
            gallery,
            website_url,
        })
    }

    async fn versions(
        &self,
        project_id: &str,
        mc: Option<&str>,
        loader: Option<LoaderKind>,
    ) -> Result<Vec<ModVersion>, Error> {
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(l) = loader {
            let loaders = serde_json::to_string(&[Self::loader_facet(l)]).unwrap();
            params.push(("loaders", urlencode(&loaders)));
        }
        if let Some(v) = mc {
            let games = serde_json::to_string(&[v]).unwrap();
            params.push(("game_versions", urlencode(&games)));
        }
        let query = if params.is_empty() {
            String::new()
        } else {
            format!(
                "?{}",
                params
                    .iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join("&")
            )
        };
        let url = format!("{}/v2/project/{}/version{}", self.base, project_id, query);
        let resp = crate::network::request::get(&url, &[("user-agent", UA)], "mods")
            .await
            .map_err(|e| Error::ModsNetwork {
                url: url.clone(),
                details: e.to_string(),
            })?;
        if !(200..300).contains(&resp.status) {
            return Err(Error::ModsNetwork {
                url,
                details: format!("HTTP {}", resp.status),
            });
        }
        let raws: Vec<types::Version> =
            serde_json::from_slice(&resp.body).map_err(|e| Error::ModsDecode {
                platform: "modrinth".into(),
                details: e.to_string(),
            })?;
        let versions: Vec<ModVersion> = raws.into_iter().map(convert_version).collect();
        // Defend against upstream loader mis-tagging (e.g. Xaero's Minimap
        // tags its NeoForge 1.20.4 builds with the `forge` loader). The
        // server-side `loaders` facet trusts that wrong tag; the filename
        // does not.
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
                DepProjectRef::Modrinth { project_id, .. } => project_id.clone(),
                DepProjectRef::Curseforge { .. } => {
                    // Cross-source dep we can't resolve on this platform — only
                    // worth flagging when it's required.
                    if dep.kind == DepKind::Required {
                        unresolvable.push(dep.project_ref.clone());
                    }
                    continue;
                }
            };
            let versions = self.versions(&pid, Some(mc), Some(loader)).await?;
            if let Some(v) = versions.into_iter().next() {
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
                // A *required* dep with no compatible build is a real problem
                // worth surfacing ("install anyway?"). An *optional* one simply
                // isn't available for this MC/loader — skip it silently rather
                // than alarm the user.
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

    async fn summaries(&self, ids: &[&str]) -> Result<Vec<ModSummary>, Error> {
        let mut out = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(BATCH_CHUNK) {
            // Modrinth: GET /v2/projects?ids=["a","b",…] → array of the same
            // Project shape as /v2/project/{id}. Unknown ids are omitted.
            // Serialising a slice of string ids to a JSON array is infallible.
            let ids_json = serde_json::to_string(chunk)
                .expect("a slice of ids always serializes to a JSON array");
            let url = format!("{}/v2/projects?ids={}", self.base, urlencode(&ids_json));
            let resp = crate::network::request::get(&url, &[("user-agent", UA)], "mods")
                .await
                .map_err(|e| Error::ModsNetwork {
                    url: url.clone(),
                    details: e.to_string(),
                })?;
            if !(200..300).contains(&resp.status) {
                return Err(Error::ModsNetwork {
                    url,
                    details: format!("HTTP {}", resp.status),
                });
            }
            let projects: Vec<types::Project> =
                serde_json::from_slice(&resp.body).map_err(|e| Error::ModsDecode {
                    platform: "modrinth".into(),
                    details: e.to_string(),
                })?;
            out.extend(projects.iter().map(summary_from_project));
        }
        Ok(out)
    }

    async fn versions_by_ids(&self, version_ids: &[&str]) -> Result<Vec<ModVersion>, Error> {
        let mut out = Vec::with_capacity(version_ids.len());
        for chunk in version_ids.chunks(BATCH_CHUNK) {
            // Modrinth: GET /v2/versions?ids=["v1","v2",…] → array of Version
            // objects, each carrying its `dependencies`. Unknown ids omitted.
            // Serialising a slice of string ids to a JSON array is infallible.
            let ids_json = serde_json::to_string(chunk)
                .expect("a slice of ids always serializes to a JSON array");
            let url = format!("{}/v2/versions?ids={}", self.base, urlencode(&ids_json));
            let resp = crate::network::request::get(&url, &[("user-agent", UA)], "mods")
                .await
                .map_err(|e| Error::ModsNetwork {
                    url: url.clone(),
                    details: e.to_string(),
                })?;
            if !(200..300).contains(&resp.status) {
                return Err(Error::ModsNetwork {
                    url,
                    details: format!("HTTP {}", resp.status),
                });
            }
            let raws: Vec<types::Version> =
                serde_json::from_slice(&resp.body).map_err(|e| Error::ModsDecode {
                    platform: "modrinth".into(),
                    details: e.to_string(),
                })?;
            out.extend(raws.into_iter().map(convert_version));
        }
        Ok(out)
    }
}

/// Map a Modrinth `Project` to the normalized summary. Shared by `project()`
/// and the batched `summaries()` so both paths agree on the field mapping
/// (author = team, no `updated_at` — the project endpoint omits it).
fn summary_from_project(p: &types::Project) -> ModSummary {
    ModSummary {
        source: ModSource::Modrinth,
        project_id: p.id.clone(),
        slug: Some(p.slug.clone()),
        name: p.title.clone(),
        summary: p.description.clone(),
        icon_url: p.icon_url.clone(),
        downloads: p.downloads as f64,
        author: p.team.clone(),
        updated_at: None,
    }
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

fn convert_version(v: types::Version) -> ModVersion {
    let primary = v
        .files
        .iter()
        .find(|f| f.primary)
        .or_else(|| v.files.first())
        .cloned();
    let pf = primary
        .map(|f| ModFile {
            filename: f.filename,
            url: f.url,
            sha1: Some(f.hashes.sha1),
            size: f.size as f64,
            distribution_allowed: true,
        })
        .unwrap_or(ModFile {
            filename: "missing".into(),
            url: "about:blank".into(),
            sha1: None,
            size: 0.0,
            distribution_allowed: false,
        });
    ModVersion {
        source: ModSource::Modrinth,
        project_id: v.project_id,
        version_id: v.id,
        name: v.name,
        version_number: v.version_number,
        mc_versions: v.game_versions,
        loaders: v
            .loaders
            .into_iter()
            .filter_map(|s| match s.as_str() {
                "fabric" => Some(LoaderKind::Fabric),
                "quilt" => Some(LoaderKind::Quilt),
                "forge" => Some(LoaderKind::Forge),
                "neoforge" => Some(LoaderKind::NeoForge),
                "minecraft" => Some(LoaderKind::Vanilla),
                _ => None,
            })
            .collect(),
        primary_file: pf,
        deps: v
            .dependencies
            .into_iter()
            .filter_map(|d| {
                let kind = match d.dependency_type.as_str() {
                    "required" => DepKind::Required,
                    "optional" => DepKind::Optional,
                    "incompatible" => DepKind::Incompatible,
                    "embedded" => DepKind::Embedded,
                    _ => return None,
                };
                let project_ref = DepProjectRef::Modrinth {
                    project_id: d.project_id?,
                    version_id: d.version_id,
                };
                Some(ModDepLink { kind, project_ref })
            })
            .collect(),
        published_at: v.date_published,
    }
}

fn project_type_facet(kind: ContentKind) -> &'static str {
    match kind {
        ContentKind::Mod => "project_type:mod",
        ContentKind::ResourcePack => "project_type:resourcepack",
        ContentKind::Shader => "project_type:shader",
    }
}

fn build_facets(
    kind: ContentKind,
    mc_version: Option<&str>,
    loader: Option<LoaderKind>,
) -> Vec<Vec<String>> {
    let mut facets: Vec<Vec<String>> = vec![vec![project_type_facet(kind).into()]];
    if let Some(mc) = mc_version {
        facets.push(vec![format!("versions:{mc}")]);
    }
    // The Java loader facet applies to mods ONLY. Resource packs have no
    // loader, and Modrinth shader categories are iris/optifine/canvas — passing
    // `categories:<loader>` to a shader search returns almost nothing.
    if kind == ContentKind::Mod {
        if let Some(l) = loader {
            facets.push(vec![format!(
                "categories:{}",
                ModrinthClient::loader_facet(l)
            )]);
        }
    }
    facets
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::test_env_lock()
    }

    #[test]
    fn facets_use_project_type_per_kind_and_skip_loader_for_resourcepack() {
        let f = build_facets(ContentKind::Mod, Some("1.20.4"), Some(LoaderKind::Fabric));
        assert!(f.contains(&vec!["project_type:mod".to_string()]));
        assert!(f
            .iter()
            .any(|g| g.iter().any(|s| s.starts_with("categories:"))));
        assert!(f.contains(&vec!["versions:1.20.4".to_string()]));

        let f = build_facets(
            ContentKind::ResourcePack,
            Some("1.20.4"),
            Some(LoaderKind::Fabric),
        );
        assert!(f.contains(&vec!["project_type:resourcepack".to_string()]));
        assert!(!f
            .iter()
            .any(|g| g.iter().any(|s| s.starts_with("categories:"))));

        // Shaders use iris/optifine/canvas categories, NOT the Java loader.
        // Passing `categories:fabric` to a shader search returns almost nothing,
        // so the loader facet must be omitted for shaders (mods only).
        let f = build_facets(ContentKind::Shader, None, Some(LoaderKind::Fabric));
        assert!(f.contains(&vec!["project_type:shader".to_string()]));
        assert!(!f
            .iter()
            .any(|g| g.iter().any(|s| s.starts_with("categories:"))));
    }

    async fn server() -> MockServer {
        MockServer::start().await
    }

    #[tokio::test]
    async fn summaries_batches_projects_in_one_request() {
        let _g = test_lock();
        let s = server().await;
        Mock::given(method("GET"))
            .and(path("/v2/projects"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"[
                  {"id":"jei","slug":"jei","title":"JEI","description":"Items",
                   "body":"","icon_url":"https://media.modrinth.com/i.png","downloads":10,
                   "source_url":null,"wiki_url":null,"team":"t","gallery":[]},
                  {"id":"sodium","slug":"sodium","title":"Sodium","description":"Perf",
                   "body":"","icon_url":null,"downloads":99,
                   "source_url":null,"wiki_url":null,"team":"jelly","gallery":[]}
                ]"#,
            ))
            .mount(&s)
            .await;
        let c = ModrinthClient::with_base(s.uri());
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let out = c.summaries(&["jei", "sodium"]).await.unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].name, "JEI");
        assert_eq!(out[0].slug.as_deref(), Some("jei"));
        assert_eq!(out[1].author, "jelly");
    }

    #[tokio::test]
    async fn summaries_empty_ids_makes_no_request() {
        let _g = test_lock();
        // No mocks mounted — any outbound request would 404 and the call would
        // error. An empty input must short-circuit to Ok(empty).
        let s = server().await;
        let c = ModrinthClient::with_base(s.uri());
        let out = c.summaries(&[]).await.unwrap();
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn versions_by_ids_parses_versions_with_deps() {
        let _g = test_lock();
        let s = server().await;
        Mock::given(method("GET"))
            .and(path("/v2/versions"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"[{
                  "id":"vid1","project_id":"jei","name":"JEI 15","version_number":"15.0.0",
                  "game_versions":["1.20.1"],"loaders":["fabric"],"date_published":"2026-05-01T00:00:00Z",
                  "files":[{"url":"https://cdn/x.jar","filename":"jei.jar","hashes":{"sha1":"abc"},"size":1,"primary":true}],
                  "dependencies":[{"project_id":"fabric-api","version_id":null,"dependency_type":"required"}]
                }]"#,
            ))
            .mount(&s)
            .await;
        let c = ModrinthClient::with_base(s.uri());
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let out = c.versions_by_ids(&["vid1"]).await.unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].project_id, "jei");
        assert_eq!(out[0].deps.len(), 1);
        assert_eq!(out[0].deps[0].kind, DepKind::Required);
    }

    #[tokio::test]
    async fn search_parses_hits() {
        let _g = test_lock();
        let s = server().await;
        Mock::given(method("GET"))
            .and(path("/v2/search"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{
                "hits": [{
                    "project_id":"u6dRKJwZ","slug":"jei","title":"JEI","description":"Items",
                    "icon_url":null,"downloads":1234,"author":"mezz","date_modified":"2026-05-01T00:00:00Z"
                }],
                "total_hits":1,"offset":0,"limit":20
            }"#,
            ))
            .mount(&s)
            .await;
        let c = ModrinthClient::with_base(s.uri());
        let q = ModSearchQuery {
            source: ModSource::Modrinth,
            kind: ContentKind::Mod,
            query: "jei".into(),
            mc_version: Some("1.20.1".into()),
            loader: Some(LoaderKind::Fabric),
            sort: ModSort::Downloads,
            page_size: 20,
            offset: 0,
        };
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let page = c.search(&q).await.unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert_eq!(page.total, 1);
        assert_eq!(page.hits[0].name, "JEI");
        assert_eq!(page.hits[0].project_id, "u6dRKJwZ");
        assert_eq!(page.hits[0].source, ModSource::Modrinth);
    }

    #[tokio::test]
    async fn search_5xx_maps_to_network_error() {
        let _g = test_lock();
        let s = server().await;
        Mock::given(method("GET"))
            .and(path("/v2/search"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&s)
            .await;
        let c = ModrinthClient::with_base(s.uri());
        let q = ModSearchQuery {
            source: ModSource::Modrinth,
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
        assert!(matches!(err, Error::ModsNetwork { .. }), "got: {err:?}");
    }

    #[tokio::test]
    async fn project_404_maps_to_not_found() {
        let _g = test_lock();
        let s = server().await;
        Mock::given(method("GET"))
            .and(path("/v2/project/missing"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&s)
            .await;
        let c = ModrinthClient::with_base(s.uri());
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let err = c.project("missing").await.unwrap_err();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert!(matches!(err, Error::ModsNotFound { .. }), "got: {err:?}");
    }

    #[tokio::test]
    async fn project_renders_body_and_orders_gallery() {
        let _g = test_lock();
        let s = server().await;
        Mock::given(method("GET"))
            .and(path("/v2/project/jei"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r##"{"id":"u6dRKJwZ","slug":"jei","title":"JEI","description":"Items",
                   "body":"# Hello\n\n![s](https://media.modrinth.com/b.png)",
                   "icon_url":null,"downloads":10,"source_url":null,"wiki_url":null,"team":"t",
                   "gallery":[
                     {"url":"https://media.modrinth.com/a.png","title":"A","featured":false,"ordering":2},
                     {"url":"https://media.modrinth.com/f.png","title":"F","featured":true,"ordering":9}
                   ]}"##,
            ))
            .mount(&s)
            .await;
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let c = ModrinthClient::with_base(s.uri());
        let p = c.project("jei").await.unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert!(p.body_html.contains("<h1>"));
        assert!(p.body_html.contains("https://media.modrinth.com/b.png"));
        // Featured image sorts first regardless of ordering value.
        assert_eq!(p.gallery[0].url, "https://media.modrinth.com/f.png");
        assert_eq!(p.gallery[1].url, "https://media.modrinth.com/a.png");
    }

    #[tokio::test]
    async fn versions_parses_primary_file_and_deps() {
        let _g = test_lock();
        let s = server().await;
        Mock::given(method("GET"))
            .and(path("/v2/project/jei/version"))
            .and(query_param("loaders", r#"["fabric"]"#))
            .and(query_param("game_versions", r#"["1.20.1"]"#))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"[{
                "id":"vid1","project_id":"jei","name":"JEI 15","version_number":"15.0.0",
                "game_versions":["1.20.1"],"loaders":["fabric"],"date_published":"2026-05-01T00:00:00Z",
                "files":[{"url":"https://cdn.modrinth.com/x.jar","filename":"jei-15.0.0.jar",
                          "hashes":{"sha1":"abc"},"size":100,"primary":true}],
                "dependencies":[{"project_id":"fabric-api","version_id":null,"dependency_type":"required"}]
            }]"#,
            ))
            .mount(&s)
            .await;
        let c = ModrinthClient::with_base(s.uri());
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let vs = c
            .versions("jei", Some("1.20.1"), Some(LoaderKind::Fabric))
            .await
            .unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert_eq!(vs.len(), 1);
        assert_eq!(vs[0].primary_file.filename, "jei-15.0.0.jar");
        assert_eq!(vs[0].primary_file.sha1.as_deref(), Some("abc"));
        assert_eq!(vs[0].deps.len(), 1);
        assert_eq!(vs[0].deps[0].kind, DepKind::Required);
    }

    #[tokio::test]
    async fn versions_drops_neoforge_jar_mistagged_as_forge() {
        // Real Xaero's Minimap 1.20.4 data: the author tags BOTH the Forge and
        // the NeoForge build with the `forge` loader, and the NeoForge build is
        // newest (so it sorts first). A Forge request must not install it.
        let _g = test_lock();
        let s = server().await;
        Mock::given(method("GET"))
            .and(path("/v2/project/xaeros-minimap/version"))
            .and(query_param("loaders", r#"["forge"]"#))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"[
                {"id":"v1","project_id":"xaeros-minimap","name":"neoforge-1.20.4-25.3.13",
                 "version_number":"neoforge-1.20.4-25.3.13","game_versions":["1.20.4"],
                 "loaders":["forge"],"date_published":"2026-03-13T00:00:00Z",
                 "files":[{"url":"https://cdn/n.jar","filename":"xaerominimap-neoforge-1.20.4-25.3.13.jar",
                           "hashes":{"sha1":"n1"},"size":100,"primary":true}],
                 "dependencies":[]},
                {"id":"v2","project_id":"xaeros-minimap","name":"forge-1.20.4-25.3.13",
                 "version_number":"forge-1.20.4-25.3.13","game_versions":["1.20.4"],
                 "loaders":["forge"],"date_published":"2026-03-13T00:00:00Z",
                 "files":[{"url":"https://cdn/f.jar","filename":"xaerominimap-forge-1.20.4-25.3.13.jar",
                           "hashes":{"sha1":"f1"},"size":100,"primary":true}],
                 "dependencies":[]}
            ]"#,
            ))
            .mount(&s)
            .await;
        let c = ModrinthClient::with_base(s.uri());
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let vs = c
            .versions("xaeros-minimap", Some("1.20.4"), Some(LoaderKind::Forge))
            .await
            .unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert_eq!(vs.len(), 1, "the mis-tagged NeoForge jar must be dropped");
        assert_eq!(
            vs[0].primary_file.filename,
            "xaerominimap-forge-1.20.4-25.3.13.jar"
        );
    }

    #[tokio::test]
    async fn resolve_deps_flags_only_required_when_no_compatible_version() {
        let _g = test_lock();
        let s = server().await;
        // Neither dep has a compatible build (both endpoints return []).
        for pid in ["optdep", "reqdep"] {
            Mock::given(method("GET"))
                .and(path(format!("/v2/project/{pid}/version")))
                .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
                .mount(&s)
                .await;
        }
        let c = ModrinthClient::with_base(s.uri());
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");

        let v = ModVersion {
            source: ModSource::Modrinth,
            project_id: "primary".into(),
            version_id: "vp".into(),
            name: "Primary".into(),
            version_number: "1.0".into(),
            mc_versions: vec!["1.20.4".into()],
            loaders: vec![LoaderKind::Forge],
            primary_file: ModFile {
                filename: "p.jar".into(),
                url: "https://cdn/p.jar".into(),
                sha1: Some("aa".into()),
                size: 1.0,
                distribution_allowed: true,
            },
            deps: vec![
                ModDepLink {
                    kind: DepKind::Optional,
                    project_ref: DepProjectRef::Modrinth {
                        project_id: "optdep".into(),
                        version_id: None,
                    },
                },
                ModDepLink {
                    kind: DepKind::Required,
                    project_ref: DepProjectRef::Modrinth {
                        project_id: "reqdep".into(),
                        version_id: None,
                    },
                },
            ],
            published_at: None,
        };
        let rd = c
            .resolve_deps(&v, "1.20.4", LoaderKind::Forge)
            .await
            .unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");

        // The optional dep with no compatible build is skipped silently; only
        // the missing *required* dep is surfaced as unresolvable.
        assert!(rd.optional.is_empty(), "optional dep should not resolve");
        assert!(rd.required.is_empty(), "required dep has no build");
        assert_eq!(rd.unresolvable.len(), 1, "only the required dep is flagged");
        match &rd.unresolvable[0] {
            DepProjectRef::Modrinth { project_id, .. } => assert_eq!(project_id, "reqdep"),
            other => panic!("expected modrinth reqdep ref, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn server_side_bulk_extracts_support_per_project() {
        let _g = test_lock();
        let s = server().await;
        Mock::given(method("GET"))
            .and(path("/v2/projects"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"[
                  {"id":"betterf3","server_side":"unsupported"},
                  {"id":"jei","server_side":"optional"},
                  {"id":"voicechat","server_side":"required"},
                  {"id":"weird","server_side":"unknown"},
                  {"id":"nofield"}
                ]"#,
            ))
            .mount(&s)
            .await;
        let c = ModrinthClient::with_base(s.uri());
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let m = c
            .server_side_bulk(&["betterf3", "jei", "voicechat", "weird", "nofield"])
            .await
            .unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert_eq!(m.get("betterf3"), Some(&ServerSideSupport::Unsupported));
        assert_eq!(m.get("jei"), Some(&ServerSideSupport::Optional));
        assert_eq!(m.get("voicechat"), Some(&ServerSideSupport::Required));
        assert_eq!(m.get("weird"), Some(&ServerSideSupport::Unknown));
        // A project that omits `server_side` decodes as Unknown, not an error.
        assert_eq!(m.get("nofield"), Some(&ServerSideSupport::Unknown));
    }

    #[tokio::test]
    async fn server_side_bulk_empty_ids_makes_no_request() {
        let _g = test_lock();
        // No mock mounted — an empty input must short-circuit to Ok(empty).
        let s = server().await;
        let c = ModrinthClient::with_base(s.uri());
        let m = c.server_side_bulk(&[]).await.unwrap();
        assert!(m.is_empty());
    }

    #[tokio::test]
    async fn project_ids_by_hash_maps_sha_to_project_lowercased() {
        let _g = test_lock();
        let s = server().await;
        Mock::given(method("POST"))
            .and(path("/v2/version_files"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{
                  "aabbcc":{"project_id":"betterf3","id":"v1"},
                  "ddeeff":{"project_id":"jei","id":"v2"}
                }"#,
            ))
            .mount(&s)
            .await;
        let c = ModrinthClient::with_base(s.uri());
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let m = c.project_ids_by_hash(&["AABBCC", "ddeeff"]).await.unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert_eq!(m.get("aabbcc"), Some(&"betterf3".to_string()));
        assert_eq!(m.get("ddeeff"), Some(&"jei".to_string()));
    }
}
