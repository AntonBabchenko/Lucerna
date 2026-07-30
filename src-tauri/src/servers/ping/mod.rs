//! Server List Ping (modern protocol, MC 1.7+).
//!
//! The socket comes from [`crate::network::consent`], which refuses to dial
//! unless the user turned the permission on; this module only speaks the
//! protocol. [`exchange`] is generic over the byte stream, so every protocol
//! case is tested over `tokio::io::duplex()` with no network involved.
//!
//! Not supported, on purpose: legacy `0xFE 0x01` servers (MC ≤ 1.6) and hosts
//! that publish their real port only via a `_minecraft._tcp` SRV record. Both
//! surface as [`ServerPingOutcome::NoAnswer`], which the UI words as "no
//! answer" — never as a claim that the server is down.

pub mod codec;
pub mod status;

use serde::Serialize;
use specta::Type;
use status::Status;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Protocol version sent in the handshake. `-1` means "undetermined", which is
/// what a status-only client is expected to send.
const PROTOCOL_VERSION_UNDETERMINED: i32 = -1;
/// Upper bound on a status packet. A MOTD plus a 64×64 base64 favicon runs to a
/// few tens of KiB; a quarter-megabyte is generous and still bounded.
const MAX_PACKET_LEN: usize = 256 * 1024;
/// Budget for handshake + status response once connected.
const EXCHANGE_TIMEOUT: Duration = Duration::from_secs(5);

/// What a ping found. `NoAnswer` covers refused, timed out and malformed alike:
/// the difference is diagnostic noise, not something to show a player.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ServerPingOutcome {
    Online {
        players_online: Option<u32>,
        players_max: Option<u32>,
        version_name: Option<String>,
        motd: Option<String>,
        /// Time from the handshake being written to the status response
        /// arriving. The TCP handshake is not included.
        latency_ms: u32,
    },
    NoAnswer,
}

/// Ping one saved server. Consent is checked inside `ConsentedTcp::open`, so a
/// disabled permission surfaces as `Err(ConsentedChannelDisabled)` with no
/// packet sent. Everything else that goes wrong is `Ok(NoAnswer)` — a server
/// being unreachable is an answer, not a launcher error.
pub async fn ping_address(
    app: &tauri::AppHandle,
    address: &str,
) -> crate::error::Result<ServerPingOutcome> {
    // One shared parser with the launch path, so ping and connect can never
    // disagree about which host:port an entry means.
    let (host, port) = crate::launch::quick_play::parse_server_address(address)?;
    let mut io = match crate::network::consent::ConsentedTcp::open(
        app,
        crate::network::consent::ConsentedChannel::ServerPing,
        &host,
        port,
    )
    .await
    {
        Ok(io) => io,
        // A disabled permission is a real error the UI must surface; a failed
        // dial is just "no answer". Addresses are never logged either way.
        Err(e @ crate::error::Error::ConsentedChannelDisabled { .. }) => return Err(e),
        Err(_) => return Ok(ServerPingOutcome::NoAnswer),
    };

    let started = std::time::Instant::now();
    match exchange(&mut io, &host, port, EXCHANGE_TIMEOUT).await {
        Ok(s) => Ok(ServerPingOutcome::Online {
            players_online: s.players_online,
            players_max: s.players_max,
            version_name: s.version_name,
            motd: s.motd,
            latency_ms: started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32,
        }),
        Err(()) => {
            // Host withheld deliberately: saved-server addresses are user data
            // and the launcher log is shareable.
            crate::diag!("slp: no usable status response from a saved server");
            Ok(ServerPingOutcome::NoAnswer)
        }
    }
}

/// Handshake → status request → status response, over any byte stream, bounded
/// by `timeout`.
pub async fn exchange<S: AsyncRead + AsyncWrite + Unpin>(
    io: &mut S,
    host: &str,
    port: u16,
    timeout: Duration,
) -> Result<Status, ()> {
    match tokio::time::timeout(timeout, exchange_inner(io, host, port)).await {
        Ok(result) => result,
        Err(_) => Err(()),
    }
}

async fn exchange_inner<S: AsyncRead + AsyncWrite + Unpin>(
    io: &mut S,
    host: &str,
    port: u16,
) -> Result<Status, ()> {
    let mut handshake = Vec::new();
    codec::write_varint(&mut handshake, 0x00); // packet id: handshake
    codec::write_varint(&mut handshake, PROTOCOL_VERSION_UNDETERMINED);
    codec::write_string(&mut handshake, host);
    handshake.extend_from_slice(&port.to_be_bytes());
    codec::write_varint(&mut handshake, 1); // next state: status

    let mut out = Vec::new();
    codec::write_varint(&mut out, handshake.len() as i32);
    out.extend_from_slice(&handshake);
    // Status request: one packet whose body is just the id 0x00.
    codec::write_varint(&mut out, 1);
    codec::write_varint(&mut out, 0x00);

    io.write_all(&out).await.map_err(|_| ())?;
    io.flush().await.map_err(|_| ())?;

    let len = read_varint_from(io).await?;
    // Refuse an absurd declared length before allocating or waiting for it.
    if len <= 0 || len as usize > MAX_PACKET_LEN {
        return Err(());
    }
    let mut body = vec![0u8; len as usize];
    io.read_exact(&mut body).await.map_err(|_| ())?;

    let (packet_id, used) = codec::read_varint(&body).map_err(|_| ())?;
    if packet_id != 0x00 {
        return Err(());
    }
    let (json, _) = codec::read_string(&body[used..], MAX_PACKET_LEN).map_err(|_| ())?;
    status::parse_status(&json)
}

/// Read a VarInt one byte at a time — the only way to frame it on a stream.
/// Bounded to 5 bytes, so a peer sending continuation bytes forever cannot
/// stall us past the caller's timeout with unbounded buffering.
async fn read_varint_from<S: AsyncRead + Unpin>(io: &mut S) -> Result<i32, ()> {
    let mut buf = Vec::with_capacity(5);
    for _ in 0..5 {
        let mut b = [0u8; 1];
        io.read_exact(&mut b).await.map_err(|_| ())?;
        buf.push(b[0]);
        match codec::read_varint(&buf) {
            Ok((v, _)) => return Ok(v),
            Err(codec::CodecError::Incomplete) => continue,
            Err(_) => return Err(()),
        }
    }
    Err(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: Duration = Duration::from_millis(200);

    /// Frame a status JSON the way a real server does: a length-prefixed packet
    /// whose body is the id 0x00 followed by a length-prefixed JSON string.
    fn framed_status(json: &str) -> Vec<u8> {
        let mut body = Vec::new();
        codec::write_varint(&mut body, 0x00);
        codec::write_string(&mut body, json);
        let mut out = Vec::new();
        codec::write_varint(&mut out, body.len() as i32);
        out.extend_from_slice(&body);
        out
    }

    /// Drive `exchange` against a scripted peer over an in-memory duplex.
    async fn exchange_against(reply: Vec<u8>) -> Result<Status, ()> {
        let (mut client, mut server) = tokio::io::duplex(64 * 1024);
        let peer = tokio::spawn(async move {
            let mut scratch = [0u8; 1024];
            // Consume whatever the client sends (handshake + status request).
            let _ = server.read(&mut scratch).await;
            let _ = server.write_all(&reply).await;
            let _ = server.flush().await;
        });
        let out = exchange(&mut client, "mc.example.net", 25565, T).await;
        let _ = peer.await;
        out
    }

    #[tokio::test]
    async fn happy_path_reads_players_version_and_motd() {
        let reply = framed_status(
            r#"{"version":{"name":"1.21.4"},"players":{"online":7,"max":40},"description":"Hi"}"#,
        );
        let s = exchange_against(reply).await.expect("status");
        assert_eq!(s.players_online, Some(7));
        assert_eq!(s.players_max, Some(40));
        assert_eq!(s.version_name.as_deref(), Some("1.21.4"));
        assert_eq!(s.motd.as_deref(), Some("Hi"));
    }

    #[tokio::test]
    async fn http_style_garbage_is_an_error() {
        let reply = b"HTTP/1.1 400 Bad Request\r\n\r\n".to_vec();
        assert!(exchange_against(reply).await.is_err());
    }

    #[tokio::test]
    async fn wrong_packet_id_is_an_error() {
        let mut body = Vec::new();
        codec::write_varint(&mut body, 0x01); // pong, not a status response
        codec::write_string(&mut body, "{}");
        let mut reply = Vec::new();
        codec::write_varint(&mut reply, body.len() as i32);
        reply.extend_from_slice(&body);
        assert!(exchange_against(reply).await.is_err());
    }

    #[tokio::test]
    async fn non_status_json_is_an_error() {
        assert!(exchange_against(framed_status("[1,2,3]")).await.is_err());
    }

    #[tokio::test]
    async fn oversized_declared_length_is_refused_up_front() {
        // Declares 8 MiB and sends nothing: must fail on the length, promptly,
        // instead of waiting for 8 MiB to arrive.
        let mut reply = Vec::new();
        codec::write_varint(&mut reply, 8 * 1024 * 1024);
        let started = std::time::Instant::now();
        assert!(exchange_against(reply).await.is_err());
        assert!(
            started.elapsed() < T,
            "must not wait for the declared bytes"
        );
    }

    #[tokio::test]
    async fn silent_peer_times_out() {
        // `_server` stays bound so the duplex is not closed — the read blocks
        // rather than hitting EOF, which is what a real silent server does.
        let (mut client, _server) = tokio::io::duplex(4096);
        let r = exchange(&mut client, "h", 25565, Duration::from_millis(30)).await;
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn handshake_carries_the_host_and_port_it_was_given() {
        // The server matches the handshake's address/port against its own config
        // (virtual hosts, BungeeCord), so getting these wrong silently changes
        // which server answers.
        let mut expected = Vec::new();
        codec::write_varint(&mut expected, 0x00);
        codec::write_varint(&mut expected, PROTOCOL_VERSION_UNDETERMINED);
        codec::write_string(&mut expected, "mc.example.net");
        expected.extend_from_slice(&25566u16.to_be_bytes());
        codec::write_varint(&mut expected, 1);

        let (mut client, mut server) = tokio::io::duplex(4096);
        let peer = tokio::spawn(async move {
            let mut buf = vec![0u8; 128];
            let n = server.read(&mut buf).await.expect("reads handshake");
            buf.truncate(n);
            buf
        });
        // No reply is written, so the exchange times out — we only care about
        // what went out on the wire.
        let _ = exchange(
            &mut client,
            "mc.example.net",
            25566,
            Duration::from_millis(30),
        )
        .await;
        let sent = peer.await.expect("peer task");
        assert!(
            sent.starts_with(&{
                let mut framed = Vec::new();
                codec::write_varint(&mut framed, expected.len() as i32);
                framed.extend_from_slice(&expected);
                framed
            }),
            "handshake bytes did not match"
        );
    }
}
