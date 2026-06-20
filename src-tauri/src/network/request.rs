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

/// Maximum number of automatic retries after an initial 429 (so up to
/// `MAX_RETRIES + 1` total sends).
const MAX_RETRIES: u32 = 3;

/// Upper bound on the wait between 429 retries, regardless of what the
/// `X-Ratelimit-Reset` header asks for or how far the exponential backoff has
/// climbed.
const MAX_BACKOFF: std::time::Duration = std::time::Duration::from_secs(60);

/// Seconds to wait after a 429: `X-Ratelimit-Reset` (Modrinth) if present and
/// sane, else exponential backoff (1s, 2s, 4s) by attempt. Capped at MAX_BACKOFF.
fn backoff_after_429(headers: &reqwest::header::HeaderMap, attempt: u32) -> std::time::Duration {
    let from_header = headers
        .get("x-ratelimit-reset")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(std::time::Duration::from_secs);
    let dur = from_header.unwrap_or_else(|| std::time::Duration::from_secs(1u64 << attempt.min(6)));
    dur.min(MAX_BACKOFF)
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
    let host = host_of(url);
    // `RequestBuilder` is consumed by `.send()`, so to retry on a 429 we hold
    // the builder in an `Option` and `try_clone()` it for non-final attempts,
    // moving the original out on the final attempt (or when it can't be cloned).
    let mut req = Some(req);
    for attempt in 0..=MAX_RETRIES {
        // Throttle: pace per host, with the current task's priority. Re-paced
        // per attempt so a retry takes a fresh token rather than reusing one.
        if let Some(h) = &host {
            crate::network::throttle::gate_for(h)
                .acquire(crate::network::throttle::current_priority())
                .await;
        }
        // Take a builder for this attempt: clone for non-final attempts so we
        // can retry; move the original on the final attempt (or if not cloneable).
        let this = if attempt < MAX_RETRIES {
            match req.as_ref().and_then(|r| r.try_clone()) {
                Some(c) => c,
                None => match req.take() {
                    Some(r) => r,
                    None => break,
                },
            }
        } else {
            match req.take() {
                Some(r) => r,
                None => break,
            }
        };
        let resp = this.send().await.map_err(|e| {
            crate::diag!(
                "network: {initiator} {method} {url} — request failed (source unreachable?): {e}"
            );
            Error::network(url, e)
        })?;
        let status = resp.status().as_u16();
        // On a 429, freeze the host gate and back off before retrying — but only
        // while we still have a builder for the next attempt. `X-Ratelimit-Reset`
        // (Modrinth) drives the wait; otherwise exponential backoff applies.
        if status == 429 && attempt < MAX_RETRIES && req.is_some() {
            let wait = backoff_after_429(resp.headers(), attempt);
            if let Some(h) = &host {
                crate::network::throttle::gate_for(h).freeze(wait);
            }
            crate::diag!(
                "network: {initiator} {method} {url} — HTTP 429, retry in {:?}",
                wait
            );
            tokio::time::sleep(wait).await;
            continue;
        }
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
        return Ok(HttpResponse {
            status,
            body: body.to_vec(),
        });
    }
    // Unreachable in practice: the final attempt always takes `req` and returns
    // (success path) or propagates a transport error. Surface a clear error if
    // the loop ever exits without producing a response.
    Err(Error::network(
        url,
        "retry loop exhausted without a response",
    ))
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
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/req-200"))
            .respond_with(ResponseTemplate::new(200).set_body_string("hello"))
            .mount(&server)
            .await;
        let url = format!("{}/req-200", server.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let r = get(&url, &[], "test").await.unwrap();
        assert_eq!(r.status, 200);
        assert_eq!(r.body, b"hello");
    }

    #[tokio::test]
    async fn returns_ok_for_404_not_err() {
        // A non-2xx status is NOT an error at this layer — the caller
        // owns status-to-error mapping.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/req-404"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let url = format!("{}/req-404", server.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let r = get(&url, &[], "test").await.unwrap();
        assert_eq!(r.status, 404);
    }

    #[tokio::test]
    async fn applies_request_headers() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/req-hdr"))
            .and(header("x-api-key", "secret-123"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        let url = format!("{}/req-hdr", server.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let r = get(&url, &[("x-api-key", "secret-123")], "test").await;
        assert!(r.is_ok(), "header should have matched the mock: {r:?}");
    }

    #[tokio::test]
    async fn transport_failure_is_err() {
        // Port 1 is unreachable — the connection fails before any status.
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let r = get("http://127.0.0.1:1/nope", &[], "test").await;
        assert!(matches!(r, Err(Error::Network { .. })), "got: {r:?}");
    }

    #[tokio::test]
    async fn post_sends_body_and_returns_ok() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/req-post"))
            .and(body_string("{\"k\":1}"))
            .respond_with(ResponseTemplate::new(200).set_body_string("done"))
            .mount(&server)
            .await;
        let url = format!("{}/req-post", server.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let r = post(&url, &[], b"{\"k\":1}", "test").await.unwrap();
        assert_eq!(r.status, 200);
        assert_eq!(r.body, b"done");
    }

    #[tokio::test]
    async fn post_applies_headers() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/req-post-hdr"))
            .and(header("x-api-key", "k-9"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        let url = format!("{}/req-post-hdr", server.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let r = post(&url, &[("x-api-key", "k-9")], b"{}", "test").await;
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
    async fn retries_on_429_then_succeeds() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rl"))
            .respond_with(ResponseTemplate::new(429).insert_header("x-ratelimit-reset", "0"))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/rl"))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .mount(&server)
            .await;
        let url = format!("{}/rl", server.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let r = get(&url, &[], "test").await.unwrap();
        assert_eq!(r.status, 200, "should have retried past the 429");
    }

    #[test]
    fn backoff_prefers_reset_header_then_falls_back() {
        use reqwest::header::HeaderMap;
        let mut h = HeaderMap::new();
        h.insert("x-ratelimit-reset", "3".parse().unwrap());
        assert_eq!(backoff_after_429(&h, 0), std::time::Duration::from_secs(3));
        let empty = HeaderMap::new();
        assert_eq!(
            backoff_after_429(&empty, 0),
            std::time::Duration::from_secs(1)
        );
        assert_eq!(
            backoff_after_429(&empty, 2),
            std::time::Duration::from_secs(4)
        );
        let mut big = HeaderMap::new();
        big.insert("x-ratelimit-reset", "9999".parse().unwrap());
        assert_eq!(backoff_after_429(&big, 0), MAX_BACKOFF);
    }

    #[tokio::test]
    async fn sustained_429_gives_up_and_returns_429() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/always"))
            .respond_with(ResponseTemplate::new(429).insert_header("x-ratelimit-reset", "0"))
            .mount(&server)
            .await;
        let url = format!("{}/always", server.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let r = get(&url, &[], "test").await.unwrap();
        assert_eq!(
            r.status, 429,
            "sustained 429 must return 429, not hang/loop"
        );
    }

    #[tokio::test]
    async fn get_still_succeeds_through_throttle() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/t"))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .mount(&server)
            .await;
        let url = format!("{}/t", server.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let r = get(&url, &[], "test").await.unwrap(); // 127.0.0.1 = unlimited host
        assert_eq!(r.status, 200);
    }
}
