//! Parser for "modded connection rejected — missing/mismatched mods"
//! lines in a Minecraft client log. Pure: no I/O, no network. One
//! recognizer per supported loader/format (anchors captured in the
//! Phase 0 spike). `parse_server_mod_rejection` tries each in order and
//! returns the first non-empty result; an unrecognized log yields an
//! empty vec so the feature simply does not appear (zero false positives).

use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use specta::Type;

/// One mod the server said the client is missing or running at the wrong
/// version. `id` is the raw mod-id from the log (e.g. "farmersdelight",
/// "create") — NOT a Modrinth slug or platform id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CitedMod {
    pub id: String,
    pub version: Option<String>,
    pub kind: CitedKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CitedKind {
    Missing,
    VersionMismatch,
}

/// Parse a (capped) client-log body into the cited mods. Empty when no
/// known reject format is recognized. Dedups by `id` (first occurrence
/// wins, preserving order).
pub fn parse_server_mod_rejection(log: &str) -> Vec<CitedMod> {
    if let Some(mods) = parse_forge_fml_channels(log) {
        return mods;
    }
    Vec::new()
}

/// Modern Forge / FML (1.13+, confirmed against 47.4.10 in Phase 0) rejects a
/// client that lacks mods registering a mandatory network channel. The client
/// log reads, e.g.:
///
/// ```text
/// ...HandshakeHandler/FMLHANDSHAKE]: Channels [farmersdelight:main] rejected their client side version number
/// ...HandshakeHandler/FMLHANDSHAKE]: Terminating connection with server, mismatched mod list
/// ```
///
/// Each bracket token is `<modid>:<channel>`; we take the mod-id. Only mods
/// with an enforced channel appear here — but that is exactly the set that
/// actually blocked the connection. FML gives no version on this line and does
/// not distinguish "absent" from "wrong version", so kind is always `Missing`.
fn parse_forge_fml_channels(log: &str) -> Option<Vec<CitedMod>> {
    // Gate on the definitive terminator so a stray "Channels [...]" log line
    // (e.g. a channel-registration message) never trips the parser.
    if !log.contains("mismatched mod list") {
        return None;
    }
    let mut out: Vec<CitedMod> = Vec::new();
    for caps in FORGE_CHANNEL_REJECT_RE.captures_iter(log) {
        for token in caps[1].split(',') {
            let modid = token.split(':').next().unwrap_or("").trim();
            if modid.is_empty() {
                continue;
            }
            if out.iter().any(|m| m.id.eq_ignore_ascii_case(modid)) {
                continue; // dedup by id, first wins
            }
            out.push(CitedMod {
                id: modid.to_string(),
                version: None,
                kind: CitedKind::Missing,
            });
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Captures the bracketed `modid:channel` list from a FML channel-rejection
/// line: `Channels [a:main, b:net] rejected their client side version number`.
static FORGE_CHANNEL_REJECT_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"Channels \[([^\]]+)\] rejected their (?:client|server) side version number")
        .expect("forge channel-reject regex compiles — covered by tests")
});

#[cfg(test)]
mod tests {
    use super::*;

    // Real excerpt captured in the Phase 0 spike: modern Forge 47.4.10 client
    // missing Farmer's Delight (which registers a mandatory network channel),
    // joining a server that has it. Timestamps simplified; reject text verbatim.
    const FORGE_REJECT: &str = "\
[01:39:12.984] [Netty Client IO #1/ERROR] [net.minecraftforge.network.HandshakeHandler/FMLHANDSHAKE]: Channels [farmersdelight:main] rejected their client side version number
[01:39:12.985] [Netty Client IO #1/ERROR] [net.minecraftforge.network.HandshakeHandler/FMLHANDSHAKE]: Terminating connection with server, mismatched mod list
[01:39:20.191] [Render thread/INFO] [net.minecraft.client.Minecraft/]: Stopping!";

    #[test]
    fn returns_empty_for_unrecognized_log() {
        let log = "[12:00:00] [main/INFO]: Connecting to play.example.com\n\
                   [12:00:01] [main/INFO]: Connected, joining world";
        assert!(parse_server_mod_rejection(log).is_empty());
    }

    #[test]
    fn parses_forge_fml_channel_reject() {
        let mods = parse_server_mod_rejection(FORGE_REJECT);
        assert_eq!(mods.len(), 1);
        assert_eq!(mods[0].id, "farmersdelight");
        assert_eq!(mods[0].kind, CitedKind::Missing);
        assert_eq!(mods[0].version, None);
    }

    #[test]
    fn parses_multiple_channels_and_dedups() {
        let log = "[ERROR] [HandshakeHandler/FMLHANDSHAKE]: Channels [create:main, farmersdelight:main, create:main] rejected their client side version number\n\
                   [ERROR] [HandshakeHandler/FMLHANDSHAKE]: Terminating connection with server, mismatched mod list";
        let mods = parse_server_mod_rejection(log);
        let ids: Vec<&str> = mods.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["create", "farmersdelight"]);
    }

    #[test]
    fn no_match_without_mismatch_terminator() {
        // A stray channel mention without the terminator is a registration
        // message, not a rejection — must not produce false positives.
        let log = "[INFO]: Registered Channels [foo:main] for mod foo";
        assert!(parse_server_mod_rejection(log).is_empty());
    }
}
