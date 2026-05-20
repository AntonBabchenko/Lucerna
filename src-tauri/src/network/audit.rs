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

/// Push `entry`, evicting the oldest first if the buffer is at `max`.
/// Pulled out of `record` so the capacity/eviction behaviour can be
/// unit-tested on a local `VecDeque` — without touching the
/// process-global buffer, which parallel tests in other modules also
/// write to.
fn push_capped(buf: &mut VecDeque<AuditEntry>, entry: AuditEntry, max: usize) {
    if buf.len() == max {
        buf.pop_front();
    }
    buf.push_back(entry);
}

pub fn record(entry: AuditEntry) {
    let mut buf = buffer().lock().expect("audit buffer mutex poisoned");
    push_capped(&mut buf, entry, MAX_ENTRIES);
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
    use std::sync::MutexGuard;

    // Serializes this module's tests so their `-probe` audit entries do not
    // pile up in the shared 200-slot ring buffer fast enough to evict one
    // another under concurrent `record()` traffic from other test modules.
    // The tests are isolation-correct without this lock (each filters
    // `recent()` / `audit_violations()` to its own unique `-probe` URLs),
    // but the lock keeps buffer pressure bounded. A module-local mutex
    // guard does this without pulling in serial_test.
    fn test_lock() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

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

    // The tests below assert only on their OWN, unique-URL entries —
    // filtered out of the process-global buffer by a `-probe` suffix.
    // They never assert exact global counts, so a concurrent `record()`
    // from another module's parallel test cannot break them. They also
    // do NOT call `clear_for_test()`: clearing the shared buffer is a
    // cross-module hazard (it can wipe an entry another module's test
    // just recorded). `ring_buffer_evicts_oldest_at_capacity` is fully
    // isolated — it exercises `push_capped` on a local `VecDeque`.

    #[test]
    fn record_then_recent_returns_in_order() {
        let _g = test_lock();
        record(entry("https://order-probe-1/"));
        record(entry("https://order-probe-2/"));
        let mine: Vec<_> = recent()
            .into_iter()
            .filter(|e| e.url.starts_with("https://order-probe-"))
            .collect();
        assert_eq!(mine.len(), 2);
        assert_eq!(mine[0].url, "https://order-probe-1/");
        assert_eq!(mine[1].url, "https://order-probe-2/");
    }

    #[test]
    fn ring_buffer_evicts_oldest_at_capacity() {
        // Fully isolated: exercises eviction on a LOCAL VecDeque, so it
        // touches no global state and needs no test_lock/clear_for_test.
        let mut buf = VecDeque::new();
        for i in 0..(MAX_ENTRIES + 5) {
            push_capped(&mut buf, entry(&format!("https://x/{i}")), MAX_ENTRIES);
        }
        assert_eq!(buf.len(), MAX_ENTRIES);
        // First five were evicted: oldest remaining should be /5.
        assert_eq!(buf[0].url, "https://x/5");
        assert_eq!(buf[MAX_ENTRIES - 1].url, format!("https://x/{}", MAX_ENTRIES + 4));
    }

    #[test]
    fn audit_violations_empty_when_all_hosts_allowed() {
        let _g = test_lock();
        record(entry("https://auth.mojang.com/allowed-probe-1"));
        record(entry("https://api.github.com/allowed-probe-2"));
        let v = audit_violations();
        assert!(
            !v.iter().any(|e| e.url == "https://auth.mojang.com/allowed-probe-1"),
            "allowed mojang URL flagged as violation, got {v:?}"
        );
        assert!(
            !v.iter().any(|e| e.url == "https://api.github.com/allowed-probe-2"),
            "allowed github URL flagged as violation, got {v:?}"
        );
    }

    #[test]
    fn audit_violations_flags_disallowed_host() {
        let _g = test_lock();
        record(entry("https://evil-probe.example/x"));
        record(entry("https://auth.mojang.com/probe-y"));
        let v = audit_violations();
        assert!(
            v.iter().any(|e| e.url.contains("evil-probe.example")),
            "disallowed host not flagged, got {v:?}"
        );
        assert!(
            !v.iter().any(|e| e.url.contains("auth.mojang.com/probe-y")),
            "allowed host flagged as violation, got {v:?}"
        );
    }

    #[test]
    fn audit_violations_flags_malformed_url() {
        let _g = test_lock();
        record(entry("not a url probe-xyz"));
        let v = audit_violations();
        assert!(
            v.iter().any(|e| e.url == "not a url probe-xyz"),
            "malformed url not flagged, got {v:?}"
        );
    }

    #[test]
    fn audit_violations_flags_urls_without_host() {
        let _g = test_lock();
        record(entry("file:///etc/passwd-probe"));
        record(entry("data:text/plain,probe"));
        let v = audit_violations();
        assert!(
            v.iter().any(|e| e.url == "file:///etc/passwd-probe"),
            "file:// url not flagged, got {v:?}"
        );
        assert!(
            v.iter().any(|e| e.url == "data:text/plain,probe"),
            "data: url not flagged, got {v:?}"
        );
    }
}
