//! Modrinth-only modpack search. Thin wrapper around the
//! `api.modrinth.com/v2/search` endpoint with the
//! `project_type:modpack` facet pre-applied so the Modpacks tab's
//! Browse view never sees regular mod hits.
//!
//! CurseForge modpack search is out of scope for sub-feature 4 —
//! users can still import CF modpacks via local file pick.

use serde::Deserialize;

use crate::error::Error;
use crate::mods::modpack::schema::*;
use crate::mods::platform::LoaderKind;

const UA: &str = "AntonBabchenko/Lucerna (github.com/AntonBabchenko/Lucerna)";

#[derive(Debug, Deserialize)]
struct MrSearch {
    hits: Vec<MrHit>,
    total_hits: u32,
    offset: u32,
    limit: u32,
}

#[derive(Debug, Deserialize)]
struct MrHit {
    project_id: String,
    slug: String,
    title: String,
    description: String,
    icon_url: Option<String>,
    downloads: u64,
    latest_version: Option<String>,
    /// Project owner, as returned by `/v2/search`. Absent on some hits.
    #[serde(default)]
    author: Option<String>,
    categories: Vec<String>,
}

/// Minimal percent-encoder for query-string values. Encodes anything
/// outside the URL-safe unreserved set `[A-Za-z0-9-_.~]` using uppercase
/// `%HH` form. Mirrors the helper in `mods/modrinth/mod.rs` — kept
/// local to avoid pulling `serde_urlencoded` features that are disabled
/// by our `reqwest` flags and to avoid a new `urlencoding` dependency.
pub(crate) fn urlencode(s: &str) -> String {
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

pub async fn search(
    base: &str,
    query: &str,
    page: u32,
    mc_version: Option<&str>,
    loader: Option<LoaderKind>,
    sort: ModpackSort,
    page_size: u32,
) -> Result<ModpackSearchPage, Error> {
    let limit: u32 = page_size;
    let offset = page * limit;
    let mut facets: Vec<Vec<String>> = vec![vec!["project_type:modpack".into()]];
    if let Some(mc) = mc_version {
        facets.push(vec![format!("versions:{mc}")]);
    }
    if let Some(l) = loader {
        let f = match l {
            LoaderKind::Fabric => "fabric",
            LoaderKind::Quilt => "quilt",
            LoaderKind::Forge => "forge",
            LoaderKind::NeoForge => "neoforge",
            LoaderKind::Vanilla => "minecraft",
        };
        facets.push(vec![format!("categories:{f}")]);
    }
    let index = match sort {
        ModpackSort::Relevance => "relevance",
        ModpackSort::Downloads => "downloads",
        ModpackSort::Newest => "newest",
        ModpackSort::Updated => "updated",
    };
    // unreachable: `Vec<Vec<String>>` always serializes — no non-UTF-8 keys,
    // no recursive cycles. Per CLAUDE.md `.unwrap()` rule.
    let facets_json = serde_json::to_string(&facets).unwrap();
    let q = urlencode(query);
    let f = urlencode(&facets_json);
    let url = format!(
        "{base}/v2/search?query={q}&limit={limit}&offset={offset}&index={index}&facets={f}"
    );
    let resp = crate::network::request::get(&url, &[("user-agent", UA)], "modpacks")
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
    let s: MrSearch = serde_json::from_slice(&resp.body).map_err(|e| Error::ModsDecode {
        platform: "modrinth".into(),
        details: e.to_string(),
    })?;

    let hits = s
        .hits
        .into_iter()
        .map(|h| {
            let supported_loaders: Vec<LoaderKind> = h
                .categories
                .iter()
                .filter_map(|c| match c.as_str() {
                    "fabric" => Some(LoaderKind::Fabric),
                    "quilt" => Some(LoaderKind::Quilt),
                    "forge" => Some(LoaderKind::Forge),
                    "neoforge" => Some(LoaderKind::NeoForge),
                    _ => None,
                })
                .collect();
            ModpackHit {
                project_id: h.project_id,
                slug: h.slug,
                title: h.title,
                description: h.description,
                icon_url: h.icon_url,
                downloads: h.downloads as f64,
                latest_mc_version: h.latest_version,
                supported_loaders,
                source: crate::mods::platform::ModSource::Modrinth,
                distribution_allowed: None,
                author: h.author,
            }
        })
        .collect();

    Ok(ModpackSearchPage {
        hits,
        total: s.total_hits,
        offset: s.offset,
        limit: s.limit,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::platform::ModSource;
    use wiremock::matchers::{method, path, query_param_contains};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::test_env_lock()
    }

    #[tokio::test]
    async fn search_returns_normalised_hits() {
        let _g = test_lock();
        let s = MockServer::start().await;
        let resp = serde_json::json!({
            "hits": [{
                "project_id": "PaCk1",
                "slug": "test-pack",
                "title": "Test Pack",
                "description": "...",
                "icon_url": "https://cdn.modrinth.com/data/.../icon.png",
                "downloads": 1234,
                "latest_version": "1.20.1",
                "author": "PackOwner",
                "categories": ["fabric", "magic"]
            }],
            "total_hits": 1,
            "offset": 0,
            "limit": 20
        });
        Mock::given(method("GET"))
            .and(path("/v2/search"))
            .and(query_param_contains("facets", "project_type:modpack"))
            .respond_with(ResponseTemplate::new(200).set_body_json(resp))
            .mount(&s)
            .await;

        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let r = search(&s.uri(), "test", 0, None, None, ModpackSort::Relevance, 20)
            .await
            .unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert_eq!(r.total, 1);
        assert_eq!(r.hits[0].project_id, "PaCk1");
        assert_eq!(r.hits[0].supported_loaders, vec![LoaderKind::Fabric]);
        assert_eq!(r.hits[0].source, ModSource::Modrinth);
        assert!(r.hits[0].distribution_allowed.is_none());
        assert_eq!(r.hits[0].author.as_deref(), Some("PackOwner"));
    }

    #[tokio::test]
    async fn search_with_filters_facets() {
        let _g = test_lock();
        let s = MockServer::start().await;
        let resp = serde_json::json!({
            "hits": [],
            "total_hits": 0,
            "offset": 0,
            "limit": 20
        });
        Mock::given(method("GET"))
            .and(path("/v2/search"))
            .and(query_param_contains("facets", "versions:1.20.1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(resp))
            .mount(&s)
            .await;

        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let r = search(
            &s.uri(),
            "x",
            0,
            Some("1.20.1"),
            None,
            ModpackSort::Relevance,
            20,
        )
        .await
        .unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert_eq!(r.total, 0);
    }

    #[tokio::test]
    async fn search_with_sort_includes_index_param() {
        let _g = test_lock();
        let s = MockServer::start().await;
        let resp = serde_json::json!({
            "hits": [],
            "total_hits": 0,
            "offset": 0,
            "limit": 20
        });
        Mock::given(method("GET"))
            .and(path("/v2/search"))
            .and(query_param_contains("index", "downloads"))
            .respond_with(ResponseTemplate::new(200).set_body_json(resp))
            .mount(&s)
            .await;

        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let r = search(&s.uri(), "x", 0, None, None, ModpackSort::Downloads, 20)
            .await
            .unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert_eq!(r.total, 0);
    }
}
