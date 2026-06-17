//! Lower-level chokepoint primitives: a GET or POST with caller-supplied
//! request headers. Returns the response for ANY received HTTP status;
//! `Err` only on a transport-level failure (no status received). The
//! caller owns status-to-error mapping and body decoding.
//!
//! All requests pass through the host allowlist before being sent.
//! `get_json` / `get_text` / `get_bytes` are thin wrappers over this.
//! `download_with_sha` stays separate — it streams to disk rather than
//! buffering the body.

use crate::error::{Error, Result};
use crate::network::client::http;

/// A received HTTP response: the status code and the fully-buffered body.
#[derive(Debug)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

/// Send a built request and return the response for any HTTP status.
/// `Err` only on a transport-level failure (send failure, or a body-read
/// failure after a status was received).
async fn send(
    req: reqwest::RequestBuilder,
    method: &str,
    url: &str,
    initiator: &str,
) -> Result<HttpResponse> {
    crate::network::allowlist::check_url_allowed(url, initiator)?;
    // Throttle: pace per host, with the current task's priority.
    if let Some(host) = host_of(url) {
        let gate = crate::network::throttle::gate_for(&host);
        gate.acquire(crate::network::throttle::current_priority())
            .await;
    }
    let resp = req.send().await.map_err(|e| {
        crate::diag!(
            "network: {initiator} {method} {url} — request failed (source unreachable?): {e}"
        );
        Error::network(url, e)
    })?;
    let status = resp.status().as_u16();
    // A body-read failure (mid-body drop, read timeout) is still a transport-level
    // failure.
    let body = resp.bytes().await.map_err(|e| {
        crate::diag!("network: {initiator} {method} {url} — response body read failed: {e}");
        Error::network(url, e)
    })?;
    // Record notable non-success statuses in the launcher log (rate limits =
    // HTTP 429, auth = 401/403, server outages = 5xx). 404 is excluded — callers
    // routinely treat it as "not found / no match" (loader probes, hash
    // enrichment) where it is expected, not a failure worth logging.
    if should_log_status(status) {
        crate::diag!("network: {initiator} {method} {url} — HTTP {status}");
    }
    Ok(HttpResponse {
        status,
        body: body.to_vec(),
    })
}

/// Whether a received HTTP status is worth recording in the launcher log.
/// Non-2xx is notable (429 rate-limit, 401/403 auth, 5xx outage) — except 404,
/// which callers routinely use as an expected "not found / no match" signal
/// (loader probes, hash enrichment) rather than a real failure.
fn should_log_status(status: u16) -> bool {
    !(200..300).contains(&status) && status != 404
}

/// GET `url` on the shared chokepoint client with `headers` applied.
///
/// Returns `Ok(HttpResponse)` for any received HTTP status (2xx or not);
/// returns `Err(Error::Network)` only when no status was received.
///
/// `initiator` is the module name that triggered the request.
pub async fn get(url: &str, headers: &[(&str, &str)], initiator: &str) -> Result<HttpResponse> {
    let mut req = http().get(url);
    for (name, value) in headers {
        req = req.header(*name, *value);
    }
    send(req, "GET", url, initiator).await
}

/// POST `body` to `url` on the shared chokepoint client with `headers`
/// applied. See `get` for the error contract — `post` is identical
/// except for the HTTP method and the request body.
pub async fn post(
    url: &str,
    headers: &[(&str, &str)],
    body: &[u8],
    initiator: &str,
) -> Result<HttpResponse> {
    let mut req = http().post(url).body(body.to_vec());
    for (name, value) in headers {
        req = req.header(*name, *value);
    }
    send(req, "POST", url, initiator).await
}

/// Parse the host out of a URL for gate lookup. Returns None for an unparseable
/// URL (the request still proceeds, just unthrottled).
fn host_of(url: &str) -> Option<String> {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::test_env_lock()
    }

    #[test]
    fn should_log_status_flags_errors_but_not_ok_or_404() {
        assert!(!should_log_status(200));
        assert!(!should_log_status(204));
        assert!(!should_log_status(404)); // expected "not found / no match"
        assert!(should_log_status(429)); // rate limited
        assert!(should_log_status(403)); // auth / region block
        assert!(should_log_status(500)); // server outage
        assert!(should_log_status(503));
    }

    #[tokio::test]
    async fn returns_ok_for_200_with_body() {
        let _g = test_lock();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/req-200"))
            .respond_with(ResponseTemplate::new(200).set_body_string("hello"))
            .mount(&server)
            .await;
        let url = format!("{}/req-200", server.uri());
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let r = get(&url, &[], "test").await.unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert_eq!(r.status, 200);
        assert_eq!(r.body, b"hello");
    }

    #[tokio::test]
    async fn returns_ok_for_404_not_err() {
        let _g = test_lock();
        // A non-2xx status is NOT an error at this layer — the caller
        // owns status-to-error mapping.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/req-404"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let url = format!("{}/req-404", server.uri());
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let r = get(&url, &[], "test").await.unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert_eq!(r.status, 404);
    }

    #[tokio::test]
    async fn applies_request_headers() {
        let _g = test_lock();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/req-hdr"))
            .and(header("x-api-key", "secret-123"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        let url = format!("{}/req-hdr", server.uri());
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let r = get(&url, &[("x-api-key", "secret-123")], "test").await;
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert!(r.is_ok(), "header should have matched the mock: {r:?}");
    }

    #[tokio::test]
    async fn transport_failure_is_err() {
        let _g = test_lock();
        // Port 1 is unreachable — the connection fails before any status.
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let r = get("http://127.0.0.1:1/nope", &[], "test").await;
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert!(matches!(r, Err(Error::Network { .. })), "got: {r:?}");
    }

    #[tokio::test]
    async fn post_sends_body_and_returns_ok() {
        let _g = test_lock();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/req-post"))
            .and(body_string("{\"k\":1}"))
            .respond_with(ResponseTemplate::new(200).set_body_string("done"))
            .mount(&server)
            .await;
        let url = format!("{}/req-post", server.uri());
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let r = post(&url, &[], b"{\"k\":1}", "test").await.unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert_eq!(r.status, 200);
        assert_eq!(r.body, b"done");
    }

    #[tokio::test]
    async fn post_applies_headers() {
        let _g = test_lock();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/req-post-hdr"))
            .and(header("x-api-key", "k-9"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        let url = format!("{}/req-post-hdr", server.uri());
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let r = post(&url, &[("x-api-key", "k-9")], b"{}", "test").await;
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert!(r.is_ok(), "header should have matched the mock: {r:?}");
    }

    #[tokio::test]
    async fn get_to_non_allowlisted_host_is_rejected() {
        let _g = test_lock();
        // No server started — if the host check fails to fire, this would
        // be a transport error instead; we assert specifically HostNotAllowed.
        let r = get("https://evil.example/x", &[], "test").await;
        assert!(matches!(r, Err(Error::HostNotAllowed { .. })), "got: {r:?}");
    }

    #[test]
    fn host_of_parses_api_host() {
        assert_eq!(
            host_of("https://api.modrinth.com/v2/project/x/version?a=b").as_deref(),
            Some("api.modrinth.com")
        );
        assert_eq!(host_of("not a url"), None);
    }

    #[tokio::test]
    async fn get_still_succeeds_through_throttle() {
        let _g = test_lock();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/t"))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .mount(&server)
            .await;
        let url = format!("{}/t", server.uri());
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let r = get(&url, &[], "test").await.unwrap(); // 127.0.0.1 = unlimited host
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert_eq!(r.status, 200);
    }
}
