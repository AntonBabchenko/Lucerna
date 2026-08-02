//! The single seam for talking to a server on this machine.
//!
//! The host allowlist covers destinations Lucerna chooses on the internet.
//! A user's local model server is neither — it is on 127.0.0.1, and adding
//! that to `ALLOWED_PATTERNS` would let *every* code path in the launcher
//! reach *every* local port. This module is the narrow alternative: the host
//! is a compile-time constant, only the port is caller-supplied, and
//! `structural_loopback_confined.rs` fails the build if anything outside
//! `l10n::prefill` calls it.
//!
//! Consent is enforced before this is reachable, but not by this module and
//! not by an ordering rule: the only caller is `l10n::prefill::provider`, and
//! every function there that reaches a model requires a
//! `network::consent::AiConsent` — a token whose field is private to
//! `network::consent`, so the only way to hold one is to have passed the
//! permission check. Confinement (above) plus that token is what makes the
//! 127.0.0.1 bypass safe.
//!
//! Uses the generation client (no read timeout): a local model on CPU can
//! take minutes to produce its first token. The caller's total timeout is the
//! only bound.

use crate::error::{Error, Result};
use crate::network::request::HttpResponse;
use std::time::Duration;

const LOOPBACK_HOST: &str = "127.0.0.1";

fn loopback_url(port: u16, path: &str) -> String {
    format!("http://{LOOPBACK_HOST}:{port}{path}")
}

/// POST a JSON body to a local server. Like `network::request`, any received
/// status is `Ok` and only a transport failure is `Err`.
pub async fn post_json(
    port: u16,
    path: &str,
    body: &[u8],
    timeout: Duration,
) -> Result<HttpResponse> {
    let url = loopback_url(port, path);
    let resp = crate::network::client::http_generation()
        .post(&url)
        .header("content-type", "application/json")
        .timeout(timeout)
        .body(body.to_vec())
        .send()
        .await
        .map_err(|e| Error::network(url.clone(), e))?;
    let status = resp.status().as_u16();
    let bytes = resp.bytes().await.map_err(|e| Error::network(url, e))?;
    Ok(HttpResponse {
        status,
        body: bytes.to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_is_always_loopback_regardless_of_port() {
        assert_eq!(
            loopback_url(11434, "/v1/chat/completions"),
            "http://127.0.0.1:11434/v1/chat/completions"
        );
        assert_eq!(loopback_url(1234, "/x"), "http://127.0.0.1:1234/x");
    }
}
