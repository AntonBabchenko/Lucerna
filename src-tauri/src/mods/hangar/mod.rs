//! Hangar (hangar.papermc.io) — PaperMC's plugin repository, read-only api/v1.
//! Search + versions for the server plugin browser. Hosted files come from
//! hangarcdn.papermc.io with sha256; externally-hosted versions (externalUrl)
//! are NEVER downloaded in-app — distribution_allowed=false and url = the
//! external page, which the UI opens in the system browser.
//! Hangar has no PURPUR platform: Purpur servers also query platform=PAPER.
//! Documented rate limit 20 req / 5 s — see network::throttle::config_for.

mod types;

use async_trait::async_trait;

use crate::error::Error;
use crate::mods::platform::*;

const BASE_DEFAULT: &str = "https://hangar.papermc.io";
const UA: &str = "AntonBabchenko/Lucerna (github.com/AntonBabchenko/Lucerna)";
/// Cap on `/api/v1/projects` `limit` (live-verified 2026-07-09: limit=30 is
/// honored as 30; limit=100 clamps to 50). The shared browser UI offers page
/// sizes up to 100, so a larger request fans out into back-to-back windows of
/// this size — see `search()`.
const SEARCH_MAX_PAGE: u32 = 50;
/// Cap on `/api/v1/projects/{id}/versions` `limit` (live-verified 2026-07-09:
/// limit=30 clamps to 25). Results come newest-first and callers pick the
/// newest satisfying build, so deeper pagination is not needed there.
const VERSIONS_MAX_PAGE: u32 = 25;

pub struct HangarClient {
    base: String,
}

impl HangarClient {
    pub fn new() -> Self {
        Self {
            base: BASE_DEFAULT.into(),
        }
    }

    /// Tests inject a wiremock URL here.
    pub fn with_base(base: impl Into<String>) -> Self {
        Self { base: base.into() }
    }

    fn sort_key(sort: ModSort) -> &'static str {
        match sort {
            // Hangar has no distinct "relevance" sort key — downloads is the
            // closest analogue and what the search box defaults to upstream.
            ModSort::Relevance => "-downloads",
            ModSort::Downloads => "-downloads",
            // Live-verified: "-updated" sorts by lastUpdated. ("-newest"
            // sorts by createdAt — project creation date — which is NOT what
            // the Updated sort means.)
            ModSort::Updated => "-updated",
        }
    }
}

impl Default for HangarClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ModPlatform for HangarClient {
    async fn search(&self, q: &ModSearchQuery) -> Result<ModSearchPage, Error> {
        // Hangar caps `limit` at SEARCH_MAX_PAGE; a UI request for more (the
        // shared browser offers up to 100 and computes offset = page ×
        // page_size) is served by stitching back-to-back windows that tile
        // [offset, offset + page_size) — mirroring the CurseForge client's
        // windowing. page_size <= the cap stays a single request. Windows
        // stop early once one underfills (end of results).
        let want = q.page_size.max(1);
        let mut hits: Vec<ModSummary> = Vec::with_capacity(want as usize);
        let mut total = 0u32;
        let mut fetched = 0u32;
        while fetched < want {
            let chunk = (want - fetched).min(SEARCH_MAX_PAGE);
            let offset = q.offset + fetched;
            let mut url = format!(
                "{}/api/v1/projects?query={}&platform=PAPER&sort={}&limit={}&offset={}",
                self.base,
                urlencode(&q.query),
                Self::sort_key(q.sort),
                chunk,
                offset,
            );
            if let Some(mc) = q.mc_version.as_deref() {
                url.push_str(&format!("&version={}", urlencode(mc)));
            }
            let resp = crate::network::request::get(&url, &[("user-agent", UA)], "mods")
                .await
                .map_err(|e| Error::mods_network(url.clone(), e))?;
            if resp.status == 404 {
                return Err(Error::ModsNotFound {
                    platform: "hangar".into(),
                });
            }
            if !(200..300).contains(&resp.status) {
                return Err(Error::ModsNetwork {
                    url,
                    details: format!("HTTP {}", resp.status),
                });
            }
            let body: types::HangarPage<types::HangarProject> = serde_json::from_slice(&resp.body)
                .map_err(|e| Error::ModsDecode {
                    platform: "hangar".into(),
                    details: e.to_string(),
                })?;
            if fetched == 0 {
                total = body.pagination.count;
            }
            let got = body.result.len() as u32;
            hits.extend(body.result.into_iter().map(convert_summary));
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
        let url = format!("{}/api/v1/projects/{}", self.base, project_id);
        let resp = crate::network::request::get(&url, &[("user-agent", UA)], "mods")
            .await
            .map_err(|e| Error::mods_network(url.clone(), e))?;
        if resp.status == 404 {
            return Err(Error::ModsNotFound {
                platform: "hangar".into(),
            });
        }
        if !(200..300).contains(&resp.status) {
            return Err(Error::ModsNetwork {
                url,
                details: format!("HTTP {}", resp.status),
            });
        }
        let p: types::HangarProject =
            serde_json::from_slice(&resp.body).map_err(|e| Error::ModsDecode {
                platform: "hangar".into(),
                details: e.to_string(),
            })?;
        Ok(ModProject {
            summary: convert_summary(p),
            // Hangar's project detail page content isn't fetched by this
            // minimal implementation — the plugin browser's detail view
            // falls back to `summary` when `body_html` is empty.
            body_html: String::new(),
            gallery: Vec::new(),
            website_url: None,
        })
    }

    async fn versions(
        &self,
        project_id: &str,
        mc_version: Option<&str>,
        _loader: Option<LoaderKind>,
    ) -> Result<Vec<ModVersion>, Error> {
        // Hangar has no `LoaderKind` — every version query goes through the
        // plugin-loader path.
        self.plugin_versions(project_id, mc_version, &["paper"])
            .await
    }

    async fn plugin_versions(
        &self,
        project_id: &str,
        mc_version: Option<&str>,
        _plugin_loaders: &[&str],
    ) -> Result<Vec<ModVersion>, Error> {
        let mut url = format!(
            "{}/api/v1/projects/{}/versions?platform=PAPER&limit={}",
            self.base, project_id, VERSIONS_MAX_PAGE
        );
        if let Some(mc) = mc_version {
            url.push_str(&format!("&platformVersion={}", urlencode(mc)));
        }
        let resp = crate::network::request::get(&url, &[("user-agent", UA)], "mods")
            .await
            .map_err(|e| Error::mods_network(url.clone(), e))?;
        if resp.status == 404 {
            return Err(Error::ModsNotFound {
                platform: "hangar".into(),
            });
        }
        if !(200..300).contains(&resp.status) {
            return Err(Error::ModsNetwork {
                url,
                details: format!("HTTP {}", resp.status),
            });
        }
        let body: types::HangarPage<types::HangarVersion> = serde_json::from_slice(&resp.body)
            .map_err(|e| Error::ModsDecode {
                platform: "hangar".into(),
                details: e.to_string(),
            })?;
        Ok(body
            .result
            .into_iter()
            // A version with no PAPER entry at all has nothing installable
            // for this platform — defensively skip it rather than fabricate
            // an empty file.
            .filter(|v| v.downloads.contains_key("PAPER"))
            .map(|v| convert_version(project_id, v))
            .collect())
    }

    async fn resolve_deps(
        &self,
        _version: &ModVersion,
        _mc_version: &str,
        _loader: LoaderKind,
    ) -> Result<ResolvedDeps, Error> {
        // Hangar plugin dependencies are name-based (see `pluginDependencies`
        // in the live version payload), not project-id-based like Modrinth's
        // dependency graph. Cross-registry dependency resolution is a
        // non-goal for this client — the install pipeline surfaces Hangar
        // deps informationally, not as an auto-install plan.
        Ok(ResolvedDeps {
            required: Vec::new(),
            optional: Vec::new(),
            incompatible: Vec::new(),
            unresolvable: Vec::new(),
        })
    }
}

/// Map a Hangar project to the normalized summary. Shared by `search()` and
/// `project()` so both paths agree on the field mapping (project_id = slug,
/// author = namespace owner, updated_at = lastUpdated).
fn convert_summary(p: types::HangarProject) -> ModSummary {
    ModSummary {
        source: ModSource::Hangar,
        project_id: p.namespace.slug.clone(),
        slug: Some(p.namespace.slug),
        name: p.name,
        summary: p.description.unwrap_or_default(),
        icon_url: p.avatar_url,
        downloads: p.stats.downloads as f64,
        author: p.namespace.owner,
        updated_at: p.last_updated,
    }
}

/// Map one Hangar version's `PAPER` download entry to a `ModVersion`. Callers
/// filter out versions with no `PAPER` entry before calling this.
fn convert_version(project_id: &str, v: types::HangarVersion) -> ModVersion {
    let dl = v.downloads.get("PAPER");
    let file = match dl.and_then(|d| d.file_info.as_ref().zip(d.download_url.as_ref())) {
        Some((fi, url)) => ModFile {
            filename: fi.name.clone(),
            url: url.clone(),
            sha1: None,
            sha256: Some(fi.sha256_hash.to_ascii_lowercase()),
            size: fi.size_bytes,
            distribution_allowed: true,
        },
        // Externally-hosted version (no hosted downloadUrl). Usually there is
        // no fileInfo either, but when one is present keep its name for
        // display — only the download semantics differ from the hosted arm:
        // distribution_allowed=false and url = the external page.
        None => ModFile {
            filename: dl
                .and_then(|d| d.file_info.as_ref())
                .map(|fi| fi.name.clone())
                .unwrap_or_default(),
            url: dl.and_then(|d| d.external_url.clone()).unwrap_or_default(),
            sha1: None,
            sha256: None,
            size: 0.0,
            distribution_allowed: false,
        },
    };
    ModVersion {
        source: ModSource::Hangar,
        project_id: project_id.to_string(),
        version_id: v.name.clone(),
        name: v.name.clone(),
        version_number: v.name,
        mc_versions: v
            .platform_dependencies
            .get("PAPER")
            .cloned()
            .unwrap_or_default(),
        loaders: Vec::new(),
        primary_file: file,
        // Hangar plugin deps are name-based, not project-id-based — cross-
        // registry dependency linking is a non-goal (see `resolve_deps`).
        deps: Vec::new(),
        published_at: v.created_at,
    }
}

/// Minimal percent-encoder for query-string values. Encodes anything outside
/// the URL-safe unreserved set [A-Za-z0-9-_.~] using uppercase %HH form.
/// Mirrors the identical helper in `mods::modrinth` — kept local to avoid a
/// cross-module dependency for a five-line function.
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

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn search_maps_envelope_to_page() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/projects"))
            .and(query_param("platform", "PAPER"))
            .and(query_param("version", "1.21.4"))
            .and(query_param("sort", "-downloads"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"pagination":{"limit":25,"offset":0,"count":1},
                    "result":[{"name":"LuckPerms","namespace":{"owner":"Luck","slug":"LuckPerms"},
                               "stats":{"downloads":123456},"description":"Permissions plugin",
                               "avatarUrl":"https://hangarcdn.papermc.io/avatars/project/1.webp",
                               "category":"admin_tools","lastUpdated":"2026-06-01T00:00:00Z"}]}"#,
            ))
            .mount(&s)
            .await;
        let c = HangarClient::with_base(s.uri());
        let q = ModSearchQuery {
            source: ModSource::Hangar,
            kind: ContentKind::Plugin,
            query: "luck".into(),
            mc_version: Some("1.21.4".into()),
            loader: None,
            plugin_core: Some(crate::servers_runtime::schema::ServerCore::Paper),
            sort: ModSort::Downloads,
            page_size: 20,
            offset: 0,
        };
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let page = c.search(&q).await.unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.hits[0].project_id, "LuckPerms");
        assert_eq!(page.hits[0].author, "Luck");
        assert_eq!(page.hits[0].source, ModSource::Hangar);
        assert_eq!(
            page.hits[0].updated_at.as_deref(),
            Some("2026-06-01T00:00:00Z"),
            "updated_at comes from the project's lastUpdated field"
        );
    }

    #[tokio::test]
    async fn search_sort_updated_maps_to_dash_updated() {
        // Live-verified: Hangar's "-updated" sorts by lastUpdated while
        // "-newest" sorts by createdAt (project creation date). The mock only
        // matches sort=-updated, so a regression back to "-newest" would fall
        // through to wiremock's 404 and fail the unwrap.
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/projects"))
            .and(query_param("sort", "-updated"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(
                    r#"{"pagination":{"limit":25,"offset":0,"count":0},"result":[]}"#,
                ),
            )
            .mount(&s)
            .await;
        let c = HangarClient::with_base(s.uri());
        let q = ModSearchQuery {
            source: ModSource::Hangar,
            kind: ContentKind::Plugin,
            query: "x".into(),
            mc_version: None,
            loader: None,
            plugin_core: None,
            sort: ModSort::Updated,
            page_size: 20,
            offset: 0,
        };
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let page = c.search(&q).await.unwrap();
        assert_eq!(page.total, 0);
    }

    #[tokio::test]
    async fn plugin_versions_maps_hosted_and_external_files() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/projects/Essentials/versions"))
            .and(query_param("platform", "PAPER"))
            .and(query_param("platformVersion", "1.21.4"))
            .and(query_param("limit", "25"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"pagination":{"limit":25,"offset":0,"count":3},
                    "result":[
                      {"name":"2.21.0","createdAt":"2026-06-01T00:00:00Z",
                       "downloads":{"PAPER":{"downloadUrl":"https://hangarcdn.papermc.io/plugins/E/E.jar",
                                             "externalUrl":null,
                                             "fileInfo":{"name":"Essentials-2.21.0.jar","sizeBytes":900,"sha256Hash":"FF00"}}},
                       "platformDependencies":{"PAPER":["1.21.4"]}},
                      {"name":"2.20.0","createdAt":"2026-01-01T00:00:00Z",
                       "downloads":{"PAPER":{"downloadUrl":null,
                                             "externalUrl":"https://github.com/EssentialsX/releases",
                                             "fileInfo":null}},
                       "platformDependencies":{"PAPER":["1.21.4"]}},
                      {"name":"2.19.0","createdAt":"2025-06-01T00:00:00Z",
                       "downloads":{"PAPER":{"downloadUrl":null,
                                             "externalUrl":"https://example.com/dl",
                                             "fileInfo":{"name":"Essentials-2.19.0.jar","sizeBytes":800,"sha256Hash":"AB"}}},
                       "platformDependencies":{"PAPER":["1.21.4"]}}
                    ]}"#,
            ))
            .mount(&s)
            .await;
        let c = HangarClient::with_base(s.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let vs = c
            .plugin_versions("Essentials", Some("1.21.4"), &["bukkit", "spigot", "paper"])
            .await
            .unwrap();
        assert_eq!(vs.len(), 3);
        // Hosted file: installable, sha256 carried LOWERCASED, distribution allowed.
        assert!(vs[0].primary_file.distribution_allowed);
        assert_eq!(vs[0].primary_file.sha256.as_deref(), Some("ff00"));
        assert_eq!(vs[0].primary_file.filename, "Essentials-2.21.0.jar");
        // External file: NOT downloadable in-app; url = external page.
        assert!(!vs[1].primary_file.distribution_allowed);
        assert_eq!(
            vs[1].primary_file.url,
            "https://github.com/EssentialsX/releases"
        );
        // External file WITH fileInfo: still not downloadable, but the
        // published filename is kept for display.
        assert!(!vs[2].primary_file.distribution_allowed);
        assert_eq!(vs[2].primary_file.filename, "Essentials-2.19.0.jar");
        assert_eq!(vs[2].primary_file.url, "https://example.com/dl");
    }

    #[tokio::test]
    async fn plugin_versions_skips_versions_with_no_paper_entry() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/projects/Velocious/versions"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"pagination":{"limit":25,"offset":0,"count":1},
                    "result":[
                      {"name":"1.0.0","createdAt":"2026-01-01T00:00:00Z",
                       "downloads":{"VELOCITY":{"downloadUrl":"https://hangarcdn.papermc.io/v.jar",
                                                "externalUrl":null,
                                                "fileInfo":{"name":"v.jar","sizeBytes":1,"sha256Hash":"AA"}}},
                       "platformDependencies":{"VELOCITY":["3.4"]}}
                    ]}"#,
            ))
            .mount(&s)
            .await;
        let c = HangarClient::with_base(s.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let vs = c
            .plugin_versions("Velocious", None, &["bukkit", "spigot", "paper"])
            .await
            .unwrap();
        assert!(
            vs.is_empty(),
            "a version with no PAPER downloads entry must be skipped"
        );
    }

    #[tokio::test]
    async fn search_5xx_maps_to_network_error() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/projects"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&s)
            .await;
        let c = HangarClient::with_base(s.uri());
        let q = ModSearchQuery {
            source: ModSource::Hangar,
            kind: ContentKind::Plugin,
            query: "x".into(),
            mc_version: None,
            loader: None,
            plugin_core: None,
            sort: ModSort::Relevance,
            page_size: 20,
            offset: 0,
        };
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let err = c.search(&q).await.unwrap_err();
        assert!(matches!(err, Error::ModsNetwork { .. }), "got: {err:?}");
    }

    /// One minimal search-hit JSON object for the windowing tests.
    fn hit_json(i: u32) -> String {
        format!(
            r#"{{"name":"P{i}","namespace":{{"owner":"o","slug":"p{i}"}},"stats":{{"downloads":1}},"description":"d"}}"#
        )
    }

    /// A search window body carrying the hits for `range` and the given total.
    fn window_json(range: std::ops::Range<u32>, count: u32) -> String {
        let hits: Vec<String> = range.map(hit_json).collect();
        format!(
            r#"{{"pagination":{{"limit":50,"offset":0,"count":{count}}},"result":[{}]}}"#,
            hits.join(",")
        )
    }

    #[tokio::test]
    async fn search_fans_out_over_the_50_cap_and_stitches_windows() {
        // page_size=100 must fan out into two <=50 windows (offset 0 + 50),
        // stitched in order. The second window underfills (10 < 50), which
        // also ends the loop before a guaranteed-empty third request.
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/projects"))
            .and(query_param("limit", "50"))
            .and(query_param("offset", "0"))
            .respond_with(ResponseTemplate::new(200).set_body_string(window_json(0..50, 60)))
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v1/projects"))
            .and(query_param("limit", "50"))
            .and(query_param("offset", "50"))
            .respond_with(ResponseTemplate::new(200).set_body_string(window_json(50..60, 60)))
            .mount(&s)
            .await;
        let c = HangarClient::with_base(s.uri());
        let q = ModSearchQuery {
            source: ModSource::Hangar,
            kind: ContentKind::Plugin,
            query: "x".into(),
            mc_version: None,
            loader: None,
            plugin_core: None,
            sort: ModSort::Downloads,
            page_size: 100,
            offset: 0,
        };
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let page = c.search(&q).await.unwrap();
        assert_eq!(page.hits.len(), 60, "two windows (50+10) stitch to 60");
        assert_eq!(page.total, 60);
        assert_eq!(
            page.hits[50].project_id, "p50",
            "second window continues in order"
        );
        assert_eq!(page.page_size, 100, "requested page size is echoed");
    }

    #[tokio::test]
    async fn search_requests_never_exceed_limit_50_per_window() {
        // The mock ONLY matches limit=50 — if the client passed the raw
        // page_size (100) or an over-tight clamp (25), wiremock would answer
        // 404 and the unwrap below would fail.
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/projects"))
            .and(query_param("limit", "50"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(
                    r#"{"pagination":{"limit":50,"offset":0,"count":0},"result":[]}"#,
                ),
            )
            .mount(&s)
            .await;
        let c = HangarClient::with_base(s.uri());
        let q = ModSearchQuery {
            source: ModSource::Hangar,
            kind: ContentKind::Plugin,
            query: "x".into(),
            mc_version: None,
            loader: None,
            plugin_core: None,
            sort: ModSort::Downloads,
            page_size: 100,
            offset: 0,
        };
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let page = c.search(&q).await.unwrap();
        assert!(page.hits.is_empty(), "empty first window ends the fan-out");
        assert_eq!(page.page_size, 100, "requested page size is echoed");
    }

    #[tokio::test]
    async fn project_404_maps_to_not_found() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/projects/missing"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&s)
            .await;
        let c = HangarClient::with_base(s.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let err = c.project("missing").await.unwrap_err();
        assert!(matches!(err, Error::ModsNotFound { .. }), "got: {err:?}");
    }

    #[tokio::test]
    async fn resolve_deps_returns_empty_plan() {
        let c = HangarClient::with_base("http://127.0.0.1:1");
        let v = ModVersion {
            source: ModSource::Hangar,
            project_id: "p".into(),
            version_id: "1.0".into(),
            name: "1.0".into(),
            version_number: "1.0".into(),
            mc_versions: vec!["1.21.4".into()],
            loaders: Vec::new(),
            primary_file: ModFile {
                filename: "p.jar".into(),
                url: "https://cdn/p.jar".into(),
                sha1: None,
                sha256: Some("aa".into()),
                size: 1.0,
                distribution_allowed: true,
            },
            deps: Vec::new(),
            published_at: None,
        };
        let rd = c
            .resolve_deps(&v, "1.21.4", LoaderKind::Fabric)
            .await
            .unwrap();
        assert!(rd.required.is_empty());
        assert!(rd.optional.is_empty());
        assert!(rd.incompatible.is_empty());
        assert!(rd.unresolvable.is_empty());
    }
}
