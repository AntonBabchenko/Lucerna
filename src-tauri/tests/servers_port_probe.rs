//! The own-server port probe must not listen. A listening socket is what makes
//! Windows Defender Firewall ask whether to let Lucerna accept connections, and
//! an unanswered prompt leaves «Query User» Block rules behind
//! (`docs/superpowers/specs/2026-11-04-port-probe-no-listen-design.md`).
//!
//! "Not listening" is observed the way a peer would: a connect to the probe's
//! socket is not accepted. How it fails depends on the OS: Windows and Linux
//! answer the SYN with a reset (`ConnectionRefused`; Windows retries for about
//! two seconds first), macOS drops it unanswered, so the connect times out. A
//! listening socket on loopback would accept at once, so neither failure can
//! hide a listener. These tests dial, so they live here: the dial guard
//! (`structural_consented_dial.rs`) scans `src/`, where a dial outside
//! `network::consent` is forbidden.

use std::io::ErrorKind;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::time::Duration;

use lucerna_lib::servers_runtime::port_probe;

/// Long enough for Windows' ~2 s of SYN retries before it reports the reset;
/// also how long each macOS leg waits for its unanswered SYN.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

fn bound_port(probe: &port_probe::BoundProbe) -> u16 {
    probe.local_addr().expect("local addr").port()
}

/// Whether a failed connect shows that nothing accepted it: refused (Windows,
/// Linux) or unanswered (macOS).
fn not_accepted(kind: ErrorKind) -> bool {
    matches!(kind, ErrorKind::ConnectionRefused | ErrorKind::TimedOut)
}

fn assert_not_accepted(target: SocketAddr) {
    match TcpStream::connect_timeout(&target, CONNECT_TIMEOUT) {
        Ok(_) => panic!("{target} accepted a connection: the probe is listening"),
        Err(e) => assert!(
            not_accepted(e.kind()),
            "{target}: expected a refused or unanswered connect, got {e}"
        ),
    }
}

/// Like [`assert_not_accepted`], for `[::1]`, which a host can lack even when it
/// can create IPv6 sockets. "No IPv6 loopback" says nothing about the probe, so
/// that leg is skipped; an accepted connection still fails the test.
fn assert_not_accepted_unless_no_ipv6_loopback(target: SocketAddr) {
    match TcpStream::connect_timeout(&target, CONNECT_TIMEOUT) {
        Ok(_) => panic!("{target} accepted a connection: the probe is listening"),
        Err(e)
            if matches!(
                e.kind(),
                ErrorKind::AddrNotAvailable | ErrorKind::NetworkUnreachable
            ) =>
        {
            eprintln!("skipping {target}: this host has no IPv6 loopback ({e})");
        }
        Err(e) => assert!(
            not_accepted(e.kind()),
            "{target}: expected a refused or unanswered connect, got {e}"
        ),
    }
}

#[test]
fn the_probe_socket_accepts_no_connection() {
    let socket = port_probe::bind_like_the_server(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .expect("bind on loopback");
    let port = bound_port(&socket);
    assert_not_accepted(SocketAddr::from((Ipv4Addr::LOCALHOST, port)));
}

/// The path an empty `server-ip` takes: dual-stack `[::]` (or `0.0.0.0` on a
/// host without IPv6). Neither loopback family may reach it.
#[test]
fn the_all_interfaces_probe_accepts_no_connection() {
    let socket = port_probe::bind_all_interfaces_like_the_server(0).expect("bind all interfaces");
    let port = bound_port(&socket);
    assert_not_accepted(SocketAddr::from((Ipv4Addr::LOCALHOST, port)));
    if socket.local_addr().expect("local addr").is_ipv6() {
        assert_not_accepted_unless_no_ipv6_loopback(SocketAddr::from((Ipv6Addr::LOCALHOST, port)));
    }
}
