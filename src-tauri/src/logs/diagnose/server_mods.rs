//! Parser for "modded connection rejected — missing/mismatched mods"
//! blocks in a Minecraft game log. Pure: no I/O, no network. One
//! recognizer per supported loader/format (anchors captured in the
//! Phase 0 spike). `parse_server_mod_rejection` tries each in order and
//! returns the first non-empty result; an unrecognized log yields an
//! empty vec so the feature simply does not appear (zero false positives).

use serde::{Deserialize, Serialize};
use specta::Type;

/// One mod the server said the client is missing or running at the wrong
/// version. `id` is the raw mod-id from the log (e.g. "jei", "create") —
/// NOT a Modrinth slug or platform id.
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

/// Parse a (capped) game-log body into the cited mods. Empty when no
/// known reject format is recognized. Dedups by `id` (first occurrence
/// wins, preserving order).
pub fn parse_server_mod_rejection(log: &str) -> Vec<CitedMod> {
    let _ = log;
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_empty_for_unrecognized_log() {
        let log = "[12:00:00] [main/INFO]: Connecting to play.example.com\n\
                   [12:00:01] [main/INFO]: Connected, joining world";
        assert!(parse_server_mod_rejection(log).is_empty());
    }
}
