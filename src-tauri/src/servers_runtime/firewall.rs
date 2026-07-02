//! Windows-Firewall help for a running server: deterministic allow-rule name +
//! state mapping. The OS calls (netsh) live in `process::`; this module is pure.
//!
//! It also tracks which ports we added an allow-rule for, in a per-server
//! `firewall.json` sidecar, so *every* rule we created can be removed when the
//! server is deleted and the old rule migrated when its port changes — the
//! current `server.properties` port alone doesn't know about rules left behind
//! by earlier ports.

use serde::Serialize;
use specta::Type;
use std::path::{Path, PathBuf};

/// What the Connect tab shows. `NotApplicable` = non-Windows; `Unknown` = port
/// not yet known (server.properties not generated).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum FirewallState {
    Allowed,
    NeedsRule,
    Unknown,
    NotApplicable,
}

/// Deterministic inbound-allow rule name for a server port. ASCII only (so the
/// presence check is locale-robust against `netsh`'s localized "no rules" text).
pub fn rule_name(port: u16) -> String {
    format!("Lucerna Minecraft Server (TCP {port})")
}

/// Map (port, rule-present) → state. `None` port → `Unknown`.
pub fn status_from(port: Option<u16>, present: bool) -> FirewallState {
    match port {
        None => FirewallState::Unknown,
        Some(_) if present => FirewallState::Allowed,
        Some(_) => FirewallState::NeedsRule,
    }
}

/// Sidecar tracking every port we added a firewall allow-rule for. Persisted at
/// `<server_root>/firewall.json`. Absent/unreadable → an empty set (back-compat:
/// servers created before this tracking existed simply have no recorded ports,
/// so delete falls back to the current-port rule only).
fn firewall_sidecar(root: &Path) -> PathBuf {
    root.join("firewall.json")
}

/// Read the recorded set of ports we added rules for (deduped, sorted). Never
/// errors: a corrupt/absent sidecar reads as empty.
pub fn recorded_ports(root: &Path) -> Vec<u16> {
    std::fs::read_to_string(firewall_sidecar(root))
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<u16>>(&s).ok())
        .map(|mut v| {
            v.sort_unstable();
            v.dedup();
            v
        })
        .unwrap_or_default()
}

fn write_ports(root: &Path, ports: &[u16]) {
    // Best-effort: firewall bookkeeping must never fail the caller. A create dir
    // failure or write failure just means the sidecar isn't updated this time.
    if std::fs::create_dir_all(root).is_err() {
        return;
    }
    if let Ok(json) = serde_json::to_string(ports) {
        let _ = std::fs::write(firewall_sidecar(root), json);
    }
}

/// Record that we added an allow-rule for `port` (idempotent). Call after a
/// successful rule add so delete can remove every rule we created.
pub fn record_added_port(root: &Path, port: u16) {
    let mut ports = recorded_ports(root);
    if !ports.contains(&port) {
        ports.push(port);
        ports.sort_unstable();
        write_ports(root, &ports);
    }
}

/// Forget a port from the tracking sidecar (after its rule was removed).
pub fn forget_port(root: &Path, port: u16) {
    let mut ports = recorded_ports(root);
    let before = ports.len();
    ports.retain(|&p| p != port);
    if ports.len() != before {
        write_ports(root, &ports);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_name_is_ascii_and_port_specific() {
        assert_eq!(rule_name(25565), "Lucerna Minecraft Server (TCP 25565)");
        assert!(rule_name(25565).is_ascii());
        assert_ne!(rule_name(25565), rule_name(25566));
    }

    #[test]
    fn status_mapping() {
        assert_eq!(status_from(None, false), FirewallState::Unknown);
        assert_eq!(status_from(Some(25565), true), FirewallState::Allowed);
        assert_eq!(status_from(Some(25565), false), FirewallState::NeedsRule);
    }

    #[test]
    fn recorded_ports_absent_reads_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(recorded_ports(dir.path()).is_empty());
    }

    #[test]
    fn record_and_forget_ports_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        record_added_port(dir.path(), 25565);
        record_added_port(dir.path(), 25565); // idempotent
        record_added_port(dir.path(), 25570);
        assert_eq!(recorded_ports(dir.path()), vec![25565, 25570]);
        forget_port(dir.path(), 25565);
        assert_eq!(recorded_ports(dir.path()), vec![25570]);
        // Forgetting an absent port is a no-op.
        forget_port(dir.path(), 40000);
        assert_eq!(recorded_ports(dir.path()), vec![25570]);
    }

    #[test]
    fn corrupt_sidecar_reads_empty() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("firewall.json"), b"{ not json").unwrap();
        assert!(recorded_ports(dir.path()).is_empty());
    }
}
