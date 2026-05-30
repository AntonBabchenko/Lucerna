//! Minecraft services API — exchange XSTS for an MC access_token,
//! then fetch the user's MC profile.

use crate::error::{Error, Result};
use serde::Deserialize;
use serde_json::json;

pub const LOGIN_DEFAULT: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";
pub const PROFILE_DEFAULT: &str = "https://api.minecraftservices.com/minecraft/profile";

pub fn login_url() -> String {
    std::env::var("LUCERNA_MC_LOGIN_URL_OVERRIDE").unwrap_or_else(|_| LOGIN_DEFAULT.to_string())
}
pub fn profile_url() -> String {
    std::env::var("LUCERNA_MC_PROFILE_URL_OVERRIDE")
        .unwrap_or_else(|_| PROFILE_DEFAULT.to_string())
}

#[derive(Debug, Deserialize)]
pub struct LoginResponse {
    pub access_token: String,
    pub expires_in: u64,
}

#[derive(Debug, Deserialize)]
pub struct ProfileResponse {
    /// MC UUID without dashes — we hyphenate it before storing in account.json.
    pub id: String,
    pub name: String,
}

pub async fn login_with_xbox(userhash: &str, xsts_token: &str) -> Result<LoginResponse> {
    let identity_token = format!("XBL3.0 x={userhash};{xsts_token}");
    let body = json!({ "identityToken": identity_token });
    let url = login_url();
    crate::network::allowlist::check_url_allowed(&url, "microsoft_auth")?;
    let resp = crate::network::http()
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| Error::network(&url, e))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        // Microsoft returns 403 "Invalid app registration" until the
        // Azure app registration is approved. Map this specific case to
        // a typed pending variant so the UI can surface it as a friendly
        // "approval pending" message rather than a raw HTTP error. When
        // MS approves, the response becomes 200 and this branch simply
        // stops firing — no other code needs to change.
        //
        // The exact substring "Invalid app registration" was observed from
        // real api.minecraftservices.com responses on 2026-05-14 with the
        // Lucerna Azure app in approval-pending state. If Microsoft
        // changes the wording, the live integration silently degrades to
        // generic AuthFailed (safe direction); automated tests do NOT catch
        // this drift because mocks control both sides. Manual integration
        // testing is the guard — keep the substring and the mock body in
        // sync, and update both when MS changes the response text.
        if status.as_u16() == 403 && text.contains("Invalid app registration") {
            return Err(Error::AuthPendingApproval);
        }
        return Err(Error::AuthFailed {
            stage: "mc_services".into(),
            details: format!("HTTP {status}: {text}"),
        });
    }
    resp.json::<LoginResponse>()
        .await
        .map_err(|e| Error::AuthFailed {
            stage: "mc_services".into(),
            details: format!("parse: {e}"),
        })
}

pub async fn fetch_profile(mc_access_token: &str) -> Result<ProfileResponse> {
    let url = profile_url();
    crate::network::allowlist::check_url_allowed(&url, "microsoft_auth")?;
    let resp = crate::network::http()
        .get(&url)
        .header("Authorization", format!("Bearer {mc_access_token}"))
        .send()
        .await
        .map_err(|e| Error::network(&url, e))?;
    let status = resp.status();
    if status.as_u16() == 404 {
        return Err(Error::NoMinecraftProfile);
    }
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(Error::AuthFailed {
            stage: "mc_services".into(),
            details: format!("HTTP {status}: {text}"),
        });
    }
    resp.json::<ProfileResponse>()
        .await
        .map_err(|e| Error::AuthFailed {
            stage: "mc_services".into(),
            details: format!("parse: {e}"),
        })
}

/// Convert MC UUID (32 hex chars, no dashes) to canonical hyphenated form.
pub fn hyphenate_uuid(no_dashes: &str) -> Result<String> {
    if no_dashes.len() != 32 {
        return Err(Error::AuthFailed {
            stage: "mc_services".into(),
            details: format!("profile.id wrong length: {no_dashes}"),
        });
    }
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &no_dashes[0..8],
        &no_dashes[8..12],
        &no_dashes[12..16],
        &no_dashes[16..20],
        &no_dashes[20..32],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn hyphenate_uuid_canonical_form() {
        assert_eq!(
            hyphenate_uuid("7e8d9c0a123456789abcdef012345678").unwrap(),
            "7e8d9c0a-1234-5678-9abc-def012345678"
        );
    }

    #[test]
    fn hyphenate_uuid_rejects_wrong_length() {
        assert!(hyphenate_uuid("tooshort").is_err());
    }

    #[tokio::test]
    async fn login_with_xbox_happy_path() {
        let _guard = super::super::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/authentication/login_with_xbox"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(r#"{"access_token":"mc-tok","expires_in":86400}"#),
            )
            .mount(&server)
            .await;
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        std::env::set_var(
            "LUCERNA_MC_LOGIN_URL_OVERRIDE",
            format!("{}/authentication/login_with_xbox", server.uri()),
        );

        let r = login_with_xbox("uhs", "xsts-tok").await.unwrap();
        assert_eq!(r.access_token, "mc-tok");
        assert_eq!(r.expires_in, 86_400);

        std::env::remove_var("LUCERNA_MC_LOGIN_URL_OVERRIDE");
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
    }

    #[tokio::test]
    async fn fetch_profile_happy_path_no_dashes_id() {
        let _guard = super::super::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/minecraft/profile"))
            .and(header("Authorization", "Bearer mc-tok"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(
                    r#"{"id":"7e8d9c0a123456789abcdef012345678","name":"AntonMC"}"#,
                ),
            )
            .mount(&server)
            .await;
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        std::env::set_var(
            "LUCERNA_MC_PROFILE_URL_OVERRIDE",
            format!("{}/minecraft/profile", server.uri()),
        );

        let profile = fetch_profile("mc-tok").await.unwrap();
        assert_eq!(profile.id, "7e8d9c0a123456789abcdef012345678");
        assert_eq!(profile.name, "AntonMC");

        std::env::remove_var("LUCERNA_MC_PROFILE_URL_OVERRIDE");
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
    }

    #[tokio::test]
    async fn fetch_profile_404_no_minecraft_profile() {
        let _guard = super::super::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/minecraft/profile"))
            .respond_with(ResponseTemplate::new(404).set_body_string("{}"))
            .mount(&server)
            .await;
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        std::env::set_var(
            "LUCERNA_MC_PROFILE_URL_OVERRIDE",
            format!("{}/minecraft/profile", server.uri()),
        );

        let result = fetch_profile("mc-tok").await;
        assert!(matches!(result, Err(Error::NoMinecraftProfile)));

        std::env::remove_var("LUCERNA_MC_PROFILE_URL_OVERRIDE");
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
    }
}

#[cfg(test)]
mod variant_a_tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn login_with_xbox_403_invalid_app_registration_maps_to_pending_approval() {
        let _guard = super::super::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/authentication/login_with_xbox"))
            .respond_with(
                ResponseTemplate::new(403).set_body_string(
                    // Body observed from api.minecraftservices.com on 2026-05-14
                    // (per project_msauth_deferred memory). If MS changes the wording,
                    // the live integration silently degrades to AuthFailed; update both
                    // this mock body AND the substring in login_with_xbox at the same
                    // time. The mock alone will not catch wording drift.
                    r#"{"error":"FORBIDDEN","errorMessage":"Invalid app registration, do not retry without configuring the application."}"#,
                ),
            )
            .expect(1)
            .mount(&server)
            .await;

        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1");
        std::env::set_var(
            "LUCERNA_MC_LOGIN_URL_OVERRIDE",
            format!("{}/authentication/login_with_xbox", server.uri()),
        );

        let result = login_with_xbox("uhs-abc", "xsts-xyz").await;

        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        std::env::remove_var("LUCERNA_MC_LOGIN_URL_OVERRIDE");

        assert!(
            matches!(result, Err(crate::error::Error::AuthPendingApproval)),
            "expected Err(AuthPendingApproval), got {:?}",
            result
        );
    }

    #[tokio::test]
    async fn login_with_xbox_403_other_body_falls_through_to_auth_failed() {
        let _guard = super::super::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/authentication/login_with_xbox"))
            .respond_with(ResponseTemplate::new(403).set_body_string(
                // Non-matching 403 body to test fallthrough behavior.
                r#"{"error":"FORBIDDEN","errorMessage":"Some other forbidden reason."}"#,
            ))
            .expect(1)
            .mount(&server)
            .await;

        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1");
        std::env::set_var(
            "LUCERNA_MC_LOGIN_URL_OVERRIDE",
            format!("{}/authentication/login_with_xbox", server.uri()),
        );

        let result = login_with_xbox("uhs-abc", "xsts-xyz").await;

        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        std::env::remove_var("LUCERNA_MC_LOGIN_URL_OVERRIDE");

        assert!(
            matches!(
                result,
                Err(crate::error::Error::AuthFailed { ref stage, .. })
                    if stage == "mc_services"
            ),
            "expected Err(AuthFailed{{stage=mc_services}}), got {:?}",
            result
        );
    }
}
