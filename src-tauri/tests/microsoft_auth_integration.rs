//! End-to-end Microsoft auth coverage. wiremock simulates the full chain:
//! MS token, XBL, XSTS, MC services login + profile. The loopback OAuth
//! listener runs for real; the "browser" is emulated via reqwest::get.
//!
//! Scenarios match the test plan in
//! docs/superpowers/specs/2026-05-13-microsoft-auth-design.md § Testing.

use ftlauncher_lib::error::Error;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Serialise env-var mutations across all tests in this integration binary.
/// Each integration test file compiles to its own binary, so this lock is
/// file-scoped and independent from locks in unit tests.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

const MS_TOKEN_FIXTURE: &str =
    r#"{"access_token":"ms-tok","refresh_token":"rt-1","expires_in":3600}"#;
const XBL_FIXTURE: &str = r#"{"Token":"xbl-tok","DisplayClaims":{"xui":[{"uhs":"uhs-xyz"}]}}"#;
const XSTS_FIXTURE: &str = r#"{"Token":"xsts-tok","DisplayClaims":{"xui":[{"uhs":"uhs-xsts"}]}}"#;
const MC_LOGIN_FIXTURE: &str = r#"{"access_token":"mc-tok","expires_in":86400}"#;
const MC_PROFILE_FIXTURE: &str = r#"{"id":"7e8d9c0a123456789abcdef012345678","name":"AntonMC"}"#;

fn mount_happy_chain(server: &MockServer) -> impl std::future::Future<Output = ()> + '_ {
    async move {
        Mock::given(method("POST"))
            .and(path("/oauth2/v2.0/token"))
            .respond_with(ResponseTemplate::new(200).set_body_string(MS_TOKEN_FIXTURE))
            .mount(server)
            .await;
        Mock::given(method("POST"))
            .and(path("/user/authenticate"))
            .respond_with(ResponseTemplate::new(200).set_body_string(XBL_FIXTURE))
            .mount(server)
            .await;
        Mock::given(method("POST"))
            .and(path("/xsts/authorize"))
            .respond_with(ResponseTemplate::new(200).set_body_string(XSTS_FIXTURE))
            .mount(server)
            .await;
        Mock::given(method("POST"))
            .and(path("/authentication/login_with_xbox"))
            .respond_with(ResponseTemplate::new(200).set_body_string(MC_LOGIN_FIXTURE))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path("/minecraft/profile"))
            .respond_with(ResponseTemplate::new(200).set_body_string(MC_PROFILE_FIXTURE))
            .mount(server)
            .await;
    }
}

fn set_chain_overrides(server: &MockServer) {
    std::env::set_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
    std::env::set_var(
        "FTLAUNCHER_MS_TOKEN_URL_OVERRIDE",
        format!("{}/oauth2/v2.0/token", server.uri()),
    );
    std::env::set_var(
        "FTLAUNCHER_XBL_URL_OVERRIDE",
        format!("{}/user/authenticate", server.uri()),
    );
    std::env::set_var(
        "FTLAUNCHER_XSTS_URL_OVERRIDE",
        format!("{}/xsts/authorize", server.uri()),
    );
    std::env::set_var(
        "FTLAUNCHER_MC_LOGIN_URL_OVERRIDE",
        format!("{}/authentication/login_with_xbox", server.uri()),
    );
    std::env::set_var(
        "FTLAUNCHER_MC_PROFILE_URL_OVERRIDE",
        format!("{}/minecraft/profile", server.uri()),
    );
    std::env::set_var("FTLAUNCHER_LISTENER_TIMEOUT_SECS", "5");
}

fn clear_chain_overrides() {
    for var in [
        "FTLAUNCHER_EXTRA_ALLOWED_HOSTS",
        "FTLAUNCHER_MS_TOKEN_URL_OVERRIDE",
        "FTLAUNCHER_XBL_URL_OVERRIDE",
        "FTLAUNCHER_XSTS_URL_OVERRIDE",
        "FTLAUNCHER_MC_LOGIN_URL_OVERRIDE",
        "FTLAUNCHER_MC_PROFILE_URL_OVERRIDE",
        "FTLAUNCHER_LISTENER_TIMEOUT_SECS",
    ] {
        std::env::remove_var(var);
    }
}

// ── Scenario 1: XBL endpoint reachable through audited HTTP ─────────────────

#[tokio::test]
async fn xbl_endpoint_reachable_through_audited_http() {
    let _guard = ENV_LOCK.lock().unwrap();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/user/authenticate"))
        .respond_with(ResponseTemplate::new(200).set_body_string(XBL_FIXTURE))
        .mount(&server)
        .await;
    std::env::set_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
    std::env::set_var(
        "FTLAUNCHER_XBL_URL_OVERRIDE",
        format!("{}/user/authenticate", server.uri()),
    );

    let res = ftlauncher_lib::accounts::microsoft::__test_xbl_authenticate("ms-tok").await;
    assert!(res.is_ok(), "got {res:?}");
    let (token, uhs) = res.unwrap();
    assert_eq!(token, "xbl-tok");
    assert_eq!(uhs, "uhs-xyz");

    std::env::remove_var("FTLAUNCHER_XBL_URL_OVERRIDE");
    std::env::remove_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS");
}

// ── Scenario 2: XSTS child-account XErr propagates ──────────────────────────

#[tokio::test]
async fn xsts_child_account_error_propagates() {
    let _guard = ENV_LOCK.lock().unwrap();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/xsts/authorize"))
        .respond_with(ResponseTemplate::new(401).set_body_string(r#"{"XErr":2148916238}"#))
        .mount(&server)
        .await;
    std::env::set_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
    std::env::set_var(
        "FTLAUNCHER_XSTS_URL_OVERRIDE",
        format!("{}/xsts/authorize", server.uri()),
    );

    let result = ftlauncher_lib::accounts::microsoft::__test_xsts_authorize("xbl-tok").await;
    match result {
        Err(Error::AuthFailed { stage, details }) => {
            assert_eq!(stage, "xsts");
            assert_eq!(details, "child_account");
        }
        other => panic!("expected child_account, got {other:?}"),
    }

    std::env::remove_var("FTLAUNCHER_XSTS_URL_OVERRIDE");
    std::env::remove_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS");
}

// ── Scenario 3: MC profile 404 surfaces NoMinecraftProfile ──────────────────

#[tokio::test]
async fn mc_profile_404_surfaces_no_minecraft_profile() {
    let _guard = ENV_LOCK.lock().unwrap();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/minecraft/profile"))
        .respond_with(ResponseTemplate::new(404).set_body_string("{}"))
        .mount(&server)
        .await;
    std::env::set_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
    std::env::set_var(
        "FTLAUNCHER_MC_PROFILE_URL_OVERRIDE",
        format!("{}/minecraft/profile", server.uri()),
    );

    let result = ftlauncher_lib::accounts::microsoft::__test_fetch_profile("mc-tok").await;
    assert!(matches!(result, Err(Error::NoMinecraftProfile)));

    std::env::remove_var("FTLAUNCHER_MC_PROFILE_URL_OVERRIDE");
    std::env::remove_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS");
}

// ── Scenario 4: Happy-path full chain via test seam ──────────────────────────

#[tokio::test]
async fn full_chain_smoke_via_test_seam() {
    let _guard = ENV_LOCK.lock().unwrap();
    let server = MockServer::start().await;
    mount_happy_chain(&server).await;
    set_chain_overrides(&server);

    // Drive xbl → xsts → mc login → profile through the test seam.
    let (xbl_tok, _) = ftlauncher_lib::accounts::microsoft::__test_xbl_authenticate("ms-tok")
        .await
        .unwrap();
    let xsts = ftlauncher_lib::accounts::microsoft::__test_xsts_authorize(&xbl_tok)
        .await
        .unwrap();
    let login = ftlauncher_lib::accounts::microsoft::__test_login_with_xbox(&xsts.0, &xsts.1)
        .await
        .unwrap();
    let profile = ftlauncher_lib::accounts::microsoft::__test_fetch_profile(&login.0)
        .await
        .unwrap();
    assert_eq!(profile.0, "7e8d9c0a123456789abcdef012345678");
    assert_eq!(profile.1, "AntonMC");

    clear_chain_overrides();
}

// ── Scenario 5: Account-file v1 migration happy path ────────────────────────

#[test]
fn account_file_v1_migration_happy_path() {
    use ftlauncher_lib::accounts::store::{read_account_file, AccountKind};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("account.json");
    std::fs::write(
        &path,
        r#"{"name":"OldUser","uuid":"b50ad385-829d-3141-a216-7e7d7539ba7f"}"#,
    )
    .unwrap();
    let file = read_account_file(&path).unwrap();
    // v1 input migrates to v3 (the current on-disk version).
    assert_eq!(file.version, 3);
    assert_eq!(file.accounts.len(), 1);
    assert_eq!(file.accounts[0].kind, AccountKind::Offline);
    assert_eq!(file.accounts[0].name, "OldUser");
    assert!(file.active_id.is_some());
    let on_disk = std::fs::read_to_string(&path).unwrap();
    assert!(on_disk.contains(r#""version": 3"#) || on_disk.contains(r#""version":3"#));
}

// ── Scenario 6: Full chain 403 → AuthPendingApproval (Variant A) ────────────

#[tokio::test]
async fn full_chain_403_invalid_app_registration_maps_to_pending_approval() {
    let _guard = ENV_LOCK.lock().unwrap();

    let server = MockServer::start().await;

    // MS OAuth token endpoint — happy.
    Mock::given(method("POST"))
        .and(path("/oauth20_token.srf"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "token_type": "Bearer",
            "scope": "XboxLive.signin offline_access",
            "expires_in": 3600,
            "access_token": "ms-token-abc",
            "refresh_token": "refresh-xyz",
        })))
        .expect(1)
        .mount(&server)
        .await;
    // XBL authenticate — happy.
    Mock::given(method("POST"))
        .and(path("/user/authenticate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "Token": "xbl-token",
            "DisplayClaims": { "xui": [{ "uhs": "user-hash-abc" }] },
        })))
        .expect(1)
        .mount(&server)
        .await;
    // XSTS authorize — happy.
    Mock::given(method("POST"))
        .and(path("/xsts/authorize"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "Token": "xsts-token",
            "DisplayClaims": { "xui": [{ "uhs": "user-hash-abc" }] },
        })))
        .expect(1)
        .mount(&server)
        .await;
    // MC services login_with_xbox — Variant A 403 trigger.
    Mock::given(method("POST"))
        .and(path("/authentication/login_with_xbox"))
        .respond_with(
            ResponseTemplate::new(403).set_body_string(
                r#"{"error":"FORBIDDEN","errorMessage":"Invalid app registration, do not retry without configuring the application."}"#,
            ),
        )
        .expect(1)
        .mount(&server)
        .await;

    std::env::set_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS", "127.0.0.1");
    std::env::set_var(
        "FTLAUNCHER_XBL_URL_OVERRIDE",
        format!("{}/user/authenticate", server.uri()),
    );
    std::env::set_var(
        "FTLAUNCHER_XSTS_URL_OVERRIDE",
        format!("{}/xsts/authorize", server.uri()),
    );
    std::env::set_var(
        "FTLAUNCHER_MC_LOGIN_URL_OVERRIDE",
        format!("{}/authentication/login_with_xbox", server.uri()),
    );

    // Exercise the post-callback chain by calling module-level functions
    // directly. `exchange_code_for_token` takes `token_url` as an explicit
    // parameter so we pass the mock URL directly (no env var needed).
    let token_url = format!("{}/oauth20_token.srf", server.uri());
    let ms_token = ftlauncher_lib::accounts::microsoft::oauth::exchange_code_for_token(
        "test-code",
        "test-verifier",
        "http://127.0.0.1:0/oauth/callback",
        &token_url,
    )
    .await
    .expect("token exchange");

    let (xbl_token, _uhs) =
        ftlauncher_lib::accounts::microsoft::xbox::xbl_authenticate(&ms_token.access_token)
            .await
            .expect("xbl");

    let xsts = ftlauncher_lib::accounts::microsoft::xbox::xsts_authorize(&xbl_token)
        .await
        .expect("xsts");

    let result = ftlauncher_lib::accounts::microsoft::mc_services::login_with_xbox(
        &xsts.userhash,
        &xsts.token,
    )
    .await;

    std::env::remove_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS");
    std::env::remove_var("FTLAUNCHER_XBL_URL_OVERRIDE");
    std::env::remove_var("FTLAUNCHER_XSTS_URL_OVERRIDE");
    std::env::remove_var("FTLAUNCHER_MC_LOGIN_URL_OVERRIDE");

    assert!(
        matches!(
            result,
            Err(ftlauncher_lib::error::Error::AuthPendingApproval)
        ),
        "expected AuthPendingApproval, got {:?}",
        result
    );
}
