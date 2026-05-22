//! Integration test for `versions::manifest::list_manifest` against a
//! mock HTTP server. Covers happy path, caching, and the parse+sort
//! pipeline against a realistic fixture.
//!
//! Tests share the global cache via `clear_cache_for_test()` and
//! must run single-threaded.

use ftlauncher_lib::versions::manifest::{clear_cache_for_test, list_manifest};
use ftlauncher_lib::versions::VersionType;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const FIXTURE: &str = r#"{
  "latest": {"release": "1.20.4", "snapshot": "24w08a"},
  "versions": [
    {
      "id": "24w08a",
      "type": "snapshot",
      "url": "https://example.com/24w08a.json",
      "time": "2024-02-21T13:00:00+00:00",
      "releaseTime": "2024-02-21T13:00:00+00:00",
      "sha1": "deadbeef",
      "complianceLevel": 1
    },
    {
      "id": "1.20.4",
      "type": "release",
      "url": "https://example.com/1.20.4.json",
      "time": "2023-12-07T12:00:00+00:00",
      "releaseTime": "2023-12-07T12:00:00+00:00",
      "sha1": "cafebabe",
      "complianceLevel": 1
    },
    {
      "id": "1.0",
      "type": "old_beta",
      "url": "https://example.com/1.0.json",
      "time": "2011-11-18T00:00:00+00:00",
      "releaseTime": "2011-11-18T00:00:00+00:00",
      "sha1": "aaaaaaaa",
      "complianceLevel": 0
    }
  ]
}"#;

#[tokio::test]
async fn list_manifest_fetches_parses_sorts_and_caches() {
    clear_cache_for_test();

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/mc/game/version_manifest_v2.json"))
        .respond_with(ResponseTemplate::new(200).set_body_string(FIXTURE))
        .mount(&server)
        .await;

    // Override the production URL via env var for the duration of this
    // test. Single-threaded execution makes this safe — set/unset
    // happens within the same test body.
    std::env::set_var(
        "FTLAUNCHER_MANIFEST_URL_OVERRIDE",
        format!("{}/mc/game/version_manifest_v2.json", server.uri()),
    );
    std::env::set_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");

    let first = list_manifest().await.expect("first fetch");
    assert_eq!(first.len(), 3);
    assert_eq!(first[0].id, "1.20.4");
    assert_eq!(first[0].version_type, VersionType::Release);
    assert_eq!(first[1].id, "24w08a");
    assert_eq!(first[1].version_type, VersionType::Snapshot);
    assert_eq!(first[2].id, "1.0");
    assert_eq!(first[2].version_type, VersionType::OldBeta);

    // Verify the first call hit the network exactly once.
    assert_eq!(
        server.received_requests().await.unwrap().len(),
        1,
        "first call should hit network once"
    );

    // Second call within TTL must NOT hit the network.
    let second = list_manifest().await.expect("second fetch");
    assert_eq!(second, first);
    assert_eq!(
        server.received_requests().await.unwrap().len(),
        1,
        "cached call should not produce additional network requests"
    );

    std::env::remove_var("FTLAUNCHER_MANIFEST_URL_OVERRIDE");
    std::env::remove_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS");
}
