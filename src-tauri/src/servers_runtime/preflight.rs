//! Pure pre-spawn checks. Each helper returns `None`/`false` when the condition
//! is fine; the command layer composes them and maps a finding to a fixable
//! `ServerDiagnosis`. Side-effect-free except the transient port bind probe.

use std::net::TcpListener;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreflightFinding {
    EulaNotAccepted,
    PortInUse(u16),
    OrphanRunning(u32),
    LowDisk,
}

/// True iff `port` cannot be bound on 0.0.0.0 right now (a hint — the TOCTOU
/// race is acceptable for a diagnosis). Port 0 (unset) is never "in use".
pub fn port_in_use(port: u16) -> bool {
    if port == 0 {
        return false;
    }
    TcpListener::bind(("0.0.0.0", port)).is_err()
}

/// `EulaNotAccepted` when the stored flag is false.
pub fn eula_finding(eula_accepted: bool) -> Option<PreflightFinding> {
    (!eula_accepted).then_some(PreflightFinding::EulaNotAccepted)
}

/// `OrphanRunning(pid)` when the persisted PID is still a live java process —
/// a server from a prior launcher session still holding the world.
pub fn orphan_finding(recorded_pid: Option<u32>) -> Option<PreflightFinding> {
    let pid = recorded_pid?;
    if crate::platform::process_alive(pid) && crate::platform::process_image_matches(pid, "java") {
        Some(PreflightFinding::OrphanRunning(pid))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_zero_is_never_in_use() {
        assert!(!port_in_use(0));
    }

    #[test]
    fn bound_port_reports_in_use() {
        let l = TcpListener::bind(("0.0.0.0", 0)).unwrap();
        let port = l.local_addr().unwrap().port();
        assert!(port_in_use(port), "held port {port} must read as in use");
    }

    #[test]
    fn eula_finding_only_when_false() {
        assert_eq!(eula_finding(true), None);
        assert_eq!(eula_finding(false), Some(PreflightFinding::EulaNotAccepted));
    }

    #[test]
    fn orphan_finding_none_without_pid_or_dead_pid() {
        assert_eq!(orphan_finding(None), None);
        assert_eq!(orphan_finding(Some(u32::MAX)), None);
    }
}
