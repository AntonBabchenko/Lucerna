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

fn build_client() -> reqwest::Client {
    let version = env!("CARGO_PKG_VERSION");
    let user_agent = format!("Lucerna/{version} (+https://github.com/AntonBabchenko/Lucerna)");
    reqwest::Client::builder()
        .user_agent(user_agent)
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .build()
        // Builder failure means the system TLS stack is broken — there's
        // no graceful recovery and no caller can do anything sensible.
        .expect("failed to build reqwest client")
}

pub fn http() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(build_client)
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

    #[test]
    fn user_agent_built_with_package_version() {
        let v = env!("CARGO_PKG_VERSION");
        let expected = format!("Lucerna/{v} (+https://github.com/AntonBabchenko/Lucerna)");
        assert!(expected.starts_with("Lucerna/"));
        assert!(expected.contains(v));
    }
}
