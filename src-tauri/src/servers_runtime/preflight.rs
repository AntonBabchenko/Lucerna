//! Pure pre-spawn checks. Each helper returns `None`/`false` when the condition
//! is fine; the command layer composes them and maps a finding to a fixable
//! `ServerDiagnosis`. Side-effect-free except the transient port bind probe.

use std::net::TcpListener;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreflightFinding {
    EulaNotAccepted,
    PortInUse(u16),
    OrphanRunning(u32),
    LowDisk,
}

/// Advisory low-disk threshold (MB). Below this, a server create/start is
/// likely to fail or leave a half-written world; we surface an advisory but
/// never auto-act on the user's storage.
pub const LOW_DISK_THRESHOLD_MB: u64 = 500;

/// Pure threshold decision, separated so it is testable without a filesystem.
/// `None` free space (unreadable) is treated as "not low" — we never block on
/// an unknown, only on a confirmed-low reading.
pub fn low_disk_from_free(free_mb: Option<u64>, threshold_mb: u64) -> bool {
    matches!(free_mb, Some(free) if free < threshold_mb)
}

/// True when the filesystem holding the server runtime dir is below the
/// advisory threshold. Best-effort: an unreadable figure is "not low".
pub fn low_disk(runtime: &Path) -> bool {
    low_disk_from_free(crate::platform::free_disk_mb(runtime), LOW_DISK_THRESHOLD_MB)
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

    #[test]
    fn low_disk_from_free_uses_threshold() {
        assert!(low_disk_from_free(Some(100), 500), "100 MB free < 500 MB → low");
        assert!(!low_disk_from_free(Some(800), 500), "800 MB free ≥ 500 MB → ok");
        assert!(!low_disk_from_free(None, 500), "unknown free space → not flagged");
        assert!(!low_disk_from_free(Some(500), 500), "exactly at threshold → ok");
    }
}
