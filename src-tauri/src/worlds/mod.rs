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

use crate::error::{Error, Result};
use std::path::PathBuf;

/// Enumerate singleplayer worlds in `instance_id`. A world is a
/// direct subdirectory of `<instance>/.minecraft/saves/`. Files
/// stray under saves/ are silently skipped (MC ignores them).
/// Missing saves/ dir → empty Vec, not an error.
pub fn list_worlds(app: &tauri::AppHandle, instance_id: &str) -> Result<Vec<World>> {
    let saves_dir = saves_dir(app, instance_id)?;
    if !saves_dir.exists() {
        return Ok(vec![]);
    }
    let backups_root = backups_root(app, instance_id)?;
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&saves_dir)
        .map_err(|e| Error::io(saves_dir.display().to_string(), e))?
    {
        let entry = entry.map_err(|e| Error::io(saves_dir.display().to_string(), e))?;
        let meta = entry
            .metadata()
            .map_err(|e| Error::io(entry.path().display().to_string(), e))?;
        if !meta.is_dir() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(String::from) else {
            // Non-UTF-8 names: skip (MC saves are user-typed, but
            // a rogue rename via OS could produce one — best-effort).
            continue;
        };
        // Reuse validate_segment as a defensive filter: anything we
        // would reject on input we also skip on listing (keeps the
        // UI from showing e.g. a `.git` dir if a user is hacking).
        if fs::validate_segment(&name).is_err() {
            continue;
        }
        let path = entry.path();
        let size_bytes = fs::dir_size(&path)? as f64;
        let modified_unix_ms = fs::dir_mtime_recursive(&path)? as f64;
        let backup_count = count_backups(&backups_root, &name)?;
        out.push(World {
            folder_name: name,
            size_bytes,
            modified_unix_ms,
            backup_count,
        });
    }
    // Sort by mtime desc — "most recently played" at top.
    out.sort_by(|a, b| {
        b.modified_unix_ms
            .partial_cmp(&a.modified_unix_ms)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(out)
}

/// `<instance>/.minecraft/saves/`. Created lazily on first MC launch;
/// may not exist on a fresh install (handled by caller).
pub fn saves_dir(app: &tauri::AppHandle, instance_id: &str) -> Result<PathBuf> {
    crate::paths::minecraft_dir(app, instance_id)
        .map(|p| p.join("saves"))
        .map_err(|e| Error::io("<saves_dir>", e))
}

/// `<instance>/backups/`. Created lazily on first backup.
pub fn backups_root(app: &tauri::AppHandle, instance_id: &str) -> Result<PathBuf> {
    crate::paths::instance_dir(app, instance_id)
        .map(|p| p.join("backups"))
        .map_err(|e| Error::io("<backups_root>", e))
}

fn count_backups(backups_root: &std::path::Path, world_folder: &str) -> Result<u32> {
    let world_backups = backups_root.join(world_folder);
    if !world_backups.exists() {
        return Ok(0);
    }
    let mut n: u32 = 0;
    for entry in std::fs::read_dir(&world_backups)
        .map_err(|e| Error::io(world_backups.display().to_string(), e))?
    {
        let entry = entry.map_err(|e| Error::io(world_backups.display().to_string(), e))?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("zip") {
            n = n.saturating_add(1);
        }
    }
    Ok(n)
}
