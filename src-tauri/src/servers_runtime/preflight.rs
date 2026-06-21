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
    low_disk_from_free(
        crate::platform::free_disk_mb(runtime),
        LOW_DISK_THRESHOLD_MB,
    )
}

/// True iff `port` cannot be bound on 0.0.0.0 right now (a hint — the TOCTOU
/// race is acceptable for a diagnosis). Port 0 (unset) is never "in use".
pub fn port_in_use(port: u16) -> bool {
    if port == 0 {
        return false;
    }
    TcpListener::bind(("0.0.0.0", port)).is_err()
}

/// How many ports above the busy one to probe before giving up. A small bounded
/// scan keeps the "Use port N" suggestion cheap; 64 candidates is far more than
/// any realistic local clash.
const FREE_PORT_SCAN: u16 = 64;

/// Find the next actually-bindable port at or after `from`, never equal to
/// `avoid` (the currently-configured port — suggesting it would be a no-op).
/// Returns `None` if the whole bounded window is busy. Used to drive the
/// "Use port N" one-click fix with a port that is genuinely free, instead of a
/// blind `current + 1` that might also be taken or unchanged.
pub fn next_free_port(from: u16, avoid: u16) -> Option<u16> {
    let start = from.max(1);
    (0..FREE_PORT_SCAN)
        .filter_map(|i| start.checked_add(i))
        .find(|&p| p != avoid && !port_in_use(p))
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
    fn next_free_port_skips_an_in_use_port() {
        // Hold a port so it reads as in-use; the finder must skip past it. Bind on
        // 0.0.0.0 to match `port_in_use`'s probe address (a 127.0.0.1 hold would not
        // collide on Windows, so the port would read as free). Assert only race-free
        // invariants (NOT `!port_in_use(found)`, which re-binds and would flake under
        // parallel tests). Guard the u16::MAX scan-window edge.
        let l = TcpListener::bind(("0.0.0.0", 0)).unwrap();
        let held = l.local_addr().unwrap().port();
        if held >= u16::MAX - FREE_PORT_SCAN {
            return; // degenerate top-of-range: window can't fit, not worth asserting
        }
        let found = next_free_port(held, 0).expect("a free port above the held one exists");
        assert_ne!(found, held, "an in-use port must be skipped");
        assert!(found > held, "scan moves forward from the in-use port");
    }

    #[test]
    fn next_free_port_never_returns_the_avoid_port() {
        // 25565 (Minecraft's default) is almost certainly free in the test env;
        // even so, with avoid == from the function must skip it and move forward.
        // Deterministic: `avoid` is excluded regardless of whether it is bindable.
        let found = next_free_port(25565, 25565).expect("a nearby port is free");
        assert_ne!(found, 25565, "must never suggest the avoid (current) port");
        assert!(found > 25565, "scans forward from the avoid port");
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
        assert!(
            low_disk_from_free(Some(100), 500),
            "100 MB free < 500 MB → low"
        );
        assert!(
            !low_disk_from_free(Some(800), 500),
            "800 MB free ≥ 500 MB → ok"
        );
        assert!(
            !low_disk_from_free(None, 500),
            "unknown free space → not flagged"
        );
        assert!(
            !low_disk_from_free(Some(500), 500),
            "exactly at threshold → ok"
        );
    }
}
