//! Xbox Live + XSTS authentication.

use crate::error::{Error, Result};
use serde::Deserialize;
use serde_json::json;

pub const XBL_DEFAULT: &str = "https://user.auth.xboxlive.com/user/authenticate";
pub const XSTS_DEFAULT: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";

pub fn xbl_url() -> String {
    std::env::var("LUCERNA_XBL_URL_OVERRIDE").unwrap_or_else(|_| XBL_DEFAULT.to_string())
}
pub fn xsts_url() -> String {
    std::env::var("LUCERNA_XSTS_URL_OVERRIDE").unwrap_or_else(|_| XSTS_DEFAULT.to_string())
}

#[derive(Debug, Deserialize)]
struct XblResponse {
    #[serde(rename = "Token")]
    token: String,
    #[serde(rename = "DisplayClaims")]
    display_claims: XblDisplayClaims,
}

#[derive(Debug, Deserialize)]
struct XblDisplayClaims {
    xui: Vec<XblXui>,
}

#[derive(Debug, Deserialize)]
struct XblXui {
    uhs: String,
}

#[derive(Debug, Deserialize)]
struct XstsErrorResponse {
    #[serde(rename = "XErr", default)]
    x_err: u64,
}

#[derive(Debug, Clone)]
pub struct XstsResult {
    pub token: String,
    pub userhash: String,
}

pub async fn xbl_authenticate(ms_access_token: &str) -> Result<(String, String)> {
    let body = json!({
        "Properties": {
            "AuthMethod": "RPS",
            "SiteName": "user.auth.xboxlive.com",
            "RpsTicket": format!("d={ms_access_token}"),
        },
        "RelyingParty": "http://auth.xboxlive.com",
        "TokenType": "JWT"
    });
    let url = xbl_url();
    crate::network::allowlist::check_url_allowed(&url, "microsoft_auth")?;
    let resp = crate::network::http()
        .post(&url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| Error::network(&url, e))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(Error::AuthFailed {
            stage: "xbox_live".into(),
            details: format!("HTTP {status}: {text}"),
        });
    }
    let parsed: XblResponse = resp.json().await.map_err(|e| Error::AuthFailed {
        stage: "xbox_live".into(),
        details: format!("parse: {e}"),
    })?;
    let userhash = parsed
        .display_claims
        .xui
        .into_iter()
        .next()
        .ok_or_else(|| Error::AuthFailed {
            stage: "xbox_live".into(),
            details: "missing DisplayClaims.xui[0]".into(),
        })?
        .uhs;
    Ok((parsed.token, userhash))
}

pub async fn xsts_authorize(xbl_token: &str) -> Result<XstsResult> {
    let body = json!({
        "Properties": {
            "SandboxId": "RETAIL",
            "UserTokens": [xbl_token],
        },
        "RelyingParty": "rp://api.minecraftservices.com/",
        "TokenType": "JWT"
    });
    let url = xsts_url();
    crate::network::allowlist::check_url_allowed(&url, "microsoft_auth")?;
    let resp = crate::network::http()
        .post(&url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| Error::network(&url, e))?;

    let status = resp.status();
    if status.as_u16() == 401 {
        let text = resp.text().await.unwrap_or_default();
        let x_err = serde_json::from_str::<XstsErrorResponse>(&text)
            .map(|x| x.x_err)
            .unwrap_or(0);
        let details = match x_err {
            2_148_916_233 => "no_xbox_account".into(),
            2_148_916_238 => "child_account".into(),
            other if other != 0 => format!("xerr_{other}"),
            _ => format!("HTTP 401: {text}"),
        };
        return Err(Error::AuthFailed {
            stage: "xsts".into(),
            details,
        });
    }
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(Error::AuthFailed {
            stage: "xsts".into(),
            details: format!("HTTP {status}: {text}"),
        });
    }

    let parsed: XblResponse = resp.json().await.map_err(|e| Error::AuthFailed {
        stage: "xsts".into(),
        details: format!("parse: {e}"),
    })?;
    let userhash = parsed
        .display_claims
        .xui
        .into_iter()
        .next()
        .ok_or_else(|| Error::AuthFailed {
            stage: "xsts".into(),
            details: "missing DisplayClaims.xui[0]".into(),
        })?
        .uhs;
    Ok(XstsResult {
        token: parsed.token,
        userhash,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const XBL_FIXTURE: &str = r#"{
      "Token": "xbl-token-abc",
      "DisplayClaims": { "xui": [ { "uhs": "userhash-xyz" } ] }
    }"#;

    const XSTS_FIXTURE: &str = r#"{
      "Token": "xsts-token-abc",
      "DisplayClaims": { "xui": [ { "uhs": "userhash-xsts" } ] }
    }"#;

    const XSTS_CHILD_ERROR: &str = r#"{"XErr": 2148916238, "Message": "child"}"#;
    const XSTS_NO_XBOX_ERROR: &str = r#"{"XErr": 2148916233, "Message": "no_xbox"}"#;

    #[tokio::test]
    async fn xbl_authenticate_happy_path() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/user/authenticate"))
            .respond_with(ResponseTemplate::new(200).set_body_string(XBL_FIXTURE))
            .mount(&server)
            .await;
        let _guard = super::super::env_lock();
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        std::env::set_var(
            "LUCERNA_XBL_URL_OVERRIDE",
            format!("{}/user/authenticate", server.uri()),
        );

        let (token, uhs) = xbl_authenticate("ms-access-tok").await.unwrap();
        assert_eq!(token, "xbl-token-abc");
        assert_eq!(uhs, "userhash-xyz");

        std::env::remove_var("LUCERNA_XBL_URL_OVERRIDE");
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
    }

    #[tokio::test]
    async fn xsts_authorize_happy_path() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/xsts/authorize"))
            .respond_with(ResponseTemplate::new(200).set_body_string(XSTS_FIXTURE))
            .mount(&server)
            .await;
        let _guard = super::super::env_lock();
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        std::env::set_var(
            "LUCERNA_XSTS_URL_OVERRIDE",
            format!("{}/xsts/authorize", server.uri()),
        );

        let result = xsts_authorize("xbl-tok").await.unwrap();
        assert_eq!(result.token, "xsts-token-abc");
        assert_eq!(result.userhash, "userhash-xsts");

        std::env::remove_var("LUCERNA_XSTS_URL_OVERRIDE");
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
    }

    #[tokio::test]
    async fn xsts_authorize_child_account_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/xsts/authorize"))
            .respond_with(ResponseTemplate::new(401).set_body_string(XSTS_CHILD_ERROR))
            .mount(&server)
            .await;
        let _guard = super::super::env_lock();
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        std::env::set_var(
            "LUCERNA_XSTS_URL_OVERRIDE",
            format!("{}/xsts/authorize", server.uri()),
        );

        let result = xsts_authorize("xbl-tok").await;
        match result {
            Err(Error::AuthFailed { stage, details }) => {
                assert_eq!(stage, "xsts");
                assert_eq!(details, "child_account");
            }
            other => panic!("expected child_account AuthFailed, got {other:?}"),
        }

        std::env::remove_var("LUCERNA_XSTS_URL_OVERRIDE");
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
    }

    #[tokio::test]
    async fn xsts_authorize_no_xbox_account_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/xsts/authorize"))
            .respond_with(ResponseTemplate::new(401).set_body_string(XSTS_NO_XBOX_ERROR))
            .mount(&server)
            .await;
        let _guard = super::super::env_lock();
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        std::env::set_var(
            "LUCERNA_XSTS_URL_OVERRIDE",
            format!("{}/xsts/authorize", server.uri()),
        );

        let result = xsts_authorize("xbl-tok").await;
        match result {
            Err(Error::AuthFailed { stage, details }) => {
                assert_eq!(stage, "xsts");
                assert_eq!(details, "no_xbox_account");
            }
            other => panic!("expected no_xbox_account, got {other:?}"),
        }

        std::env::remove_var("LUCERNA_XSTS_URL_OVERRIDE");
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
    }
}
