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

/// Modrinth loader tags → `LoaderKind`. Shared by `search` (which reads them
/// from a hit's `categories`) and `fetch_project_hit` (which reads the project's
/// `loaders`), so the two cannot drift on which tags are recognised.
fn loaders_from_tags(tags: &[String]) -> Vec<LoaderKind> {
    tags.iter()
        .filter_map(|c| match c.as_str() {
            "fabric" => Some(LoaderKind::Fabric),
            "quilt" => Some(LoaderKind::Quilt),
            "forge" => Some(LoaderKind::Forge),
            "neoforge" => Some(LoaderKind::NeoForge),
            _ => None,
        })
        .collect()
}

/// `/v2/project/{id-or-slug}` — the single-project shape. Only the fields a
/// `ModpackHit` needs, plus `project_type` so a link pointing at a mod (the most
/// likely user mistake) can be rejected with a message that says so.
#[derive(Debug, Deserialize)]
struct MrProject {
    id: String,
    slug: String,
    title: String,
    description: String,
    icon_url: Option<String>,
    downloads: u64,
    project_type: String,
    #[serde(default)]
    game_versions: Vec<String>,
    #[serde(default)]
    loaders: Vec<String>,
}

/// Resolve ONE Modrinth project reference (slug or id — Modrinth accepts either
/// on this route) to the same `ModpackHit` shape search produces, so an inbound
/// import link lands in the existing detail-modal → picker → import path instead
/// of a parallel flow.
///
/// Read-only: one metadata GET, no download, no install.
pub async fn fetch_project_hit(base: &str, project_ref: &str) -> Result<ModpackHit, Error> {
    let url = format!("{base}/v2/project/{}", urlencode(project_ref));
    let resp = crate::network::request::get(&url, &[("user-agent", UA)], "modpacks")
        .await
        .map_err(|e| Error::mods_network(url.clone(), e))?;
    if resp.status == 404 {
        return Err(Error::ImportUrlInvalid {
            reason: format!("Modrinth has no project '{project_ref}'"),
        });
    }
    if !(200..300).contains(&resp.status) {
        return Err(Error::ModsNetwork {
            url,
            details: format!("HTTP {}", resp.status),
        });
    }
    let p: MrProject = serde_json::from_slice(&resp.body).map_err(|e| Error::ModsDecode {
        platform: "modrinth".into(),
        details: e.to_string(),
    })?;
    if p.project_type != "modpack" {
        return Err(Error::ImportUrlInvalid {
            reason: format!(
                "that Modrinth page is a {} — only modpack links can be imported",
                p.project_type
            ),
        });
    }
    Ok(ModpackHit {
        project_id: p.id,
        slug: p.slug,
        title: p.title,
        description: p.description,
        icon_url: p.icon_url,
        downloads: p.downloads as f64,
        // `game_versions` is ascending, so the tail is the newest MC version —
        // the same value search reports as `latest_version`.
        latest_mc_version: p.game_versions.last().cloned(),
        supported_loaders: loaders_from_tags(&p.loaders),
        source: crate::mods::platform::ModSource::Modrinth,
        distribution_allowed: None,
        // The project route exposes a team id, not a display name; the detail
        // modal shows the author from a search hit only.
        author: None,
    })
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
        .map_err(|e| Error::mods_network(url.clone(), e))?;
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
            let supported_loaders = loaders_from_tags(&h.categories);
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

    #[tokio::test]
    async fn search_returns_normalised_hits() {
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

        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let r = search(&s.uri(), "test", 0, None, None, ModpackSort::Relevance, 20)
            .await
            .unwrap();
        assert_eq!(r.total, 1);
        assert_eq!(r.hits[0].project_id, "PaCk1");
        assert_eq!(r.hits[0].supported_loaders, vec![LoaderKind::Fabric]);
        assert_eq!(r.hits[0].source, ModSource::Modrinth);
        assert!(r.hits[0].distribution_allowed.is_none());
        assert_eq!(r.hits[0].author.as_deref(), Some("PackOwner"));
    }

    /// `/v2/project/<ref>` body, parameterised by project type.
    fn project_body(project_type: &str) -> serde_json::Value {
        serde_json::json!({
            "id": "PaCk1",
            "slug": "cobblemon",
            "title": "Cobblemon",
            "description": "Pokemon-inspired pack",
            "icon_url": "https://cdn.modrinth.com/data/x/icon.png",
            "downloads": 987,
            "project_type": project_type,
            "game_versions": ["1.20.1", "1.21.1"],
            "loaders": ["fabric", "some-unknown-loader"]
        })
    }

    #[tokio::test]
    async fn resolve_maps_a_modpack_project_to_a_hit() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v2/project/cobblemon"))
            .respond_with(ResponseTemplate::new(200).set_body_json(project_body("modpack")))
            .mount(&s)
            .await;

        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let hit = fetch_project_hit(&s.uri(), "cobblemon").await.unwrap();
        assert_eq!(hit.project_id, "PaCk1");
        assert_eq!(hit.slug, "cobblemon");
        assert_eq!(hit.title, "Cobblemon");
        assert_eq!(hit.source, ModSource::Modrinth);
        // Newest MC version is the tail of the ascending list.
        assert_eq!(hit.latest_mc_version.as_deref(), Some("1.21.1"));
        // Unknown loader tags are dropped, not guessed at.
        assert_eq!(hit.supported_loaders, vec![LoaderKind::Fabric]);
    }

    #[tokio::test]
    async fn resolve_rejects_a_project_that_is_not_a_modpack() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v2/project/sodium"))
            .respond_with(ResponseTemplate::new(200).set_body_json(project_body("mod")))
            .mount(&s)
            .await;

        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        match fetch_project_hit(&s.uri(), "sodium").await {
            Err(Error::ImportUrlInvalid { reason }) => {
                assert!(
                    reason.contains("mod"),
                    "reason should name the type: {reason}"
                );
            }
            other => panic!("expected ImportUrlInvalid, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn resolve_reports_an_unknown_project_as_a_link_problem() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v2/project/nope"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&s)
            .await;

        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        assert!(matches!(
            fetch_project_hit(&s.uri(), "nope").await,
            Err(Error::ImportUrlInvalid { .. })
        ));
    }

    #[tokio::test]
    async fn search_with_filters_facets() {
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

        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
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
        assert_eq!(r.total, 0);
    }

    #[tokio::test]
    async fn search_with_sort_includes_index_param() {
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

        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let r = search(&s.uri(), "x", 0, None, None, ModpackSort::Downloads, 20)
            .await
            .unwrap();
        assert_eq!(r.total, 0);
    }
}
