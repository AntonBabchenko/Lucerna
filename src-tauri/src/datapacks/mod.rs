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
pub mod detect;
pub mod format;
pub mod guard;
pub mod level_dat;
pub mod library;
pub mod overview;
pub mod pack_meta;
pub mod presence;
pub mod registry;
pub mod state;
pub mod update;
pub mod vanillatweaks;
pub mod verdict;
pub mod world_link;

/// One datapack in an instance's library. Mirrors `mods::platform::InstalledAsset`;
/// it deliberately carries no `enabled` field — one library entry fans out to N
/// worlds, each with its own state in its own `level.dat`.
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
pub struct InstalledDatapack {
    pub filename: String,
    pub sha1: String,
    pub size_bytes: f64,
    /// Display name: the plain text of `pack.description` (rich text
    /// flattened, `§` codes stripped), else the filename without its
    /// extension. Re-derived from the file whenever the registry re-reads its
    /// declaration.
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
    /// Named in Enabled but no loadable pack has this id; the game logs
    /// "Missing data pack", skips it, and drops the id at the next save.
    Orphaned,
    /// Something is on disk under this name, but the game does not load it
    /// (spec §2 N.1, §0.5 A1). The row's `ignored_reason` says why.
    Ignored,
}

/// What removing one entry from one world would do to it — the answer the
/// "remove from this world" confirmation words itself by (spec 2026-09-24
/// §4 U1). Computed by `world_link::world_entry_kind_at`, which resolves the
/// name exactly as the removal does (R2) and judges it with the same identity
/// rule the library's placement scan uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorldEntryKind {
    /// A file byte-identical to the library's copy: removing it leaves the
    /// pack in the library.
    LibraryCopy,
    /// A file that is not the library's copy (or no library copy exists):
    /// removing it deletes the only copy.
    OwnFile,
    /// A folder pack. Library entries are zips, so a folder is never the
    /// library's copy: removing deletes the folder.
    OwnFolder,
    /// Nothing on disk under this name: removing only clears the level.dat
    /// entry.
    Missing,
}

/// Minecraft's own verdict on a pack's declared formats for this instance's
/// version (`PackCompatibility`, §1 C3). Every kind except `WontLoad` is a
/// pack the game LOADS. `made_for`/`game` are display labels ("34–48",
/// "107.1") from `verdict`'s one formatter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PackCompat {
    /// The declared range covers this version.
    Compatible,
    /// "Made for an older version of Minecraft" — still loads.
    TooOld { made_for: String, game: String },
    /// "Made for a newer version of Minecraft" — still loads.
    TooNew { made_for: String, game: String },
    /// The version fields fail this version's own validation: the game marks
    /// the pack "(Broken or incompatible)" and still loads it.
    Broken,
    /// This version skips the pack entirely.
    WontLoad { reason: WontLoadReason },
    /// Not decidable: no recorded declaration, a field this build cannot
    /// parse, or no readable game format.
    Unknown,
}

/// Why this version of the game skips a pack (it logs "Failed to read pack
/// metadata" and loads nothing from it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum WontLoadReason {
    /// No `pack.mcmeta` at the zip's top level.
    NoPackMcmeta,
    /// `pack.mcmeta` has no `pack` object.
    NoPackSection,
    /// `pack` has no `description`, which every era requires.
    NoDescription,
    /// No `pack_format`, which versions before 1.21.9 require.
    NoPackFormat,
}

/// One datapack as it appears for a single world.
#[derive(Debug, Clone, Serialize, Type)]
pub struct WorldDatapack {
    pub filename: String,
    pub state: WorldPackState,
    /// `Some` exactly when `state` is `Ignored`. Both come from `state::derive`.
    pub ignored_reason: Option<crate::datapacks::detect::IgnoredReason>,
    /// False for a file the user (or a world import) put in the world folder
    /// directly. Supported, not an error — only "remove from library" is
    /// unavailable for it.
    pub in_library: bool,
    /// The game's own verdict for this instance's Minecraft. `Unknown` for
    /// an entry that is not the library's own copy — hand-dropped, a folder,
    /// or a same-named zip with other bytes (§0.5 A1).
    pub compat: PackCompat,
}

/// What `datapacks_list_for_world` returns: the world's `level.dat`
/// presence, and the rows the game would load (§3 L.6).
#[derive(Debug, Clone, Serialize, Type)]
pub struct WorldDatapackListing {
    /// `Present` is the only state Lucerna edits. `OnlyOld`: `packs` shows
    /// what `level.dat_old` holds — the copy the game opens the world from
    /// and restores `level.dat` out of — and every add, toggle and removal is
    /// refused until `level.dat` is back (the same-name refresh is the
    /// documented exception, §0.2 I7). `Absent`: the game does not list the
    /// folder as a world and loads nothing from it, so `packs` is empty.
    pub level_dat: presence::LevelDatPresence,
    pub packs: Vec<WorldDatapack>,
}

/// One world's view of one library pack.
#[derive(Debug, Clone, Serialize, Type, PartialEq)]
pub struct DatapackPlacementView {
    pub world: String,
    /// The state the game would load: from `level.dat`, or from
    /// `level.dat_old` when only the backup is left. `None` when those
    /// lists could not be read, or when the folder holds neither file (see
    /// `level_dat`) — unknown, or no world to hold a state, rather than
    /// guessed.
    pub state: Option<WorldPackState>,
    /// `Some` when `state` is `Ignored`; both come from `state::derive`.
    /// Also `Some(Unreadable)` with `state: None` for a placement Lucerna
    /// could not check at all — the world's `datapacks/` could not be read,
    /// or R2 could not tell which entry the name denotes. That mark is what
    /// tells "could not tell" apart from a folder with no level file, where
    /// `state: None` is a fact.
    pub ignored_reason: Option<crate::datapacks::detect::IgnoredReason>,
    /// This world's `level.dat` presence; `None` when it could not be told.
    /// Anything but `Some(Present)` means Lucerna adds, toggles or removes
    /// nothing in this world. A library-wide cascade removal or renamed update
    /// still unlinks or relinks the file in an `Absent` folder without
    /// touching `level.dat` (§0.5 A3), and a same-name refresh is not gated
    /// (§0.2 I7).
    pub level_dat: Option<presence::LevelDatPresence>,
}

/// One world folder the library view saw, whether or not it holds any
/// pack — the world picker needs the worlds a pack is NOT in yet.
#[derive(Debug, Clone, Serialize, Type, PartialEq)]
pub struct DatapackWorldView {
    pub world: String,
    /// `None` when the presence could not be told.
    pub level_dat: Option<presence::LevelDatPresence>,
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
    /// Per-INSTANCE, not per-world: the game's verdict on the pack's declared
    /// formats for the instance's Minecraft. No world is an input, so
    /// rendering it per world would print N identical copies.
    pub compat: PackCompat,
    /// Empty ⟺ "in no world" — the state the library screen exists to surface.
    pub placements: Vec<DatapackPlacementView>,
}

/// Everything the library screen renders, in one read.
#[derive(Debug, Clone, Serialize, Type)]
pub struct DatapackLibraryView {
    pub entries: Vec<DatapackLibraryEntry>,
    /// Every world folder the listing accepts under `saves/`, sorted
    /// case-insensitively, each with its `level.dat` presence — including
    /// worlds no pack is in. A folder whose metadata cannot be read, or whose
    /// name is not a usable world folder name, is left out, and the world
    /// picker then shows it as unknown.
    pub worlds: Vec<DatapackWorldView>,
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
    /// Relinked in a world whose `level.dat` was read, carrying the pack's
    /// enabled state across the rename. Produced only by
    /// `world_link::migrate_placements`, which actually read that state.
    ///
    /// level.dat is rewritten only when an entry had to move. A world with no
    /// `DataPacks` compound is left unchanged: the game reads the missing
    /// compound as its default and enables the present, unlisted pack itself
    /// (spec §0.5 A15), which is what `was_enabled: true` reports there.
    Migrated {
        world: String,
        was_enabled: bool,
    },
    /// The file was relinked in a `saves/` folder that has neither
    /// `level.dat` nor `level.dat_old` (spec §0.5 A3). Minecraft does not
    /// treat that folder as a world and loads nothing from it, so no level
    /// file was read or written and no enabled/disabled state is claimed. A
    /// separate variant from [`WorldMigration::Migrated`] for the same reason
    /// as [`WorldMigration::Refreshed`]: this path does not know the state.
    Relinked {
        world: String,
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
    /// `false` ⟹ a world failed to clean up, or the cascade could not check
    /// whether a world names the pack, so the library copy and its
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
