//! Client-side datapacks.
//!
//! Three levels on disk:
//!   * library — `<instance>/datapacks/<file>.zip`, the physical file;
//!   * world   — `<instance>/.minecraft/saves/<world>/datapacks/<file>.zip`,
//!               a hardlink to the library file;
//!   * registry — `<instance>/lucerna/installed-datapacks.json`, metadata only,
//!               reconciled against the library dir on every read.
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
pub mod pack_meta;
pub mod registry;
pub mod state;
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
    /// Always `None` in slice 1 (local files only). Reserved for the catalog.
    pub source: Option<crate::mods::platform::ModSource>,
    pub project_id: Option<String>,
    pub version_id: Option<String>,
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

/// `<instance>/.minecraft/saves/<world>/datapacks/`. The caller MUST have
/// validated `world` with `crate::worlds::fs::validate_segment` first.
pub fn world_datapacks_dir_at(instance_root: &Path, world: &str) -> PathBuf {
    instance_root
        .join(".minecraft")
        .join("saves")
        .join(world)
        .join("datapacks")
}

/// The value Minecraft writes into `level.dat`'s Enabled/Disabled lists for a
/// pack loaded from the world's `datapacks/` folder.
pub fn level_dat_entry(filename: &str) -> String {
    format!("file/{filename}")
}

/// `<instance>/datapacks/` for a live app handle.
pub fn library_dir(app: &tauri::AppHandle, instance_id: &str) -> Result<PathBuf> {
    crate::paths::instance_dir(app, instance_id)
        .map(|p| library_dir_at(&p))
        .map_err(|e| Error::io("<datapacks_library_dir>", e))
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
            world_datapacks_dir_at(root, "Survival"),
            Path::new("/inst/Foo/.minecraft/saves/Survival/datapacks")
        );
    }

    #[test]
    fn level_dat_entry_prefixes_with_file() {
        assert_eq!(level_dat_entry("veinminer.zip"), "file/veinminer.zip");
    }
}
