//! `get_text(url, initiator)` — raw text fetch through the single
//! chokepoint. A thin wrapper over `network::request::get`. Returns
//! the body as a UTF-8 `String`.

use crate::error::{Error, Result};
use crate::network::request;

pub async fn get_text(url: &str, initiator: &str) -> Result<String> {
    let resp = request::get(url, &[], initiator).await?;
    if !(200..300).contains(&resp.status) {
        return Err(Error::network(url, format!("HTTP {}", resp.status)));
    }
    String::from_utf8(resp.body).map_err(|e| Error::network(url, format!("not valid UTF-8: {e}")))
}
