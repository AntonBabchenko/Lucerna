//! Per-instance singleplayer-world backup + restore module.
//!
//! Public surface: list/backup/restore/delete commands + the four
//! specta-exported types. Implementation split across submodules:
//! `fs` for path-safety + size helpers, `zip` for the archive ops
//! with zip-slip defense, `backup` for the backup-side flow, and
//! `restore` for the multi-step replace/as_copy flow.

pub mod backup;
pub mod fs;
pub mod import;
pub mod orphans;
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

/// Lightweight world entry for the sidebar Play-button dropdown: folder
/// name + a recency proxy only. Cheaper than `World` (no recursive size or
/// backup-count walk), so it is safe to load on every instance switch.
#[derive(Debug, Clone, Serialize, Type)]
pub struct WorldQuickEntry {
    pub folder_name: String,
    pub modified_unix_ms: f64,
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
    for entry in
        std::fs::read_dir(&saves_dir).map_err(|e| Error::io(saves_dir.display().to_string(), e))?
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
        let (size_bytes, modified_unix_ms) = fs::dir_size_and_mtime(&path)?;
        let backup_count = count_backups(&backups_root, &name)?;
        out.push(World {
            folder_name: name,
            size_bytes: size_bytes as f64,
            modified_unix_ms: modified_unix_ms as f64,
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

/// Pure core of `list_world_names`: enumerate world folders directly under
/// a concrete `saves/` dir, newest-played first. Testable without a Tauri
/// `AppHandle`. Missing dir → empty Vec (not an error), matching
/// `list_worlds`. Same `validate_segment` filter so stray/hidden entries
/// never surface.
pub fn list_world_names_in(saves_dir: &std::path::Path) -> Result<Vec<WorldQuickEntry>> {
    if !saves_dir.exists() {
        return Ok(vec![]);
    }
    let mut out = Vec::new();
    for entry in
        std::fs::read_dir(saves_dir).map_err(|e| Error::io(saves_dir.display().to_string(), e))?
    {
        let entry = entry.map_err(|e| Error::io(saves_dir.display().to_string(), e))?;
        let meta = entry
            .metadata()
            .map_err(|e| Error::io(entry.path().display().to_string(), e))?;
        if !meta.is_dir() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(String::from) else {
            continue;
        };
        if fs::validate_segment(&name).is_err() {
            continue;
        }
        let modified_unix_ms = fs::world_recency_ms(&entry.path()) as f64;
        out.push(WorldQuickEntry {
            folder_name: name,
            modified_unix_ms,
        });
    }
    out.sort_by(|a, b| {
        b.modified_unix_ms
            .partial_cmp(&a.modified_unix_ms)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(out)
}

/// Lightweight world list for the sidebar Play dropdown. Resolves the
/// instance's `saves/` dir then defers to `list_world_names_in`.
pub fn list_world_names(app: &tauri::AppHandle, instance_id: &str) -> Result<Vec<WorldQuickEntry>> {
    let saves_dir = saves_dir(app, instance_id)?;
    list_world_names_in(&saves_dir)
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

/// Validate a world folder name and resolve it under a concrete `saves/` dir.
/// The three-step validate → join → `is_dir` sequence was duplicated at every
/// call site; it lives here now. Testable without a Tauri `AppHandle`.
///
/// The `WorldNotFound` this returns carries an empty `instance_id` — this core
/// has no handle to one. `Error::WorldNotFound`'s `Display` interpolates that
/// field into a user-facing string, so a caller that has the real instance id
/// should fill it in, as `world_dir` does below.
pub fn world_dir_at(saves_dir: &std::path::Path, world_folder_name: &str) -> Result<PathBuf> {
    fs::validate_segment(world_folder_name)?;
    let world_path = saves_dir.join(world_folder_name);
    if !world_path.is_dir() {
        return Err(Error::WorldNotFound {
            instance_id: String::new(),
            folder_name: world_folder_name.into(),
        });
    }
    Ok(world_path)
}

/// `world_dir_at` for a live app handle; fills in `instance_id` on the
/// not-found error, which the `*_at` core cannot know.
pub fn world_dir(
    app: &tauri::AppHandle,
    instance_id: &str,
    world_folder_name: &str,
) -> Result<PathBuf> {
    let saves = saves_dir(app, instance_id)?;
    world_dir_at(&saves, world_folder_name).map_err(|e| match e {
        Error::WorldNotFound { folder_name, .. } => Error::WorldNotFound {
            instance_id: instance_id.into(),
            folder_name,
        },
        other => other,
    })
}

/// Delete a world folder AND its associated backups directory.
/// Errors with WorldNotFound on missing world; best-effort cleanup
/// of the backups subdirectory (silently ignores a missing backups
/// subdirectory).
pub fn delete_world(
    app: &tauri::AppHandle,
    instance_id: &str,
    world_folder_name: &str,
) -> Result<()> {
    let world_path = world_dir(app, instance_id, world_folder_name)?;
    std::fs::remove_dir_all(&world_path).map_err(|e| {
        // A running Minecraft holds region/lock files open — surface that as
        // the friendly typed WorldInUse instead of a raw IO error. Windows:
        // sharing violation (32) / lock violation (33) / access denied (5).
        if matches!(e.raw_os_error(), Some(5) | Some(32) | Some(33)) {
            Error::WorldInUse {
                folder_name: world_folder_name.to_string(),
            }
        } else {
            Error::io(world_path.display().to_string(), e)
        }
    })?;
    // Cascade: drop the backups dir for this world if it exists.
    let backups_for_world = backups_root(app, instance_id)?.join(world_folder_name);
    if backups_for_world.exists() {
        std::fs::remove_dir_all(&backups_for_world)
            .map_err(|e| Error::io(backups_for_world.display().to_string(), e))?;
    }
    Ok(())
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

#[cfg(test)]
mod quick_list_tests {
    use super::*;
    use std::fs;
    use std::time::Duration;
    use tempfile::tempdir;

    fn make_world(saves: &std::path::Path, name: &str) {
        let dir = saves.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("level.dat"), b"x").unwrap();
    }

    #[test]
    fn missing_saves_dir_is_empty() {
        let td = tempdir().unwrap();
        let saves = td.path().join("saves"); // not created
        assert!(list_world_names_in(&saves).unwrap().is_empty());
    }

    #[test]
    fn lists_world_folders_newest_first() {
        let td = tempdir().unwrap();
        let saves = td.path().join("saves");
        fs::create_dir_all(&saves).unwrap();
        make_world(&saves, "Older");
        std::thread::sleep(Duration::from_millis(50));
        make_world(&saves, "Newer");
        let got = list_world_names_in(&saves).unwrap();
        let names: Vec<&str> = got.iter().map(|w| w.folder_name.as_str()).collect();
        assert_eq!(names, vec!["Newer", "Older"]);
    }

    #[test]
    fn skips_stray_files_and_invalid_segments() {
        let td = tempdir().unwrap();
        let saves = td.path().join("saves");
        fs::create_dir_all(&saves).unwrap();
        make_world(&saves, "Good");
        fs::write(saves.join("loose.txt"), b"x").unwrap(); // a file, not a world
        fs::create_dir_all(saves.join(".hidden")).unwrap(); // rejected by validate_segment
        let got = list_world_names_in(&saves).unwrap();
        let names: Vec<&str> = got.iter().map(|w| w.folder_name.as_str()).collect();
        assert_eq!(names, vec!["Good"]);
    }

    #[test]
    fn world_dir_at_rejects_a_path_separator() {
        let td = tempfile::tempdir().unwrap();
        let err = world_dir_at(td.path(), "a/b").unwrap_err();
        assert!(matches!(err, crate::error::Error::WorldPathInvalid { .. }));
    }

    #[test]
    fn world_dir_at_reports_missing_world() {
        let td = tempfile::tempdir().unwrap();
        let err = world_dir_at(td.path(), "Nope").unwrap_err();
        assert!(matches!(err, crate::error::Error::WorldNotFound { .. }));
    }

    #[test]
    fn world_dir_at_returns_the_dir_when_it_exists() {
        let td = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(td.path().join("Survival")).unwrap();
        assert_eq!(
            world_dir_at(td.path(), "Survival").unwrap(),
            td.path().join("Survival")
        );
    }
}
