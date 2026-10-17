//! What a mod-data copy takes (spec 2026-10-15 §A2). Each item lists the roots a
//! transfer copies and the roots a clone copies. They differ for the map mods:
//! Xaero and JourneyMap key single-player maps by world NAME, and a transfer
//! does not carry worlds, so it copies multiplayer data only; a clone carries
//! the same worlds under the same names, so it copies the whole trees.

use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum TransferItemId {
    Schematics,
    Xaero,
    Journeymap,
    Flashback,
    Replaymod,
    DistantHorizons,
    Voxy,
    Bobby,
}

pub const ALL: [TransferItemId; 8] = [
    TransferItemId::Schematics,
    TransferItemId::Xaero,
    TransferItemId::Journeymap,
    TransferItemId::Flashback,
    TransferItemId::Replaymod,
    TransferItemId::DistantHorizons,
    TransferItemId::Voxy,
    TransferItemId::Bobby,
];

/// Which part of a root a copy takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Everything under the root.
    WholeTree,
    /// Only the child folders whose name starts with one of these, each whole.
    ChildFolders(&'static [&'static str]),
    /// Only the regular files directly in the root.
    TopLevelFiles,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CopyRoot {
    /// `/`-separated, relative to the game dir.
    pub rel: &'static str,
    pub scope: Scope,
}

/// Xaero's folder names for a server (`Multiplayer_<address>`) and a Realm.
pub const MULTIPLAYER: &[&str] = &["Multiplayer_", "Realms_"];

const fn whole(rel: &'static str) -> CopyRoot {
    CopyRoot {
        rel,
        scope: Scope::WholeTree,
    }
}

const fn multiplayer(rel: &'static str) -> CopyRoot {
    CopyRoot {
        rel,
        scope: Scope::ChildFolders(MULTIPLAYER),
    }
}

const SCHEMATICS: &[CopyRoot] = &[whole("schematics")];
const XAERO_TRANSFER: &[CopyRoot] = &[
    multiplayer("xaero/minimap"),
    multiplayer("xaero/world-map"),
    multiplayer("XaeroWaypoints"),
    multiplayer("XaeroWorldMap"),
];
const XAERO_CLONE: &[CopyRoot] = &[
    whole("xaero"),
    whole("XaeroWaypoints"),
    whole("XaeroWorldMap"),
];
const JOURNEYMAP_TRANSFER: &[CopyRoot] = &[whole("journeymap/data/mp")];
const JOURNEYMAP_CLONE: &[CopyRoot] = &[whole("journeymap")];
const FLASHBACK: &[CopyRoot] = &[whole("flashback/replays")];
// `raw/` and `recording/` hold recordings in progress; ReplayMod moves whatever
// it finds there at its next start, so a copied one would be "recovered" twice.
const REPLAYMOD: &[CopyRoot] = &[CopyRoot {
    rel: "replay_recordings",
    scope: Scope::TopLevelFiles,
}];
const DISTANT_HORIZONS: &[CopyRoot] = &[whole("Distant_Horizons_server_data")];
const VOXY: &[CopyRoot] = &[whole(".voxy")];
const BOBBY: &[CopyRoot] = &[whole(".bobby")];

impl TransferItemId {
    pub fn transfer_roots(self) -> &'static [CopyRoot] {
        match self {
            Self::Schematics => SCHEMATICS,
            Self::Xaero => XAERO_TRANSFER,
            Self::Journeymap => JOURNEYMAP_TRANSFER,
            Self::Flashback => FLASHBACK,
            Self::Replaymod => REPLAYMOD,
            Self::DistantHorizons => DISTANT_HORIZONS,
            Self::Voxy => VOXY,
            Self::Bobby => BOBBY,
        }
    }

    pub fn clone_roots(self) -> &'static [CopyRoot] {
        match self {
            Self::Xaero => XAERO_CLONE,
            Self::Journeymap => JOURNEYMAP_CLONE,
            other => other.transfer_roots(),
        }
    }

    /// A folder-name-safe key: the staging folder of this item.
    pub fn key(self) -> &'static str {
        match self {
            Self::Schematics => "schematics",
            Self::Xaero => "xaero",
            Self::Journeymap => "journeymap",
            Self::Flashback => "flashback",
            Self::Replaymod => "replaymod",
            Self::DistantHorizons => "distant_horizons",
            Self::Voxy => "voxy",
            Self::Bobby => "bobby",
        }
    }
}

/// Top-level folders "Other folders" never offers, folded (see [`fold`]): the
/// catalog's own roots, folders the launcher manages (copying into `mods/` would
/// bypass the hardlink store; `config/` belongs to the mod versions), and caches
/// the game or the loader regenerates.
const NOT_OTHER: &[&str] = &[
    "schematics",
    "xaero",
    "xaerowaypoints",
    "xaeroworldmap",
    "journeymap",
    "flashback",
    "replay_recordings",
    "distant_horizons_server_data",
    ".voxy",
    ".bobby",
    "mods",
    "saves",
    "resourcepacks",
    "shaderpacks",
    "config",
    "logs",
    "crash-reports",
    "downloads",
    "server-resource-packs",
    ".fabric",
    ".quilt",
    ".mixin.out",
];

/// A name as Windows and macOS compare it: case-blind, and on Windows trailing
/// dots and spaces do not count (`Config.` IS `config`).
pub fn fold(name: &str) -> String {
    name.trim_end_matches(['.', ' ']).to_lowercase()
}

/// Whether "Other folders" offers a top-level folder of this name.
pub fn is_offered_other(name: &str) -> bool {
    valid_other_name(name) && !NOT_OTHER.contains(&fold(name).as_str())
}

/// One safe path segment: `pathsafe::validate_segment`'s rules minus its
/// leading-dot ban — `.replay_cache`, `.bobby` and `.voxy` are real mod folders.
pub fn valid_other_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && name != "."
        && !name.contains("..")
        && !name.contains(['/', '\\', ':'])
        && !name.chars().any(char::is_control)
        && !crate::pathsafe::is_reserved_windows_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_item_has_roots_and_a_unique_key() {
        let mut keys: Vec<&str> = ALL.iter().map(|i| i.key()).collect();
        for item in ALL {
            assert!(!item.transfer_roots().is_empty(), "{item:?}");
            assert!(!item.clone_roots().is_empty(), "{item:?}");
        }
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), ALL.len());
    }

    // R1 (spec §9): the map mods transfer multiplayer data only. Flipping R1 is
    // this test and the two `*_TRANSFER` constants it names.
    #[test]
    fn transfer_copies_multiplayer_maps_only_and_clone_copies_whole_trees() {
        for root in TransferItemId::Xaero.transfer_roots() {
            assert_eq!(root.scope, Scope::ChildFolders(MULTIPLAYER), "{}", root.rel);
        }
        assert_eq!(
            TransferItemId::Journeymap.transfer_roots()[0].rel,
            "journeymap/data/mp"
        );
        for item in [TransferItemId::Xaero, TransferItemId::Journeymap] {
            assert!(item
                .clone_roots()
                .iter()
                .all(|r| r.scope == Scope::WholeTree));
        }
    }

    #[test]
    fn replays_skip_temporary_folders() {
        assert_eq!(
            TransferItemId::Flashback.transfer_roots()[0].rel,
            "flashback/replays"
        );
        assert_eq!(
            TransferItemId::Replaymod.transfer_roots()[0].scope,
            Scope::TopLevelFiles
        );
        assert_eq!(
            TransferItemId::Replaymod.clone_roots()[0].scope,
            Scope::TopLevelFiles
        );
    }

    #[test]
    fn other_folders_exclude_managed_names_case_blind() {
        for name in [
            "mods",
            "Mods",
            "Config.",
            "config ",
            "SAVES",
            "Schematics",
            ".Voxy",
            "logs",
        ] {
            assert!(!is_offered_other(name), "{name} must not be offered");
        }
        for name in ["kubejs", ".replay_cache", "moddata", "patchouli_books"] {
            assert!(is_offered_other(name), "{name} must be offered");
        }
    }

    #[test]
    fn other_names_are_single_safe_segments() {
        for bad in [
            "", "a/b", "a\\b", "c:", "..", ".", "a..b", "CON", "nul", "x\u{7}",
        ] {
            assert!(!valid_other_name(bad), "{bad:?}");
        }
        assert!(valid_other_name(".replay_cache"));
    }
}
