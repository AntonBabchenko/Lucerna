//! `get_json::<T>(url, initiator)` — typed JSON fetch through the
//! single chokepoint. A thin wrapper over `network::request::get`:
//! the primitive does the send, this layer maps a non-2xx
//! status to `Error::http_status` and deserialises the body.

use crate::error::{Error, Result};
use crate::network::request;
use serde::de::DeserializeOwned;

pub async fn get_json<T: DeserializeOwned>(url: &str, initiator: &str) -> Result<T> {
    let resp = request::get(url, &[], initiator).await?;
    if !(200..300).contains(&resp.status) {
        return Err(Error::http_status(url, resp.status));
    }
    serde_json::from_slice::<T>(&resp.body)
        .map_err(|e| crate::network::failure::bad_response(initiator, url, format!("parse: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::NetworkFailure;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn an_unreadable_answer_is_a_bad_response_not_a_connection_failure() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/json-garbage"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>not json</html>"))
            .mount(&server)
            .await;
        let url = format!("{}/json-garbage", server.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let r = get_json::<serde_json::Value>(&url, "test").await;
        assert!(
            matches!(
                r,
                Err(Error::Network {
                    failure: NetworkFailure::BadResponse,
                    ..
                })
            ),
            "got: {r:?}"
        );
    }
}
