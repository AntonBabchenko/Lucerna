//! Per-instance singleplayer-world backup + restore module.
//!
//! Public surface: list/backup/restore/delete commands + the four
//! specta-exported types. Implementation split across submodules:
//! `fs` for path-safety + size helpers, `zip` for the archive ops
//! with zip-slip defense, `backup` for the backup-side flow, and
//! `restore` for the multi-step replace/as_copy flow.

pub mod backup;
pub mod fs;
pub mod restore;
pub mod zip;

use serde::{Deserialize, Serialize};
use specta::Type;

/// A singleplayer world inside an instance, surfaced to the UI.
/// Display name = `folder_name` in v1 (no NBT parsing).
#[derive(Debug, Clone, Serialize, Type)]
pub struct World {
    pub folder_name: String,
    pub size_bytes: f64,
    pub modified_unix_ms: f64,
    pub backup_count: u32,
}

/// One on-disk backup zip for a world.
#[derive(Debug, Clone, Serialize, Type)]
pub struct Backup {
    /// Filename under `<instance>/backups/<world>/`. Encodes
    /// timestamp; see `backup::parse_timestamp_from_filename`.
    pub filename: String,
    pub size_bytes: f64,
    /// Convenience: timestamp parsed from the filename. ms since epoch.
    pub created_unix_ms: f64,
}

/// Returned by `restore_backup` so the UI knows where the restored
/// world landed. Equals the original `world_folder_name` for
/// `RestoreMode::Replace`; suffixed for `RestoreMode::AsCopy`.
#[derive(Debug, Clone, Serialize, Type)]
pub struct RestoredWorld {
    pub final_folder_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RestoreMode {
    Replace,
    AsCopy,
}
