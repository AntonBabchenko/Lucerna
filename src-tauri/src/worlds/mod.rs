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

/// Delete a world folder AND its backups directory.
///
/// `WorldNotFound` on a missing world; `WorldInUse` when the world tree is
/// held open (see `remove_world_dir_at`). The world is removed FIRST, then
/// `backups/<world>/`: a set that fails to go after the world is gone is an
/// `Io` error naming the set's path — honest about what happened (the world
/// is deleted, its backups are not) — whereas the reverse order could remove
/// every backup and then refuse the world with `WorldInUse`.
///
/// The cascade discriminates (`remove_backup_set_at`): an absent set is
/// nothing to do; any other failure is an error. Nothing here is best-effort.
pub fn delete_world(
    app: &tauri::AppHandle,
    instance_id: &str,
    world_folder_name: &str,
) -> Result<()> {
    let world_path = world_dir(app, instance_id, world_folder_name)?;
    remove_world_dir_at(&world_path, world_folder_name)?;
    let backups_for_world = backups_root(app, instance_id)?.join(world_folder_name);
    remove_backup_set_at(&backups_for_world)
}

/// Remove one world directory, mapping a held-open tree to `WorldInUse`.
///
/// Path-based so it is testable without an `AppHandle` and reusable by any
/// caller that has already resolved the world's path: `delete_world` above,
/// and the world-migration source removal (`worlds::migrate`, PR-B). It
/// removes ONLY `world_path` — a world's backups live outside the saves tree
/// and are the caller's business (`delete_world` cascades; a migration moves
/// them with `backup::move_set_at` instead).
///
/// A running Minecraft holds region/lock files open — surface that as the
/// friendly typed `WorldInUse` instead of a raw IO error. Windows: sharing
/// violation (32) / lock violation (33) / access denied (5). Every other
/// failure, INCLUDING a path that does not exist, is `Error::Io` naming the
/// path: "absent" is not "held open", and `WorldInUse`'s text tells the user
/// to quit Minecraft, which would be false advice for a missing folder.
pub fn remove_world_dir_at(world_path: &std::path::Path, folder_name: &str) -> Result<()> {
    std::fs::remove_dir_all(world_path).map_err(|e| {
        if matches!(e.raw_os_error(), Some(5) | Some(32) | Some(33)) {
            Error::WorldInUse {
                folder_name: folder_name.to_string(),
            }
        } else {
            Error::io(world_path.display().to_string(), e)
        }
    })
}

/// The backups cascade of `delete_world`: remove `<backups_root>/<world>/`.
/// Private and path-based so the discrimination below is test-pinned; the
/// cascade itself stays `delete_world`'s (the only caller). Migration does
/// not use it — it moves backup sets with `backup::move_set_at`.
///
/// Fallback discipline: the removal is attempted unconditionally and ITS
/// error is discriminated, replacing an `exists()` pre-check that answered
/// `false` for any stat failure and skipped the removal silently.
///
/// - `NotFound` ⇒ `Ok(())`: a world that was never backed up has no set —
///   `<instance>/backups/` itself is created lazily on the first backup, so
///   the whole root may be absent (Windows reports a missing parent as
///   ERROR_PATH_NOT_FOUND, which also decodes to `NotFound`). Absent is the
///   common case and genuinely nothing to do — that is discrimination (Q2),
///   not a swallow.
/// - any other error ⇒ `Err(Io)` naming the set: permission denied, a zip
///   held open, a transient I/O failure — "could not remove", never "absent".
///   Direction (Q1): restrictive. The world is already gone when this runs,
///   so the caller must be told that its backups are not (Q3/Q4: the
///   cleanup's own result is checked and reaches the user).
fn remove_backup_set_at(backups_for_world: &std::path::Path) -> Result<()> {
    match std::fs::remove_dir_all(backups_for_world) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::io(backups_for_world.display().to_string(), e)),
    }
}

/// Number of `*.zip` entries directly under `<backups_root>/<world_folder>/`;
/// `0` when that directory is absent. Used by `list_worlds` for the row's
/// backup count and by `worlds::migrate` to report how many archives a Move
/// will carry along.
pub(crate) fn count_backups(backups_root: &std::path::Path, world_folder: &str) -> Result<u32> {
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

#[cfg(test)]
mod remove_tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn remove_world_dir_at_removes_a_populated_world_directory() {
        let td = tempdir().unwrap();
        let saves = td.path().join("saves");
        let world = saves.join("Survival");
        fs::create_dir_all(world.join("region")).unwrap();
        fs::write(world.join("level.dat"), b"x").unwrap();
        fs::write(world.join("region").join("r.0.0.mca"), b"y").unwrap();

        remove_world_dir_at(&world, "Survival").unwrap();

        assert!(!world.try_exists().unwrap(), "the world dir must be gone");
        assert!(
            saves.try_exists().unwrap(),
            "only the world dir is removed — never its parent"
        );
    }

    #[test]
    fn remove_world_dir_at_reports_a_missing_path_as_io_not_world_in_use() {
        // A path that does not exist fails with NotFound — errno 2, or
        // ERROR_PATH_NOT_FOUND (3) on Windows when the parent is missing too —
        // never one of the three held-open codes (5/32/33). `WorldInUse`'s text
        // tells the user to quit Minecraft; for an absent folder that advice
        // would be false, so the mapping must not reach it.
        let td = tempdir().unwrap();
        let missing = td.path().join("saves").join("Gone");

        let err = remove_world_dir_at(&missing, "Gone").unwrap_err();

        assert!(
            !matches!(err, Error::WorldInUse { .. }),
            "a missing path must not render as 'quit Minecraft': {err:?}"
        );
        match err {
            Error::Io { path, .. } => assert_eq!(path, missing.display().to_string()),
            other => panic!("expected Error::Io naming the world path, got {other:?}"),
        }
    }

    // The errno 5/32/33 → WorldInUse branch is NOT tested here: it needs a
    // handle held open by another process with Windows sharing semantics, which
    // cannot be provoked portably from a unit test (POSIX lets `remove_dir_all`
    // succeed on a held-open tree). Faking it with a mocked error would test the
    // mock, not the mapping. The branch is byte-identical to the one `delete_world`
    // shipped with, and its Windows behaviour is covered by the dev smoke.

    #[test]
    fn remove_backup_set_at_removes_a_populated_set() {
        let td = tempdir().unwrap();
        let root = td.path().join("backups");
        let set = root.join("Survival");
        fs::create_dir_all(&set).unwrap();
        fs::write(set.join("Survival-2026-01-01_00-00-00.zip"), b"z").unwrap();

        remove_backup_set_at(&set).unwrap();

        assert!(!set.try_exists().unwrap(), "the set must be gone");
        assert!(
            root.try_exists().unwrap(),
            "only the world's set is removed — never the backups root"
        );
    }

    #[test]
    fn remove_backup_set_at_treats_an_absent_set_as_nothing_to_do() {
        // A world that was never backed up has no `backups/<world>/` — the
        // common case for `delete_world`, and genuinely nothing to remove.
        let td = tempdir().unwrap();
        let root = td.path().join("backups");
        fs::create_dir_all(&root).unwrap();

        remove_backup_set_at(&root.join("NeverBackedUp")).unwrap();
    }

    #[test]
    fn remove_backup_set_at_treats_an_absent_backups_root_as_nothing_to_do() {
        // `<instance>/backups/` is created lazily on the first backup, so a
        // fresh instance has no root at all. Windows reports a missing PARENT
        // as ERROR_PATH_NOT_FOUND (3), not ERROR_FILE_NOT_FOUND (2); both
        // decode to `ErrorKind::NotFound`, and this pins that the cascade
        // relies on the kind, not on the raw code.
        let td = tempdir().unwrap();
        let set = td.path().join("backups").join("Survival"); // neither exists

        remove_backup_set_at(&set).unwrap();
    }

    #[test]
    fn remove_backup_set_at_reports_any_failure_other_than_not_found() {
        // A regular file where the set directory should be: `remove_dir_all`
        // is documented to fail when the path is not a directory (ENOTDIR on
        // POSIX, ERROR_DIRECTORY on Windows) — a portable "could not remove"
        // that is NOT NotFound. The stat failures the old `exists()` pre-check
        // laundered into "absent" (permission denied, transient I/O) cannot be
        // provoked portably, so the restrictive direction is pinned through
        // this one: anything but NotFound must surface, naming the set.
        let td = tempdir().unwrap();
        let root = td.path().join("backups");
        fs::create_dir_all(&root).unwrap();
        let set = root.join("Survival");
        fs::write(&set, b"not a directory").unwrap();

        let err = remove_backup_set_at(&set).unwrap_err();

        match err {
            Error::Io { path, .. } => assert_eq!(path, set.display().to_string()),
            other => panic!("expected Error::Io naming the set, got {other:?}"),
        }
    }
}
