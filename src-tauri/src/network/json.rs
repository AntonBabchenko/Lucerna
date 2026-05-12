//! `get_json::<T>(url, initiator)` — typed JSON fetch through the
//! single chokepoint. Every call is recorded in the audit log,
//! whether the response was a 2xx, a non-2xx, or a network error
//! before any status was received.

use crate::error::{Error, Result};
use crate::network::audit::{now_ms, record, AuditEntry};
use crate::network::client::http;
use serde::de::DeserializeOwned;

pub async fn get_json<T: DeserializeOwned>(url: &str, initiator: &str) -> Result<T> {
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

    serde_json::from_slice::<T>(&bytes)
        .map_err(|e| Error::network(url, format!("parse: {e}")))
}
