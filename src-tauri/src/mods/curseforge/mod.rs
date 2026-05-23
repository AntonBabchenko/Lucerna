//! CurseForge Eternal API client (v1).

pub mod keyring;
mod types;

use async_trait::async_trait;

use crate::error::Error;
use crate::mods::platform::*;

const BASE_DEFAULT: &str = "https://api.curseforge.com";

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
        let mut params: Vec<(&str, String)> = vec![
            ("gameId", types::GAME_MINECRAFT.to_string()),
            ("searchFilter", q.query.clone()),
            ("pageSize", q.page_size.to_string()),
            ("index", q.offset.to_string()),
        ];
        if let Some(mc) = &q.mc_version {
            params.push(("gameVersion", mc.clone()));
        }
        if let Some(l) = q.loader {
            params.push(("modLoaderType", types::loader_type(l).to_string()));
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
        let total = env
            .pagination
            .as_ref()
            .map(|p| p.total_count)
            .unwrap_or(env.data.len() as u32);
        let offset = env.pagination.as_ref().map(|p| p.index).unwrap_or(q.offset);
        let page_size = env
            .pagination
            .as_ref()
            .map(|p| p.page_size)
            .unwrap_or(q.page_size);
        Ok(ModSearchPage {
            hits: env.data.into_iter().map(convert_mod_summary).collect(),
            total,
            offset,
            page_size,
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
        let summary = convert_mod_summary(env.data);
        // CF mod summary discards `links.websiteUrl` during conversion; the
        // detail drawer falls back to constructing the canonical CurseForge URL
        // from `slug`. Keep `website_url` None for v1.
        Ok(ModProject {
            summary,
            description: String::new(),
            website_url: None,
        })
    }

    async fn versions(
        &self,
        project_id: &str,
        mc: &str,
        loader: LoaderKind,
    ) -> Result<Vec<ModVersion>, Error> {
        let auth = self.auth()?;
        let mut params: Vec<(&str, String)> = vec![
            ("gameVersion", mc.to_string()),
            ("pageSize", "50".to_string()),
        ];
        if loader != LoaderKind::Vanilla {
            params.push(("modLoaderType", types::loader_type(loader).to_string()));
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
        Ok(env
            .data
            .into_iter()
            .filter_map(|f| convert_version(f, project_id))
            .collect())
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
                DepProjectRef::Curseforge { mod_id, .. } => mod_id.to_string(),
                DepProjectRef::Modrinth { .. } => {
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
            let vs = self.versions(&pid, mc, loader).await?;
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
    Some(ModVersion {
        source: ModSource::Curseforge,
        project_id: project_id.to_string(),
        version_id: f.id.to_string(),
        name: f.display_name,
        version_number: f.file_name.clone(),
        mc_versions: f.game_versions,
        loaders: Vec::new(), // CF doesn't tag loaders on files; UI filters by query param
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
    use wiremock::matchers::{header, method, path};
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
        match err {
            Error::ModsPlatformAuth { kind } => {
                assert_eq!(kind, crate::error::ModsAuthKind::Invalid)
            }
            _ => panic!("expected ModsPlatformAuth::Invalid, got {err:?}"),
        }
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
            query: "jei".into(),
            mc_version: Some("1.20.1".into()),
            loader: Some(LoaderKind::Fabric),
            sort: ModSort::Downloads,
            page_size: 20,
            offset: 0,
        };
        std::env::set_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let page = client(s.uri()).search(&q).await.unwrap();
        std::env::remove_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS");
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
        std::env::set_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let v = client(s.uri())
            .versions("12345", "1.20.1", LoaderKind::Fabric)
            .await
            .unwrap();
        std::env::remove_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS");
        assert_eq!(v.len(), 1);
        assert!(!v[0].primary_file.distribution_allowed);
    }
}
