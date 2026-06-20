//! `download_with_sha(url, dest, expected_sha, initiator)` — the only
//! way to put bytes from the network onto disk.
//!
//! Streams the body to `dest`, hashing as it goes. Verifies SHA-1
//! against `expected_sha` after the last byte. Emits `DownloadProgress`
//! events through tauri-specta.

use crate::error::{Error, Result};
use crate::network::client::http;
use futures_util::StreamExt;
use md5::Md5;
use serde::Serialize;
use sha1::{Digest, Sha1};
use specta::Type;
use std::path::Path;
use tauri_specta::Event;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

/// Progress event emitted during a download. The UI subscribes via
/// `listen<DownloadProgress>("download:progress", ...)`.
#[derive(Debug, Clone, Serialize, Type, Event)]
pub struct DownloadProgress {
    pub url: String,
    /// Bytes downloaded so far. `f64` for JS number compatibility (the
    /// 2^53 safe-integer range holds files up to ~9 petabytes).
    pub bytes_done: f64,
    /// `None` if the server did not send `Content-Length`.
    pub bytes_total: Option<f64>,
}

/// What digest to verify a download against. The download always *also*
/// computes sha1 (the universal identity anchor) and returns it; for
/// `Md5` it additionally computes md5 to verify the vendor digest.
pub enum Checksum {
    /// Lowercase sha1 hex. Empty string = skip verification (legacy behaviour).
    Sha1(String),
    /// Lowercase md5 hex (ATLauncher server/direct mods).
    Md5(String),
}

/// Shared streaming-download core used by the `download_with_sha` /
/// `download_no_emit` wrappers AND directly by callers that need a
/// progress callback without a Tauri `AppHandle` (e.g. `mods::install`).
/// Streams the body of `url` to `dest`, hashing as it goes;
/// verifies against `checksum` after the last byte;
/// always computes and returns sha1 (the universal identity anchor).
/// `Err(HashMismatch)` deletes the partial file.
///
/// `download_with_sha` / `download_no_emit` are thin wrappers that
/// supply the `emit` closure (Tauri-event emission, or a no-op).
pub(crate) async fn download_inner(
    url: &str,
    dest: &Path,
    checksum: Checksum,
    initiator: &str,
    mut emit: impl FnMut(DownloadProgress),
) -> Result<String> {
    crate::network::allowlist::check_url_allowed(url, initiator)?;
    let resp = http().get(url).send().await.map_err(|e| {
        crate::diag!(
            "network: {initiator} download {url} — request failed (source unreachable?): {e}"
        );
        Error::network(url, e)
    })?;

    let status = resp.status();
    if !status.is_success() {
        // A download is fetching a concrete artifact, so any non-success
        // (incl. 404 = missing file, 429 = rate-limited, 5xx = outage) is a
        // real failure worth recording.
        crate::diag!("network: {initiator} download {url} — HTTP {status}");
        return Err(Error::network(url, format!("HTTP {status}")));
    }

    let bytes_total = resp.content_length().map(|n| n as f64);

    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| Error::io(parent.display().to_string(), e))?;
    }
    let mut file = File::create(dest)
        .await
        .map_err(|e| Error::io(dest.display().to_string(), e))?;

    let mut sha1_hasher = Sha1::new();
    let mut md5_hasher = Md5::new();
    let mut bytes_done: f64 = 0.0;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| {
            crate::diag!("network: {initiator} download {url} — interrupted mid-stream: {e}");
            Error::network(url, e)
        })?;
        sha1_hasher.update(&chunk);
        if matches!(checksum, Checksum::Md5(_)) {
            md5_hasher.update(&chunk);
        }
        file.write_all(&chunk)
            .await
            .map_err(|e| Error::io(dest.display().to_string(), e))?;
        bytes_done += chunk.len() as f64;

        emit(DownloadProgress {
            url: url.to_string(),
            bytes_done,
            bytes_total,
        });
    }
    file.flush()
        .await
        .map_err(|e| Error::io(dest.display().to_string(), e))?;

    let got_sha1 = hex::encode(sha1_hasher.finalize());

    let (expected, got_for_compare) = match &checksum {
        Checksum::Sha1(h) => (h.as_str(), got_sha1.clone()),
        Checksum::Md5(h) => (h.as_str(), hex::encode(md5_hasher.finalize())),
    };
    if !expected.is_empty() && got_for_compare != expected.to_ascii_lowercase() {
        // Drop the bad file so a retry starts fresh.
        let _ = tokio::fs::remove_file(dest).await;
        return Err(Error::HashMismatch {
            path: dest.display().to_string(),
            expected: expected.to_string(),
            got: got_for_compare,
        });
    }

    Ok(got_sha1)
}

/// Download `url` to `dest`, verify SHA-1 equals `expected_sha_hex`,
/// and emit a `DownloadProgress` Tauri event per chunk.
///
/// `initiator` is the module name that triggered the download
/// (e.g. `"versions"`, `"jre"`, `"assets"`).
pub async fn download_with_sha(
    app: &tauri::AppHandle,
    url: &str,
    dest: &Path,
    expected_sha_hex: &str,
    initiator: &str,
) -> Result<()> {
    download_inner(
        url,
        dest,
        Checksum::Sha1(expected_sha_hex.to_string()),
        initiator,
        |p| {
            // Best-effort: if the UI isn't listening, dropping the event is fine.
            let _ = p.emit(app);
        },
    )
    .await
    .map(|_| ())
}

/// Same as `download_with_sha` but without event emission. Used by integration
/// tests (which cannot construct a `tauri::AppHandle`) and by AppHandle-free
/// production helpers that don't need progress events (e.g. server assembly,
/// forge installer tooling). Still routes through the allowlist chokepoint.
#[doc(hidden)]
pub async fn download_no_emit(
    url: &str,
    dest: &Path,
    expected_sha_hex: &str,
    initiator: &str,
) -> Result<()> {
    download_inner(
        url,
        dest,
        Checksum::Sha1(expected_sha_hex.to_string()),
        initiator,
        |_| {},
    )
    .await
    .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::test_env_lock()
    }

    #[tokio::test]
    async fn download_inner_md5_verifies_and_returns_sha1() {
        let body = b"atlauncher-mod-bytes";
        let md5_hex = hex::encode(Md5::digest(body));
        let sha1_hex = hex::encode(Sha1::digest(body));
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/mod.jar"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body.to_vec()))
            .mount(&s)
            .await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("mod.jar");
        let got_sha1 = download_inner(
            &format!("{}/mod.jar", s.uri()),
            &dest,
            Checksum::Md5(md5_hex.clone()),
            "test",
            |_| {},
        )
        .await
        .unwrap();
        assert_eq!(
            got_sha1, sha1_hex,
            "must return the sha1 computed over the bytes"
        );
    }

    #[tokio::test]
    async fn download_inner_md5_mismatch_errors_and_deletes() {
        let body = b"atlauncher-mod-bytes";
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/mod.jar"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body.to_vec()))
            .mount(&s)
            .await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("mod.jar");
        let r = download_inner(
            &format!("{}/mod.jar", s.uri()),
            &dest,
            Checksum::Md5("00000000000000000000000000000000".into()),
            "test",
            |_| {},
        )
        .await;
        assert!(matches!(r, Err(Error::HashMismatch { .. })));
        assert!(!dest.exists(), "partial file must be deleted on mismatch");
    }

    #[tokio::test]
    async fn empty_expected_sha_skips_verification() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/loader-lib.jar"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"fake jar bytes"))
            .mount(&server)
            .await;

        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("loader-lib.jar");
        let url = format!("{}/loader-lib.jar", server.uri());

        let result = download_no_emit(&url, &dest, "", "test").await;

        assert!(result.is_ok(), "expected ok, got {result:?}");
        assert!(dest.exists());
        let written = std::fs::read(&dest).unwrap();
        assert_eq!(written, b"fake jar bytes");
    }

    #[tokio::test]
    async fn nonempty_sha_mismatch_still_errors() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/x.jar"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"hello"))
            .mount(&server)
            .await;

        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("x.jar");
        let url = format!("{}/x.jar", server.uri());

        let result = download_no_emit(
            &url,
            &dest,
            "0000000000000000000000000000000000000000",
            "test",
        )
        .await;

        assert!(matches!(result, Err(Error::HashMismatch { .. })));
        assert!(
            !dest.exists(),
            "bad file should be removed after sha mismatch"
        );
    }

    #[tokio::test]
    async fn download_to_non_allowlisted_host_is_rejected() {
        let _g = test_lock();
        let dir = tempdir().unwrap();
        let dest = dir.path().join("x.jar");
        let r = download_no_emit("https://evil.example/x.jar", &dest, "", "test").await;
        assert!(matches!(r, Err(Error::HostNotAllowed { .. })), "got: {r:?}");
        assert!(
            !dest.exists(),
            "no file should be created for a rejected host"
        );
    }
}
