//! World migration between instances: move or copy a world from one
//! instance's `saves/` into another's, re-link its datapacks to the target
//! library, and carry its backups along on a move (spec
//! `2026-08-16-world-migration-design.md`).
//!
//! One phase per file: `plan` (read-only compatibility verdicts), `stage`
//! (claim a hidden stage; rename, or copy with verification), `relink`
//! (datapacks, never fatal), `finalise` (the one rename to the final name —
//! the point of no return). This file holds the IPC types, the path bundle,
//! the injected seams and the orchestrator.
//!
//! This module owns NO write primitive: bytes reach the target only through
//! `worlds::import::copy_tree`, `mods::store::materialize` and
//! `datapacks::library::install_named_at`; everything else here is
//! `create_dir`, `rename`, `remove_dir`/`remove_dir_all` and reads.

mod finalise;
mod plan;
mod relink;
mod stage;

pub(crate) use finalise::{finalise_at, remove_source_after_copy_at};
pub use plan::plan_migration_at;
pub(crate) use relink::relink_datapacks_at;
pub(crate) use stage::stage_world_at;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::DatapackRejection;
use crate::instances::schema::LoaderKind;

/// D1: copy (default — the original survives) or move.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MigrationMode {
    Copy,
    Move,
}

/// Which §4 path actually ran: one same-volume rename, or a verified copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MigrationPath {
    Renamed,
    Copied,
}

/// Why a version verdict could not be reached (§6). Typed so the dialog
/// renders each as its own honest sentence — never a raw string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum UnknownReason {
    NoLevelDat,
    Unreadable,
    NotRecorded,
    TargetVersionUnset,
    TargetNotInstalled,
    /// The target jar is installed but records no DataVersion: no
    /// `version.json` (clients before 1.14) or no integer `world_version`.
    TargetNotRecorded,
}

/// Ordered by DataVersion (integer), never by version name (A6).
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VersionVerdict {
    Same,
    WillUpgrade,
    WorldIsNewer,
    Unknown { reason: UnknownReason },
}

/// Why a world datapack stayed a plain copy instead of a library link (§5).
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LeftReason {
    NameHeldByDifferentPack,
    NotADatapack { reason: DatapackRejection },
    TooLarge,
    LinkFailed,
    Unreadable,
    Io,
}

/// Per-`.zip` result of the datapack step. `CopiedNotLinked` is
/// `materialize`'s documented fallback on a filesystem that cannot hardlink —
/// the toast must not call a copy "linked".
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DatapackResult {
    Linked,
    CopiedNotLinked,
    Adopted,
    LeftAsCopy { reason: LeftReason },
}

/// Plan-time prediction for one `.zip`: `Linked`, `Adopted`, or
/// `LeftAsCopy { NameHeldByDifferentPack }`.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct DatapackPlan {
    pub filename: String,
    pub predicted: DatapackResult,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct DatapackMigration {
    pub filename: String,
    pub result: DatapackResult,
}

/// What is left in the source after the point of no return (A4). `reason` is
/// the io error text for Logs / the details panel, never shown raw as the
/// toast sentence.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceState {
    Untouched,
    Removed,
    LeftIntact { reason: String },
    LeftPartial { reason: String },
}

/// The read-only plan for one world → one target (A13: size and backup count
/// come from the `World` row the dialog was opened from, not from here).
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct MigrationPlan {
    /// `Data.Version.Name` — display only, never compared.
    pub world_version_name: Option<String>,
    pub verdict: VersionVerdict,
    pub source_loader: LoaderKind,
    pub target_loader: LoaderKind,
    /// A7: source mods absent from the target, by `project_id` else `sha1`.
    pub mods_missing_in_target: u32,
    pub datapacks: Vec<DatapackPlan>,
    /// Folder packs: copied as they are, never adopted or linked.
    pub datapacks_folders: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MigrationPhase {
    Moving,
    Copying,
    Verifying,
    Linking,
    Backups,
    Finalising,
}

/// `f64`, not `u64`: specta-typescript rejects `u64` (A10).
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct MigrationProgress {
    pub phase: MigrationPhase,
    pub current: f64,
    pub total: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct MigrationOutcome {
    /// Suffixed if the name was taken in the target.
    pub final_folder_name: String,
    pub path: MigrationPath,
    pub datapacks: Vec<DatapackMigration>,
    pub datapacks_folders_copied: u32,
    /// Copy path only: symlinks `copy_tree` did not follow.
    pub links_skipped: u32,
    pub source_state: SourceState,
    pub backups_moved: u32,
    /// Still in the source's `backups/<world>/`, where the orphan UI shows them.
    pub backups_left: u32,
}

/// Every path the core needs, resolved by the command layer — the repo's
/// no-`AppHandle` test boundary. `src_root` / `dst_root` are the instance
/// roots `datapacks::library` / `registry` and `mods::installed` take
/// (`paths::instance_dir`); `*_saves` = `<root>/.minecraft/saves`,
/// `*_backups_root` = `<root>/backups`. `target_instance_name` is the display
/// NAME, for `WorldMigratePartialLeft`.
#[derive(Debug, Clone)]
pub struct MigrationLocations {
    pub src_saves: PathBuf,
    pub src_backups_root: PathBuf,
    pub src_root: PathBuf,
    pub dst_saves: PathBuf,
    pub dst_backups_root: PathBuf,
    pub dst_root: PathBuf,
    pub world_folder: String,
    pub target_instance_name: String,
}

/// Injected `rename` / `remove` so every rollback branch is deterministic in
/// a plain `#[test]` on every platform — the `restore::swap_in_place` shape.
/// `Arc` because the orchestrator hands them across `spawn_blocking`.
#[derive(Clone)]
pub struct MigrationSeams {
    pub rename: Arc<dyn Fn(&Path, &Path) -> std::io::Result<()> + Send + Sync>,
    pub remove: Arc<dyn Fn(&Path) -> std::io::Result<()> + Send + Sync>,
}

impl MigrationSeams {
    /// Production seams: `std::fs::rename` and `std::fs::remove_dir_all`.
    pub fn real() -> Self {
        Self {
            rename: Arc::new(|from: &Path, to: &Path| std::fs::rename(from, to)),
            remove: Arc::new(|path: &Path| std::fs::remove_dir_all(path)),
        }
    }
}

/// The world sitting in its hidden stage, ready for relink + finalise.
#[derive(Debug)]
pub(crate) struct Staged {
    pub stage: PathBuf,
    pub path: MigrationPath,
    pub links_skipped: u32,
}

/// Result of the datapack step over the stage.
#[derive(Debug)]
pub(crate) struct Relinked {
    pub datapacks: Vec<DatapackMigration>,
    pub folders_copied: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn data_carrying_enums_are_kind_tagged_and_snake_cased() {
        assert_eq!(
            serde_json::to_value(VersionVerdict::Unknown {
                reason: UnknownReason::NoLevelDat,
            })
            .unwrap(),
            json!({"kind": "unknown", "reason": "no_level_dat"})
        );
        assert_eq!(
            serde_json::to_value(DatapackResult::LeftAsCopy {
                reason: LeftReason::NotADatapack {
                    reason: DatapackRejection::NotAPack,
                },
            })
            .unwrap(),
            json!({"kind": "left_as_copy", "reason": {"kind": "not_a_datapack", "reason": "not_a_pack"}})
        );
        assert_eq!(
            serde_json::to_value(SourceState::LeftIntact {
                reason: "EBUSY".into(),
            })
            .unwrap(),
            json!({"kind": "left_intact", "reason": "EBUSY"})
        );
    }

    #[test]
    fn unit_enums_are_bare_snake_case_and_mode_round_trips() {
        assert_eq!(
            serde_json::to_value(MigrationPath::Renamed).unwrap(),
            json!("renamed")
        );
        assert_eq!(
            serde_json::to_value(MigrationPhase::Finalising).unwrap(),
            json!("finalising")
        );
        let mode: MigrationMode = serde_json::from_value(json!("move")).unwrap();
        assert_eq!(mode, MigrationMode::Move);
    }

    #[test]
    fn real_seams_rename_and_remove_on_disk() {
        let td = tempfile::tempdir().unwrap();
        let a = td.path().join("a");
        std::fs::create_dir(&a).unwrap();
        let b = td.path().join("b");
        let seams = MigrationSeams::real();
        (seams.rename)(&a, &b).unwrap();
        assert!(b.is_dir());
        assert!(!a.exists());
        (seams.remove)(&b).unwrap();
        assert!(!b.exists());
    }
}
