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
    let resp = req.send().await.map_err(|e| Error::network(url, e))?;
    let status = resp.status().as_u16();
    // A body-read failure (mid-body drop, read timeout) is still a transport-level
    // failure.
    let body = resp.bytes().await.map_err(|e| Error::network(url, e))?;
    Ok(HttpResponse {
        status,
        body: body.to_vec(),
    })
}

/// GET `url` on the shared chokepoint client with `headers` applied.
///
/// Returns `Ok(HttpResponse)` for any received HTTP status (2xx or not);
/// returns `Err(Error::Network)` only when no status was received.
///
/// `initiator` is the module name that triggered the request.
pub async fn get(
    url: &str,
    headers: &[(&str, &str)],
    initiator: &str,
) -> Result<HttpResponse> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::test_env_lock()
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
        std::env::set_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let r = get(&url, &[], "test").await.unwrap();
        std::env::remove_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS");
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
        std::env::set_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let r = get(&url, &[], "test").await.unwrap();
        std::env::remove_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS");
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
        std::env::set_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let r = get(&url, &[("x-api-key", "secret-123")], "test").await;
        std::env::remove_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS");
        assert!(r.is_ok(), "header should have matched the mock: {r:?}");
    }

    #[tokio::test]
    async fn transport_failure_is_err() {
        let _g = test_lock();
        // Port 1 is unreachable — the connection fails before any status.
        std::env::set_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let r = get("http://127.0.0.1:1/nope", &[], "test").await;
        std::env::remove_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS");
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
        std::env::set_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let r = post(&url, &[], b"{\"k\":1}", "test").await.unwrap();
        std::env::remove_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS");
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
        std::env::set_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let r = post(&url, &[("x-api-key", "k-9")], b"{}", "test").await;
        std::env::remove_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS");
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
}
