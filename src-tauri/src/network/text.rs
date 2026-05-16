//! `get_text(url, initiator)` — raw text fetch through the single
//! chokepoint. Mirrors `get_json` shape exactly; returns the body as
//! a UTF-8 `String` instead of deserialising. Used for XML
//! (`maven-metadata.xml`) and untyped JSON bodies (`promotions_slim.json`
//! is parsed downstream by the Forge meta layer, not here).

use crate::error::{Error, Result};
use crate::network::audit::{now_ms, record, AuditEntry};
use crate::network::client::http;

pub async fn get_text(url: &str, initiator: &str) -> Result<String> {
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

    String::from_utf8(bytes.to_vec())
        .map_err(|e| Error::network(url, format!("not valid UTF-8: {e}")))
}
