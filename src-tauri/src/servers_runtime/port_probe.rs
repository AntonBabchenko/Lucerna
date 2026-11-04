//! Would the Minecraft server's own bind on this port succeed? Answered by
//! binding a socket the way the server's JVM binds one, and **never listening**.
//!
//! Listening is what makes Windows Defender Firewall ask whether to let the
//! program accept connections; unanswered, it leaves «Query User» inbound Block
//! rules for the exe. Lucerna accepts no connections, so it must not listen. A
//! bound socket that never listens accepts nothing (a connect is refused), and
//! the OS decides a port conflict at `bind()` — `listen()` adds nothing to that
//! check on Windows or macOS. On Linux it adds one case no real program creates:
//! a holder bound with `SO_REUSEADDR` but not listening does not block this
//! bind, yet would make the server's `listen()` fail.
//!
//! The options mirror the JVM, measured against JDK 8 and 17 on Windows 11
//! (`docs/superpowers/specs/2026-11-04-port-probe-no-listen-design.md`):
//! - Windows: `SO_EXCLUSIVEADDRUSE`, the JVM's default (`sun.net.useExclusiveBind`).
//!   Without it a bind on all interfaces succeeds beside a holder on one
//!   address (127.0.0.1, the LAN IP), while the server's exclusive bind fails.
//! - Unix: `SO_REUSEADDR`, which the JVM sets on server sockets and std's
//!   `TcpListener` set for the probe before this module existed.
//! - IPv6 sockets are dual-stack (`IPV6_V6ONLY` off), as the JVM makes them.
//!
//! `socket2::Socket` can also dial. It never leaves this file — callers get a
//! [`BoundProbe`], which can do nothing but report where it is bound — and
//! `tests/structural_consented_dial.rs` keeps socket2 in this file only and
//! this file free of any dial.

use socket2::{Domain, Protocol, Socket, Type};
use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};

/// A bound socket that is not listening. Opaque on purpose: it offers no way to
/// listen or to dial, only where it is bound. Drop it to release the port.
#[derive(Debug)]
pub struct BoundProbe(Socket);

impl BoundProbe {
    /// The address the probe holds (with the OS-chosen port when bound to 0).
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.0
            .local_addr()?
            .as_socket()
            .ok_or_else(|| io::Error::other("probe socket has no IP address"))
    }
}

/// Bind a TCP socket on `addr` with the JVM's server-socket options. The socket
/// is returned bound and NOT listening.
pub fn bind_like_the_server(addr: SocketAddr) -> io::Result<BoundProbe> {
    let socket = Socket::new(Domain::for_address(addr), Type::STREAM, Some(Protocol::TCP))?;
    bind_configured(socket, addr)
}

/// Bind on every interface the way the JVM does when `server-ip` is empty: a
/// dual-stack `[::]` socket, or `0.0.0.0` when this host cannot create an IPv6
/// socket at all. That fallback is the JVM's own — it uses IPv4 whenever
/// creating an IPv6 socket fails, for any reason — so the IPv4 bind checks the
/// address the server would bind. Only socket creation falls back; an error
/// from the bind itself is the answer.
pub fn bind_all_interfaces_like_the_server(port: u16) -> io::Result<BoundProbe> {
    match Socket::new(Domain::IPV6, Type::STREAM, Some(Protocol::TCP)) {
        Ok(socket) => bind_configured(socket, SocketAddr::from((Ipv6Addr::UNSPECIFIED, port))),
        Err(_no_ipv6_socket) => {
            bind_like_the_server(SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)))
        }
    }
}

fn bind_configured(socket: Socket, addr: SocketAddr) -> io::Result<BoundProbe> {
    if addr.is_ipv6() {
        socket.set_only_v6(false)?;
    }
    set_server_bind_options(&socket)?;
    socket.bind(&addr.into())?;
    Ok(BoundProbe(socket))
}

#[cfg(windows)]
fn set_server_bind_options(socket: &Socket) -> io::Result<()> {
    use std::os::windows::io::AsRawSocket;
    use windows_sys::Win32::Networking::WinSock::{
        setsockopt, WSAGetLastError, SOCKET, SOL_SOCKET, SO_EXCLUSIVEADDRUSE,
    };
    let on: i32 = 1;
    // SAFETY: `socket` owns a live socket handle for the whole call; `optval`
    // points at a local i32 that outlives the call, and `optlen` is its exact
    // size, which is what SO_EXCLUSIVEADDRUSE (a BOOL) reads.
    let rc = unsafe {
        setsockopt(
            socket.as_raw_socket() as SOCKET,
            SOL_SOCKET,
            SO_EXCLUSIVEADDRUSE,
            (&on as *const i32).cast::<u8>(),
            std::mem::size_of::<i32>() as i32,
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        // SAFETY: reads the calling thread's last Winsock error; no arguments.
        Err(io::Error::from_raw_os_error(unsafe { WSAGetLastError() }))
    }
}

#[cfg(not(windows))]
fn set_server_bind_options(socket: &Socket) -> io::Result<()> {
    socket.set_reuse_address(true)
}
