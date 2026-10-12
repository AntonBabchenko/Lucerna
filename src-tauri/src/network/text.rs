//! `get_text(url, initiator)` — raw text fetch through the single
//! chokepoint. A thin wrapper over `network::request::get`. Returns
//! the body as a UTF-8 `String`.

use crate::error::{Error, Result};
use crate::network::request;

pub async fn get_text(url: &str, initiator: &str) -> Result<String> {
    let resp = request::get(url, &[], initiator).await?;
    if !(200..300).contains(&resp.status) {
        return Err(Error::http_status(url, resp.status));
    }
    String::from_utf8(resp.body).map_err(|e| {
        crate::network::failure::bad_response(initiator, url, format!("not valid UTF-8: {e}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::NetworkFailure;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn a_server_error_is_a_typed_status_not_a_connection_failure() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/text-503"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        let url = format!("{}/text-503", server.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let r = get_text(&url, "test").await;
        assert!(
            matches!(
                r,
                Err(Error::Network {
                    failure: NetworkFailure::HttpStatus { status: 503 },
                    ..
                })
            ),
            "got: {r:?}"
        );
    }
}
