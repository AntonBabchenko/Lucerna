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
use crate::mods::modpack::schema::{ModpackHit, ModpackSearchPage, ModpackSort, ModpackVersionEntry};
use crate::mods::platform::{LoaderKind, ModSource};

/// CurseForge `gameId` for Minecraft.
const GAME_MINECRAFT: &str = "432";
/// CurseForge `classId` for the Modpacks category.
const CLASS_MODPACKS: &str = "4471";

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
    index: u32,
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
}

#[derive(Debug, Deserialize)]
struct CfLogo {
    url: Option<String>,
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
    let k = key.ok_or(Error::ModsPlatformAuth { kind: ModsAuthKind::Missing })?;
    if k.chars().any(|c| c.is_control()) {
        return Err(Error::ModsPlatformAuth { kind: ModsAuthKind::Invalid });
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
        return Err(Error::ModsPlatformAuth { kind: ModsAuthKind::Invalid });
    }
    if resp.status == 404 {
        return Err(Error::ModsNotFound { platform: "curseforge".into() });
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
/// zero-based page index; the page size is fixed at 20 to match the
/// Modrinth modpack search.
pub async fn search(
    base: &str,
    key: Option<&str>,
    query: &str,
    page: u32,
    mc_version: Option<&str>,
    loader: Option<LoaderKind>,
    sort: ModpackSort,
) -> Result<ModpackSearchPage, Error> {
    let key = require_key(key)?;
    let limit: u32 = 20;
    let offset = page * limit;
    let mut params: Vec<(&str, String)> = vec![
        ("gameId", GAME_MINECRAFT.to_string()),
        ("classId", CLASS_MODPACKS.to_string()),
        ("searchFilter", query.to_string()),
        ("pageSize", limit.to_string()),
        ("index", offset.to_string()),
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
        .map_err(|e| Error::ModsNetwork { url: url.clone(), details: e.to_string() })?;
    check_status(&resp, &url)?;
    let env: ListEnv<CfMod> = serde_json::from_slice(&resp.body)
        .map_err(|e| Error::ModsDecode { platform: "curseforge".into(), details: e.to_string() })?;

    let total = env.pagination.as_ref().map(|p| p.total_count).unwrap_or(env.data.len() as u32);
    let offset = env.pagination.as_ref().map(|p| p.index).unwrap_or(offset);
    let limit = env.pagination.as_ref().map(|p| p.page_size).unwrap_or(limit);
    let hits = env
        .data
        .into_iter()
        .map(|m| ModpackHit {
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
        })
        .collect();
    Ok(ModpackSearchPage { hits, total, offset, limit })
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
        .map_err(|e| Error::ModsNetwork { url: url.clone(), details: e.to_string() })?;
    check_status(&resp, &url)?;
    // `.pagination` is intentionally ignored here — the version drawer
    // shows a fixed window of the most recent files (pageSize=50).
    let env: ListEnv<CfFile> = serde_json::from_slice(&resp.body)
        .map_err(|e| Error::ModsDecode { platform: "curseforge".into(), details: e.to_string() })?;
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
        .map_err(|e| Error::ModsNetwork { url: url.clone(), details: e.to_string() })?;
    if !(200..300).contains(&resp.status) {
        return Ok((None, None));
    }
    let env: Env<CfMod> = serde_json::from_slice(&resp.body)
        .map_err(|e| Error::ModsDecode { platform: "curseforge".into(), details: e.to_string() })?;
    Ok((Some(env.data.name), Some(env.data.summary)))
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
        .map_err(|e| Error::ModsNetwork { url: url.clone(), details: e.to_string() })?;
    check_status(&resp, &url)?;
    let env: Env<CfFile> = serde_json::from_slice(&resp.body)
        .map_err(|e| Error::ModsDecode { platform: "curseforge".into(), details: e.to_string() })?;
    env.data.download_url.ok_or(Error::ModpackCfDistributionDisabled {
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
        std::env::set_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let r = search(&s.uri(), Some("k"), "rl", 0, None, None, ModpackSort::Relevance)
            .await
            .unwrap();
        std::env::remove_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS");
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
        std::env::set_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let r = search(&s.uri(), Some("k"), "x", 0, None, None, ModpackSort::Relevance)
            .await
            .unwrap();
        std::env::remove_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS");
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
        std::env::set_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        search(&s.uri(), Some("k"), "x", 0, None, None, ModpackSort::Downloads)
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
        search(&s2.uri(), Some("k"), "x", 0, None, None, ModpackSort::Newest)
            .await
            .unwrap();
        std::env::remove_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS");
    }

    #[tokio::test]
    async fn search_missing_key_is_auth_missing_with_no_request() {
        // No MockServer mounted: if search hit the network the call
        // would still resolve (wiremock 404s unmatched paths), so the
        // assertion is on the error kind — a missing key must error
        // before the request is built.
        let err = search("http://127.0.0.1:1", None, "x", 0, None, None, ModpackSort::Relevance)
            .await
            .unwrap_err();
        assert!(matches!(err, Error::ModsPlatformAuth { kind: ModsAuthKind::Missing }));
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
        std::env::set_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let v = list_files(&s.uri(), Some("k"), "1234").await.unwrap();
        std::env::remove_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS");
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].id, "22"); // newest first
        assert_eq!(v[0].name, "RLCraft 2.9.3");
        assert_eq!(v[0].version_number, "rl-2.9.3.zip");
        assert_eq!(v[0].game_versions, vec!["1.12.2"]);
        assert!(v[0].loaders.is_empty());
    }

    #[tokio::test]
    async fn list_files_missing_key_is_auth_missing() {
        let err = list_files("http://127.0.0.1:1", None, "1234").await.unwrap_err();
        assert!(matches!(err, Error::ModsPlatformAuth { kind: ModsAuthKind::Missing }));
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
        std::env::set_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let url = resolve_file_download(&s.uri(), Some("k"), "1234", "22").await.unwrap();
        std::env::remove_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS");
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
        std::env::set_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let (name, summary) = fetch_summary(&s.uri(), Some("k"), "1234").await.unwrap();
        std::env::remove_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS");
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
        std::env::set_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let (name, summary) = fetch_summary(&s.uri(), Some("k"), "1234").await.unwrap();
        std::env::remove_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS");
        assert!(name.is_none() && summary.is_none());
    }

    #[tokio::test]
    async fn fetch_summary_no_key_is_none() {
        let (name, summary) = fetch_summary("http://127.0.0.1:1", None, "1234").await.unwrap();
        assert!(name.is_none() && summary.is_none());
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
        std::env::set_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let err = resolve_file_download(&s.uri(), Some("k"), "1234", "22").await.unwrap_err();
        std::env::remove_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS");
        match err {
            Error::ModpackCfDistributionDisabled { pack_name } => {
                assert_eq!(pack_name, "Locked Pack");
            }
            other => panic!("expected ModpackCfDistributionDisabled, got {other:?}"),
        }
    }
}
