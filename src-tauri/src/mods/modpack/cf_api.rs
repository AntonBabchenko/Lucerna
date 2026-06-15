//! CurseForge Eternal API (v1) client for modpack *discovery* —
//! search, version (file) lists, single-file download resolution, and
//! a project-summary lookup. All HTTP routes through the `network::`
//! chokepoint with the `x-api-key` header (CLAUDE.md forbids `reqwest`
//! outside `network::`).
//!
//! Local per-file serde structs (rather than sharing
//! `mods/curseforge/types`) match the modpack subsystem's pattern —
//! `search.rs` and `curseforge.rs` both define their own.

use serde::Deserialize;

use crate::error::{Error, ModsAuthKind};
use crate::mods::modpack::schema::{
    ModpackHit, ModpackSearchPage, ModpackSort, ModpackVersionEntry,
};
use crate::mods::platform::{LoaderKind, ModSource};

/// CurseForge `gameId` for Minecraft.
const GAME_MINECRAFT: &str = "432";
/// CurseForge `classId` for the Modpacks category.
const CLASS_MODPACKS: &str = "4471";
/// CurseForge's `/v1/mods/search` caps `pageSize` at 50 (HTTP 400 above it).
/// Mirrors the constant in `mods/curseforge` — an external-API limit, kept
/// local rather than coupling the two CF clients for one value. See `search()`.
const CF_MAX_PAGE_SIZE: u32 = 50;

// ---- response shapes -------------------------------------------------

#[derive(Debug, Deserialize)]
struct ListEnv<T> {
    data: Vec<T>,
    pagination: Option<Pag>,
}

/// Single-item CurseForge envelope (`/v1/mods/{id}` and the single-file
/// endpoint).
#[derive(Debug, Deserialize)]
struct Env<T> {
    data: T,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pag {
    // `index`/`page_size` echo the request; search() drives pagination from the
    // requested offset/page_size, so only `total_count` is read back.
    #[allow(dead_code)]
    index: u32,
    #[allow(dead_code)]
    page_size: u32,
    total_count: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfMod {
    id: u32,
    slug: String,
    name: String,
    summary: String,
    download_count: u64,
    logo: Option<CfLogo>,
    /// CurseForge project-level distribution flag — nullable in the API.
    allow_mod_distribution: Option<bool>,
    #[serde(default)]
    screenshots: Vec<CfScreenshot>,
    #[serde(default)]
    links: Option<CfLinks>,
}

#[derive(Debug, Deserialize)]
struct CfLogo {
    url: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct CfScreenshot {
    url: String,
    #[serde(default)]
    title: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfLinks {
    #[serde(default)]
    website_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfFile {
    id: u32,
    display_name: String,
    file_name: String,
    #[serde(default)]
    game_versions: Vec<String>,
    download_url: Option<String>,
    file_date: Option<String>,
}

// ---- helpers ---------------------------------------------------------

/// Minimal percent-encoder for query-string values — same approach as
/// `search.rs` / the CF mod client (avoids `serde_urlencoded` features
/// disabled by our `reqwest` flags).
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push('%');
            out.push_str(&format!("{:02X}", b));
        }
    }
    out
}

fn encode_pairs(pairs: &[(&str, String)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", urlencode(k), urlencode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// CurseForge `modLoaderType` integer. 0=Any 1=Forge 4=Fabric 5=Quilt
/// 6=NeoForge. Kept local so `cf_api` does not depend on the private
/// `mods/curseforge/types` module.
fn loader_type(loader: LoaderKind) -> u32 {
    match loader {
        LoaderKind::Forge => 1,
        LoaderKind::Fabric => 4,
        LoaderKind::Quilt => 5,
        LoaderKind::NeoForge => 6,
        LoaderKind::Vanilla => 0,
    }
}

/// Require an API key — a missing key short-circuits to a typed auth
/// error before any network call. A key containing control characters
/// is a paste error (a real key is an opaque printable token) and is
/// rejected as `Invalid`, mirroring `CurseForgeClient::auth`.
fn require_key(key: Option<&str>) -> Result<&str, Error> {
    let k = key.ok_or(Error::ModsPlatformAuth {
        kind: ModsAuthKind::Missing,
    })?;
    if k.chars().any(|c| c.is_control()) {
        return Err(Error::ModsPlatformAuth {
            kind: ModsAuthKind::Invalid,
        });
    }
    Ok(k)
}

/// Map a non-2xx CurseForge response to a typed error. 401/403 clears
/// the stored key (it is invalid) so the UI re-prompts. Takes the whole
/// `HttpResponse` by reference so the exact integer type of `.status`
/// (defined by the `network::request` wrapper) never has to be spelled
/// here.
fn check_status(resp: &crate::network::request::HttpResponse, url: &str) -> Result<(), Error> {
    if resp.status == 401 || resp.status == 403 {
        crate::mods::curseforge::keyring::clear().ok();
        return Err(Error::ModsPlatformAuth {
            kind: ModsAuthKind::Invalid,
        });
    }
    if resp.status == 404 {
        return Err(Error::ModsNotFound {
            platform: "curseforge".into(),
        });
    }
    if !(200..300).contains(&resp.status) {
        return Err(Error::ModsNetwork {
            url: url.to_string(),
            details: format!("HTTP {}", resp.status),
        });
    }
    Ok(())
}

// ---- search ----------------------------------------------------------

/// Search the CurseForge modpack catalogue (`classId` 4471). `page` is a
/// zero-based page index; `page_size` controls how many results per page.
pub async fn search(
    base: &str,
    key: Option<&str>,
    query: &str,
    page: u32,
    mc_version: Option<&str>,
    loader: Option<LoaderKind>,
    sort: ModpackSort,
    page_size: u32,
) -> Result<ModpackSearchPage, Error> {
    let key = require_key(key)?;
    let want = page_size.max(1);
    let base_offset = page * want;

    // CurseForge caps pageSize at CF_MAX_PAGE_SIZE; a request for more (the
    // shared browser offers up to 100, which Modrinth allows) is served by
    // stitching back-to-back windows that tile [base_offset, base_offset +
    // page_size). page_size <= the cap stays a single request. Windows stop
    // early once one underfills (end of results).
    //
    // CF also enforces index + pageSize <= 10_000; a deep-page request then
    // returns HTTP 400, surfacing as a ModsNetwork error — acceptable, as CF's
    // own catalogue stops paging there too.
    let mut hits: Vec<ModpackHit> = Vec::with_capacity(want as usize);
    let mut total = 0u32;
    let mut fetched = 0u32;
    while fetched < want {
        let chunk = (want - fetched).min(CF_MAX_PAGE_SIZE);
        let index = base_offset + fetched;
        let mut params: Vec<(&str, String)> = vec![
            ("gameId", GAME_MINECRAFT.to_string()),
            ("classId", CLASS_MODPACKS.to_string()),
            ("searchFilter", query.to_string()),
            ("pageSize", chunk.to_string()),
            ("index", index.to_string()),
        ];
        if let Some(mc) = mc_version {
            params.push(("gameVersion", mc.to_string()));
        }
        if let Some(l) = loader {
            if l != LoaderKind::Vanilla {
                params.push(("modLoaderType", loader_type(l).to_string()));
            }
        }
        // ModpackSort -> CurseForge sortField. Relevance = no sortField
        // (CurseForge default relevance order). Codes per the CF API docs:
        // 3 = LastUpdated, 6 = TotalDownloads, 11 = ReleaseDate.
        match sort {
            ModpackSort::Relevance => {}
            ModpackSort::Downloads => {
                params.push(("sortField", "6".into()));
                params.push(("sortOrder", "desc".into()));
            }
            ModpackSort::Updated => {
                params.push(("sortField", "3".into()));
                params.push(("sortOrder", "desc".into()));
            }
            ModpackSort::Newest => {
                params.push(("sortField", "11".into()));
                params.push(("sortOrder", "desc".into()));
            }
        }
        let url = format!("{base}/v1/mods/search?{}", encode_pairs(&params));
        let resp = crate::network::request::get(&url, &[("x-api-key", key)], "modpacks")
            .await
            .map_err(|e| Error::ModsNetwork {
                url: url.clone(),
                details: e.to_string(),
            })?;
        check_status(&resp, &url)?;
        let env: ListEnv<CfMod> =
            serde_json::from_slice(&resp.body).map_err(|e| Error::ModsDecode {
                platform: "curseforge".into(),
                details: e.to_string(),
            })?;
        let got = env.data.len() as u32;
        total = env
            .pagination
            .as_ref()
            .map(|p| p.total_count)
            .unwrap_or(fetched + got);
        hits.extend(env.data.into_iter().map(|m| ModpackHit {
            project_id: m.id.to_string(),
            slug: m.slug,
            title: m.name,
            description: m.summary,
            icon_url: m.logo.and_then(|l| l.url),
            downloads: m.download_count as f64,
            // The modpack card does not render these two; CurseForge
            // search hits leave them empty/None.
            latest_mc_version: None,
            supported_loaders: vec![],
            source: ModSource::Curseforge,
            distribution_allowed: m.allow_mod_distribution,
            // CurseForge modpack browse is not wired into the UI (CF packs
            // are import-by-file only), so the author is left unset here
            // rather than adding CF-only deserialization.
            author: None,
        }));
        fetched += got;
        // A short window means the catalogue is exhausted — stop rather than
        // firing a guaranteed-empty follow-up request.
        if got < chunk {
            break;
        }
    }
    Ok(ModpackSearchPage {
        hits,
        total,
        offset: base_offset,
        limit: page_size,
    })
}

// ---- version (file) list --------------------------------------------

/// List a CurseForge modpack project's files, newest-first, as
/// `ModpackVersionEntry` rows for the version drawer. CurseForge files
/// carry no loader tag, so `loaders` is left empty (the Modrinth-shaped
/// `ModpackVersionEntry` tolerates this — the CF mod client does the
/// same for mod files).
pub async fn list_files(
    base: &str,
    key: Option<&str>,
    project_id: &str,
) -> Result<Vec<ModpackVersionEntry>, Error> {
    let key = require_key(key)?;
    // pageSize=50: the version drawer shows the most recent files; 50 is
    // ample for a "pick a version to install" list even for large packs.
    let url = format!("{base}/v1/mods/{project_id}/files?pageSize=50");
    let resp = crate::network::request::get(&url, &[("x-api-key", key)], "modpacks")
        .await
        .map_err(|e| Error::ModsNetwork {
            url: url.clone(),
            details: e.to_string(),
        })?;
    check_status(&resp, &url)?;
    // `.pagination` is intentionally ignored here — the version drawer
    // shows a fixed window of the most recent files (pageSize=50).
    let env: ListEnv<CfFile> =
        serde_json::from_slice(&resp.body).map_err(|e| Error::ModsDecode {
            platform: "curseforge".into(),
            details: e.to_string(),
        })?;
    let mut entries: Vec<ModpackVersionEntry> = env
        .data
        .into_iter()
        .map(|f| ModpackVersionEntry {
            id: f.id.to_string(),
            name: f.display_name,
            version_number: f.file_name,
            game_versions: f.game_versions,
            loaders: vec![],
            date_published: f.file_date.unwrap_or_default(),
        })
        .collect();
    // Newest-first by publish date (lexical sort is correct for the
    // RFC 3339 timestamps CurseForge returns).
    entries.sort_by(|a, b| b.date_published.cmp(&a.date_published));
    Ok(entries)
}

// ---- project summary -------------------------------------------------

/// Best-effort fetch of a CurseForge modpack's project `name` and short
/// `summary`, used for instance naming and the Imported drawer. Returns
/// `(name, summary)`. Mirrors the Modrinth project fetch in `import.rs`:
///   - missing key or non-2xx response → `Ok((None, None))`;
///   - network or decode failure → `Err(...)` (caller uses `.unwrap_or((None, None))`).
pub async fn fetch_summary(
    base: &str,
    key: Option<&str>,
    project_id: &str,
) -> Result<(Option<String>, Option<String>), Error> {
    let Some(key) = key else {
        return Ok((None, None));
    };
    let url = format!("{base}/v1/mods/{project_id}");
    let resp = crate::network::request::get(&url, &[("x-api-key", key)], "modpacks")
        .await
        .map_err(|e| Error::ModsNetwork {
            url: url.clone(),
            details: e.to_string(),
        })?;
    if !(200..300).contains(&resp.status) {
        return Ok((None, None));
    }
    let env: Env<CfMod> = serde_json::from_slice(&resp.body).map_err(|e| Error::ModsDecode {
        platform: "curseforge".into(),
        details: e.to_string(),
    })?;
    Ok((Some(env.data.name), Some(env.data.summary)))
}

/// Fetch a CurseForge modpack's gallery + long description for the detail
/// modal's Overview tab. The description is a second hop to the dedicated
/// HTML endpoint; a failure there degrades to an empty body.
pub async fn fetch_project_detail(
    base: &str,
    key: Option<&str>,
    project_id: &str,
) -> Result<crate::mods::modpack::schema::ModpackProject, Error> {
    let key = require_key(key)?;
    let url = format!("{base}/v1/mods/{project_id}");
    let resp = crate::network::request::get(&url, &[("x-api-key", key)], "modpacks")
        .await
        .map_err(|e| Error::ModsNetwork {
            url: url.clone(),
            details: e.to_string(),
        })?;
    check_status(&resp, &url)?;
    let env: Env<CfMod> = serde_json::from_slice(&resp.body).map_err(|e| Error::ModsDecode {
        platform: "curseforge".into(),
        details: e.to_string(),
    })?;
    let gallery = env
        .data
        .screenshots
        .into_iter()
        .filter(|s| crate::mods::render::is_safe_image_url(&s.url))
        .map(|s| crate::mods::platform::GalleryImage {
            url: s.url,
            title: s.title,
        })
        .collect();
    let website_url = env.data.links.and_then(|l| l.website_url);

    let desc_url = format!("{base}/v1/mods/{project_id}/description");
    let body_html =
        match crate::network::request::get(&desc_url, &[("x-api-key", key)], "modpacks").await {
            Ok(r) if (200..300).contains(&r.status) => {
                match serde_json::from_slice::<Env<String>>(&r.body) {
                    Ok(e) => crate::mods::render::sanitize_html(&e.data),
                    Err(_) => String::new(),
                }
            }
            _ => String::new(),
        };

    Ok(crate::mods::modpack::schema::ModpackProject {
        body_html,
        gallery,
        website_url,
    })
}

// ---- bulk file resolution (shared with FTB cf-ref pass) -------------

/// Result of resolving a single CurseForge file id via the bulk-files API.
#[derive(Debug)]
pub(crate) struct CfResolvedFile {
    /// `None` when the author disabled third-party distribution.
    pub download_url: Option<String>,
    /// SHA-1 hash (lowercased), when the CF response carries one (algo=1).
    pub sha1: Option<String>,
}

/// POST `/v1/mods/files` for the given `file_ids` and return a map from
/// file id → `CfResolvedFile`. Files missing from the response are absent
/// from the returned map; callers treat absence the same as
/// `download_url: None`.
///
/// `key` follows the same optional-but-validated pattern as the rest of this
/// module: `None` short-circuits to `ModsPlatformAuth::Missing`.
pub(crate) async fn resolve_files(
    base: &str,
    key: Option<&str>,
    file_ids: &[u64],
) -> Result<std::collections::HashMap<u64, CfResolvedFile>, Error> {
    if file_ids.is_empty() {
        return Ok(Default::default());
    }
    let key = require_key(key)?;
    let url = format!("{base}/v1/mods/files");
    let body = serde_json::json!({ "fileIds": file_ids });
    let body_bytes = serde_json::to_vec(&body)
        .expect("infallible: a serde_json::Value constructed via json!() always serializes");
    let resp = crate::network::request::post(
        &url,
        &[("x-api-key", key), ("content-type", "application/json")],
        &body_bytes,
        "modpacks",
    )
    .await
    .map_err(|e| Error::ModsNetwork {
        url: url.clone(),
        details: e.to_string(),
    })?;
    check_status(&resp, &url)?;

    #[derive(Deserialize)]
    struct BulkResp {
        data: Vec<BulkFile>,
    }
    #[derive(Deserialize)]
    struct BulkFile {
        id: u64,
        #[serde(rename = "downloadUrl")]
        download_url: Option<String>,
        #[serde(default)]
        hashes: Vec<BulkHash>,
    }
    #[derive(Deserialize)]
    struct BulkHash {
        value: String,
        algo: u32, // 1 = SHA-1, 2 = MD5
    }

    let bulk: BulkResp = serde_json::from_slice(&resp.body).map_err(|e| Error::ModsDecode {
        platform: "curseforge".into(),
        details: e.to_string(),
    })?;

    let map = bulk
        .data
        .into_iter()
        .map(|f| {
            let sha1 = f
                .hashes
                .iter()
                .find(|h| h.algo == 1)
                .map(|h| h.value.to_ascii_lowercase());
            (
                f.id,
                CfResolvedFile {
                    download_url: f.download_url,
                    sha1,
                },
            )
        })
        .collect();
    Ok(map)
}

// ---- single-file download resolution --------------------------------

/// Resolve a CurseForge modpack file to its `.zip` download URL. A
/// `null` `downloadUrl` means the author opted out of third-party
/// distribution — surfaced as `ModpackCfDistributionDisabled` so the
/// UI can show the "Open on CurseForge" fallback.
pub async fn resolve_file_download(
    base: &str,
    key: Option<&str>,
    project_id: &str,
    file_id: &str,
) -> Result<String, Error> {
    let key = require_key(key)?;
    let url = format!("{base}/v1/mods/{project_id}/files/{file_id}");
    let resp = crate::network::request::get(&url, &[("x-api-key", key)], "modpacks")
        .await
        .map_err(|e| Error::ModsNetwork {
            url: url.clone(),
            details: e.to_string(),
        })?;
    check_status(&resp, &url)?;
    let env: Env<CfFile> = serde_json::from_slice(&resp.body).map_err(|e| Error::ModsDecode {
        platform: "curseforge".into(),
        details: e.to_string(),
    })?;
    env.data
        .download_url
        .ok_or(Error::ModpackCfDistributionDisabled {
            pack_name: env.data.display_name,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::test_env_lock()
    }

    fn search_body() -> serde_json::Value {
        serde_json::json!({
            "data": [{
                "id": 1234,
                "slug": "rlcraft",
                "name": "RLCraft",
                "summary": "A hard pack",
                "downloadCount": 9000000,
                "logo": { "url": "https://media.forgecdn.net/x.png" },
                "allowModDistribution": false
            }, {
                "id": 5678,
                "slug": "atm9",
                "name": "All the Mods 9",
                "summary": "Kitchen sink",
                "downloadCount": 5000000,
                "logo": null
            }],
            "pagination": { "index": 0, "pageSize": 20, "resultCount": 2, "totalCount": 2 }
        })
    }

    /// Build `count` minimal CF modpack JSON entries, ids `start..start+count`.
    fn pack_entries(start: u32, count: u32) -> Vec<serde_json::Value> {
        (start..start + count)
            .map(|i| {
                serde_json::json!({
                    "id": i, "slug": format!("p{i}"), "name": format!("Pack {i}"),
                    "summary": "s", "downloadCount": 1, "logo": null
                })
            })
            .collect()
    }

    #[tokio::test]
    async fn search_chunks_page_size_above_cf_max() {
        // CurseForge rejects pageSize > 50 with HTTP 400. A request for 100
        // packs/page must fan out into two ≤50 windows (index 0 + 50) and
        // stitch them — never sending pageSize=100. The mocks only match
        // pageSize=50, so a single pageSize=100 request would 404 and fail.
        let _g = test_lock();
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/search"))
            .and(query_param("pageSize", "50"))
            .and(query_param("index", "0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": pack_entries(0, 50),
                "pagination": {"index": 0, "pageSize": 50, "resultCount": 50, "totalCount": 120}
            })))
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/search"))
            .and(query_param("pageSize", "50"))
            .and(query_param("index", "50"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": pack_entries(50, 50),
                "pagination": {"index": 50, "pageSize": 50, "resultCount": 50, "totalCount": 120}
            })))
            .mount(&s)
            .await;
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let r = search(
            &s.uri(),
            Some("k"),
            "",
            0,
            None,
            None,
            ModpackSort::Downloads,
            100,
        )
        .await
        .unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert_eq!(r.hits.len(), 100, "two 50-windows should stitch to 100");
        assert_eq!(r.total, 120);
        assert_eq!(r.limit, 100, "returns the requested page size");
        assert_eq!(r.offset, 0);
        assert_eq!(r.hits[0].project_id, "0");
        assert_eq!(r.hits[99].project_id, "99");
    }

    #[tokio::test]
    async fn search_stops_early_when_window_underfills() {
        // Only 30 packs exist though 100 were requested: the first window
        // returns < pageSize, so no second request is made.
        let _g = test_lock();
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/search"))
            .and(query_param("index", "0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": pack_entries(0, 30),
                "pagination": {"index": 0, "pageSize": 50, "resultCount": 30, "totalCount": 30}
            })))
            .mount(&s)
            .await;
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let r = search(
            &s.uri(),
            Some("k"),
            "",
            0,
            None,
            None,
            ModpackSort::Relevance,
            100,
        )
        .await
        .unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert_eq!(r.hits.len(), 30);
        assert_eq!(r.total, 30);
    }

    #[tokio::test]
    async fn search_sends_game_and_class_ids() {
        let _g = test_lock();
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/search"))
            .and(query_param("gameId", "432"))
            .and(query_param("classId", "4471"))
            .respond_with(ResponseTemplate::new(200).set_body_json(search_body()))
            .mount(&s)
            .await;
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let r = search(
            &s.uri(),
            Some("k"),
            "rl",
            0,
            None,
            None,
            ModpackSort::Relevance,
            20,
        )
        .await
        .unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert_eq!(r.total, 2);
        assert_eq!(r.hits[0].title, "RLCraft");
        assert_eq!(r.hits[0].project_id, "1234");
        assert_eq!(r.hits[0].source, ModSource::Curseforge);
    }

    #[tokio::test]
    async fn search_maps_allow_mod_distribution() {
        let _g = test_lock();
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(search_body()))
            .mount(&s)
            .await;
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let r = search(
            &s.uri(),
            Some("k"),
            "x",
            0,
            None,
            None,
            ModpackSort::Relevance,
            20,
        )
        .await
        .unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        // First hit has allowModDistribution:false; second omits it.
        assert_eq!(r.hits[0].distribution_allowed, Some(false));
        assert_eq!(r.hits[1].distribution_allowed, None);
    }

    #[tokio::test]
    async fn search_maps_sort_to_curseforge_sort_field() {
        let _g = test_lock();
        // Downloads -> sortField=6.
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/search"))
            .and(query_param("sortField", "6"))
            .and(query_param("sortOrder", "desc"))
            .respond_with(ResponseTemplate::new(200).set_body_json(search_body()))
            .mount(&s)
            .await;
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        search(
            &s.uri(),
            Some("k"),
            "x",
            0,
            None,
            None,
            ModpackSort::Downloads,
            20,
        )
        .await
        .unwrap();

        // Newest -> sortField=11.
        let s2 = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/search"))
            .and(query_param("sortField", "11"))
            .respond_with(ResponseTemplate::new(200).set_body_json(search_body()))
            .mount(&s2)
            .await;
        search(
            &s2.uri(),
            Some("k"),
            "x",
            0,
            None,
            None,
            ModpackSort::Newest,
            20,
        )
        .await
        .unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
    }

    #[tokio::test]
    async fn search_missing_key_is_auth_missing_with_no_request() {
        // No MockServer mounted: if search hit the network the call
        // would still resolve (wiremock 404s unmatched paths), so the
        // assertion is on the error kind — a missing key must error
        // before the request is built.
        let err = search(
            "http://127.0.0.1:1",
            None,
            "x",
            0,
            None,
            None,
            ModpackSort::Relevance,
            20,
        )
        .await
        .unwrap_err();
        assert!(matches!(
            err,
            Error::ModsPlatformAuth {
                kind: ModsAuthKind::Missing
            }
        ));
    }

    #[tokio::test]
    async fn list_files_maps_and_sorts_newest_first() {
        let _g = test_lock();
        let s = MockServer::start().await;
        let body = serde_json::json!({
            "data": [
                { "id": 11, "displayName": "RLCraft 2.9.2", "fileName": "rl-2.9.2.zip",
                  "gameVersions": ["1.12.2"], "downloadUrl": "https://edge.forgecdn.net/a.zip",
                  "fileDate": "2026-01-01T00:00:00Z" },
                { "id": 22, "displayName": "RLCraft 2.9.3", "fileName": "rl-2.9.3.zip",
                  "gameVersions": ["1.12.2"], "downloadUrl": "https://edge.forgecdn.net/b.zip",
                  "fileDate": "2026-03-01T00:00:00Z" }
            ],
            "pagination": null
        });
        Mock::given(method("GET"))
            .and(path("/v1/mods/1234/files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&s)
            .await;
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let v = list_files(&s.uri(), Some("k"), "1234").await.unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].id, "22"); // newest first
        assert_eq!(v[0].name, "RLCraft 2.9.3");
        assert_eq!(v[0].version_number, "rl-2.9.3.zip");
        assert_eq!(v[0].game_versions, vec!["1.12.2"]);
        assert!(v[0].loaders.is_empty());
    }

    #[tokio::test]
    async fn list_files_missing_key_is_auth_missing() {
        let err = list_files("http://127.0.0.1:1", None, "1234")
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            Error::ModsPlatformAuth {
                kind: ModsAuthKind::Missing
            }
        ));
    }

    #[tokio::test]
    async fn resolve_file_download_returns_url() {
        let _g = test_lock();
        let s = MockServer::start().await;
        let body = serde_json::json!({
            "data": { "id": 22, "displayName": "RLCraft 2.9.3", "fileName": "rl.zip",
                      "gameVersions": ["1.12.2"],
                      "downloadUrl": "https://edge.forgecdn.net/files/22/rl.zip",
                      "fileDate": "2026-03-01T00:00:00Z" }
        });
        Mock::given(method("GET"))
            .and(path("/v1/mods/1234/files/22"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&s)
            .await;
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let url = resolve_file_download(&s.uri(), Some("k"), "1234", "22")
            .await
            .unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert_eq!(url, "https://edge.forgecdn.net/files/22/rl.zip");
    }

    #[tokio::test]
    async fn fetch_summary_returns_name_and_summary() {
        let _g = test_lock();
        let s = MockServer::start().await;
        let body = serde_json::json!({
            "data": { "id": 1234, "slug": "rlcraft", "name": "RLCraft",
                      "summary": "A hard pack", "downloadCount": 9000000, "logo": null }
        });
        Mock::given(method("GET"))
            .and(path("/v1/mods/1234"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&s)
            .await;
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let (name, summary) = fetch_summary(&s.uri(), Some("k"), "1234").await.unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert_eq!(name.as_deref(), Some("RLCraft"));
        assert_eq!(summary.as_deref(), Some("A hard pack"));
    }

    #[tokio::test]
    async fn fetch_summary_non_2xx_is_none() {
        let _g = test_lock();
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/1234"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&s)
            .await;
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let (name, summary) = fetch_summary(&s.uri(), Some("k"), "1234").await.unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert!(name.is_none() && summary.is_none());
    }

    #[tokio::test]
    async fn fetch_summary_no_key_is_none() {
        let (name, summary) = fetch_summary("http://127.0.0.1:1", None, "1234")
            .await
            .unwrap();
        assert!(name.is_none() && summary.is_none());
    }

    #[tokio::test]
    async fn fetch_project_detail_maps_screenshots_and_description() {
        let _g = test_lock();
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/1234"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": { "id": 1234, "slug": "rlcraft", "name": "RLCraft",
                          "summary": "hard", "downloadCount": 1, "logo": null,
                          "screenshots": [{"url":"https://media.forgecdn.net/p.png","title":"P"}],
                          "links": {"websiteUrl":"https://rlcraft.example"} }
            })))
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/1234/description"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": "<p>Pack</p><script>x()</script>"
            })))
            .mount(&s)
            .await;
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let d = fetch_project_detail(&s.uri(), Some("k"), "1234")
            .await
            .unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert_eq!(d.gallery.len(), 1);
        assert_eq!(d.gallery[0].url, "https://media.forgecdn.net/p.png");
        assert!(d.body_html.contains("<p>Pack</p>"));
        assert!(!d.body_html.contains("<script"));
        assert_eq!(d.website_url.as_deref(), Some("https://rlcraft.example"));
    }

    #[tokio::test]
    async fn resolve_files_maps_download_url_and_sha1() {
        let _g = test_lock();
        let s = MockServer::start().await;
        let body = serde_json::json!({
            "data": [
                {
                    "id": 111u64, "downloadUrl": "https://edge.forgecdn.net/files/1/1/mod-a.jar",
                    "hashes": [{ "value": "aabbccdd", "algo": 1 }]
                },
                {
                    "id": 222u64, "downloadUrl": null,
                    "hashes": [{ "value": "deadbeef", "algo": 1 }]
                }
            ]
        });
        Mock::given(method("POST"))
            .and(path("/v1/mods/files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&s)
            .await;
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let map = resolve_files(&s.uri(), Some("k"), &[111, 222])
            .await
            .unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        // File 111: has a downloadUrl and a sha1.
        let f111 = map.get(&111).expect("file 111 must be present");
        assert_eq!(
            f111.download_url.as_deref(),
            Some("https://edge.forgecdn.net/files/1/1/mod-a.jar")
        );
        assert_eq!(f111.sha1.as_deref(), Some("aabbccdd"));
        // File 222: distribution disabled (null downloadUrl); sha1 still present.
        let f222 = map.get(&222).expect("file 222 must be present");
        assert!(
            f222.download_url.is_none(),
            "null downloadUrl must map to None"
        );
        assert_eq!(f222.sha1.as_deref(), Some("deadbeef"));
    }

    #[tokio::test]
    async fn resolve_files_empty_ids_returns_empty_map() {
        // No network call should be made for an empty id list.
        let map = resolve_files("http://127.0.0.1:1", Some("k"), &[])
            .await
            .unwrap();
        assert!(map.is_empty());
    }

    #[tokio::test]
    async fn resolve_files_missing_key_is_auth_missing() {
        let err = resolve_files("http://127.0.0.1:1", None, &[1])
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            Error::ModsPlatformAuth {
                kind: ModsAuthKind::Missing
            }
        ));
    }

    #[tokio::test]
    async fn resolve_file_download_null_url_is_distribution_disabled() {
        let _g = test_lock();
        let s = MockServer::start().await;
        let body = serde_json::json!({
            "data": { "id": 22, "displayName": "Locked Pack", "fileName": "lp.zip",
                      "gameVersions": ["1.20.1"], "downloadUrl": null,
                      "fileDate": "2026-03-01T00:00:00Z" }
        });
        Mock::given(method("GET"))
            .and(path("/v1/mods/1234/files/22"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&s)
            .await;
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let err = resolve_file_download(&s.uri(), Some("k"), "1234", "22")
            .await
            .unwrap_err();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        match err {
            Error::ModpackCfDistributionDisabled { pack_name } => {
                assert_eq!(pack_name, "Locked Pack");
            }
            other => panic!("expected ModpackCfDistributionDisabled, got {other:?}"),
        }
    }
}
