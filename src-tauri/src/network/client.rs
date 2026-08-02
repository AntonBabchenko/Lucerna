//! The one `reqwest::Client` instance for the whole process. Reusing a
//! single client enables connection pooling and shared TLS session
//! cache.
//!
//! The User-Agent identifies Lucerna and the launcher version so
//! upstream hosts (Mojang, Modrinth) can see what client is hitting
//! their endpoints. This is the only identifying header we send.
//!
//! TLS roots come from the OS trust store via `rustls-native-certs`.
//! This is load-bearing for the Microsoft Azure submission — the
//! Microsoft Trusted Root Program is the source of truth on Windows,
//! and changing the cert source (e.g. switching to `webpki-roots`)
//! silently moves the launcher off the OS root store. If you change
//! the rustls feature set, confirm the OS-trust path stays intact
//! and update this comment.

use std::sync::OnceLock;
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const READ_TIMEOUT: Duration = Duration::from_secs(60);

fn build_client_with(read_timeout: Option<Duration>) -> reqwest::Client {
    let version = env!("CARGO_PKG_VERSION");
    let user_agent = format!("Lucerna/{version} (+https://github.com/AntonBabchenko/Lucerna)");
    let builder = reqwest::Client::builder()
        .user_agent(user_agent)
        .connect_timeout(CONNECT_TIMEOUT);
    let builder = match read_timeout {
        Some(t) => builder.read_timeout(t),
        None => builder,
    };
    builder
        .build()
        // Builder failure means the system TLS stack is broken — there's
        // no graceful recovery and no caller can do anything sensible.
        .expect("failed to build reqwest client")
}

pub fn http() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| build_client_with(Some(READ_TIMEOUT)))
}

/// A second client for calls that WAIT on generation rather than stream a
/// file. `READ_TIMEOUT` is client-level in reqwest and fires while waiting
/// for the first byte, so a model that thinks for two minutes would be cut
/// off at 60 s no matter what per-request budget the caller sets. This client
/// drops the read timeout; callers MUST pass their own total timeout, which
/// then becomes the only bound. Connect timeout and User-Agent are unchanged.
pub fn http_generation() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| build_client_with(None))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_returns_same_instance_on_repeat() {
        let a = http();
        let b = http();
        assert!(std::ptr::eq(a, b));
    }

    #[tokio::test]
    async fn a_read_timeout_fires_while_waiting_for_the_first_byte() {
        // Pins WHY http_generation exists: read_timeout is not a body-only
        // stall detector, and it is not per-request overridable. If this ever
        // stops being true, the second client can go away.
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_delay(std::time::Duration::from_millis(300)),
            )
            .mount(&server)
            .await;
        let url = format!("{}/slow", server.uri());

        let capped = build_client_with(Some(std::time::Duration::from_millis(80)));
        assert!(
            capped.get(&url).send().await.is_err(),
            "an 80ms read timeout must not tolerate a 300ms time-to-first-byte"
        );

        let uncapped = build_client_with(None);
        assert!(
            uncapped
                .get(&url)
                .timeout(std::time::Duration::from_secs(5))
                .send()
                .await
                .is_ok(),
            "with no read timeout, a 5s total budget must cover a 300ms response"
        );
    }

    #[test]
    fn user_agent_built_with_package_version() {
        let v = env!("CARGO_PKG_VERSION");
        let expected = format!("Lucerna/{v} (+https://github.com/AntonBabchenko/Lucerna)");
        assert!(expected.starts_with("Lucerna/"));
        assert!(expected.contains(v));
    }
}
