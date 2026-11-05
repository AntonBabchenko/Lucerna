//! Pure pre-spawn checks. Each helper returns `None`/`false` when the condition
//! is fine; the command layer composes them and maps a finding to a fixable
//! `ServerDiagnosis`. Side-effect-free except the transient port bind probe.

use std::io;
use std::net::{IpAddr, Ipv6Addr, SocketAddr};
use std::path::Path;

use crate::servers_runtime::port_probe;

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

/// Where the server's own bind lands, per `server-ip` in `server.properties`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindAddr {
    /// `server-ip` unset or empty: every interface.
    AllInterfaces,
    /// `server-ip` is an IP literal.
    Ip(IpAddr),
    /// Not knowable here: `server-ip` is a host name (the server resolves it at
    /// start; resolving it here would be a DNS lookup outside `network::`), or
    /// `server.properties` exists but cannot be read.
    Unknown,
}

/// The address the server binds for a raw `server-ip` value (`None` = key
/// absent). Mirrors the server: Java `Properties` drops a value's leading
/// whitespace and keeps the trailing, an empty value binds every interface,
/// `InetAddress.getByName` takes a bracketed IPv6 literal and returns a
/// v4-mapped one as IPv4. Anything else is a host name.
pub fn bind_addr_from(server_ip: Option<&str>) -> BindAddr {
    let value = server_ip.map(str::trim_start).unwrap_or_default();
    if value.is_empty() {
        return BindAddr::AllInterfaces;
    }
    let literal = match value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
        Some(inner) => inner.parse::<Ipv6Addr>().map(IpAddr::V6),
        None => value.parse::<IpAddr>(),
    };
    match literal {
        Ok(ip) => BindAddr::Ip(ip.to_canonical()),
        Err(_not_an_ip_literal) => BindAddr::Unknown,
    }
}

/// What a port probe established.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortCheck {
    /// The server's bind would succeed right now.
    Free,
    /// The server's bind would fail: something holds the port.
    InUse,
    /// Nothing established: no port (0 = unset), an unknowable bind address, or
    /// a bind error that says nothing about the port being taken.
    Unchecked,
}

/// Would the server's bind on `addr:port` succeed right now? A hint — the TOCTOU
/// race is acceptable for a diagnosis. The probe binds and never listens, so
/// Windows Firewall never asks to let Lucerna accept connections
/// (see [`port_probe`]).
pub fn check_port(addr: BindAddr, port: u16) -> PortCheck {
    if port == 0 {
        return PortCheck::Unchecked;
    }
    let bound = match addr {
        BindAddr::AllInterfaces => port_probe::bind_all_interfaces_like_the_server(port),
        BindAddr::Ip(ip) => port_probe::bind_like_the_server(SocketAddr::new(ip, port)),
        BindAddr::Unknown => return PortCheck::Unchecked,
    };
    classify_bind(bound.map(drop))
}

/// Which bind outcomes mean the port is taken. `AddrInUse` is the usual answer;
/// Windows answers `PermissionDenied` (WSAEACCES) when the holder is a JVM,
/// whose sockets are exclusive — measured against JDK 8 and 17. Any other
/// error (e.g. `server-ip` not assigned to this machine) says nothing about the
/// port, so it is not reported as "in use".
pub fn classify_bind(result: io::Result<()>) -> PortCheck {
    match result {
        Ok(()) => PortCheck::Free,
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::AddrInUse | io::ErrorKind::PermissionDenied
            ) =>
        {
            PortCheck::InUse
        }
        Err(_says_nothing_about_the_port) => PortCheck::Unchecked,
    }
}

/// How many ports above the busy one to probe before giving up. A small bounded
/// scan keeps the "Use port N" suggestion cheap; 64 candidates is far more than
/// any realistic local clash.
const FREE_PORT_SCAN: u16 = 64;

/// Find the next port at or after `from` that checks [`PortCheck::Free`] on
/// `addr`, never equal to `avoid` (the currently-configured port — suggesting it
/// would be a no-op). An unchecked port is skipped like a busy one: the "Use
/// port N" one-click fix must offer a port that is genuinely free. `None` if the
/// whole bounded window is busy or unchecked.
pub fn next_free_port(addr: BindAddr, from: u16, avoid: u16) -> Option<u16> {
    let start = from.max(1);
    (0..FREE_PORT_SCAN)
        .filter_map(|i| start.checked_add(i))
        .find(|&p| p != avoid && check_port(addr, p) == PortCheck::Free)
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
    use std::net::{Ipv4Addr, TcpListener};

    // Every port holder below listens on LOOPBACK only. A listener on all
    // interfaces makes Windows Firewall prompt for the test binary — the defect
    // this module's probe fixes; the old tests here did exactly that.
    const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

    fn hold_loopback_port() -> (TcpListener, u16) {
        let l = TcpListener::bind((LOOPBACK, 0)).unwrap();
        let port = l.local_addr().unwrap().port();
        (l, port)
    }

    #[test]
    fn port_zero_is_unchecked() {
        assert_eq!(check_port(BindAddr::AllInterfaces, 0), PortCheck::Unchecked);
    }

    #[test]
    fn a_held_port_reads_in_use_on_the_same_address() {
        let (_held, port) = hold_loopback_port();
        assert_eq!(
            check_port(BindAddr::Ip(LOOPBACK), port),
            PortCheck::InUse,
            "port {port} held on 127.0.0.1 must read as in use for server-ip=127.0.0.1"
        );
    }

    /// The server's bind on every interface fails while anything holds the port
    /// on one address — on Windows because the JVM binds exclusively, on Linux
    /// because the holder listens. Before the probe mirrored the JVM's
    /// `SO_EXCLUSIVEADDRUSE`, Windows read this port as free. macOS is left out
    /// on purpose: BSD `SO_REUSEADDR` lets the wildcard bind coexist with the
    /// loopback one, and the JVM there binds successfully too, so "free" is the
    /// true answer.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn a_port_held_on_loopback_reads_in_use_on_all_interfaces() {
        let (_held, port) = hold_loopback_port();
        assert_eq!(
            check_port(BindAddr::AllInterfaces, port),
            PortCheck::InUse,
            "port {port} held on 127.0.0.1 must read as in use for an empty server-ip"
        );
    }

    #[test]
    fn an_unknown_bind_address_is_never_checked_or_suggested() {
        assert_eq!(check_port(BindAddr::Unknown, 25565), PortCheck::Unchecked);
        assert_eq!(next_free_port(BindAddr::Unknown, 25565, 0), None);
    }

    #[test]
    fn classify_bind_reads_only_the_taken_errors_as_in_use() {
        use io::ErrorKind::*;
        assert_eq!(classify_bind(Ok(())), PortCheck::Free);
        assert_eq!(classify_bind(Err(AddrInUse.into())), PortCheck::InUse);
        // Windows' answer when a JVM holds the port exclusively.
        assert_eq!(
            classify_bind(Err(PermissionDenied.into())),
            PortCheck::InUse
        );
        // server-ip not assigned to this machine: not a port conflict.
        assert_eq!(
            classify_bind(Err(AddrNotAvailable.into())),
            PortCheck::Unchecked
        );
        assert_eq!(classify_bind(Err(Other.into())), PortCheck::Unchecked);
    }

    #[test]
    fn bind_addr_from_mirrors_the_servers_reading_of_server_ip() {
        use BindAddr::*;
        assert_eq!(bind_addr_from(None), AllInterfaces);
        assert_eq!(bind_addr_from(Some("")), AllInterfaces);
        assert_eq!(bind_addr_from(Some("   ")), AllInterfaces);
        assert_eq!(bind_addr_from(Some("127.0.0.1")), Ip(LOOPBACK));
        assert_eq!(bind_addr_from(Some("  127.0.0.1")), Ip(LOOPBACK));
        assert_eq!(
            bind_addr_from(Some("::1")),
            Ip(IpAddr::V6(Ipv6Addr::LOCALHOST))
        );
        assert_eq!(
            bind_addr_from(Some("[::1]")),
            Ip(IpAddr::V6(Ipv6Addr::LOCALHOST))
        );
        // getByName returns an Inet4Address for a v4-mapped literal.
        assert_eq!(bind_addr_from(Some("::ffff:127.0.0.1")), Ip(LOOPBACK));
        // Brackets are for IPv6 only; trailing whitespace survives Properties.
        assert_eq!(bind_addr_from(Some("[127.0.0.1]")), Unknown);
        assert_eq!(bind_addr_from(Some("127.0.0.1 ")), Unknown);
        assert_eq!(bind_addr_from(Some("play.example.org")), Unknown);
    }

    #[test]
    fn next_free_port_skips_an_in_use_port() {
        // Assert only race-free invariants (NOT `check_port(found) == Free`,
        // which re-binds and would flake under parallel tests). Guard the
        // u16::MAX scan-window edge.
        let (_held, held) = hold_loopback_port();
        if held >= u16::MAX - FREE_PORT_SCAN {
            return; // degenerate top-of-range: window can't fit, not worth asserting
        }
        let found = next_free_port(BindAddr::Ip(LOOPBACK), held, 0)
            .expect("a free port above the held one exists");
        assert_ne!(found, held, "an in-use port must be skipped");
        assert!(found > held, "scan moves forward from the in-use port");
    }

    #[test]
    fn next_free_port_never_returns_the_avoid_port() {
        // 25565 (Minecraft's default) is almost certainly free in the test env;
        // even so, with avoid == from the function must skip it and move forward.
        // Deterministic: `avoid` is excluded regardless of whether it is bindable.
        let found =
            next_free_port(BindAddr::AllInterfaces, 25565, 25565).expect("a nearby port is free");
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
