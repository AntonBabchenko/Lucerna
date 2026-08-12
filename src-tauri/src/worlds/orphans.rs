//! Discovery and recovery of the two artefacts a failed restore can leave
//! behind: a backup set whose world is gone, and a world stranded in a
//! `.tmp-restoring-*` directory.
//!
//! Both are invisible to every existing listing — `validate_segment` rejects a
//! leading dot, and the world list is keyed on `saves/`, not `backups/`. This
//! module is the only thing that looks for them.
//!
//! Nothing here deletes anything. A stranded directory may be the user's only
//! copy of a world; the single mutation is a rename back to the name it came
//! from, and it refuses rather than overwrite.

use crate::error::{Error, Result};
use crate::worlds::fs as wfs;
use crate::worlds::restore::world_folder_of_tmp_dir;
use serde::Serialize;
use specta::Type;
use std::path::Path;

/// A backup directory with no matching world.
#[derive(Debug, Clone, Serialize, Type)]
pub struct OrphanedBackupSet {
    pub world_folder: String,
    pub backup_count: u32,
    /// Milliseconds since the epoch. `f64` because specta has no `u64`.
    pub newest_unix_ms: f64,
}

/// A world left behind by a restore that could not put it back.
#[derive(Debug, Clone, Serialize, Type)]
pub struct StrandedWorld {
    /// The on-disk directory name, e.g. `.tmp-restoring-My World-0`.
    pub dir_name: String,
    /// The name it should be restored to.
    pub world_folder: String,
}

/// Backup sets under `backups` with no matching directory under `saves`.
///
/// Names that fail `validate_segment` are skipped: `restore_backup` rejects the
/// same names at its own boundary, so a row built from one would be a dead
/// button. Empty directories are skipped too — there is nothing to offer.
pub fn orphaned_backup_sets_at(saves: &Path, backups: &Path) -> Vec<OrphanedBackupSet> {
    let Ok(entries) = std::fs::read_dir(backups) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if wfs::validate_segment(&name).is_err() || saves.join(&name).is_dir() {
            continue;
        }
        let Ok(zips) = std::fs::read_dir(entry.path()) else {
            continue;
        };
        let mut count = 0u32;
        let mut newest = 0f64;
        for z in zips.flatten() {
            if z.path().extension().map(|e| e == "zip").unwrap_or(false) {
                count += 1;
                if let Some(ms) = modified_unix_ms(&z.path()) {
                    newest = newest.max(ms);
                }
            }
        }
        if count > 0 {
            out.push(OrphanedBackupSet {
                world_folder: name,
                backup_count: count,
                newest_unix_ms: newest,
            });
        }
    }
    out.sort_by(|a, b| a.world_folder.cmp(&b.world_folder));
    out
}

fn modified_unix_ms(p: &Path) -> Option<f64> {
    let t = std::fs::metadata(p).ok()?.modified().ok()?;
    let d = t.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(d.as_millis() as f64)
}

/// Directories under `saves` holding a world a restore could not put back.
///
/// Staging directories are NOT included: they hold extracted backup bytes, which
/// can always be produced again, whereas a stranded directory may be the only
/// copy of the world. `world_folder_of_tmp_dir` is what tells them apart.
pub fn stranded_worlds_at(saves: &Path) -> Vec<StrandedWorld> {
    let Ok(entries) = std::fs::read_dir(saves) else {
        return Vec::new();
    };
    let mut out: Vec<StrandedWorld> = entries
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|e| {
            let dir_name = e.file_name().to_string_lossy().into_owned();
            let world_folder = world_folder_of_tmp_dir(&dir_name)?;
            Some(StrandedWorld {
                dir_name,
                world_folder,
            })
        })
        .collect();
    out.sort_by(|a, b| a.dir_name.cmp(&b.dir_name));
    out
}

/// Rename a stranded directory back to the world name encoded in it. Returns
/// that name.
///
/// Refuses when the destination exists: overwriting a live world to recover an
/// older copy of it would be a worse loss than the one being repaired. Uses
/// `try_exists`, not `exists`, so a stat failure reads as occupied — the
/// restrictive direction.
pub fn recover_stranded_at(saves: &Path, dir_name: &str) -> Result<String> {
    let Some(world_folder) = world_folder_of_tmp_dir(dir_name) else {
        return Err(Error::WorldPathInvalid {
            name: dir_name.to_string(),
            reason: "not a stranded-world directory".into(),
        });
    };
    wfs::validate_segment(&world_folder)?;
    let to = saves.join(&world_folder);
    if to.try_exists().unwrap_or(true) {
        return Err(Error::WorldRecoverTargetOccupied { world_folder });
    }
    std::fs::rename(saves.join(dir_name), &to)
        .map_err(|e| Error::io(to.display().to_string(), e))?;
    Ok(world_folder)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn fixture() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
        let td = tempdir().unwrap();
        let saves = td.path().join("saves");
        let backups = td.path().join("backups");
        fs::create_dir_all(&saves).unwrap();
        fs::create_dir_all(&backups).unwrap();
        (td, saves, backups)
    }

    #[test]
    fn orphans_are_backup_dirs_with_no_world() {
        let (_td, saves, backups) = fixture();
        fs::create_dir_all(saves.join("Alive")).unwrap();
        fs::create_dir_all(backups.join("Alive")).unwrap();
        fs::write(backups.join("Alive").join("2026-01-01T00-00-00.zip"), b"z").unwrap();
        fs::create_dir_all(backups.join("Gone")).unwrap();
        fs::write(backups.join("Gone").join("2026-01-02T00-00-00.zip"), b"z").unwrap();

        let got = orphaned_backup_sets_at(&saves, &backups);

        assert_eq!(got.len(), 1, "got {got:?}");
        assert_eq!(got[0].world_folder, "Gone");
        assert_eq!(got[0].backup_count, 1);
    }

    #[test]
    fn orphans_exclude_names_that_fail_validate_segment() {
        let (_td, saves, backups) = fixture();
        // A dot-prefixed directory would render a row whose Restore button dies
        // at the command boundary with WorldPathInvalid — a dead button.
        fs::create_dir_all(backups.join(".hidden")).unwrap();
        fs::write(backups.join(".hidden").join("a.zip"), b"z").unwrap();

        assert!(orphaned_backup_sets_at(&saves, &backups).is_empty());
    }

    #[test]
    fn orphans_exclude_empty_backup_dirs() {
        let (_td, saves, backups) = fixture();
        fs::create_dir_all(backups.join("Gone")).unwrap();

        assert!(
            orphaned_backup_sets_at(&saves, &backups).is_empty(),
            "a backups dir with no zips is nothing to offer"
        );
    }

    #[test]
    fn stranded_worlds_are_tmp_restoring_dirs_only() {
        let (_td, saves, _backups) = fixture();
        fs::create_dir_all(saves.join(".tmp-restoring-My World-0")).unwrap();
        fs::create_dir_all(saves.join(".tmp-restore-stage-My World-0")).unwrap();
        fs::create_dir_all(saves.join("Normal")).unwrap();

        let got = stranded_worlds_at(&saves);

        assert_eq!(
            got.len(),
            1,
            "a staging dir holds extracted backup bytes, not a world: {got:?}"
        );
        assert_eq!(got[0].world_folder, "My World");
        assert_eq!(got[0].dir_name, ".tmp-restoring-My World-0");
    }

    #[test]
    fn recover_moves_the_directory_back() {
        let (_td, saves, _backups) = fixture();
        let dir = saves.join(".tmp-restoring-W-0");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("marker.txt"), b"mine").unwrap();

        let recovered = recover_stranded_at(&saves, ".tmp-restoring-W-0").unwrap();

        assert_eq!(recovered, "W");
        assert_eq!(fs::read(saves.join("W").join("marker.txt")).unwrap(), b"mine");
        assert!(!dir.exists());
    }

    #[test]
    fn recover_refuses_when_the_target_name_is_occupied() {
        let (_td, saves, _backups) = fixture();
        fs::create_dir_all(saves.join(".tmp-restoring-W-0")).unwrap();
        fs::create_dir_all(saves.join("W")).unwrap();

        let r = recover_stranded_at(&saves, ".tmp-restoring-W-0");

        assert!(r.is_err(), "must never overwrite a live world");
        assert!(
            saves.join(".tmp-restoring-W-0").is_dir(),
            "and must not destroy the stranded copy either"
        );
    }

    #[test]
    fn recover_rejects_a_name_that_is_not_ours() {
        let (_td, saves, _backups) = fixture();
        fs::create_dir_all(saves.join("Normal")).unwrap();
        assert!(recover_stranded_at(&saves, "Normal").is_err());
        assert!(recover_stranded_at(&saves, "../escape").is_err());
        assert!(recover_stranded_at(&saves, ".tmp-restore-stage-W-0").is_err());
    }
}
