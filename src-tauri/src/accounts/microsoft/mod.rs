//! Microsoft auth orchestrators: sign_in (full chain) and refresh
//! (renew tokens with the refresh_token).

pub mod cosmetics;
pub mod mc_services;
pub mod oauth;
pub mod xbox;

use crate::accounts::keychain;
use crate::accounts::store::{self, Account};
use crate::error::{Error, Result};
use crate::paths::account_file;
use rand::RngCore;
use std::time::Duration;

const LISTENER_TIMEOUT_DEFAULT: Duration = Duration::from_secs(5 * 60);

fn fresh_state() -> String {
    let mut buf = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut buf);
    hex::encode(buf)
}

/// Full interactive sign-in. Blocks until the user completes the browser
/// flow (or the listener times out — `Error::AuthCancelled` in that case).
pub async fn sign_in(app: &tauri::AppHandle) -> Result<Account> {
    let pkce = oauth::Pkce::new();
    let state = fresh_state();
    let (listener, redirect_uri) = oauth::bind_listener().await?;
    let auth_url = oauth::build_authorize_url(&redirect_uri, &pkce.challenge, &state);

    // Open the browser. If the opener fails, fail the sign-in NOW and say
    // why. Nothing in the UI ever displays `auth_url`, so "the user may
    // copy/paste the URL manually" is not a recovery path that exists —
    // continuing would burn the 5-minute listener timeout in silence and
    // then report "Microsoft sign-in cancelled", which is not what
    // happened. The URL is deliberately NOT carried in the error either:
    // it is only actionable while this listener is alive, and returning
    // here drops the listener — an error offering a dead URL would rebuild
    // the same false story in typed form.
    use tauri_plugin_opener::OpenerExt;
    if let Err(e) = app.opener().open_url(&auth_url, None::<&str>) {
        crate::diag!("msauth: could not open the browser for sign-in: {e}");
        return Err(Error::AuthFailed {
            stage: "open_browser".into(),
            details: e.to_string(),
        });
    }

    let timeout = crate::test_seam::resolve("LUCERNA_LISTENER_TIMEOUT_SECS")
        .and_then(|s| s.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(LISTENER_TIMEOUT_DEFAULT);

    let outcome = oauth::run_loopback_listener(listener, timeout).await?;
    let (code, callback_state) = match outcome {
        oauth::CallbackOutcome::Code { code, state } => (code, state),
        oauth::CallbackOutcome::OAuthError { description } => {
            return Err(Error::AuthFailed {
                stage: "microsoft_authorize".into(),
                details: description,
            })
        }
        oauth::CallbackOutcome::UnrelatedPath => {
            return Err(Error::AuthFailed {
                stage: "microsoft_authorize".into(),
                details: "loopback received unrelated request".into(),
            })
        }
    };
    if callback_state != state {
        return Err(Error::AuthFailed {
            stage: "microsoft_authorize".into(),
            details: "state mismatch".into(),
        });
    }

    let token_url = crate::test_seam::resolve("LUCERNA_MS_TOKEN_URL_OVERRIDE")
        .unwrap_or_else(|| oauth::MS_TOKEN_URL.to_string());
    let ms_token =
        oauth::exchange_code_for_token(&code, &pkce.verifier, &redirect_uri, &token_url).await?;

    let (xbl_token, _xbl_uhs) = xbox::xbl_authenticate(&ms_token.access_token).await?;
    let xsts = xbox::xsts_authorize(&xbl_token).await?;
    let mc_login = mc_services::login_with_xbox(&xsts.userhash, &xsts.token).await?;
    let profile = mc_services::fetch_profile(&mc_login.access_token).await?;
    let mc_uuid = mc_services::hyphenate_uuid(&profile.id)?;
    let expires_at = super::now_secs() + mc_login.expires_in as f64;

    // Resolve the account file path via paths::account_file (same pattern as
    // other functions in accounts/mod.rs), then delegate to the testable
    // store-level helper which takes &Path directly.
    let path = account_file(app).map_err(|e| Error::io("<app data dir>/account.json", e))?;
    // Compute the account entry (and its id) WITHOUT persisting yet, so the
    // keyring secrets are written first. If keyring writes fail, account.json
    // is left untouched — no signed-in-looking entry with missing tokens.
    let (updated_file, account) =
        store::prepare_upsert_microsoft_account(&path, &mc_uuid, &profile.name, Some(expires_at))?;

    keychain::store(
        &keychain::refresh_token_key(&account.id),
        &ms_token.refresh_token,
    )?;
    keychain::store(
        &keychain::mc_access_key(&account.id),
        &mc_login.access_token,
    )?;

    // Both keyring writes succeeded — now persist the account file.
    store::write_account_file(&path, &updated_file)?;

    // Best-effort skin prefetch: warm the on-disk cache so the avatar is
    // ready on first paint. Spawned so it never delays or fails sign-in;
    // the lazy display path re-fetches if this hasn't completed yet.
    let app_for_skin = app.clone();
    let uuid_for_skin = account.uuid.clone();
    tauri::async_runtime::spawn(async move {
        let _ = crate::accounts::skins::get_account_skin(&app_for_skin, &uuid_for_skin, true).await;
    });

    Ok(account)
}

/// Silent renewal using the stored refresh_token. Replaces both keychain
/// entries and updates the account's `expires_at` in account.json.
pub async fn refresh(app: &tauri::AppHandle, account_id: &str) -> Result<Account> {
    // A keyring that answers "no such entry" is a sign-in to redo; a keyring that
    // cannot be read at all is its own error (`?`), not a reason to ask for one.
    let refresh_token =
        keychain::retrieve(&keychain::refresh_token_key(account_id))?.ok_or_else(|| {
            crate::diag!("microsoft refresh: no refresh token stored for account {account_id}");
            Error::MicrosoftSignInExpired
        })?;
    let token_url = crate::test_seam::resolve("LUCERNA_MS_TOKEN_URL_OVERRIDE")
        .unwrap_or_else(|| oauth::MS_TOKEN_URL.to_string());
    let ms_token = exchange_refresh_token(&refresh_token, &token_url).await?;

    let (xbl_token, _uhs) = xbox::xbl_authenticate(&ms_token.access_token).await?;
    let xsts = xbox::xsts_authorize(&xbl_token).await?;
    let mc_login = mc_services::login_with_xbox(&xsts.userhash, &xsts.token).await?;
    let profile = mc_services::fetch_profile(&mc_login.access_token).await?;
    let mc_uuid = mc_services::hyphenate_uuid(&profile.id)?;
    let expires_at = super::now_secs() + mc_login.expires_in as f64;

    let path = account_file(app).map_err(|e| Error::io("<app data dir>/account.json", e))?;
    // Update-only, located by the id this refresh was invoked with. If the
    // account was removed since the refresh started, this returns an auth
    // error instead of resurrecting a ghost entry. Compute WITHOUT persisting
    // so the keyring secrets are written first (same ordering as sign_in).
    let (updated_file, account) = store::prepare_refresh_microsoft_account_by_id(
        &path,
        account_id,
        &mc_uuid,
        &profile.name,
        Some(expires_at),
    )?;

    keychain::store(
        &keychain::refresh_token_key(&account.id),
        &ms_token.refresh_token,
    )?;
    keychain::store(
        &keychain::mc_access_key(&account.id),
        &mc_login.access_token,
    )?;

    // Both keyring writes succeeded — now persist the account file.
    store::write_account_file(&path, &updated_file)?;

    // Best-effort skin prefetch: warm the on-disk cache so the avatar is
    // ready on first paint. Spawned so it never delays or fails the refresh;
    // the lazy display path re-fetches if this hasn't completed yet.
    let app_for_skin = app.clone();
    let uuid_for_skin = account.uuid.clone();
    tauri::async_runtime::spawn(async move {
        let _ = crate::accounts::skins::get_account_skin(&app_for_skin, &uuid_for_skin, true).await;
    });

    Ok(account)
}

#[doc(hidden)]
pub async fn __test_xbl_authenticate(ms: &str) -> Result<(String, String)> {
    xbox::xbl_authenticate(ms).await
}
#[doc(hidden)]
pub async fn __test_xsts_authorize(xbl: &str) -> Result<(String, String)> {
    let r = xbox::xsts_authorize(xbl).await?;
    Ok((r.token, r.userhash))
}
#[doc(hidden)]
pub async fn __test_login_with_xbox(uhs: &str, xsts: &str) -> Result<(String, u64)> {
    let r = mc_services::login_with_xbox(uhs, xsts).await?;
    Ok((r.access_token, r.expires_in))
}
#[doc(hidden)]
pub async fn __test_fetch_profile(mc_tok: &str) -> Result<(String, String)> {
    let r = mc_services::fetch_profile(mc_tok).await?;
    Ok((r.id, r.name))
}

async fn exchange_refresh_token(
    refresh_token: &str,
    token_url: &str,
) -> Result<oauth::MsTokenResponse> {
    let body = format!(
        "client_id={}&grant_type=refresh_token&refresh_token={}&scope={}",
        oauth::CLIENT_ID,
        urlencoding::encode(refresh_token),
        // The scope value contains a space between the two scopes; it must be
        // percent-encoded in the form body (a raw space is invalid).
        urlencoding::encode("XboxLive.signin offline_access"),
    );
    crate::network::allowlist::check_url_allowed(token_url, "microsoft_auth")?;
    let resp = crate::network::http()
        .post(token_url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .map_err(|e| crate::network::failure::transport("microsoft_auth", "POST", token_url, e))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(token_endpoint_failure(status, &text));
    }
    resp.json::<oauth::MsTokenResponse>()
        .await
        .map_err(|e| Error::AuthFailed {
            stage: "refresh".into(),
            details: format!("parse: {e}"),
        })
}

/// The error for a non-2xx answer from the token endpoint to a refresh.
/// `invalid_grant` / `interaction_required` on a 400 or 401 (RFC 6749 §5.2, as
/// the Microsoft identity platform uses them) mean the saved refresh token is no
/// longer accepted: only signing in again helps. Anything else — another OAuth
/// code (Lucerna's own registration), a 5xx, a body that is not the OAuth error
/// shape — is a renewal that failed for a reason signing in would not fix, and
/// keeps the raw answer for the log.
fn token_endpoint_failure(status: reqwest::StatusCode, body: &str) -> Error {
    #[derive(serde::Deserialize)]
    struct OAuthError {
        error: String,
    }
    let refused = matches!(status.as_u16(), 400 | 401)
        && serde_json::from_str::<OAuthError>(body)
            .is_ok_and(|e| matches!(e.error.as_str(), "invalid_grant" | "interaction_required"));
    if refused {
        crate::diag!(
            "microsoft refresh: the token endpoint refused the saved sign-in ({status}): {body}"
        );
        return Error::MicrosoftSignInExpired;
    }
    Error::AuthFailed {
        stage: "refresh".into(),
        details: format!("HTTP {status}: {body}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;

    fn oauth_error(code: &str) -> String {
        format!(
            r#"{{"error":"{code}","error_description":"AADSTS70000: …","error_codes":[70000]}}"#
        )
    }

    #[test]
    fn an_invalid_grant_asks_to_sign_in_again() {
        let e = token_endpoint_failure(StatusCode::BAD_REQUEST, &oauth_error("invalid_grant"));
        assert_eq!(e, Error::MicrosoftSignInExpired);
    }

    #[test]
    fn interaction_required_asks_to_sign_in_again() {
        let e = token_endpoint_failure(
            StatusCode::BAD_REQUEST,
            &oauth_error("interaction_required"),
        );
        assert_eq!(e, Error::MicrosoftSignInExpired);
    }

    #[test]
    fn an_invalid_grant_on_401_asks_to_sign_in_again() {
        let e = token_endpoint_failure(StatusCode::UNAUTHORIZED, &oauth_error("invalid_grant"));
        assert_eq!(e, Error::MicrosoftSignInExpired);
    }

    #[test]
    fn microsoft_being_down_is_not_a_sign_in_question() {
        // A 5xx is Microsoft's trouble, even when its body happens to carry the word.
        let e = token_endpoint_failure(
            StatusCode::SERVICE_UNAVAILABLE,
            &oauth_error("invalid_grant"),
        );
        assert!(
            matches!(&e, Error::AuthFailed { stage, details }
                if stage == "refresh" && details.starts_with("HTTP 503 Service Unavailable: ")),
            "{e:?}"
        );
    }

    #[test]
    fn another_oauth_error_stays_a_failed_renewal() {
        // invalid_client / unauthorized_client are Lucerna's registration, not the user's sign-in.
        let e = token_endpoint_failure(StatusCode::BAD_REQUEST, &oauth_error("invalid_client"));
        assert!(matches!(e, Error::AuthFailed { .. }), "{e:?}");
    }

    #[test]
    fn an_unreadable_body_stays_a_failed_renewal() {
        let e = token_endpoint_failure(StatusCode::BAD_REQUEST, "<html>gateway</html>");
        assert!(
            matches!(&e, Error::AuthFailed { details, .. }
                if details == "HTTP 400 Bad Request: <html>gateway</html>"),
            "{e:?}"
        );
    }
}
