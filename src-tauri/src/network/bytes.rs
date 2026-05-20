//! `get_bytes(url, initiator)` — raw byte fetch through the single
//! chokepoint. A thin wrapper over `network::request::get`. Returns
//! the body as `Vec<u8>` without streaming to disk.
//!
//! Large downloads that should stream to disk still use
//! `download_with_sha`.

use crate::error::{Error, Result};
use crate::network::request;

pub async fn get_bytes(url: &str, initiator: &str) -> Result<Vec<u8>> {
    let resp = request::get(url, &[], initiator).await?;
    if !(200..300).contains(&resp.status) {
        return Err(Error::network(url, format!("HTTP {}", resp.status)));
    }
    Ok(resp.body)
}
