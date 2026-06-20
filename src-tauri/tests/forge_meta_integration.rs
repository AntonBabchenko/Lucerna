//! End-to-end test for `forge::meta::list_versions`. Boots wiremock
//! as fake maven-metadata + promotions endpoints, exercises the full
//! parse + filter + sort + stable-tag pipeline.

use lucerna_lib::forge::ForgeFlavor;
use std::sync::{Mutex, MutexGuard, OnceLock};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// Serializes tests in this binary: they mutate
// LUCERNA_FORGE_META_OVERRIDE / LUCERNA_FORGE_PROMOTIONS_OVERRIDE
// and the Forge meta cache.
fn test_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

const MAVEN_METADATA: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<metadata>
  <groupId>net.minecraftforge</groupId>
  <artifactId>forge</artifactId>
  <versioning>
    <versions>
      <version>1.20.4-49.0.0</version>
      <version>1.20.4-49.0.30</version>
      <version>1.20.4-49.0.49</version>
      <version>1.12.2-14.23.5.2860</version>
      <version>1.7.10-10.13.4.1614-1.7.10</version>
    </versions>
  </versioning>
</metadata>"#;

const PROMOTIONS: &str = r#"{
  "homepage": "https://files.minecraftforge.net/",
  "promos": {
    "1.20.4-recommended": "49.0.30",
    "1.20.4-latest": "49.0.49",
    "1.12.2-recommended": "14.23.5.2860",
    "1.12.2-latest": "14.23.5.2860",
    "1.7.10-recommended": "10.13.4.1614",
    "1.7.10-latest": "10.13.4.1614"
  }
}"#;

#[tokio::test]
async fn list_versions_returns_sorted_with_recommended_tagged() {
    let _g = test_lock();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/maven-metadata.xml"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(MAVEN_METADATA, "application/xml"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/promotions_slim.json"))
        .respond_with(ResponseTemplate::new(200).set_body_string(PROMOTIONS))
        .mount(&server)
        .await;

    let _seam = lucerna_lib::test_seam::scope(&[
        ("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost"),
        ("LUCERNA_FORGE_META_OVERRIDE", &server.uri()),
        ("LUCERNA_FORGE_PROMOTIONS_OVERRIDE", &server.uri()),
    ]);
    lucerna_lib::forge::meta::clear_cache_for_test();

    let entries = lucerna_lib::forge::meta::list_versions(ForgeFlavor::Forge, "1.20.4")
        .await
        .expect("list_versions");
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].version, "49.0.49"); // latest (sorted desc)
    assert!(!entries[0].stable, "latest is not recommended (49.0.30 is)");
    assert_eq!(entries[1].version, "49.0.30");
    assert!(entries[1].stable, "49.0.30 is recommended");
    assert_eq!(entries[2].version, "49.0.0");
}

#[tokio::test]
async fn list_versions_filters_to_mc() {
    let _g = test_lock();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/maven-metadata.xml"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(MAVEN_METADATA, "application/xml"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/promotions_slim.json"))
        .respond_with(ResponseTemplate::new(200).set_body_string(PROMOTIONS))
        .mount(&server)
        .await;

    let _seam = lucerna_lib::test_seam::scope(&[
        ("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost"),
        ("LUCERNA_FORGE_META_OVERRIDE", &server.uri()),
        ("LUCERNA_FORGE_PROMOTIONS_OVERRIDE", &server.uri()),
    ]);
    lucerna_lib::forge::meta::clear_cache_for_test();

    let entries = lucerna_lib::forge::meta::list_versions(ForgeFlavor::Forge, "1.7.10")
        .await
        .expect("list_versions");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].version, "10.13.4.1614");
    assert!(entries[0].stable);
}

#[tokio::test]
async fn list_versions_promotions_404_falls_back_to_top_non_beta_stable() {
    let _g = test_lock();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/maven-metadata.xml"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(MAVEN_METADATA, "application/xml"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/promotions_slim.json"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let _seam = lucerna_lib::test_seam::scope(&[
        ("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost"),
        ("LUCERNA_FORGE_META_OVERRIDE", &server.uri()),
        ("LUCERNA_FORGE_PROMOTIONS_OVERRIDE", &server.uri()),
    ]);
    lucerna_lib::forge::meta::clear_cache_for_test();

    let entries = lucerna_lib::forge::meta::list_versions(ForgeFlavor::Forge, "1.20.4")
        .await
        .expect("list_versions");
    assert_eq!(entries.len(), 3);
    // Promotions unavailable → the fallback stable-tagging rule applies:
    // the top non-beta entry is tagged stable (same rule NeoForge always
    // uses, since it has no promotions feed). Exactly one entry is stable.
    assert_eq!(entries[0].version, "49.0.49");
    assert!(
        entries[0].stable,
        "top non-beta is the fallback stable pick"
    );
    assert_eq!(entries.iter().filter(|e| e.stable).count(), 1);
}

#[tokio::test]
async fn list_versions_unknown_mc_returns_loader_unavailable() {
    let _g = test_lock();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/maven-metadata.xml"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(MAVEN_METADATA, "application/xml"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/promotions_slim.json"))
        .respond_with(ResponseTemplate::new(200).set_body_string(PROMOTIONS))
        .mount(&server)
        .await;

    let _seam = lucerna_lib::test_seam::scope(&[
        ("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost"),
        ("LUCERNA_FORGE_META_OVERRIDE", &server.uri()),
        ("LUCERNA_FORGE_PROMOTIONS_OVERRIDE", &server.uri()),
    ]);
    lucerna_lib::forge::meta::clear_cache_for_test();

    let err = lucerna_lib::forge::meta::list_versions(ForgeFlavor::Forge, "99.99.99")
        .await
        .unwrap_err();
    match err {
        lucerna_lib::error::Error::LoaderUnavailable { loader, mc_version } => {
            assert_eq!(loader, "forge");
            assert_eq!(mc_version, "99.99.99");
        }
        other => panic!("expected LoaderUnavailable, got {other:?}"),
    }
}
