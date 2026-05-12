//! End-to-end integration test for `network::download::download_no_emit`
//! against a local mock HTTP server. We cannot construct a real
//! `tauri::AppHandle` here, so this test exercises the download +
//! hash + audit path; the event-emit branch in `download_with_sha`
//! is covered by manual e2e verification.

use ftlauncher_lib::error::Error;
use ftlauncher_lib::network::audit::{clear_for_test, recent};
use sha1::{Digest, Sha1};
use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn sha1_hex(bytes: &[u8]) -> String {
    let mut h = Sha1::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

#[tokio::test]
async fn download_succeeds_when_hash_matches() {
    clear_for_test();
    let body = b"hello, ftlauncher";
    let expected_sha = sha1_hex(body);

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/file.bin"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body.as_slice()))
        .mount(&server)
        .await;

    let dir = tempdir().unwrap();
    let dest = dir.path().join("out.bin");
    let url = format!("{}/file.bin", server.uri());

    ftlauncher_lib::network::download::download_no_emit(&url, &dest, &expected_sha, "test")
        .await
        .expect("download should succeed");

    let written = std::fs::read(&dest).unwrap();
    assert_eq!(written, body);

    let audit = recent();
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0].url, url);
    assert_eq!(audit[0].initiator, "test");
    assert_eq!(audit[0].bytes, Some(body.len() as f64));
    assert_eq!(audit[0].status, Some(200));
}

#[tokio::test]
async fn download_fails_on_hash_mismatch_and_deletes_file() {
    clear_for_test();
    let body = b"different content";
    let wrong_sha = sha1_hex(b"not this");

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wrong.bin"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body.as_slice()))
        .mount(&server)
        .await;

    let dir = tempdir().unwrap();
    let dest = dir.path().join("wrong.bin");
    let url = format!("{}/wrong.bin", server.uri());

    let err = ftlauncher_lib::network::download::download_no_emit(&url, &dest, &wrong_sha, "test")
        .await
        .expect_err("hash mismatch should fail");

    match err {
        Error::HashMismatch { expected, got, .. } => {
            assert_eq!(expected, wrong_sha);
            assert_eq!(got, sha1_hex(body));
        }
        other => panic!("expected HashMismatch, got {other:?}"),
    }

    assert!(!dest.exists(), "bad download should be removed");
}

#[tokio::test]
async fn http_error_status_returns_network_error_and_logs() {
    clear_for_test();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/nope"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let dir = tempdir().unwrap();
    let dest = dir.path().join("nope.bin");
    let url = format!("{}/nope", server.uri());

    let err = ftlauncher_lib::network::download::download_no_emit(&url, &dest, "deadbeef", "test")
        .await
        .expect_err("404 should fail");

    assert!(
        matches!(err, Error::Network { .. }),
        "expected Network error, got {err:?}"
    );
    let audit = recent();
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0].status, Some(404));
}
