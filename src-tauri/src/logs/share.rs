//! Log-sharing surface: anonymisation + mclo.gs upload.
//!
//! `anonymise()` is a best-effort regex pipeline that strips identifying or
//! sensitive substrings from a log body before it leaves the launcher. The
//! Share UI warns the user to double-check the body before sharing.
//!
//! `upload_to_mclogs()` POSTs the (optionally anonymised) log body to
//! `api.mclo.gs/1/log` via the `crate::network::request` chokepoint and
//! returns the public paste URL on success.

use crate::error::{Error, Result};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;

// ---------------------------------------------------------------------------
// Anonymisation
// ---------------------------------------------------------------------------

/// Strip identifying / sensitive substrings from a log body before
/// uploading it to a third-party paste service. Best-effort: regex-
/// based, no semantic understanding. The Share UI warns the user to
/// double-check the body before sharing.
pub fn anonymise(input: &str) -> String {
    static WIN_USER_PATH: Lazy<Regex> =
        Lazy::new(|| Regex::new(r#"(?i)([A-Z]:\\Users\\)([^\\/:*?"<>|]+)(\\)"#).unwrap());
    static WIN_USER_PATH_FWD: Lazy<Regex> =
        Lazy::new(|| Regex::new(r#"(?i)([A-Z]:/Users/)([^/:*?"<>|]+)(/)"#).unwrap());
    static MAC_USER_PATH: Lazy<Regex> = Lazy::new(|| Regex::new(r"(/Users/)([^/]+)(/)").unwrap());
    static SETTING_USER_TOKEN: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?i)(Setting user:\s+\S+\s+)([A-Za-z0-9]{30,})").unwrap());
    static ACCESS_TOKEN_FLAG: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(--accessToken\s+)([A-Za-z0-9]{30,})").unwrap());
    static SESSION_PARAM: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?i)(sessionId=)([A-Za-z0-9\-]+)").unwrap());
    static LAN_IP: Lazy<Regex> = Lazy::new(|| {
        Regex::new(concat!(
            r"\b(?:",
            r"192\.168\.\d{1,3}\.\d{1,3}",
            r"|10\.\d{1,3}\.\d{1,3}\.\d{1,3}",
            r"|172\.(?:1[6-9]|2\d|3[0-1])\.\d{1,3}\.\d{1,3}",
            r")\b",
        ))
        .unwrap()
    });

    let mut out = input.to_string();
    out = WIN_USER_PATH.replace_all(&out, r"$1<user>$3").into_owned();
    out = WIN_USER_PATH_FWD
        .replace_all(&out, "$1<user>$3")
        .into_owned();
    out = MAC_USER_PATH.replace_all(&out, "$1<user>$3").into_owned();
    out = SETTING_USER_TOKEN
        .replace_all(&out, "$1<redacted>")
        .into_owned();
    out = ACCESS_TOKEN_FLAG
        .replace_all(&out, "$1<redacted>")
        .into_owned();
    out = SESSION_PARAM.replace_all(&out, "$1<redacted>").into_owned();
    out = LAN_IP.replace_all(&out, "<lan-ip>").into_owned();
    out
}

// ---------------------------------------------------------------------------
// mclo.gs upload
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct McLogsResponse {
    success: bool,
    url: Option<String>,
    error: Option<String>,
}

/// Upload `content` to `api.mclo.gs` and return the public paste URL.
pub async fn upload_to_mclogs(content: &str) -> Result<String> {
    upload_to_mclogs_at("https://api.mclo.gs", content).await
}

/// Test-injectable variant — accepts an arbitrary base URL so wiremock
/// servers can intercept the request.
pub async fn upload_to_mclogs_at(base: &str, content: &str) -> Result<String> {
    let body = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("content", content)
        .finish();
    let endpoint = format!("{base}/1/log");
    let resp = crate::network::request::post(
        &endpoint,
        &[("content-type", "application/x-www-form-urlencoded")],
        body.as_bytes(),
        "logs-share",
    )
    .await
    .map_err(|e| Error::McLogsUpload {
        details: format!("transport: {e}"),
    })?;
    if !(200..300).contains(&resp.status) {
        return Err(Error::McLogsUpload {
            details: format!(
                "HTTP {}: {}",
                resp.status,
                String::from_utf8_lossy(&resp.body)
            ),
        });
    }
    let parsed: McLogsResponse =
        serde_json::from_slice(&resp.body).map_err(|e| Error::McLogsUpload {
            details: format!("decode: {e}"),
        })?;
    if !parsed.success {
        return Err(Error::McLogsUpload {
            details: parsed.error.unwrap_or_else(|| "unknown error".into()),
        });
    }
    parsed.url.ok_or_else(|| Error::McLogsUpload {
        details: "response missing url field".into(),
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::test_env_lock()
    }

    #[test]
    fn anonymise_strips_windows_user_path_basic() {
        let input = r"at file:/***REMOVED***/AppData/Roaming/FTlauncher/something";
        assert!(!anonymise(input).contains("Anton"));
        assert!(anonymise(input).contains("<user>"));
    }

    #[test]
    fn anonymise_strips_windows_user_path_backslashes() {
        let input = r"***REMOVED***\AppData\Roaming";
        let out = anonymise(input);
        assert!(!out.contains("Anton"));
        assert!(out.contains(r"C:\Users\<user>\"));
    }

    #[test]
    fn anonymise_strips_macos_user_path() {
        let input = "/Users/anton/Library/Application Support/minecraft";
        let out = anonymise(input);
        assert!(!out.contains("/anton/"));
        assert!(out.contains("/Users/<user>/"));
    }

    #[test]
    fn anonymise_strips_access_token_via_setting_user() {
        let input =
            "[main/INFO]: Setting user: Tester aaaaBBBBccccDDDDeeeeFFFFgggg1111HHHH2222iiii";
        let out = anonymise(input);
        assert!(!out.contains("aaaaBBBBccccDDDDeeeeFFFFgggg1111HHHH2222iiii"));
        assert!(out.contains("<redacted>"));
    }

    #[test]
    fn anonymise_strips_access_token_flag() {
        let input = "--accessToken aaaaBBBBccccDDDDeeeeFFFFgggg1111HHHH2222iiii --uuid foo";
        let out = anonymise(input);
        assert!(!out.contains("aaaaBBBBccccDDDDeeeeFFFFgggg1111HHHH2222iiii"));
    }

    #[test]
    fn anonymise_strips_session_uuid_param() {
        let input = "GET https://session.example/play?sessionId=12345678-abcd-1234-abcd-1234567890ab HTTP/1.1";
        let out = anonymise(input);
        assert!(!out.contains("12345678-abcd-1234-abcd-1234567890ab"));
        assert!(out.contains("sessionId=<redacted>"));
    }

    #[test]
    fn anonymise_strips_lan_ip_192() {
        assert!(anonymise("Connecting to 192.168.1.42:25565").contains("<lan-ip>"));
    }

    #[test]
    fn anonymise_strips_lan_ip_10() {
        assert!(anonymise("from 10.0.5.23 port 25565").contains("<lan-ip>"));
    }

    #[test]
    fn anonymise_strips_lan_ip_172_in_private_range() {
        assert!(anonymise("got 172.16.0.1 packet").contains("<lan-ip>"));
        assert!(anonymise("got 172.31.255.254 packet").contains("<lan-ip>"));
    }

    #[test]
    fn anonymise_keeps_public_ip() {
        let s = anonymise("Connecting to 123.45.67.89:25565");
        assert!(s.contains("123.45.67.89"));
        assert!(!s.contains("<lan-ip>"));
    }

    #[test]
    fn anonymise_keeps_172_15_not_in_private_range() {
        // 172.15.x.x is NOT private (private is 172.16-31).
        assert!(anonymise("from 172.15.0.1 packet").contains("172.15.0.1"));
    }

    #[tokio::test]
    async fn upload_returns_url_on_success() {
        let _g = test_lock();
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/1/log"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "id": "abcdef",
                "url": "https://mclo.gs/abcdef",
                "raw": "https://api.mclo.gs/1/raw/abcdef"
            })))
            .mount(&server)
            .await;
        std::env::set_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let url = upload_to_mclogs_at(&server.uri(), "test content")
            .await
            .expect("upload ok");
        std::env::remove_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS");
        assert_eq!(url, "https://mclo.gs/abcdef");
    }

    #[tokio::test]
    async fn upload_returns_error_on_4xx() {
        let _g = test_lock();
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/1/log"))
            .respond_with(ResponseTemplate::new(413).set_body_string("Log too large"))
            .mount(&server)
            .await;
        std::env::set_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let err = upload_to_mclogs_at(&server.uri(), "test")
            .await
            .unwrap_err();
        std::env::remove_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS");
        assert!(matches!(err, crate::error::Error::McLogsUpload { .. }));
    }

    #[tokio::test]
    async fn upload_returns_error_on_success_false() {
        let _g = test_lock();
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/1/log"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": false,
                "error": "Log too large"
            })))
            .mount(&server)
            .await;
        std::env::set_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let err = upload_to_mclogs_at(&server.uri(), "test")
            .await
            .unwrap_err();
        std::env::remove_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS");
        assert!(matches!(err, crate::error::Error::McLogsUpload { .. }));
    }
}
