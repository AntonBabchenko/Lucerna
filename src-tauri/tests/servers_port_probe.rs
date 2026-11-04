//! The own-server port probe must not listen. A listening socket is what makes
//! Windows Defender Firewall ask whether to let Lucerna accept connections, and
//! an unanswered prompt leaves «Query User» Block rules behind
//! (`docs/superpowers/specs/2026-11-04-port-probe-no-listen-design.md`).
//!
//! "Not listening" is observed the way a peer would: a connect to the probe's
//! socket is refused. These tests dial, so they live here: the dial guard
//! (`structural_consented_dial.rs`) scans `src/`, where a dial outside
//! `network::consent` is forbidden.

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::time::Duration;

use lucerna_lib::servers_runtime::port_probe;

/// Windows retries a refused SYN for about two seconds before reporting it.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

fn bound_port(probe: &port_probe::BoundProbe) -> u16 {
    probe.local_addr().expect("local addr").port()
}

fn assert_refused(target: SocketAddr) {
    match TcpStream::connect_timeout(&target, CONNECT_TIMEOUT) {
        Ok(_) => panic!("{target} accepted a connection: the probe is listening"),
        Err(e) => assert_eq!(
            e.kind(),
            std::io::ErrorKind::ConnectionRefused,
            "{target}: expected a refused connect, got {e}"
        ),
    }
}

#[test]
fn the_probe_socket_accepts_no_connection() {
    let socket = port_probe::bind_like_the_server(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .expect("bind on loopback");
    let port = bound_port(&socket);
    assert_refused(SocketAddr::from((Ipv4Addr::LOCALHOST, port)));
}

/// The path an empty `server-ip` takes: dual-stack `[::]` (or `0.0.0.0` on a
/// host without IPv6). Neither loopback family may reach it.
#[test]
fn the_all_interfaces_probe_accepts_no_connection() {
    let socket = port_probe::bind_all_interfaces_like_the_server(0).expect("bind all interfaces");
    let port = bound_port(&socket);
    assert_refused(SocketAddr::from((Ipv4Addr::LOCALHOST, port)));
    if socket.local_addr().expect("local addr").is_ipv6() {
        assert_refused(SocketAddr::from((Ipv6Addr::LOCALHOST, port)));
    }
}
