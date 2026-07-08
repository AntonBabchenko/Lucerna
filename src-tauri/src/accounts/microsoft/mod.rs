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

    // Open the browser. We don't fail if the opener errors; the user may
    // copy/paste the URL manually. The listener still waits for callback.
    use tauri_plugin_opener::OpenerExt;
    let _ = app.opener().open_url(&auth_url, None::<&str>);

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
    let refresh_token =
        keychain::retrieve(&keychain::refresh_token_key(account_id))?.ok_or_else(|| {
            Error::AuthFailed {
                stage: "refresh".into(),
                details: format!("no refresh token found for account {account_id}"),
            }
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
        .map_err(|e| Error::network(token_url, e))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(Error::AuthFailed {
            stage: "refresh".into(),
            details: format!("HTTP {status}: {text}"),
        });
    }
    resp.json::<oauth::MsTokenResponse>()
        .await
        .map_err(|e| Error::AuthFailed {
            stage: "refresh".into(),
            details: format!("parse: {e}"),
        })
}
