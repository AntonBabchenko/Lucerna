//! The Quick Play launch target + its boundary validation.
//!
//! `QuickPlay` crosses the IPC boundary (specta-exported) and is threaded
//! through `launch::start` into `args::build_argv`, where it selectively
//! enables the MC 1.20+ quick-play feature args.

use crate::error::Error;
use serde::{Deserialize, Serialize};
use specta::Type;

/// A direct-launch target. Singleplayer carries the world's save-folder
/// name (the `saves/<folder>` segment); Multiplayer carries a server
/// address (`host` or `host:port`).
// `Serialize` + `PartialEq`: a Quick Play target also travels OUT to the
// frontend as part of a `cli::LaunchIntent` (a desktop shortcut's argv), and
// the cli/shortcut round-trip tests compare targets directly.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum QuickPlay {
    Singleplayer { world: String },
    Multiplayer { address: String },
}

impl QuickPlay {
    /// Boundary validation. Singleplayer reuses the worlds path-segment
    /// gate; Multiplayer uses [`validate_server_address`].
    pub fn validate(&self) -> Result<(), Error> {
        match self {
            QuickPlay::Singleplayer { world } => crate::worlds::fs::validate_segment(world),
            QuickPlay::Multiplayer { address } => validate_server_address(address),
        }
    }
}

/// 253 (max hostname per RFC 1123) + 1 (`:`) + 5 (max port digits) + 1 slack.
const MAX_ADDRESS_LEN: usize = 260;

/// The port Minecraft uses when an address carries none.
pub const DEFAULT_SERVER_PORT: u16 = 25565;

/// Parse a `host` or `host:port` server address into its parts, applying
/// [`DEFAULT_SERVER_PORT`] when no port is given.
///
/// This is the single definition of what a server address *is*: the launch path
/// takes it through [`validate_server_address`] (same rules, parsed value
/// dropped) and the server-ping path uses the parsed pair. Keeping one parser
/// means the two can never drift into disagreeing about a port — an earlier
/// version validated here and re-split at the ping site, where a future
/// loosening of the rules (IPv6, say) would have silently substituted the
/// default port instead of failing.
///
/// The value is passed to the game as a single argv token (no shell), so this is
/// hygiene + clear UX, not shell-injection defense. Rules: non-empty, no ASCII
/// whitespace, no control chars, length <= [`MAX_ADDRESS_LEN`], and if a single
/// `:` is present the suffix must parse as a `u16` port.
pub fn parse_server_address(address: &str) -> Result<(String, u16), Error> {
    let invalid = |reason: &str| Error::QuickPlayAddressInvalid {
        address: address.to_string(),
        reason: reason.to_string(),
    };

    if address.is_empty() {
        return Err(invalid("empty address"));
    }
    if address.len() > MAX_ADDRESS_LEN {
        return Err(invalid("address too long"));
    }
    if address.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(invalid("contains whitespace or control characters"));
    }
    // Optional single `:port`. Reject multiple colons (IPv6 literals are
    // not supported in v1 — keep the validator simple and explicit).
    let Some((host, port)) = address.split_once(':') else {
        return Ok((address.to_string(), DEFAULT_SERVER_PORT));
    };
    if host.is_empty() {
        return Err(invalid("missing host before ':'"));
    }
    if port.contains(':') {
        return Err(invalid("multiple ':' separators (IPv6 not supported)"));
    }
    if port.is_empty() {
        return Err(invalid("missing port after ':'"));
    }
    let port = port
        .parse::<u16>()
        .map_err(|_| invalid("port must be a number 0-65535"))?;
    Ok((host.to_string(), port))
}

/// [`parse_server_address`] with the parsed value discarded — for callers that
/// only need the address to be well-formed.
pub fn validate_server_address(address: &str) -> Result<(), Error> {
    parse_server_address(address).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_plain_host() {
        assert!(validate_server_address("mc.example.net").is_ok());
    }

    #[test]
    fn parse_applies_the_default_port_when_none_is_given() {
        assert_eq!(
            parse_server_address("mc.example.net").expect("valid"),
            ("mc.example.net".to_string(), DEFAULT_SERVER_PORT)
        );
    }

    #[test]
    fn parse_returns_the_explicit_port() {
        assert_eq!(
            parse_server_address("mc.example.net:25566").expect("valid"),
            ("mc.example.net".to_string(), 25566)
        );
        // Port 0 is well-formed as far as parsing goes; the OS rejects the dial.
        assert_eq!(parse_server_address("h:0").expect("valid").1, 0);
        assert_eq!(parse_server_address("h:65535").expect("valid").1, 65535);
    }

    #[test]
    fn parse_never_substitutes_a_port_for_a_rejected_address() {
        // The whole reason parsing and validation share one function: a bad port
        // must fail, never silently fall back to the default.
        for bad in ["host:abc", "host:99999", "host:", "a:b:c", ":25565"] {
            assert!(parse_server_address(bad).is_err(), "{bad} must be rejected");
        }
    }

    #[test]
    fn accepts_host_with_port() {
        assert!(validate_server_address("mc.example.net:25566").is_ok());
        assert!(validate_server_address("192.168.1.10:25565").is_ok());
    }

    #[test]
    fn rejects_empty() {
        assert!(matches!(
            validate_server_address(""),
            Err(Error::QuickPlayAddressInvalid { .. })
        ));
    }

    #[test]
    fn rejects_whitespace_and_control() {
        assert!(validate_server_address("mc example.net").is_err());
        assert!(validate_server_address("mc\texample.net").is_err());
        assert!(validate_server_address("mc\u{0}.net").is_err());
    }

    #[test]
    fn rejects_bad_port() {
        assert!(validate_server_address("host:abc").is_err());
        assert!(validate_server_address("host:99999").is_err()); // > u16::MAX
        assert!(validate_server_address("host:").is_err());
    }

    #[test]
    fn rejects_missing_host() {
        assert!(validate_server_address(":25565").is_err());
    }

    #[test]
    fn rejects_multiple_colons() {
        assert!(matches!(
            validate_server_address("a:b:c"),
            Err(Error::QuickPlayAddressInvalid { ref reason, .. })
                if reason.contains("multiple")
        ));
    }

    #[test]
    fn rejects_overlong() {
        let long = format!("{}.net", "x".repeat(300));
        assert!(validate_server_address(&long).is_err());
    }

    #[test]
    fn validate_dispatches_singleplayer_through_segment_gate() {
        let bad = QuickPlay::Singleplayer {
            world: "../escape".into(),
        };
        assert!(bad.validate().is_err());
        let ok = QuickPlay::Singleplayer {
            world: "My World".into(),
        };
        assert!(ok.validate().is_ok());
    }

    #[test]
    fn validate_dispatches_multiplayer_through_address_gate() {
        let bad = QuickPlay::Multiplayer {
            address: "bad host".into(),
        };
        assert!(bad.validate().is_err());
        let ok = QuickPlay::Multiplayer {
            address: "mc.example.net".into(),
        };
        assert!(ok.validate().is_ok());
    }
}
