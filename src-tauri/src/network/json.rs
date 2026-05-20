//! `get_json::<T>(url, initiator)` — typed JSON fetch through the
//! single chokepoint. A thin wrapper over `network::request::get`:
//! the primitive does the send + audit, this layer maps a non-2xx
//! status to `Error::network` and deserialises the body.

use crate::error::{Error, Result};
use crate::network::request;
use serde::de::DeserializeOwned;

pub async fn get_json<T: DeserializeOwned>(url: &str, initiator: &str) -> Result<T> {
    let resp = request::get(url, &[], initiator).await?;
    if !(200..300).contains(&resp.status) {
        return Err(Error::network(url, format!("HTTP {}", resp.status)));
    }
    serde_json::from_slice::<T>(&resp.body)
        .map_err(|e| Error::network(url, format!("parse: {e}")))
}
