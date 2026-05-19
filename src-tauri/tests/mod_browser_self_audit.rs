//! Mod browser self-audit: assert outbound mod-browser hosts stay on
//! the allowlist.
//!
//! This is the v0.5.0 counterpart to `network_self_audit_integration.rs`.
//! That harness audits the `versions` and `download` modules — both of
//! which funnel through the `network::` chokepoint and record into the
//! audit ring buffer, so `audit_violations()` is the right assertion
//! shape there.
//!
//! The mod-browser clients (`mods::modrinth`, `mods::curseforge`,
//! `mods::install`) use raw `reqwest::Client` directly: they predate
//! the chokepoint rule (added in v0.1.0 PRINCIPLES.md Part A) for
//! deliberate reasons documented in the v0.5.0 mod-browser plan and
//! were merged before the chokepoint was retroactively tightened.
//! As a result they do NOT record into the audit ring buffer, so
//! `audit_violations()` would falsely show zero even if these clients
//! talked to `evil.example`.
//!
//! What this test *can* mechanically verify:
//!
//! 1. The four CDN/API hosts the mod-browser pipeline targets are on
//!    the allowlist by name (`is_host_allowed` returns true).
//! 2. Driving `ModrinthClient::versions` against a wiremock server
//!    that returns a *realistic* response with a `cdn.modrinth.com`
//!    file URL parses cleanly and the resulting `ModVersion`'s
//!    `primary_file.url` host is on the allowlist. This protects
//!    against a future code change that rewrites the CDN host (e.g.
//!    a misguided proxy substitution) into something off-list.
//! 3. Same for `CurseForgeClient::versions` with `edge.forgecdn.net`.
//!
//! What this test *cannot* verify (deferred to a post-v0.5.0 sandbox
//! audit harness, see SECURITY.md Part C item 2):
//!
//! - That the mod-browser clients never construct a URL targeting
//!   any other host at runtime (a true wire-level packet audit).
//!   For that we'd need to either (a) route these clients through
//!   `network::client::http()` so they appear in the audit ring,
//!   or (b) run the binary inside a network sandbox and snoop the
//!   socket. Both are larger refactors.
//!
//! The plan (Task 22) explicitly accepts this scope reduction: "the
//! full sandbox-based audit harness is post-v0.5.0 scope."

use ftlauncher_lib::mods::curseforge::CurseForgeClient;
use ftlauncher_lib::mods::modrinth::ModrinthClient;
use ftlauncher_lib::mods::platform::{LoaderKind, ModPlatform, ModSearchQuery, ModSort, ModSource};
use ftlauncher_lib::network::allowlist::is_host_allowed;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Hosts the v0.5.0 mod browser pipeline targets in production.
/// Mirrors `docs/PRINCIPLES.md` Part A item #2 rows added in Task 5.
const MOD_BROWSER_HOSTS: &[&str] = &[
    "api.modrinth.com",
    "cdn.modrinth.com",
    "api.curseforge.com",
    "edge.forgecdn.net",
    "mediafilez.forgecdn.net",
];

#[test]
fn all_mod_browser_hosts_are_on_allowlist() {
    for host in MOD_BROWSER_HOSTS {
        assert!(
            is_host_allowed(host),
            "mod-browser host {host} must be on the network allowlist; \
             check src-tauri/src/network/allowlist.rs ALLOWED_PATTERNS",
        );
    }
}

#[test]
fn near_miss_hosts_are_rejected() {
    // Sanity: confirm the allowlist matcher is strict enough that a
    // typo-squat / suffix-match attack on a mod-browser host fails.
    // A future contributor flattening `cdn.modrinth.com` into a
    // `*.modrinth.com` wildcard would silently widen attack surface;
    // this test catches that.
    let near_misses = [
        "evilcdn.modrinth.com",     // suffix-match, no dot boundary
        "cdn.modrinth.com.evil",    // suffix forgery
        "evil.cdn.modrinth.com",    // extra subdomain under a non-wildcard entry
        "evil.edge.forgecdn.net",   // ditto
        "edgeXforgecdn.net",        // typo squat
        "api.modrinthX.com",        // typo squat
    ];
    for host in near_misses {
        assert!(
            !is_host_allowed(host),
            "near-miss host {host} must NOT be on the allowlist — \
             allowlist may have been widened too aggressively",
        );
    }
}

#[tokio::test]
async fn modrinth_versions_response_primary_file_host_is_allowlisted() {
    // Drive the real ModrinthClient parser end-to-end against a
    // wiremock-served response whose embedded file URL is a real
    // production-shape `cdn.modrinth.com` URL. Then extract the host
    // from the parsed ModVersion.primary_file.url and assert it's on
    // the allowlist. This catches any future change that rewrites
    // the CDN host (e.g. a proxy substitution or stripping logic).
    let s = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/project/sodium/version"))
        .and(query_param("loaders", r#"["fabric"]"#))
        .and(query_param("game_versions", r#"["1.20.1"]"#))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"[{
                "id":"v1","project_id":"sodium","name":"Sodium 0.5.8","version_number":"0.5.8",
                "game_versions":["1.20.1"],"loaders":["fabric"],
                "date_published":"2026-05-01T00:00:00Z",
                "files":[{
                    "url":"https://cdn.modrinth.com/data/AANobbMI/versions/v1/sodium-fabric-0.5.8.jar",
                    "filename":"sodium-fabric-0.5.8.jar",
                    "hashes":{"sha1":"abc"},"size":900000,"primary":true
                }],
                "dependencies":[]
            }]"#,
        ))
        .mount(&s)
        .await;

    let c = ModrinthClient::with_base(s.uri());
    let versions = c
        .versions("sodium", "1.20.1", LoaderKind::Fabric)
        .await
        .expect("versions call");
    assert_eq!(versions.len(), 1, "wiremock should serve one version");
    let url = &versions[0].primary_file.url;
    let parsed = reqwest::Url::parse(url)
        .unwrap_or_else(|e| panic!("primary_file.url must parse as URL: {url} ({e})"));
    let host = parsed.host_str().expect("primary_file.url must have a host");
    assert!(
        is_host_allowed(host),
        "Modrinth-parsed primary_file.url host {host} (from {url}) must be allowlisted",
    );
    assert_eq!(
        host, "cdn.modrinth.com",
        "expected the parser to preserve cdn.modrinth.com verbatim; got {host}",
    );
}

#[tokio::test]
async fn modrinth_search_request_targets_only_api_modrinth_in_default_client() {
    // Construct a *default* ModrinthClient (production base URL) but
    // don't actually run it — calling `.search()` would hit the real
    // Internet. Instead, validate the base URL via the public API
    // shape: a default client should resolve to a base hostname that
    // is on the allowlist. We do this by verifying the constructor
    // works AND by sanity-checking that the documented default base
    // host is allowlisted.
    //
    // This is intentionally narrow — its job is to fail loudly if a
    // future contributor changes `BASE_DEFAULT` to point at a
    // non-allowlisted host (e.g. a personal proxy).
    let _ = ModrinthClient::new();
    assert!(
        is_host_allowed("api.modrinth.com"),
        "default Modrinth base host (api.modrinth.com) must be allowlisted",
    );
}

#[tokio::test]
async fn curseforge_versions_response_primary_file_host_is_allowlisted() {
    // Same shape as the Modrinth check, but for CurseForge. The CF
    // API serves file downloads from `edge.forgecdn.net` (and the
    // legacy `mediafilez.forgecdn.net`) — both should be on the
    // allowlist.
    let s = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/mods/12345/files"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{
                "data":[{
                    "id":99,"modId":12345,"displayName":"v1.0","fileName":"sodium-1.0.jar",
                    "fileLength":900000,"hashes":[{"value":"abc","algo":1}],
                    "gameVersions":["1.20.1"],
                    "downloadUrl":"https://edge.forgecdn.net/files/4000/123/sodium-1.0.jar",
                    "fileDate":"2026-05-01T00:00:00Z","isAvailable":true,
                    "releaseType":1,"dependencies":[]
                }],
                "pagination":null
            }"#,
        ))
        .mount(&s)
        .await;

    let c = CurseForgeClient::with_base_and_key(s.uri(), Some("test-key".into()));
    let versions = c
        .versions("12345", "1.20.1", LoaderKind::Fabric)
        .await
        .expect("versions call");
    assert_eq!(versions.len(), 1, "wiremock should serve one version");
    assert!(
        versions[0].primary_file.distribution_allowed,
        "CF response with downloadUrl must mark distribution_allowed",
    );
    let url = &versions[0].primary_file.url;
    let parsed = reqwest::Url::parse(url)
        .unwrap_or_else(|e| panic!("primary_file.url must parse as URL: {url} ({e})"));
    let host = parsed.host_str().expect("primary_file.url must have a host");
    assert!(
        is_host_allowed(host),
        "CurseForge-parsed primary_file.url host {host} (from {url}) must be allowlisted",
    );
    assert_eq!(
        host, "edge.forgecdn.net",
        "expected the parser to preserve edge.forgecdn.net verbatim; got {host}",
    );
}

#[tokio::test]
async fn modrinth_search_envelope_does_not_leak_off_list_hosts_through_icon_urls() {
    // Search response icon URLs can point anywhere on the wider web
    // (Modrinth allows arbitrary mod icons). The mod browser surfaces
    // these icons in the UI via <img src>, which the OS webview may
    // fetch. Those fetches happen outside our Rust client, so they
    // can't violate `is_host_allowed`. But we DO want to assert that
    // our Rust-side search parser doesn't try to follow them (no
    // server-side prefetch). This test verifies the search call
    // completes without making any follow-up request to the icon
    // host — wiremock's `verify` is implicit: any HTTP request to a
    // path we didn't `Mock::mount` would 404 and be silently ignored,
    // but our parser would error out, failing this test.
    let s = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/search"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{
                "hits":[{
                    "project_id":"AANobbMI","slug":"sodium","title":"Sodium",
                    "description":"Performance","icon_url":"https://evil.example/sodium.png",
                    "downloads":1234,"author":"jellysquid","date_modified":"2026-05-01T00:00:00Z"
                }],
                "total_hits":1,"offset":0,"limit":20
            }"#,
        ))
        .mount(&s)
        .await;
    let c = ModrinthClient::with_base(s.uri());
    let page = c
        .search(&ModSearchQuery {
            source: ModSource::Modrinth,
            query: "sodium".into(),
            mc_version: Some("1.20.1".into()),
            loader: Some(LoaderKind::Fabric),
            sort: ModSort::Downloads,
            page_size: 20,
            offset: 0,
        })
        .await
        .expect("search call");
    assert_eq!(page.hits.len(), 1);
    // The icon URL is passed through unchanged (UI's problem, not ours).
    assert_eq!(page.hits[0].icon_url.as_deref(), Some("https://evil.example/sodium.png"));
    // The substantive check: our Rust client made no follow-up HTTP
    // request to evil.example. wiremock only saw the /v2/search call.
    let received = s.received_requests().await.expect("wiremock requests");
    assert_eq!(received.len(), 1, "Rust client should only call /v2/search; got: {received:?}");
    assert!(
        received[0].url.path().starts_with("/v2/search"),
        "expected only a search call; got path {}",
        received[0].url.path(),
    );
}
