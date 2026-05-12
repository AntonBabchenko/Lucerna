//! `download_with_sha(url, dest, expected_sha, initiator)` — the only
//! way to put bytes from the network onto disk.
//!
//! Streams the body to `dest`, hashing as it goes. Verifies SHA-1
//! against `expected_sha` after the last byte. Emits `DownloadProgress`
//! events through tauri-specta. Records an `AuditEntry` whether the
//! call succeeds or fails.

use crate::error::{Error, Result};
use crate::network::audit::{now_ms, record, AuditEntry};
use crate::network::client::http;
use futures_util::StreamExt;
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

/// Record a partial-progress audit entry. Used to log mid-stream
/// failures (connection drop, write error) so the Network panel reflects
/// every attempted call, not just clean successes/failures.
fn audit_partial(url: &str, initiator: &str, bytes_done: f64, status: u16) {
    record(AuditEntry {
        ts: now_ms(),
        method: "GET".into(),
        url: url.into(),
        initiator: initiator.into(),
        bytes: Some(bytes_done),
        status: Some(status),
    });
}

/// Download `url` to `dest`, verify SHA-1 equals `expected_sha_hex`,
/// emit progress events, and record the call in the audit log.
///
/// `initiator` is the module name that triggered the download
/// (e.g. `"versions"`, `"jre"`, `"assets"`) — appears in the
/// Network Activity panel so users can see what asked for what.
pub async fn download_with_sha(
    app: &tauri::AppHandle,
    url: &str,
    dest: &Path,
    expected_sha_hex: &str,
    initiator: &str,
) -> Result<()> {
    let resp = http().get(url).send().await.map_err(|e| {
        record(AuditEntry {
            ts: now_ms(),
            method: "GET".into(),
            url: url.into(),
            initiator: initiator.into(),
            bytes: None,
            status: None,
        });
        Error::network(url, e)
    })?;

    let status = resp.status();
    if !status.is_success() {
        record(AuditEntry {
            ts: now_ms(),
            method: "GET".into(),
            url: url.into(),
            initiator: initiator.into(),
            bytes: None,
            status: Some(status.as_u16()),
        });
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

    let mut hasher = Sha1::new();
    let mut bytes_done: f64 = 0.0;
    let status_code = status.as_u16();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| {
            audit_partial(url, initiator, bytes_done, status_code);
            Error::network(url, e)
        })?;
        hasher.update(&chunk);
        file.write_all(&chunk).await.map_err(|e| {
            audit_partial(url, initiator, bytes_done, status_code);
            Error::io(dest.display().to_string(), e)
        })?;
        bytes_done += chunk.len() as f64;

        // Emit best-effort; if the UI isn't listening, dropping the
        // event is fine.
        let _ = DownloadProgress {
            url: url.to_string(),
            bytes_done,
            bytes_total,
        }
        .emit(app);
    }
    file.flush()
        .await
        .map_err(|e| Error::io(dest.display().to_string(), e))?;

    let got_hex = hex::encode(hasher.finalize());
    record(AuditEntry {
        ts: now_ms(),
        method: "GET".into(),
        url: url.into(),
        initiator: initiator.into(),
        bytes: Some(bytes_done),
        status: Some(status.as_u16()),
    });

    if got_hex != expected_sha_hex {
        // Drop the bad file so a retry starts fresh.
        let _ = tokio::fs::remove_file(dest).await;
        return Err(Error::HashMismatch {
            path: dest.display().to_string(),
            expected: expected_sha_hex.to_string(),
            got: got_hex,
        });
    }

    Ok(())
}

/// Same as `download_with_sha` but without event emission. Exposed for
/// integration tests that cannot construct a real `tauri::AppHandle`.
/// Not registered as a Tauri command; production callers always use
/// `download_with_sha`.
///
/// TODO(slice-4): collapse with `download_with_sha` via an inner generic
/// `download_inner(emit: impl FnMut(DownloadProgress))` once a real
/// consumer (asset/version install) lands.
#[doc(hidden)]
pub async fn download_no_emit(
    url: &str,
    dest: &Path,
    expected_sha_hex: &str,
    initiator: &str,
) -> Result<()> {
    let resp = http().get(url).send().await.map_err(|e| {
        record(AuditEntry {
            ts: now_ms(),
            method: "GET".into(),
            url: url.into(),
            initiator: initiator.into(),
            bytes: None,
            status: None,
        });
        Error::network(url, e)
    })?;

    let status = resp.status();
    if !status.is_success() {
        record(AuditEntry {
            ts: now_ms(),
            method: "GET".into(),
            url: url.into(),
            initiator: initiator.into(),
            bytes: None,
            status: Some(status.as_u16()),
        });
        return Err(Error::network(url, format!("HTTP {status}")));
    }

    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| Error::io(parent.display().to_string(), e))?;
    }
    let mut file = File::create(dest)
        .await
        .map_err(|e| Error::io(dest.display().to_string(), e))?;

    let mut hasher = Sha1::new();
    let mut bytes_done: f64 = 0.0;
    let status_code = status.as_u16();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| {
            audit_partial(url, initiator, bytes_done, status_code);
            Error::network(url, e)
        })?;
        hasher.update(&chunk);
        file.write_all(&chunk).await.map_err(|e| {
            audit_partial(url, initiator, bytes_done, status_code);
            Error::io(dest.display().to_string(), e)
        })?;
        bytes_done += chunk.len() as f64;
    }
    file.flush()
        .await
        .map_err(|e| Error::io(dest.display().to_string(), e))?;

    let got_hex = hex::encode(hasher.finalize());
    record(AuditEntry {
        ts: now_ms(),
        method: "GET".into(),
        url: url.into(),
        initiator: initiator.into(),
        bytes: Some(bytes_done),
        status: Some(status.as_u16()),
    });

    if got_hex != expected_sha_hex {
        let _ = tokio::fs::remove_file(dest).await;
        return Err(Error::HashMismatch {
            path: dest.display().to_string(),
            expected: expected_sha_hex.to_string(),
            got: got_hex,
        });
    }
    Ok(())
}
