//! In-memory ring buffer of recent network calls. Process-global because
//! the audit is a single launcher-wide record — there is no scenario
//! where two parts of the launcher should see different histories.

use serde::Serialize;
use specta::Type;
use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_ENTRIES: usize = 200;

#[derive(Debug, Clone, Serialize, Type)]
pub struct AuditEntry {
    /// Milliseconds since Unix epoch, as `f64` so JavaScript's `number`
    /// type can round-trip it without precision loss (`Date.now()`
    /// returns f64-compatible values too). UI formats with `new Date(ts)`.
    pub ts: f64,
    pub method: String,
    pub url: String,
    /// Module that initiated the call (`"versions"`, `"jre"`, etc.).
    pub initiator: String,
    /// Bytes transferred. `None` if the call is in progress or failed
    /// before any body was read.
    pub bytes: Option<f64>,
    /// HTTP status code if the response was received, else `None`
    /// (network error before status).
    pub status: Option<u16>,
}

fn buffer() -> &'static Mutex<VecDeque<AuditEntry>> {
    static BUFFER: OnceLock<Mutex<VecDeque<AuditEntry>>> = OnceLock::new();
    BUFFER.get_or_init(|| Mutex::new(VecDeque::with_capacity(MAX_ENTRIES)))
}

pub fn record(entry: AuditEntry) {
    let mut buf = buffer().lock().expect("audit buffer mutex poisoned");
    if buf.len() == MAX_ENTRIES {
        buf.pop_front();
    }
    buf.push_back(entry);
}

pub fn recent() -> Vec<AuditEntry> {
    let buf = buffer().lock().expect("audit buffer mutex poisoned");
    buf.iter().cloned().collect()
}

/// Reset the audit buffer. Used by integration tests in `tests/`, which
/// are external crates and thus cannot rely on `#[cfg(test)]` items.
/// Hidden from rustdoc to keep it out of the public API surface.
#[doc(hidden)]
pub fn clear_for_test() {
    let mut buf = buffer().lock().expect("audit buffer mutex poisoned");
    buf.clear();
}

pub fn now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

/// Audit entries whose host is NOT on the allowlist (per
/// `network::allowlist::is_host_allowed`). URLs that fail to parse
/// or have no host component are flagged too — we never legitimately
/// emit `file://` / `data:` URLs.
pub fn audit_violations() -> Vec<AuditEntry> {
    recent()
        .into_iter()
        .filter(|e| !host_is_allowed_for(&e.url))
        .collect()
}

fn host_is_allowed_for(url: &str) -> bool {
    match reqwest::Url::parse(url) {
        Ok(parsed) => match parsed.host_str() {
            Some(host) => crate::network::allowlist::is_host_allowed(host),
            None => false,
        },
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(url: &str) -> AuditEntry {
        AuditEntry {
            ts: now_ms(),
            method: "GET".into(),
            url: url.into(),
            initiator: "test".into(),
            bytes: Some(0.0),
            status: Some(200),
        }
    }

    #[test]
    fn record_then_recent_returns_in_order() {
        clear_for_test();
        record(entry("https://a/"));
        record(entry("https://b/"));
        let got = recent();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].url, "https://a/");
        assert_eq!(got[1].url, "https://b/");
    }

    #[test]
    fn ring_buffer_evicts_oldest_at_capacity() {
        clear_for_test();
        for i in 0..(MAX_ENTRIES + 5) {
            record(entry(&format!("https://x/{i}")));
        }
        let got = recent();
        assert_eq!(got.len(), MAX_ENTRIES);
        // First five were evicted: oldest remaining should be /5.
        assert_eq!(got[0].url, "https://x/5");
        assert_eq!(got[MAX_ENTRIES - 1].url, format!("https://x/{}", MAX_ENTRIES + 4));
    }

    #[test]
    fn audit_violations_empty_when_all_hosts_allowed() {
        clear_for_test();
        record(entry("https://auth.mojang.com/x"));
        record(entry("https://api.github.com/repos/y"));
        let v = audit_violations();
        assert!(v.is_empty(), "expected no violations, got {v:?}");
    }

    #[test]
    fn audit_violations_flags_disallowed_host() {
        clear_for_test();
        record(entry("https://evil.example/x"));
        record(entry("https://auth.mojang.com/y"));
        let v = audit_violations();
        assert_eq!(v.len(), 1);
        assert!(v[0].url.contains("evil.example"));
    }

    #[test]
    fn audit_violations_flags_malformed_url() {
        clear_for_test();
        record(entry("not a url"));
        let v = audit_violations();
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn audit_violations_flags_urls_without_host() {
        clear_for_test();
        record(entry("file:///etc/passwd"));
        record(entry("data:text/plain,hi"));
        let v = audit_violations();
        assert_eq!(v.len(), 2, "got: {v:?}");
    }
}
