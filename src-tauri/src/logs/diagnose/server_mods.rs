//! Parser for "modded connection rejected — missing/mismatched mods"
//! lines in a Minecraft client log. Pure: no I/O, no network. One
//! recognizer per supported loader/format (anchors captured in the
//! Phase 0 spike). `parse_server_mod_rejection` runs every recognizer and
//! merges their results; an unrecognized log yields an empty vec so the
//! feature simply does not appear (zero false positives).

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

/// Parse a (capped) client-log body into the cited mods. Runs every format
/// recognizer and MERGES their results — a single rejection can mix formats
/// (e.g. a datapack-registry failure for a library mod plus a channel mismatch
/// for others). Empty when nothing is recognized. Deduped by `id`
/// (case-insensitive, first occurrence wins, order-preserving).
pub fn parse_server_mod_rejection(log: &str) -> Vec<CitedMod> {
    let mut out: Vec<CitedMod> = Vec::new();
    let recognizers = [
        parse_forge_fml_channels(log),
        parse_forge_datapack_registries(log),
    ];
    for mods in recognizers.into_iter().flatten() {
        for m in mods {
            if !out.iter().any(|x| x.id.eq_ignore_ascii_case(&m.id)) {
                out.push(m);
            }
        }
    }
    out
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
///
/// We match only the `client side` rejection: in a client log that means the
/// server requires channels the client lacks → mods the client must install.
/// The inverse `server side` rejection — the client carrying mods the server
/// lacks (so the client rejects the server) — is NOT actionable here (we cannot
/// add mods to a remote server), so it is deliberately ignored rather than
/// mis-cited as "missing".
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
/// Deliberately anchored to `client side` only — a `server side` rejection in a
/// client log is the client-has-extra-mods case and is not actionable by
/// installing (see `parse_forge_fml_channels`).
static FORGE_CHANNEL_REJECT_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"Channels \[([^\]]+)\] rejected their client side version number")
        .expect("forge channel-reject regex compiles — covered by tests")
});

/// Modern Forge / Minecraft datapack-registry sync (confirmed against 47.4.10
/// in Phase 0): when the server has mods that register custom datapack
/// registries (e.g. the Moonlight library — a Supplementaries dependency) and
/// the client lacks them, FML rejects with one line per missing registry:
///
/// ```text
/// ...HandshakeHandler/FMLHANDSHAKE]: Missing required datapack registry: moonlight:map_markers
/// ...HandshakeHandler/FMLHANDSHAKE]: Missing required datapack registry: moonlight:soft_fluids
/// ```
///
/// A registry id is `<namespace>:<path>`; the namespace is the registering
/// mod's id (`moonlight`). We cite distinct namespaces. Installing that mod
/// adds the registries; if other mods are also missing, the next join surfaces
/// them (the channel-mismatch format above).
fn parse_forge_datapack_registries(log: &str) -> Option<Vec<CitedMod>> {
    if !log.contains("Missing required datapack registr") {
        return None;
    }
    let mut out: Vec<CitedMod> = Vec::new();
    for caps in FORGE_DATAPACK_REGISTRY_RE.captures_iter(log) {
        let ns = caps[1].trim();
        if ns.is_empty() {
            continue;
        }
        if out.iter().any(|m| m.id.eq_ignore_ascii_case(ns)) {
            continue; // dedup by id, first wins
        }
        out.push(CitedMod {
            id: ns.to_string(),
            version: None,
            kind: CitedKind::Missing,
        });
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Captures the namespace of a missing datapack-registry id from a line like
/// `Missing required datapack registry: moonlight:map_markers` (the real log
/// form — one per line). The namespace is the registering mod's id.
static FORGE_DATAPACK_REGISTRY_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"Missing required datapack registr(?:y|ies):\s*([a-z0-9_.-]+):")
        .expect("forge datapack-registry regex compiles — covered by tests")
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
    fn ignores_server_side_rejection_when_client_has_extra_mods() {
        // Inverse case (real 47.4.10 client log): the CLIENT carries mods with
        // enforced channels the SERVER lacks, so the client rejects the server —
        // "rejected their server side version number". These are NOT mods the
        // client is missing (installing them is meaningless), so nothing is
        // cited even though the "mismatched mod list" terminator is present.
        let log = "[ERROR] [NetworkRegistry/NETREGISTRY]: Channels [alexsmobs:main_channel,citadel:main_channel] rejected their server side version number\n\
                   [ERROR] [HandshakeHandler/FMLHANDSHAKE]: Terminating connection with server, mismatched mod list";
        assert!(parse_server_mod_rejection(log).is_empty());
    }

    #[test]
    fn no_match_without_mismatch_terminator() {
        // A stray channel mention without the terminator is a registration
        // message, not a rejection — must not produce false positives.
        let log = "[INFO]: Registered Channels [foo:main] for mod foo";
        assert!(parse_server_mod_rejection(log).is_empty());
    }

    // Real excerpt: client missing the Moonlight library (a Supplementaries
    // dependency) that registers custom datapack registries — one line each.
    const FORGE_DATAPACK_REJECT: &str = "\
[14:20:19.857] [Netty Client IO #0/ERROR] [net.minecraftforge.network.HandshakeHandler/FMLHANDSHAKE]: Missing required datapack registry: moonlight:map_markers
[14:20:19.857] [Netty Client IO #0/ERROR] [net.minecraftforge.network.HandshakeHandler/FMLHANDSHAKE]: Missing required datapack registry: moonlight:soft_fluids";

    #[test]
    fn parses_datapack_registry_reject_dedups_namespace() {
        let mods = parse_server_mod_rejection(FORGE_DATAPACK_REJECT);
        assert_eq!(
            mods.len(),
            1,
            "both registries share the namespace 'moonlight'"
        );
        assert_eq!(mods[0].id, "moonlight");
        assert_eq!(mods[0].kind, CitedKind::Missing);
        assert_eq!(mods[0].version, None);
    }

    #[test]
    fn merges_channel_and_datapack_formats() {
        let log = "[ERROR] [HandshakeHandler/FMLHANDSHAKE]: Missing required datapack registry: moonlight:map_markers\n\
                   [ERROR] [HandshakeHandler/FMLHANDSHAKE]: Channels [farmersdelight:main] rejected their client side version number\n\
                   [ERROR] [HandshakeHandler/FMLHANDSHAKE]: Terminating connection with server, mismatched mod list";
        let ids: Vec<String> = parse_server_mod_rejection(log)
            .iter()
            .map(|m| m.id.clone())
            .collect();
        assert!(ids.iter().any(|s| s == "farmersdelight"), "got: {ids:?}");
        assert!(ids.iter().any(|s| s == "moonlight"), "got: {ids:?}");
        assert_eq!(ids.len(), 2);
    }
}
