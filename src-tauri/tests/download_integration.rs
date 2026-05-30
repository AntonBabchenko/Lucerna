//! End-to-end integration test for `network::download::download_no_emit`
//! against a local mock HTTP server. We cannot construct a real
//! `tauri::AppHandle` here, so this test exercises the download +
//! hash path; the event-emit branch in `download_with_sha`
//! is covered by manual e2e verification.

use lucerna_lib::error::Error;
use sha1::{Digest, Sha1};
use std::sync::{Mutex, MutexGuard, OnceLock};
use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn sha1_hex(bytes: &[u8]) -> String {
    let mut h = Sha1::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

// Serialize tests so cargo's parallel runner doesn't interleave env-var
// mutations across tests.
fn test_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[tokio::test]
async fn download_succeeds_when_hash_matches() {
    let _g = test_lock();
    let body = b"hello, lucerna";
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

    std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
    lucerna_lib::network::download::download_no_emit(&url, &dest, &expected_sha, "test")
        .await
        .expect("download should succeed");
    std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");

    let written = std::fs::read(&dest).unwrap();
    assert_eq!(written, body);
}

#[tokio::test]
async fn download_fails_on_hash_mismatch_and_deletes_file() {
    let _g = test_lock();
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

    std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
    let err = lucerna_lib::network::download::download_no_emit(&url, &dest, &wrong_sha, "test")
        .await
        .expect_err("hash mismatch should fail");
    std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");

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
async fn http_error_status_returns_network_error() {
    let _g = test_lock();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/nope"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let dir = tempdir().unwrap();
    let dest = dir.path().join("nope.bin");
    let url = format!("{}/nope", server.uri());

    std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
    let err = lucerna_lib::network::download::download_no_emit(&url, &dest, "deadbeef", "test")
        .await
        .expect_err("404 should fail");
    std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");

    assert!(
        matches!(err, Error::Network { .. }),
        "expected Network error, got {err:?}"
    );
}
