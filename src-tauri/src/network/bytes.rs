//! `get_bytes(url, initiator)` — raw byte fetch through the single
//! chokepoint. Mirrors `get_json` shape exactly; returns the body as
//! `Vec<u8>` without streaming to disk. Used for small-to-medium
//! binary payloads (Forge installer JARs, ~5-50MB) that we keep in
//! memory long enough to parse out of a zip in-process.
//!
//! Large downloads that should stream to disk still use
//! `download_with_sha`.

use crate::error::{Error, Result};
use crate::network::audit::{now_ms, record, AuditEntry};
use crate::network::client::http;

pub async fn get_bytes(url: &str, initiator: &str) -> Result<Vec<u8>> {
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

    let bytes = resp.bytes().await.map_err(|e| Error::network(url, e))?;
    let byte_len = bytes.len() as f64;

    record(AuditEntry {
        ts: now_ms(),
        method: "GET".into(),
        url: url.into(),
        initiator: initiator.into(),
        bytes: Some(byte_len),
        status: Some(status.as_u16()),
    });

    Ok(bytes.to_vec())
}
