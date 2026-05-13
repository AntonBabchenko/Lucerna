//! Network self-audit integration test.
//!
//! Boots `list_manifest()` against wiremock with the
//! `FTLAUNCHER_EXTRA_ALLOWED_HOSTS=127.0.0.1,localhost` env
//! override. Asserts `audit_violations()` is empty after — proving
//! the allowlist + audit machinery catches no false positives on
//! Mojang-shaped URLs.
//!
//! Negative-path test: same flow without the env override produces
//! a violation (because the wiremock URL `127.0.0.1:<port>` isn't
//! in the production allowlist).
//!
//! These tests mutate `std::env` and process-global audit state —
//! they MUST run with `--test-threads=1`.

use ftlauncher_lib::network::{audit_violations, clear_audit_for_test};
use ftlauncher_lib::versions::{clear_manifest_cache_for_test, list_manifest};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const FIXTURE: &str = r#"{
  "latest": {"release": "1.20.4", "snapshot": "1.20.4"},
  "versions": [
    {
      "id": "1.20.4",
      "type": "release",
      "url": "https://piston-meta.mojang.com/v1/packages/abc/1.20.4.json",
      "time": "2023-12-07T12:00:00+00:00",
      "releaseTime": "2023-12-07T12:00:00+00:00",
      "sha1": "deadbeef",
      "complianceLevel": 1
    }
  ]
}"#;

async fn run_list_manifest_against(mock: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/version_manifest_v2.json"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(FIXTURE, "application/json"))
        .mount(mock)
        .await;
    std::env::set_var(
        "FTLAUNCHER_MANIFEST_URL_OVERRIDE",
        format!("{}/version_manifest_v2.json", mock.uri()),
    );
    clear_manifest_cache_for_test();
    let _ = list_manifest().await.expect("list_manifest");
    std::env::remove_var("FTLAUNCHER_MANIFEST_URL_OVERRIDE");
}

#[tokio::test]
async fn audit_has_zero_violations_with_localhost_in_extra_allowed() {
    clear_audit_for_test();
    std::env::set_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS", "127.0.0.1,localhost");
    let mock = MockServer::start().await;
    run_list_manifest_against(&mock).await;
    let v = audit_violations();
    std::env::remove_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS");
    assert!(
        v.is_empty(),
        "post-list_manifest violations should be empty with override; got: {v:?}",
    );
}

#[tokio::test]
async fn audit_flags_localhost_without_override() {
    clear_audit_for_test();
    std::env::remove_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS");
    let mock = MockServer::start().await;
    run_list_manifest_against(&mock).await;
    let v = audit_violations();
    assert!(
        !v.is_empty(),
        "post-list_manifest violations should be non-empty without override; got empty",
    );
    assert!(
        v.iter()
            .any(|e| e.url.contains("127.0.0.1") || e.url.contains("localhost")),
        "violation entry should reference the wiremock host: {v:?}"
    );
}

#[tokio::test]
async fn synthesised_evil_entry_is_flagged() {
    clear_audit_for_test();
    // Record a manual audit entry as if some module had made an
    // outbound request. Then assert the query flags it.
    use ftlauncher_lib::network::{record, AuditEntry};
    record(AuditEntry {
        ts: 0.0,
        method: "GET".into(),
        url: "https://evil.example/x".into(),
        initiator: "test".into(),
        bytes: None,
        status: None,
    });
    let v = audit_violations();
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].url, "https://evil.example/x");
}
