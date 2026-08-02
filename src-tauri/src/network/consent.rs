//! Second outbound tier: a **user-consented dial** to a host the USER supplied.
//!
//! The HTTP allowlist (`network::allowlist`) covers destinations Lucerna
//! chooses. It deliberately cannot cover a hostname the user typed — putting
//! one there would punch a hole in the guarantee that the launcher cannot
//! reach an unlisted host, and that hole would apply to downloads too. This
//! module is the narrow alternative: an opaque, consent-checked TCP stream.
//!
//! Unbypassable by construction: `ConsentedTcp`'s socket is private, so the
//! only way to obtain one is [`ConsentedTcp::open`], which refuses unless the
//! channel's Settings permission is on. `structural_consented_dial.rs` keeps
//! the socket type confined to this file and fails the build if the consent
//! check is removed. See `docs/PRINCIPLES.md` Part A commitment 4.

use crate::error::{Error, Result};
use crate::instances::schema::GeneralSettings;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Hard ceiling on simultaneous consented dials, process-wide. It lives in the
/// primitive so no caller can opt out: a launcher opening fifty sockets at once
/// would behave like a port scanner.
const MAX_CONCURRENT_DIALS: usize = 4;

/// Budget for DNS resolution plus the TCP handshake.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

/// A named outbound channel to a user-supplied host. One variant per Settings
/// permission; the "play with a friend" work will add its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsentedChannel {
    /// Server List Ping to the user's own saved multiplayer servers.
    ServerPing,
    /// AI translation pre-fill — either a cloud provider on the allowlist or
    /// a local model server via `network::loopback`.
    AiTranslation,
}

impl ConsentedChannel {
    /// Stable id carried in [`Error::ConsentedChannelDisabled`] so the UI can
    /// name the setting to turn on. Never localise or repurpose these strings.
    pub fn id(self) -> &'static str {
        match self {
            ConsentedChannel::ServerPing => "server_ping",
            ConsentedChannel::AiTranslation => "ai_translation",
        }
    }

    fn is_enabled(self, general: &GeneralSettings) -> bool {
        match self {
            ConsentedChannel::ServerPing => general.allow_server_ping,
            ConsentedChannel::AiTranslation => general.allow_ai_translation,
        }
    }
}

/// The consent gate, split out as a pure function so it is unit-testable
/// without a Tauri handle.
fn ensure_enabled(channel: ConsentedChannel, general: &GeneralSettings) -> Result<()> {
    if channel.is_enabled(general) {
        return Ok(());
    }
    Err(Error::ConsentedChannelDisabled {
        channel: channel.id().to_string(),
    })
}

fn dial_slots() -> &'static Arc<Semaphore> {
    static SLOTS: OnceLock<Arc<Semaphore>> = OnceLock::new();
    SLOTS.get_or_init(|| Arc::new(Semaphore::new(MAX_CONCURRENT_DIALS)))
}

/// An opened, consent-checked TCP stream to a user-supplied host. The
/// concurrency permit is held for the stream's whole life, so a slow peer
/// occupies one of the [`MAX_CONCURRENT_DIALS`] slots rather than being free.
pub struct ConsentedTcp {
    stream: TcpStream,
    _permit: OwnedSemaphorePermit,
}

impl ConsentedTcp {
    /// Open a consented connection to `host:port`.
    ///
    /// The settings file is re-read on every call — there is deliberately no
    /// cached consent, so turning the permission off takes effect immediately
    /// and there is no stale-permission window.
    ///
    /// `host` and `port` are user data (a saved server address): they are never
    /// logged, and the returned error carries no host either.
    pub async fn open(
        app: &tauri::AppHandle,
        channel: ConsentedChannel,
        host: &str,
        port: u16,
    ) -> Result<Self> {
        let path = crate::paths::app_file(app).map_err(|e| Error::io("<app_file>", e))?;
        let settings = crate::instances::store::read_app_json(&path)?;
        ensure_enabled(channel, &settings.general)?;

        // Wait for a slot BEFORE dialing, and outside the connect timeout, so a
        // queued dial is not charged for the time it spent waiting.
        let permit = dial_slots().clone().acquire_owned().await.map_err(|_| {
            Error::ConsentedChannelDisabled {
                channel: channel.id().to_string(),
            }
        })?;

        let stream = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect((host, port)))
            .await
            .map_err(|_| {
                Error::io(
                    "<consented-dial>",
                    std::io::Error::new(std::io::ErrorKind::TimedOut, "connect timed out"),
                )
            })?
            .map_err(|e| Error::io("<consented-dial>", e))?;

        Ok(Self {
            stream,
            _permit: permit,
        })
    }
}

/// Check a channel's consent without opening a socket. HTTP channels (the AI
/// pre-fill) need the same gate as a raw dial but reach the network through
/// `network::request` / `network::loopback` instead of [`ConsentedTcp`].
///
/// Re-reads `app.json` on every call, exactly like [`ConsentedTcp::open`]:
/// turning the permission off must take effect immediately, not at restart.
///
/// Deliberately placed BELOW `impl ConsentedTcp` so the first occurrence of
/// the consent-call expression in this file stays the one inside `open` —
/// see `structural_consented_dial.rs`.
pub fn ensure_channel_enabled(app: &tauri::AppHandle, channel: ConsentedChannel) -> Result<()> {
    let path = crate::paths::app_file(app).map_err(|e| Error::io("<app_file>", e))?;
    let settings = crate::instances::store::read_app_json(&path)?;
    ensure_enabled(channel, &settings.general)?;
    Ok(())
}

// Delegating impls keep the socket private while letting protocol code stay
// generic over `AsyncRead + AsyncWrite` — and therefore testable over
// `tokio::io::duplex()` with no network at all.
impl AsyncRead for ConsentedTcp {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_read(cx, buf)
    }
}

impl AsyncWrite for ConsentedTcp {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.stream).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_ping_disabled_by_default() {
        let g = GeneralSettings::default();
        assert!(matches!(
            ensure_enabled(ConsentedChannel::ServerPing, &g),
            Err(Error::ConsentedChannelDisabled { ref channel })
                if channel == "server_ping"
        ));
    }

    #[test]
    fn server_ping_allowed_once_opted_in() {
        let g = GeneralSettings {
            allow_server_ping: true,
            ..Default::default()
        };
        assert!(ensure_enabled(ConsentedChannel::ServerPing, &g).is_ok());
    }

    #[test]
    fn channel_id_is_stable() {
        // The id is part of the IPC contract (format-error maps it to a setting
        // name) — changing it is a breaking change, not a rename.
        assert_eq!(ConsentedChannel::ServerPing.id(), "server_ping");
    }

    #[test]
    fn ai_translation_channel_follows_its_own_setting() {
        let mut g = GeneralSettings::default();
        assert!(ensure_enabled(ConsentedChannel::AiTranslation, &g).is_err());
        g.allow_ai_translation = true;
        assert!(ensure_enabled(ConsentedChannel::AiTranslation, &g).is_ok());
        // The two channels are independent.
        assert!(ensure_enabled(ConsentedChannel::ServerPing, &g).is_err());
    }

    #[test]
    fn ai_translation_error_names_its_setting() {
        let g = GeneralSettings::default();
        let err = ensure_enabled(ConsentedChannel::AiTranslation, &g).expect_err("off by default");
        assert!(matches!(
            err,
            crate::error::Error::ConsentedChannelDisabled { ref channel } if channel == "ai_translation"
        ));
    }
}
