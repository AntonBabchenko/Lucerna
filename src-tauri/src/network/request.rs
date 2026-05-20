//! Lower-level chokepoint primitives: a GET or POST with caller-supplied
//! request headers. Returns the response for ANY received HTTP status;
//! `Err` only on a transport-level failure (no status received). The
//! caller owns status-to-error mapping and body decoding.
//!
//! Every attempt is recorded in the audit log, exactly like the higher
//! `get_json` / `get_text` / `get_bytes` helpers (which are now thin
//! wrappers over this). `download_with_sha` stays separate — it streams
//! to disk rather than buffering the body.

use crate::error::{Error, Result};
use crate::network::audit::{now_ms, record, AuditEntry};
use crate::network::client::http;

/// A received HTTP response: the status code and the fully-buffered body.
#[derive(Debug)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

/// Send a built request, record an `AuditEntry`, return the response
/// for any HTTP status. `Err` only on a transport-level failure
/// (send failure, or a body-read failure after a status was received).
/// `method` is the audit-log label ("GET" / "POST").
async fn send(
    req: reqwest::RequestBuilder,
    method: &str,
    url: &str,
    initiator: &str,
) -> Result<HttpResponse> {
    let resp = req.send().await.map_err(|e| {
        record(AuditEntry {
            ts: now_ms(),
            method: method.into(),
            url: url.into(),
            initiator: initiator.into(),
            bytes: None,
            status: None,
        });
        Error::network(url, e)
    })?;
    let status = resp.status().as_u16();
    // A body-read failure (mid-body drop, read timeout) is still a transport-level
    // failure and must be logged, with the status we already received.
    let body = resp.bytes().await.map_err(|e| {
        record(AuditEntry {
            ts: now_ms(),
            method: method.into(),
            url: url.into(),
            initiator: initiator.into(),
            bytes: None,
            status: Some(status),
        });
        Error::network(url, e)
    })?;
    record(AuditEntry {
        ts: now_ms(),
        method: method.into(),
        url: url.into(),
        initiator: initiator.into(),
        bytes: Some(body.len() as f64),
        status: Some(status),
    });
    Ok(HttpResponse {
        status,
        body: body.to_vec(),
    })
}

/// GET `url` on the shared chokepoint client with `headers` applied.
///
/// Records one `AuditEntry` for the attempt: on transport failure with
/// `status: None`, otherwise with the received status and body length.
/// Returns `Ok(HttpResponse)` for any received HTTP status (2xx or not);
/// returns `Err(Error::Network)` only when no status was received.
///
/// `initiator` is the module name shown in the Network Activity panel.
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
/// applied. See `get` for the audit + error contract — `post` is
/// identical except for the HTTP method and the request body.
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
    use crate::network::audit::recent;
    use wiremock::matchers::{body_string, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn returns_ok_for_200_with_body() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/req-200"))
            .respond_with(ResponseTemplate::new(200).set_body_string("hello"))
            .mount(&server)
            .await;
        let url = format!("{}/req-200", server.uri());
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
        let r = get(&url, &[("x-api-key", "secret-123")], "test").await;
        assert!(r.is_ok(), "header should have matched the mock: {r:?}");
    }

    #[tokio::test]
    async fn transport_failure_is_err() {
        // Port 1 is unreachable — the connection fails before any status.
        let r = get("http://127.0.0.1:1/nope", &[], "test").await;
        assert!(matches!(r, Err(Error::Network { .. })), "got: {r:?}");
    }

    #[tokio::test]
    async fn records_an_audit_entry_for_the_call() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/req-audit-probe"))
            .respond_with(ResponseTemplate::new(200).set_body_string("x"))
            .mount(&server)
            .await;
        let url = format!("{}/req-audit-probe", server.uri());
        get(&url, &[], "audit-test").await.unwrap();
        // The audit ring buffer is process-global; match on the unique
        // path so parallel tests recording other entries don't interfere.
        let logged = recent()
            .into_iter()
            .find(|e| e.url.contains("/req-audit-probe"));
        let logged = logged.expect("expected an audit entry for the call");
        assert_eq!(logged.status, Some(200));
        assert_eq!(logged.initiator, "audit-test");
        assert_eq!(logged.bytes, Some(1.0));
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
        let r = post(&url, &[("x-api-key", "k-9")], b"{}", "test").await;
        assert!(r.is_ok(), "header should have matched the mock: {r:?}");
    }

    #[tokio::test]
    async fn post_records_audit_entry_with_post_method() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/req-post-audit-probe"))
            .respond_with(ResponseTemplate::new(200).set_body_string("x"))
            .mount(&server)
            .await;
        let url = format!("{}/req-post-audit-probe", server.uri());
        post(&url, &[], b"{}", "post-audit-test").await.unwrap();
        let logged = crate::network::audit::recent()
            .into_iter()
            .find(|e| e.url.contains("/req-post-audit-probe"))
            .expect("expected an audit entry for the POST call");
        assert_eq!(logged.method, "POST");
        assert_eq!(logged.status, Some(200));
    }
}
