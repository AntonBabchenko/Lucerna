//! Client-side datapacks.
//!
//! Three levels on disk:
//!   * library — `<instance>/datapacks/<file>.zip`, the physical file;
//!   * world — `<instance>/.minecraft/saves/<world>/datapacks/<file>.zip`, a
//!     hardlink to the library file;
//!   * registry — `<instance>/lucerna/installed-datapacks.json`, metadata
//!     only, reconciled against the library dir on every read.
//!
//! Enabled/disabled is NOT file presence: it lives in the world's `level.dat`
//! under `Data.DataPacks.{Enabled,Disabled}`. See `level_dat`.
//!
//! This module contains no raw write primitives. Every byte reaches disk via
//! `crate::mods::store::{place_bytes, materialize}`, so the hardlink shared
//! with other worlds is never written through in place.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::{Error, Result};

pub mod compat;
pub mod guard;
pub mod level_dat;
pub mod library;
pub mod overview;
pub mod pack_meta;
pub mod registry;
pub mod state;
pub mod update;
pub mod vanillatweaks;
pub mod world_link;

/// One datapack in an instance's library. Mirrors `mods::platform::InstalledAsset`;
/// it deliberately carries no `enabled` field — one library entry fans out to N
/// worlds, each with its own state in its own `level.dat`.
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
pub struct InstalledDatapack {
    pub filename: String,
    pub sha1: String,
    pub size_bytes: f64,
    /// `pack.pack_format` from `pack.mcmeta`; `None` when unreadable.
    pub pack_format: Option<u32>,
    /// Display name: `pack.description` when it is a plain string, else the
    /// filename without its extension.
    pub name: String,
    /// `None` for a local install; `Some` once the catalog supplies it.
    pub source: Option<crate::mods::platform::ModSource>,
    pub project_id: Option<String>,
    /// Load-bearing for update checking: `classify_asset_update` answers
    /// `UpToDate` whenever this is `None`, so a pack installed without it can
    /// never report an available update.
    pub version_id: Option<String>,
    /// Human-readable version from the catalog (e.g. `1.20.4-2.1.0`), for the
    /// library row. Added in registry `FILE_VERSION` 2 with
    /// `#[serde(default)]`, so a v1 file reads back as `None` — `migrate`
    /// cannot backfill a field and does not try.
    #[serde(default)]
    pub version_number: Option<String>,
    /// RFC 3339.
    pub installed_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum WorldPackState {
    Enabled,
    Disabled,
    NotAdded,
    /// Named in `level.dat`'s Enabled list but the file is gone — this is what
    /// Minecraft turns into the "data packs are no longer present" screen.
    Orphaned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PackCompat {
    Compatible,
    Mismatch { pack_format: u32, expected: u32 },
    Unknown,
}

/// One datapack as it appears for a single world.
#[derive(Debug, Clone, Serialize, Type)]
pub struct WorldDatapack {
    pub filename: String,
    pub state: WorldPackState,
    /// False for a file the user (or a world import) put in the world folder
    /// directly. Supported, not an error — only "remove from library" is
    /// unavailable for it.
    pub in_library: bool,
    pub compat: PackCompat,
}

/// One world's view of one library pack.
#[derive(Debug, Clone, Serialize, Type, PartialEq)]
pub struct DatapackPlacementView {
    pub world: String,
    /// `None` when this world's `level.dat` could not be read, so the
    /// enabled/disabled answer is genuinely unknown rather than guessed. A
    /// missing level.dat is NOT this case — an unplayed world reads as two
    /// empty lists, which is a real answer.
    pub state: Option<WorldPackState>,
}

/// One row of the instance-level library screen.
#[derive(Debug, Clone, Serialize, Type)]
pub struct DatapackLibraryEntry {
    pub pack: InstalledDatapack,
    /// `false` ⟹ the pack is gone from the library but still linked in worlds.
    /// Reachable through a non-cascading removal: deleting one hardlink name
    /// provably cannot affect the others, so the pack keeps loading in game
    /// while `registry::list` drops its row on the next read. The listing is
    /// therefore the UNION of the registry and the on-disk world entries, never
    /// the registry alone.
    pub in_library: bool,
    /// Per-INSTANCE, not per-world: the verdict compares the pack's own
    /// `pack_format` against the instance's Minecraft, and no world is an
    /// input, so rendering it per world would print N identical copies.
    pub compat: PackCompat,
    /// Empty ⟺ "in no world" — the state the library screen exists to surface.
    pub placements: Vec<DatapackPlacementView>,
}

/// Everything the library screen renders, in one read.
#[derive(Debug, Clone, Serialize, Type)]
pub struct DatapackLibraryView {
    /// The instance's expected `pack_format`. Exposed here because nothing else
    /// does, and the frontend cannot compute it.
    pub expected_pack_format: Option<u32>,
    pub entries: Vec<DatapackLibraryEntry>,
}

/// Where a catalog-installed datapack came from. `None` at every local-install
/// call site; `Some` only from the catalog command.
///
/// A struct rather than four `Option` parameters: the four fields are only ever
/// meaningful together, and `install_named_at` already carries enough arguments
/// that four more positional `Option`s would be a mix-up waiting to happen.
#[derive(Debug, Clone)]
pub struct DatapackProvenance {
    pub source: crate::mods::platform::ModSource,
    pub project_id: String,
    pub version_id: String,
    pub version_number: Option<String>,
}

/// Largest datapack Lucerna will buffer. Classification and hashing both hold
/// the whole pack in memory alongside the caller's copy, so peak is roughly
/// twice this. The cap exists because slice 2 adds an AUTOMATED download path:
/// before it, every pack came from a file the user picked themselves.
/// Same class of guard as `ModpackOverridesTooLarge` / `WorldImportTooLarge`.
pub const MAX_DATAPACK_BYTES: usize = 256 * 1024 * 1024;

/// Largest Vanilla Tweaks bundle Lucerna will buffer. A build is one zip
/// holding one zip per selected pack, so `MAX_DATAPACK_BYTES` cannot bound it
/// — a legitimate twenty-pack selection would trip a per-pack limit. Eight
/// times the single-pack ceiling is a deliberate round number rather than a
/// measurement: VT packs run to tens of kilobytes, so this clears any
/// realistic selection while still refusing a response that is plainly not
/// the bundle we asked for.
pub const MAX_VT_BUNDLE_BYTES: usize = 8 * MAX_DATAPACK_BYTES;

/// The result of a library install: the registry row, plus what the same-name
/// fan-out did to each world already holding that filename.
#[derive(Debug, Clone, Serialize, Type)]
pub struct LibraryInstall {
    pub pack: InstalledDatapack,
    pub refreshed: Vec<WorldMigration>,
}

/// What happened to one world when a library pack was replaced under it —
/// either by an update to a new filename (`world_link::migrate_placements`) or
/// by a same-name reinstall (`library::install_named_at`'s fan-out).
///
/// Returned rather than logged: a datapack update touches N worlds, and the
/// user needs to know exactly which ones moved when one of them fails. The
/// previous behaviour swallowed per-world failures into a `diag!` line.
#[derive(Debug, Clone, Serialize, Type, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorldMigration {
    /// Relinked, and level.dat rewritten preserving the pack's enabled state.
    /// Produced only by `world_link::migrate_placements`, which actually read
    /// that state.
    Migrated {
        world: String,
        was_enabled: bool,
    },
    /// A same-name refresh: the world's file now holds the new bytes, and
    /// level.dat was deliberately never touched — each world's own
    /// enabled/disabled choice stands exactly as it was. A separate variant
    /// from [`WorldMigration::Migrated`] because this path does not KNOW the
    /// state; fabricating `was_enabled: true` here would tell the UI a
    /// disabled pack had been enabled.
    Refreshed {
        world: String,
    },
    /// A same-named entry whose content is not the library's — left untouched.
    /// Replacing it would destroy a pack the user put there themselves.
    SkippedNotOurs {
        world: String,
    },
    Failed {
        world: String,
        details: String,
    },
}

/// The result of `datapacks_update_one`.
#[derive(Debug, Clone, Serialize, Type)]
pub struct DatapackUpdateOutcome {
    /// The new registry row, carrying the target version's provenance.
    pub pack: InstalledDatapack,
    /// Per-world outcomes: same-name refreshes plus cross-name migrations.
    pub migrations: Vec<WorldMigration>,
    /// `false` ⟹ at least one world failed to migrate. The OLD library file
    /// and its registry row were kept — both versions sit in the library until
    /// a re-run converges, which it does because a migrated world no longer
    /// holds the old filename (§8.5: no rollback by design).
    pub completed: bool,
}

/// One world's outcome of a library removal.
#[derive(Debug, Clone, Serialize, Type, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorldRemoval {
    /// The world's link and its level.dat entries are gone.
    Removed {
        world: String,
    },
    /// A same-named entry whose content is not the library's — never touched,
    /// cascading or not. Removing it would destroy a pack the user (or a world
    /// import) put there themselves.
    KeptNotOurs {
        world: String,
    },
    /// Cascade was off; the link survives and keeps loading in game. This is
    /// exactly the state `DatapackLibraryEntry.in_library: false` renders
    /// afterwards.
    KeptNoCascade {
        world: String,
    },
    Failed {
        world: String,
        details: String,
    },
}

/// The result of removing a pack from the library. Closes F3: the removal
/// names exactly which worlds were cleaned and which still hold the pack,
/// instead of leaving the user to discover live-but-invisible content.
#[derive(Debug, Clone, Serialize, Type)]
pub struct LibraryRemoval {
    pub worlds: Vec<WorldRemoval>,
    /// `false` ⟹ a world failed to clean up, so the library copy and its
    /// registry row were kept: `placements_of`'s identity check needs the
    /// library bytes, and deleting them would make every remaining world look
    /// foreign to a retry, which could then never finish the job.
    pub removed_from_library: bool,
}

/// `<instance>/datapacks/`.
pub fn library_dir_at(instance_root: &Path) -> PathBuf {
    instance_root.join("datapacks")
}

/// `<instance>/lucerna/installed-datapacks.json`.
pub fn registry_path_at(instance_root: &Path) -> PathBuf {
    instance_root
        .join("lucerna")
        .join("installed-datapacks.json")
}

/// `<instance>/.minecraft/saves/<world>/datapacks/`.
///
/// `world` is validated here rather than trusted from the caller: it reaches us
/// from the UI and from level.dat, and a doc comment is not a guard. Mirrors
/// `servers_runtime::datapacks::datapacks_dir`, which guards `level-name` for
/// the same reason.
pub fn world_datapacks_dir_at(instance_root: &Path, world: &str) -> Result<PathBuf> {
    crate::worlds::fs::validate_segment(world)?;
    Ok(instance_root
        .join(".minecraft")
        .join("saves")
        .join(world)
        .join("datapacks"))
}

/// The value Minecraft writes into `level.dat`'s Enabled/Disabled lists for a
/// pack loaded from the world's `datapacks/` folder.
pub fn level_dat_entry(filename: &str) -> String {
    format!("file/{filename}")
}

/// `<instance>/` for a live app handle — the root every `*_at` fn takes.
pub fn instance_root(app: &tauri::AppHandle, instance_id: &str) -> Result<PathBuf> {
    crate::paths::instance_dir(app, instance_id).map_err(|e| Error::io("<instance_root>", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn library_dir_is_instance_root_datapacks() {
        let root = Path::new("/inst/Foo");
        assert_eq!(library_dir_at(root), Path::new("/inst/Foo/datapacks"));
    }

    #[test]
    fn registry_path_is_under_lucerna() {
        let root = Path::new("/inst/Foo");
        assert_eq!(
            registry_path_at(root),
            Path::new("/inst/Foo/lucerna/installed-datapacks.json")
        );
    }

    #[test]
    fn world_datapacks_dir_is_under_saves() {
        let root = Path::new("/inst/Foo");
        assert_eq!(
            world_datapacks_dir_at(root, "Survival").unwrap(),
            Path::new("/inst/Foo/.minecraft/saves/Survival/datapacks")
        );
    }

    #[test]
    fn world_datapacks_dir_rejects_a_path_separator() {
        let err = world_datapacks_dir_at(Path::new("/inst/Foo"), "../evil").unwrap_err();
        assert!(matches!(err, crate::error::Error::WorldPathInvalid { .. }));
    }

    #[test]
    fn level_dat_entry_prefixes_with_file() {
        assert_eq!(level_dat_entry("veinminer.zip"), "file/veinminer.zip");
    }
}
