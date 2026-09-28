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
use sha2::Digest as Sha2Digest;
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
/// `Md5` / `Sha256` it additionally computes that digest to verify the
/// vendor-supplied checksum.
#[derive(Debug, Clone, PartialEq)]
pub enum Checksum {
    /// Lowercase sha1 hex. Empty string = skip verification (legacy behaviour).
    Sha1(String),
    /// Lowercase md5 hex (ATLauncher server/direct mods, Purpur core jars).
    /// An empty expected string skips verification (shared legacy semantics);
    /// callers for integrity-relevant sources must never pass an empty digest
    /// (the Purpur client guarantees a non-empty md5 before constructing this).
    Md5(String),
    /// Lowercase sha256 hex (PaperMC Fill jars, Hangar-hosted plugin files).
    /// An empty expected string skips verification (shared legacy semantics);
    /// callers for integrity-relevant sources must never pass an empty digest
    /// (the Paper client guarantees a non-empty sha256 before constructing
    /// this — a STABLE build with an empty Fill sha256 is rejected during
    /// build selection instead of being returned).
    Sha256(String),
}

/// Shared streaming-download core used by the `download_with_sha` /
/// `download_no_emit` wrappers AND directly by callers that need a
/// progress callback without a Tauri `AppHandle` (e.g. `mods::install`).
///
/// Downloads are **atomic**: the body streams to a sibling `<dest>.part`
/// file, is hashed as it goes and verified against `checksum` after the
/// last byte, and only then is renamed onto `dest`. Any stream/IO error,
/// a hash mismatch, or a failed rename leaves `dest` untouched and removes
/// the temp file; a removal that itself fails is logged with the temp
/// file's path, and the caller still gets the download's own error.
/// This closes a TOFU-truncation hole: an interrupted download of an
/// empty-sha artifact used to leave a truncated file at `dest` that later
/// presence/empty-sha checks would trust forever. Because `dest` only ever
/// appears fully-written, treating an empty expected sha as "exists = ok"
/// downstream is safe.
///
/// Always computes and returns sha1 (the universal identity anchor).
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

    // Stream to a sibling temp file, then rename onto `dest`. Keeping the
    // temp in the same directory guarantees the rename is a cheap same-volume
    // move. A `.part` suffix makes a leftover temp file (after a hard crash
    // that skips the cleanup, or a cleanup that failed and was logged)
    // recognisable and disjoint from finished files.
    let part = part_path(dest);

    let streamed = stream_to_part(
        resp,
        &part,
        dest,
        &checksum,
        url,
        initiator,
        bytes_total,
        &mut emit,
    )
    .await;

    promote_or_discard(streamed, &part, dest, |line| {
        crate::diag!("network: {initiator} download {url} — {line}");
    })
    .await
}

/// Compute the sibling temp path used while a download is in flight.
fn part_path(dest: &Path) -> std::path::PathBuf {
    let mut name = dest.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    dest.with_file_name(name)
}

/// Settle a finished stream: on success rename the verified `part` onto
/// `dest`; on any failure — the stream's own, or the rename's — remove
/// `part`, so no unverified or unpromoted bytes stay behind.
///
/// The error returned is always the one describing why the download
/// failed. A removal that fails must not replace it, or the user would
/// read about a temp file instead of the cause, so it goes to
/// `report_leftover` (the launcher log, in production) with the file's
/// path. Callers match on the error's variant, so it is not folded in
/// either.
async fn promote_or_discard(
    streamed: Result<String>,
    part: &Path,
    dest: &Path,
    report_leftover: impl FnOnce(String),
) -> Result<String> {
    let failure = match streamed {
        Ok(got_sha1) => match tokio::fs::rename(part, dest).await {
            Ok(()) => return Ok(got_sha1),
            Err(e) => Error::io(dest.display().to_string(), e),
        },
        Err(e) => e,
    };
    match tokio::fs::remove_file(part).await {
        Ok(()) => {}
        // Never created (`File::create` failed) or already gone: nothing
        // was left behind, so there is nothing to report.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => report_leftover(format!(
            "could not remove the temporary file {}: {e}",
            part.display()
        )),
    }
    Err(failure)
}

/// Stream the response body into `part`, hashing and emitting progress,
/// and verify the digest. On success returns the computed sha1; the caller
/// renames `part` onto `dest`. `dest` is used only for error messages here.
#[allow(clippy::too_many_arguments)]
async fn stream_to_part(
    resp: reqwest::Response,
    part: &Path,
    dest: &Path,
    checksum: &Checksum,
    url: &str,
    initiator: &str,
    bytes_total: Option<f64>,
    emit: &mut impl FnMut(DownloadProgress),
) -> Result<String> {
    let mut file = File::create(part)
        .await
        .map_err(|e| Error::io(part.display().to_string(), e))?;

    let mut sha1_hasher = Sha1::new();
    let mut md5_hasher = Md5::new();
    let mut sha256_hasher = sha2::Sha256::new();
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
        if matches!(checksum, Checksum::Sha256(_)) {
            sha256_hasher.update(&chunk);
        }
        file.write_all(&chunk)
            .await
            .map_err(|e| Error::io(part.display().to_string(), e))?;
        bytes_done += chunk.len() as f64;

        emit(DownloadProgress {
            url: url.to_string(),
            bytes_done,
            bytes_total,
        });
    }
    file.flush()
        .await
        .map_err(|e| Error::io(part.display().to_string(), e))?;

    let got_sha1 = hex::encode(sha1_hasher.finalize());

    let (expected, got_for_compare) = match checksum {
        Checksum::Sha1(h) => (h.as_str(), got_sha1.clone()),
        Checksum::Md5(h) => (h.as_str(), hex::encode(md5_hasher.finalize())),
        Checksum::Sha256(h) => (h.as_str(), hex::encode(sha256_hasher.finalize())),
    };
    if !expected.is_empty() && got_for_compare != expected.to_ascii_lowercase() {
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

/// `download_no_emit` with an explicit checksum kind (sha256/md5/sha1).
/// Used by Paper/Purpur core provisioning and Hangar plugin installs.
pub async fn download_no_emit_with(
    url: &str,
    dest: &Path,
    checksum: Checksum,
    initiator: &str,
) -> Result<()> {
    download_inner(url, dest, checksum, initiator, |_| {})
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

    // sha256 of b"hello world" (well-known vector)
    const HELLO_SHA256: &str = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";

    #[tokio::test]
    async fn sha256_checksum_verifies_and_rejects() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/f.bin"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"hello world".to_vec()))
            .mount(&s)
            .await;
        let td = tempdir().unwrap();
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);

        // Correct digest -> file lands on dest.
        let good = td.path().join("good.bin");
        download_no_emit_with(
            &format!("{}/f.bin", s.uri()),
            &good,
            Checksum::Sha256(HELLO_SHA256.into()),
            "servers",
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read(&good).unwrap(), b"hello world");

        // Wrong digest -> HashMismatch and NO dest file (and no .part) left.
        let bad = td.path().join("bad.bin");
        let err = download_no_emit_with(
            &format!("{}/f.bin", s.uri()),
            &bad,
            Checksum::Sha256("00".repeat(32)),
            "servers",
        )
        .await
        .unwrap_err();
        assert!(matches!(err, Error::HashMismatch { .. }));
        assert!(!bad.exists());
        assert!(
            !part_path(&bad).exists(),
            "temp .part file must be cleaned up on mismatch"
        );
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
    async fn mismatch_leaves_no_part_file_and_preserves_existing_dest() {
        // Atomicity: a hash mismatch must not clobber a pre-existing good
        // file at `dest`, and must not leave a `<dest>.part` behind.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/x.jar"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"new-bad-bytes"))
            .mount(&server)
            .await;

        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("x.jar");
        std::fs::write(&dest, b"existing-good-bytes").unwrap();
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
            !part_path(&dest).exists(),
            "temp .part file must be cleaned up on mismatch"
        );
        assert_eq!(
            std::fs::read(&dest).unwrap(),
            b"existing-good-bytes",
            "pre-existing dest must be untouched by a failed download"
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

    /// A non-empty directory where a file is expected. `remove_file` on it
    /// fails with an error that is not `NotFound` on every platform (EISDIR on
    /// Linux, EPERM on macOS, access denied on Windows), and nothing can be
    /// renamed onto it. It stands in for a temp file that cannot be removed or
    /// a destination that cannot be replaced.
    fn occupied_dir(at: &Path) {
        std::fs::create_dir_all(at).unwrap();
        std::fs::write(at.join("keep"), b"x").unwrap();
    }

    #[tokio::test]
    async fn a_temp_file_that_cannot_be_removed_is_reported_and_the_download_error_kept() {
        let dir = tempdir().unwrap();
        let dest = dir.path().join("x.jar");
        let part = part_path(&dest);
        occupied_dir(&part);
        let mut leftovers = Vec::new();

        let r = promote_or_discard(
            Err(Error::network(
                "https://host/x.jar",
                "interrupted mid-stream",
            )),
            &part,
            &dest,
            |line| leftovers.push(line),
        )
        .await;

        assert!(
            matches!(r, Err(Error::Network { ref details, .. }) if details == "interrupted mid-stream"),
            "the download's own error must reach the caller, got {r:?}"
        );
        assert_eq!(
            leftovers.len(),
            1,
            "one report for the one leftover: {leftovers:?}"
        );
        assert!(
            leftovers[0].contains(&part.display().to_string()),
            "the report must name the leftover: {}",
            leftovers[0]
        );
    }

    #[tokio::test]
    async fn a_failed_promotion_whose_temp_file_cannot_be_removed_is_reported() {
        let dir = tempdir().unwrap();
        let dest = dir.path().join("x.jar");
        occupied_dir(&dest);
        let part = part_path(&dest);
        occupied_dir(&part);
        let mut leftovers = Vec::new();

        let r =
            promote_or_discard(Ok("sha".into()), &part, &dest, |line| leftovers.push(line)).await;

        assert!(
            matches!(r, Err(Error::Io { ref path, .. }) if *path == dest.display().to_string()),
            "the rename's error (naming dest) must reach the caller, got {r:?}"
        );
        assert_eq!(
            leftovers.len(),
            1,
            "one report for the one leftover: {leftovers:?}"
        );
        assert!(
            leftovers[0].contains(&part.display().to_string()),
            "the report must name the leftover: {}",
            leftovers[0]
        );
    }

    #[tokio::test]
    async fn a_temp_file_that_is_already_gone_is_not_reported() {
        // `File::create` failing leaves no temp file at all; reporting one
        // would claim a leftover that does not exist.
        let dir = tempdir().unwrap();
        let dest = dir.path().join("x.jar");
        let part = part_path(&dest);
        let mut leftovers = Vec::new();

        let r = promote_or_discard(
            Err(Error::io(part.display().to_string(), "create failed")),
            &part,
            &dest,
            |line| leftovers.push(line),
        )
        .await;

        assert!(matches!(r, Err(Error::Io { .. })), "got {r:?}");
        assert!(
            leftovers.is_empty(),
            "nothing was left behind: {leftovers:?}"
        );
    }

    #[tokio::test]
    async fn a_failed_promotion_removes_the_temp_file_and_names_the_destination() {
        let dir = tempdir().unwrap();
        let dest = dir.path().join("x.jar");
        occupied_dir(&dest);
        let part = part_path(&dest);
        std::fs::write(&part, b"verified-bytes").unwrap();
        let mut leftovers = Vec::new();

        let r =
            promote_or_discard(Ok("sha".into()), &part, &dest, |line| leftovers.push(line)).await;

        assert!(
            matches!(r, Err(Error::Io { ref path, .. }) if *path == dest.display().to_string()),
            "got {r:?}"
        );
        assert!(
            !part.exists(),
            "the unpromoted temp file must not stay behind"
        );
        assert!(leftovers.is_empty(), "the removal succeeded: {leftovers:?}");
        assert!(dest.join("keep").exists(), "dest must be untouched");
    }
}
