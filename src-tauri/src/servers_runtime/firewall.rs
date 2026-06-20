//! Windows-Firewall help for a running server: deterministic allow-rule name +
//! state mapping. The OS calls (netsh) live in `process::`; this module is pure.

use serde::Serialize;
use specta::Type;

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
}
