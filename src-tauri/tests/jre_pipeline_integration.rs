//! Integration tests for the JRE pipeline's testable pieces.
//!
//! `ensure_jre` needs a real `tauri::AppHandle`, which integration
//! tests cannot construct. So we test the lower-layer pieces: the
//! top-level manifest fetch through wiremock (proving the env-var
//! override + cache flow works), and `pick_component` against a
//! realistic manifest shape.
//!
//! The orchestrator is exercised by manual e2e — see Task 9 of the
//! implementation plan.

use lucerna_lib::jre::manifest::{
    clear_cache_for_test, fetch_top_level, mojang_platform_key, pick_component,
};
use std::sync::{Mutex, MutexGuard, OnceLock};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// Serializes the tests in this binary that mutate
// LUCERNA_JRE_TOPLEVEL_URL_OVERRIDE and the JRE manifest cache.
fn test_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

const TOP_LEVEL_FIXTURE: &str = r#"{
  "windows-x64": {
    "java-runtime-gamma": [
      {
        "availability": {"group": 1, "progress": 100},
        "manifest": {"url": "https://example/comp.json", "sha1": "aaa", "size": 1234},
        "version": {"name": "21.0.3", "released": "2024-04-16"}
      }
    ],
    "jre-legacy": [
      {
        "availability": {"group": 1, "progress": 100},
        "manifest": {"url": "https://example/legacy.json", "sha1": "bbb", "size": 5678},
        "version": {"name": "8u402", "released": "2024-01-23"}
      }
    ]
  },
  "linux": {
    "java-runtime-gamma": [
      {
        "availability": {"group": 1, "progress": 100},
        "manifest": {"url": "https://example/linux-gamma.json", "sha1": "ccc", "size": 1111},
        "version": {"name": "21.0.3", "released": "2024-04-16"}
      }
    ]
  }
}"#;

#[tokio::test]
async fn fetches_top_level_via_env_override_and_caches() {
    let _g = test_lock();
    clear_cache_for_test();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/all.json"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(TOP_LEVEL_FIXTURE, "application/json"),
        )
        .mount(&server)
        .await;

    let _seam = lucerna_lib::test_seam::scope(&[
        (
            "LUCERNA_JRE_TOPLEVEL_URL_OVERRIDE",
            &format!("{}/all.json", server.uri()),
        ),
        ("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost"),
    ]);

    let top = fetch_top_level().await.expect("fetch");
    let gamma = pick_component(&top, "windows-x64", "java-runtime-gamma").expect("pick gamma");
    assert_eq!(gamma.version.name, "21.0.3");
    assert_eq!(gamma.manifest.sha1, "aaa");

    // Second call should be cache-served — kill the mock and confirm.
    drop(server);
    let top2 = fetch_top_level().await.expect("from cache");
    assert!(top2.0.contains_key("windows-x64"));

    clear_cache_for_test();
}

#[tokio::test]
async fn pick_component_falls_through_to_unknown_version_for_missing_component() {
    let _g = test_lock();
    clear_cache_for_test();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/all.json"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(TOP_LEVEL_FIXTURE, "application/json"),
        )
        .mount(&server)
        .await;

    let _seam = lucerna_lib::test_seam::scope(&[
        (
            "LUCERNA_JRE_TOPLEVEL_URL_OVERRIDE",
            &format!("{}/all.json", server.uri()),
        ),
        ("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost"),
    ]);

    let top = fetch_top_level().await.expect("fetch");
    let err = pick_component(&top, "linux", "jre-legacy").unwrap_err();
    // linux has java-runtime-gamma but not jre-legacy in the fixture.
    assert!(format!("{err}").contains("not found"));

    clear_cache_for_test();
}

#[test]
fn mojang_platform_key_covers_documented_combos() {
    assert_eq!(
        mojang_platform_key("windows", "x64").unwrap(),
        "windows-x64"
    );
    assert_eq!(mojang_platform_key("linux", "x64").unwrap(), "linux");
    assert_eq!(
        mojang_platform_key("macos", "aarch64").unwrap(),
        "mac-os-arm64"
    );
    assert!(mojang_platform_key("plan9", "x64").is_err());
}
