//! What the manual-downloads commands answer (spec §4.2–§4.4).

use serde::Serialize;
use specta::Type;

/// What happened to one file the user handed over.
#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SupplyRow {
    /// Placed. `recorded: false` when its bookkeeping could not be written —
    /// the file is there, but may read as missing until it is added again.
    Added {
        path: String,
        key: String,
        mod_name: String,
        recorded: bool,
    },
    /// The same bytes were already in place (bookkeeping redone).
    AlreadyThere {
        path: String,
        key: String,
        mod_name: String,
    },
    /// The pack's exact file, but another file holds its place (spec D9).
    NeedsReplace {
        path: String,
        key: String,
        mod_name: String,
        existing: String,
    },
    /// Another version of a mod this pack waits for.
    OtherVersion {
        path: String,
        found: String,
        wanted: String,
    },
    /// Not a file this pack needs. `asked: false` = CurseForge could not be asked.
    NotInPack {
        path: String,
        found: Option<String>,
        asked: bool,
    },
    /// Lucerna can't tell where it goes.
    CantPlace {
        path: String,
    },
    /// The pack names another of its own files the same way.
    Conflict {
        path: String,
        existing: String,
    },
    Unreadable {
        path: String,
        reason: String,
    },
    /// Not a regular `.jar` / `.zip` file.
    WrongType {
        path: String,
    },
    /// Its bytes changed while being copied; nothing landed.
    Changed {
        path: String,
    },
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct SupplyReport {
    pub rows: Vec<SupplyRow>,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct ScanReport {
    /// The folder that was read.
    pub folder: String,
    /// Only what concerns the pack: added, already there, needs replace, changed.
    pub rows: Vec<SupplyRow>,
    /// Files that could not be read right now (still being written) — tried again next time.
    pub skipped_unreadable: u32,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct ReplaceOutcome {
    pub row: SupplyRow,
    /// The removed mod's undo receipt, for the timed Undo toast.
    pub receipt: Option<crate::mods::trash::UninstallReceipt>,
    /// Files moved to the recycle bin instead (non-mod occupants).
    pub recycled: Vec<String>,
}

/// A required pack file still missing, for the Play gate (spec §4.6).
#[derive(Debug, Clone, Serialize, serde::Deserialize, Type, PartialEq, Eq)]
pub struct ManualPending {
    pub key: String,
    pub mod_name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_are_tagged_by_kind() {
        let row = SupplyRow::NotInPack {
            path: "a.jar".into(),
            found: None,
            asked: false,
        };
        assert_eq!(
            serde_json::to_value(row).unwrap(),
            serde_json::json!({ "kind": "not_in_pack", "path": "a.jar", "found": null, "asked": false })
        );
    }
}
