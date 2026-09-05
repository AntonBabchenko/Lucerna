//! Discovery and recovery of the artefacts an interrupted restore or world
//! migration can leave behind: a backup set whose world is gone, and a world
//! parked in a `.tmp-restoring-*` (restore) or `.tmp-migrate-moved-*` (migration)
//! directory.
//!
//! All of them are invisible to every existing listing — `validate_segment`
//! rejects a leading dot (`pathsafe::validate_segment`: "starts with '.'"),
//! `list_worlds` and `list_world_names_in` apply it to every entry under
//! `saves/`, and the world list is keyed on `saves/`, not `backups/`. This
//! module is the only thing that looks for them.
//!
//! Nothing here deletes anything. A parked directory may be the user's only
//! copy of a world — on a migration's rename path it always is until the final
//! rename lands (`worlds::restore::TMP_MIGRATE_MOVED_PREFIX`). The single mutation is
//! a rename back to the name it came from, and it refuses rather than
//! overwrite.

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

/// Which operation parked the world. The UI branches its copy on this: a
/// `.tmp-restoring-*` directory is an interrupted RESTORE, a
/// `.tmp-migrate-moved-*` directory is an interrupted MOVE from another
/// instance — telling a user who moved a world that a restore didn't finish
/// is a false statement about what happened (CLAUDE.md, Fallback discipline,
/// question 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum StrandedKind {
    Restore,
    Migration,
}

/// A world-sized directory parked by a restore or by a world migration.
///
/// The name alone does NOT say the operation failed. A restore's success path
/// cleans up best-effort (`swap_in_place` logs and carries on), and process
/// death between the second rename and that cleanup leaves the same name
/// behind — in which case `saves/<world_folder>` holds the RESTORED world and
/// this directory holds the pre-restore one. `target_occupied` is what tells
/// the two apart, and the UI must branch on it: telling a user their restore
/// "didn't finish" when it did, and offering to put the old copy back over the
/// new one, is worse than showing nothing.
///
/// A `.tmp-migrate-moved-*` directory is a migration stage in the TARGET instance's
/// `saves/` (`worlds::migrate`). It holds the user's only copy of the world: a
/// MOVE renamed the source folder into it. For it, `target_occupied` means only
/// that the final name was already taken in the target (the migration would
/// have suffixed it) — never that the migration finished, and never that the
/// stage is safe to delete. A copy-path stage (`.tmp-migrate-copy-*`) is never
/// listed here: its original is intact in the source instance. `kind` says
/// which operation parked it; a consumer that must word the two differently
/// branches on that, never on a parse of `dir_name`.
#[derive(Debug, Clone, Serialize, Type)]
pub struct StrandedWorld {
    /// The on-disk directory name, e.g. `.tmp-restoring-My World-0` or
    /// `.tmp-migrate-moved-My World-0`.
    pub dir_name: String,
    /// The name it came from.
    pub world_folder: String,
    /// `saves/<world_folder>` exists. For a restore: the restore finished and
    /// this is a leftover copy of the world as it was BEFORE it — putting it
    /// back would overwrite the result the user asked for. For a migration:
    /// a world of that name already lives in this instance, so the moved
    /// world cannot be put back under its name until one of them is renamed;
    /// it is NOT a leftover of a finished operation.
    pub target_occupied: bool,
    /// Which operation parked it — see [`StrandedKind`].
    pub kind: StrandedKind,
}

/// Backup sets under `backups` with no matching directory under `saves`.
///
/// Names that fail `validate_segment` are skipped: `restore_backup` rejects the
/// same names at its own boundary, so a row built from one would be a dead
/// button. Empty directories are skipped too — there is nothing to offer.
pub fn orphaned_backup_sets_at(saves: &Path, backups: &Path) -> Vec<OrphanedBackupSet> {
    // A stranded world has no `saves/<world>`, and `restore_replace` always
    // writes a pre-restore snapshot into `backups/<world>` — so without this its
    // backup set ALSO qualifies as orphaned, and the user would see
    // "Interrupted restore … the files are safe" directly above "these backups
    // belong to worlds that are no longer in this instance" about the same
    // world. The stranded section owns those; this one must not re-list them.
    let parked: std::collections::HashSet<String> = stranded_worlds_at(saves)
        .into_iter()
        .map(|s| s.world_folder)
        .collect();
    let Ok(entries) = std::fs::read_dir(backups) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        // `try_exists`, not `is_dir`: `is_dir()` answers false for any stat
        // failure, which would file a live-but-unreadable world's backups under
        // "backups without a world".
        let world_present = saves.join(&name).try_exists().unwrap_or(true);
        if wfs::validate_segment(&name).is_err() || world_present || parked.contains(&name) {
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

/// Directories under `saves` holding a world a restore could not put back, or
/// a world a migration parked in a `.tmp-migrate-moved-<world>-<n>` stage and did
/// not finish moving under its final name
/// (`worlds::restore::TMP_MIGRATE_MOVED_PREFIX`).
///
/// Restore staging directories (`.tmp-restore-stage-*`) are NOT included: they
/// hold extracted backup bytes, which can always be produced again, whereas a
/// parked directory may be the only copy of the world — on the migration's
/// rename path it is, which is exactly why the stage must be listed here and
/// why `recover_stranded_at` must be able to put it under `saves/<world>`.
/// `parked_world_of_tmp_dir` is what tells them apart.
///
/// No world listing shows any of these: `list_worlds` and `list_world_names_in`
/// (`worlds/mod.rs`) skip every entry that fails `validate_segment`, and
/// `pathsafe::validate_segment` rejects a leading `.` ("starts with '.'"). This
/// function is therefore the ONLY surface that can show a parked world.
pub fn stranded_worlds_at(saves: &Path) -> Vec<StrandedWorld> {
    let Ok(entries) = std::fs::read_dir(saves) else {
        return Vec::new();
    };
    let mut out: Vec<StrandedWorld> = entries
        .flatten()
        // Keep the entry when `file_type()` fails: dropping it would hide a
        // parked world from the only surface in the app that can recover it.
        // `parked_world_of_tmp_dir` is the real filter.
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(true))
        .filter_map(|e| {
            let dir_name = e.file_name().to_string_lossy().into_owned();
            let (world_folder, kind) = crate::worlds::restore::parked_world_of_tmp_dir(&dir_name)?;
            let target_occupied = saves.join(&world_folder).try_exists().unwrap_or(true);
            Some(StrandedWorld {
                dir_name,
                world_folder,
                target_occupied,
                kind,
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
        assert!(!got[0].target_occupied, "saves/My World does not exist");
    }

    #[test]
    fn a_leftover_from_a_successful_restore_is_marked_occupied() {
        let (_td, saves, _backups) = fixture();
        // The success path's cleanup is best-effort. When it fails, saves/W
        // holds the RESTORED world and the parked dir holds the pre-restore one
        // — the opposite of an interrupted restore, and putting it back would
        // overwrite what the user asked for.
        fs::create_dir_all(saves.join(".tmp-restoring-W-0")).unwrap();
        fs::create_dir_all(saves.join("W")).unwrap();

        let got = stranded_worlds_at(&saves);

        assert_eq!(got.len(), 1);
        assert!(
            got[0].target_occupied,
            "must not be reported as an unfinished restore"
        );
    }

    #[test]
    fn a_stranded_world_does_not_also_appear_as_an_orphaned_backup_set() {
        let (_td, saves, backups) = fixture();
        // Exactly the state the feature exists for: W parked, saves/W gone, and
        // restore_replace's snapshot sitting in backups/W.
        fs::create_dir_all(saves.join(".tmp-restoring-W-0")).unwrap();
        fs::create_dir_all(backups.join("W")).unwrap();
        fs::write(
            backups
                .join("W")
                .join("pre-restore-2026-01-01T00-00-00.zip"),
            b"z",
        )
        .unwrap();

        assert_eq!(stranded_worlds_at(&saves).len(), 1);
        assert!(
            orphaned_backup_sets_at(&saves, &backups).is_empty(),
            "the stranded section already owns this world; two sections \
             describing it with contradictory sentences is worse than one"
        );
    }

    #[test]
    fn recover_moves_the_directory_back() {
        let (_td, saves, _backups) = fixture();
        let dir = saves.join(".tmp-restoring-W-0");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("marker.txt"), b"mine").unwrap();

        let recovered = recover_stranded_at(&saves, ".tmp-restoring-W-0").unwrap();

        assert_eq!(recovered, "W");
        assert_eq!(
            fs::read(saves.join("W").join("marker.txt")).unwrap(),
            b"mine"
        );
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

    #[test]
    fn recover_rejects_a_traversal_smuggled_through_the_prefix() {
        let (_td, saves, _backups) = fixture();
        // `dir_name` is command input, not something only our own UI produces.
        // This one PARSES — the prefix and the trailing -0 are both right — so
        // only the validate_segment gate stands between it and a rename
        // destination outside saves/. Delete that line and this test fails;
        // the three names above are all rejected earlier and would not notice.
        assert!(recover_stranded_at(&saves, ".tmp-restoring-../evil-0").is_err());
        assert!(!saves.parent().unwrap().join("evil").exists());
    }

    #[test]
    fn a_migration_stage_is_listed_as_a_stranded_world() {
        let (_td, saves, _backups) = fixture();
        // `.tmp-migrate-moved-<world>-<n>` is the hidden stage `worlds::migrate` parks
        // a world in, inside the TARGET instance's saves/. On the rename path it
        // is the user's only copy, so it must surface here — this is the only
        // listing in the app that can show it.
        fs::create_dir_all(saves.join(".tmp-migrate-moved-Survival-1")).unwrap();
        fs::create_dir_all(saves.join("Normal")).unwrap();

        let got = stranded_worlds_at(&saves);

        assert_eq!(got.len(), 1, "got {got:?}");
        assert_eq!(got[0].world_folder, "Survival");
        assert_eq!(got[0].dir_name, ".tmp-migrate-moved-Survival-1");
        assert!(!got[0].target_occupied, "saves/Survival does not exist");
    }

    #[test]
    fn a_migration_stage_is_hidden_from_the_world_list() {
        let (_td, saves, _backups) = fixture();
        fs::create_dir_all(saves.join(".tmp-migrate-moved-Survival-1")).unwrap();
        fs::create_dir_all(saves.join("Survival")).unwrap();

        let names: Vec<String> = crate::worlds::list_world_names_in(&saves)
            .unwrap()
            .into_iter()
            .map(|w| w.folder_name)
            .collect();

        // `pathsafe::validate_segment` rejects a leading dot and both world
        // listings apply it to every saves/ entry, so a stage is never a
        // visible or launchable world. Pins the claim the module doc makes.
        assert_eq!(names, vec!["Survival".to_string()]);
    }

    #[test]
    fn recover_moves_a_migration_stage_under_its_world_name() {
        let (_td, saves, _backups) = fixture();
        let dir = saves.join(".tmp-migrate-moved-Survival-1");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("level.dat"), b"mine").unwrap();

        let recovered = recover_stranded_at(&saves, ".tmp-migrate-moved-Survival-1").unwrap();

        assert_eq!(recovered, "Survival");
        assert_eq!(
            fs::read(saves.join("Survival").join("level.dat")).unwrap(),
            b"mine"
        );
        assert!(
            !dir.exists(),
            "the stage is renamed into place, never copied"
        );
    }

    #[test]
    fn recover_refuses_a_migration_stage_whose_name_is_taken() {
        let (_td, saves, _backups) = fixture();
        // The migration would have suffixed the final name; a recovery cannot,
        // so it must refuse and leave BOTH directories exactly as they were.
        let stage = saves.join(".tmp-migrate-moved-W-0");
        fs::create_dir_all(&stage).unwrap();
        fs::write(stage.join("marker.txt"), b"parked").unwrap();
        fs::create_dir_all(saves.join("W")).unwrap();
        fs::write(saves.join("W").join("marker.txt"), b"other world").unwrap();

        let got = stranded_worlds_at(&saves);
        assert_eq!(got.len(), 1, "got {got:?}");
        assert!(got[0].target_occupied, "saves/W exists");

        let r = recover_stranded_at(&saves, ".tmp-migrate-moved-W-0");

        assert!(
            matches!(r, Err(Error::WorldRecoverTargetOccupied { .. })),
            "got {r:?}"
        );
        assert_eq!(
            fs::read(stage.join("marker.txt")).unwrap(),
            b"parked",
            "the stage must survive the refusal"
        );
        assert_eq!(
            fs::read(saves.join("W").join("marker.txt")).unwrap(),
            b"other world",
            "the live world must not be touched"
        );
    }

    #[test]
    fn recover_rejects_a_traversal_smuggled_through_the_migrate_prefix() {
        let (_td, saves, _backups) = fixture();
        // This PARSES — prefix and trailing -0 are both right — so only the
        // validate_segment gate stands between it and a rename destination
        // outside saves/. Mirrors the `.tmp-restoring-` case above.
        assert!(recover_stranded_at(&saves, ".tmp-migrate-moved-../evil-0").is_err());
        assert!(!saves.parent().unwrap().join("evil").exists());
    }

    #[test]
    fn stranded_worlds_carry_the_kind_that_parked_them() {
        let td = tempfile::tempdir().unwrap();
        let saves = td.path().join("saves");
        std::fs::create_dir_all(saves.join(".tmp-restoring-A-0")).unwrap();
        std::fs::create_dir_all(saves.join(".tmp-migrate-moved-B-0")).unwrap();
        std::fs::create_dir_all(saves.join(".tmp-migrate-copy-C-0")).unwrap();
        let listed = stranded_worlds_at(&saves);
        let kinds: Vec<(String, StrandedKind)> = listed
            .iter()
            .map(|w| (w.world_folder.clone(), w.kind))
            .collect();
        assert_eq!(
            kinds,
            vec![
                ("B".to_string(), StrandedKind::Migration),
                ("A".to_string(), StrandedKind::Restore),
            ],
            "sorted by dir_name: '.tmp-migrate-moved-B-0' < '.tmp-restoring-A-0'; the copy-path stage is not listed"
        );
    }
}
