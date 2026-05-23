//! Modrinth v2 API client.

mod types;

use async_trait::async_trait;

use crate::error::Error;
use crate::mods::platform::*;

const BASE_DEFAULT: &str = "https://api.modrinth.com";
const UA: &str = "AntonBabchenko/FTlauncher (github.com/AntonBabchenko/FTlauncher)";

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
        match loader {
            LoaderKind::Fabric => "fabric",
            LoaderKind::Quilt => "quilt",
            LoaderKind::Forge => "forge",
            LoaderKind::NeoForge => "neoforge",
            LoaderKind::Vanilla => "minecraft",
        }
    }

    fn sort_key(sort: ModSort) -> &'static str {
        match sort {
            ModSort::Relevance => "relevance",
            ModSort::Downloads => "downloads",
            ModSort::Updated => "updated",
        }
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
        let mut facets: Vec<Vec<String>> = vec![vec!["project_type:mod".into()]];
        if let Some(mc) = &q.mc_version {
            facets.push(vec![format!("versions:{mc}")]);
        }
        if let Some(l) = q.loader {
            facets.push(vec![format!("categories:{}", Self::loader_facet(l))]);
        }
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
        Ok(ModProject {
            summary: ModSummary {
                source: ModSource::Modrinth,
                project_id: p.id,
                slug: Some(p.slug),
                name: p.title,
                summary: p.description,
                icon_url: p.icon_url,
                downloads: p.downloads as f64,
                author: p.team,
                updated_at: None,
            },
            description: p.body,
            website_url: p.source_url.or(p.wiki_url),
        })
    }

    async fn versions(
        &self,
        project_id: &str,
        mc: &str,
        loader: LoaderKind,
    ) -> Result<Vec<ModVersion>, Error> {
        let loaders = serde_json::to_string(&[Self::loader_facet(loader)]).unwrap();
        let games = serde_json::to_string(&[mc]).unwrap();
        let url = format!(
            "{}/v2/project/{}/version?loaders={}&game_versions={}",
            self.base,
            project_id,
            urlencode(&loaders),
            urlencode(&games),
        );
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
        Ok(raws.into_iter().map(convert_version).collect())
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
            let pid = match &dep.project_ref {
                DepProjectRef::Modrinth { project_id, .. } => project_id.clone(),
                DepProjectRef::Curseforge { .. } => {
                    unresolvable.push(dep.project_ref.clone());
                    continue;
                }
            };
            match dep.kind {
                DepKind::Incompatible => {
                    incompatible.push(dep.project_ref.clone());
                    continue;
                }
                DepKind::Embedded => continue,
                _ => {}
            }
            let versions = self.versions(&pid, mc, loader).await?;
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
            } else {
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

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::test_env_lock()
    }

    async fn server() -> MockServer {
        MockServer::start().await
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
            query: "jei".into(),
            mc_version: Some("1.20.1".into()),
            loader: Some(LoaderKind::Fabric),
            sort: ModSort::Downloads,
            page_size: 20,
            offset: 0,
        };
        std::env::set_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let page = c.search(&q).await.unwrap();
        std::env::remove_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS");
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
            query: "x".into(),
            mc_version: None,
            loader: None,
            sort: ModSort::Relevance,
            page_size: 20,
            offset: 0,
        };
        std::env::set_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let err = c.search(&q).await.unwrap_err();
        std::env::remove_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS");
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
        std::env::set_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let err = c.project("missing").await.unwrap_err();
        std::env::remove_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS");
        assert!(matches!(err, Error::ModsNotFound { .. }), "got: {err:?}");
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
        std::env::set_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let vs = c
            .versions("jei", "1.20.1", LoaderKind::Fabric)
            .await
            .unwrap();
        std::env::remove_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS");
        assert_eq!(vs.len(), 1);
        assert_eq!(vs[0].primary_file.filename, "jei-15.0.0.jar");
        assert_eq!(vs[0].primary_file.sha1.as_deref(), Some("abc"));
        assert_eq!(vs[0].deps.len(), 1);
        assert_eq!(vs[0].deps[0].kind, DepKind::Required);
    }
}
