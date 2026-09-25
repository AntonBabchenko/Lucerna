//! The datapack update's world half: move every world holding the old
//! filename onto the new one, preserving each world's own enabled/disabled
//! choice. Takes `level_dat_lock` exactly once — see the parent module doc.

use std::path::Path;

use crate::datapacks::presence::{self, LevelDatPresence};
use crate::datapacks::{level_dat, level_dat_entry, library_dir_at};
use crate::error::{Error, Result};
use crate::mods::store::{materialize, LinkPolicy};

use super::placements::{placements_of, WorldPlacement};
use super::{level_dat_lock, map_removal_err, only_old, world_dirs_checked};

/// Move every world holding `old_filename` onto `new_filename`, preserving each
/// world's own enabled/disabled choice.
///
/// Both library files must already exist: the caller installs the new one
/// first and deletes the old one afterwards. This function owns only the world
/// side.
///
/// **It lives here, and takes [`level_dat_lock`] itself, for a reason.** The
/// lock is a non-reentrant `tokio::sync::Mutex` and all four locked entry
/// points in `mutate` (add, remove, toggle and the library cascade's
/// removal) take it internally, so a caller outside this module cannot
/// compose them into a read → unlink → relink → rewrite sequence: doing so
/// deadlocks with no error, no timeout and no log line. The lock is taken ONCE
/// here, around every world, and the module-private helpers are called
/// directly.
///
/// Every world's outcome is reported so the caller can tell the user exactly
/// which worlds moved. Re-running converges, because a migrated world no
/// longer holds `old_filename`. A world that could not be checked for the old
/// pack is reported `Failed` (Fallback discipline Q1), which keeps the OLD
/// library copy. `Err` only when no world could be checked at all: the old
/// library copy or `saves/` could not be read.
pub(crate) async fn migrate_placements(
    instance_root: &Path,
    old_filename: &str,
    new_filename: &str,
) -> Result<Vec<crate::datapacks::WorldMigration>> {
    use crate::datapacks::WorldMigration;

    let src = library_dir_at(instance_root).join(new_filename);
    // The NEW library file's hash, for the foreign-under-the-new-name check in
    // `migrate_one`. Unreadable ⟹ no world can migrate, and every world that
    // holds our old file is reported Failed below. An empty report would read
    // as "nothing to migrate": `datapacks::update` would call the update
    // complete and delete the OLD library file, and the worlds' copies would
    // stop counting as ours (Fallback discipline Q1). Failed keeps the old
    // library copy, so a retry converges.
    let src_sha = tokio::fs::read(&src)
        .await
        .map(|bytes| crate::datapacks::library::sha1_hex(&bytes));

    let _guard = level_dat_lock().lock().await;

    // Snapshot INSIDE the lock: a concurrent locked removal committing between
    // an outside-the-lock snapshot and the per-world writes would hand
    // `migrate_one` a world the user just emptied, and it would re-add the new
    // pack there, enabled. Spec §8.5 places identity verification under the
    // lock for exactly this reason.
    let placements = placements_of(instance_root, old_filename).await?;

    let mut report = Vec::with_capacity(placements.found.len() + placements.unchecked.len());
    for p in placements.found {
        if !p.is_ours {
            report.push(WorldMigration::SkippedNotOurs { world: p.world });
            continue;
        }
        let src_sha = match &src_sha {
            Ok(sha) => sha,
            Err(e) => {
                report.push(WorldMigration::Failed {
                    world: p.world,
                    details: Error::io(src.display().to_string(), e).to_string(),
                });
                continue;
            }
        };
        match migrate_one(instance_root, &src, src_sha, &p, new_filename).await {
            Ok(Some(was_enabled)) => report.push(WorldMigration::Migrated {
                world: p.world,
                was_enabled,
            }),
            // A folder with no level file: the file moved, and no state was
            // read, so none is claimed.
            Ok(None) => report.push(WorldMigration::Relinked { world: p.world }),
            Err(e) => report.push(WorldMigration::Failed {
                world: p.world,
                details: e.to_string(),
            }),
        }
    }
    // It may still hold the old pack: not migrated, and not "nothing to do".
    for (world, details) in placements.unchecked {
        report.push(WorldMigration::Failed { world, details });
    }
    Ok(report)
}

/// One world's half of [`migrate_placements`]. Assumes `level_dat_lock` is
/// ALREADY held by the caller — it takes no lock of its own, and must never
/// call the four locked entry points in `mutate`.
///
/// Step order is link-new → rewrite-level.dat → delete-old-LAST, and the
/// order is load-bearing for retry convergence. The retry finds a world by
/// its still-present OLD file (`placements_of`), so the old file must be the
/// last thing to go: with delete-first, a failure in the middle left the
/// world holding neither a findable old file nor a listed new one — invisible
/// to the re-run, which then reported success and deleted the old library
/// file, permanently. With delete-last, every failure position leaves the old
/// file in place and the re-run picks the world up again. The cost is a
/// transient both-files window in the failed state (the new file may sit
/// unlisted, which Minecraft auto-enables), but that state is REPORTED as
/// Failed and converges on retry — the opposite trade of silent permanent
/// loss.
///
/// Returns the enabled state it carried across, read from the world's
/// `level.dat`, or `None` for a folder with no level file, where nothing was
/// read and no state is claimed.
async fn migrate_one(
    instance_root: &Path,
    src: &Path,
    src_sha: &str,
    placement: &WorldPlacement,
    new_filename: &str,
) -> Result<Option<bool>> {
    let (world_dir, dp_dir) = world_dirs_checked(instance_root, &placement.world)?;

    // D2 first (spec §3 L.4). Nothing below may run in a world waiting to be
    // restored from level.dat_old: it reports Failed, `datapacks::update`
    // keeps the old library copy, and a retry converges once the world has
    // been opened. A folder with neither file is not a world to the game,
    // which loads nothing from it. The update still moves the FILE there so
    // the library row can be updated at all (§0.5 A3), and it neither reads
    // nor writes a level.dat.
    let level = match presence::of(&world_dir)? {
        LevelDatPresence::Present => Some(level_dat::read_at(&world_dir)?),
        LevelDatPresence::OnlyOld => return Err(only_old(&placement.world)),
        LevelDatPresence::Absent => None,
    };

    // The engine's id for the old pack is `file/` + its ON-DISK spelling
    // (N.0), which `placements_of` resolved by R2 — not the library's spelling.
    let old_entry = level_dat_entry(&placement.entry_name);
    let new_entry = level_dat_entry(new_filename);
    // Read the CURRENT state before changing anything, with the engine's rule
    // (`level_dat::carried_enabled`): enabled unless listed ONLY in Disabled,
    // on exact ids. A present pack named in neither list is ENABLED (the game
    // auto-adds it), and one in both lists is ENABLED (an available Enabled id
    // stays selected). But the NEW entry wins when it is already listed: a
    // previous partial run may have written it (and forgotten the old entry)
    // before failing at the old-file removal. Deriving from the old entry on
    // that retry would read "in neither list" = enabled and flip a disabled
    // pack back on — the exact reversal this whole function exists to prevent.
    let (was_enabled, edit) = match level {
        Some((mut root, framing)) => {
            let (enabled, disabled) = level_dat::lists(&root);
            let was_enabled =
                level_dat::carried_enabled(&enabled, &disabled, &old_entry, &new_entry);
            // Apply the level.dat edit in memory now, so a malformed DataPacks
            // refuses before the new file is linked. R3 (N.3): the old id, plus
            // any case variant of it no present entry spells exactly — a ghost
            // the engine already drops. A listing failure clears the exact
            // old id only.
            let present = match super::entry_names_of(&dp_dir).await {
                Ok(n) => Some(n),
                Err(e) => {
                    crate::diag!(
                        "datapacks: could not list {}: {e}; clearing the exact old id only",
                        dp_dir.display()
                    );
                    None
                }
            };
            let changed_forget =
                level_dat::forget_with_case_ghosts(&mut root, &old_entry, present.as_deref())?;
            let changed_set = level_dat::set_enabled(&mut root, &new_entry, was_enabled)?;
            (
                Some(was_enabled),
                (changed_forget || changed_set).then_some((root, framing)),
            )
        }
        // No level file: nothing to read, nothing to rewrite, and no state is
        // claimed. The caller reports `WorldMigration::Relinked`.
        None => (None, None),
    };

    tokio::fs::create_dir_all(&dp_dir)
        .await
        .map_err(|e| Error::ModsInstancePath {
            path: dp_dir.display().to_string(),
            details: e.to_string(),
        })?;
    let dest = dp_dir.join(new_filename);

    // The NEW name's slot in this world may already be occupied by something
    // that is not the new library file — the same F5 hazard as everywhere
    // else, one name over. `materialize` replaces its destination
    // unconditionally, so check first. Matching content falls through (a
    // previous partial run already linked it).
    match tokio::fs::metadata(&dest).await {
        Ok(meta) if meta.is_dir() => {
            return Err(Error::ModsFilenameConflict {
                filename: new_filename.to_string(),
                existing_sha: String::new(),
                incoming_sha: src_sha.to_string(),
            });
        }
        Ok(_) => {
            let existing_sha = match tokio::fs::read(&dest).await {
                Ok(bytes) => crate::datapacks::library::sha1_hex(&bytes),
                Err(_) => String::new(),
            };
            if existing_sha != src_sha {
                return Err(Error::ModsFilenameConflict {
                    filename: new_filename.to_string(),
                    existing_sha,
                    incoming_sha: src_sha.to_string(),
                });
            }
        }
        // Absent is a fact; any other error is ignorance. `materialize` replaces
        // its destination unconditionally, so reading "could not stat" as "the
        // slot is free" would let it overwrite a file we never identified.
        // Mirrors the discrimination at the removal site below.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(Error::ModsInstancePath {
                path: dest.display().to_string(),
                details: e.to_string(),
            })
        }
    }

    materialize(src, &dest, LinkPolicy::LinkIfPossible)
        .await
        .map_err(|e| Error::ModsInstancePath {
            path: e.path.display().to_string(),
            details: e.details(),
        })?;

    if let Some((root, framing)) = edit {
        level_dat::write_at(&world_dir, &root, framing).await?;
    }

    // Delete the OLD world-side entry, LAST (see the fn doc for why). Not a
    // tidiness step: left permanently, the stale file is present-and-unlisted
    // after the forget above, which Minecraft auto-enables — the world would
    // load BOTH versions.
    match tokio::fs::metadata(&placement.path).await {
        Ok(meta) => {
            // Same type-directed removal `remove_from_world_at` uses: Minecraft
            // loads folder datapacks too, and `remove_file` on a directory
            // fails with OS error 5 on Windows, which `map_removal_err` would
            // then mistranslate into "quit Minecraft and try again".
            let removal = if meta.is_dir() {
                tokio::fs::remove_dir_all(&placement.path).await
            } else {
                tokio::fs::remove_file(&placement.path).await
            };
            if let Err(e) = removal {
                return Err(map_removal_err(&placement.path, e, &placement.world));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(Error::ModsInstancePath {
                path: placement.path.display().to_string(),
                details: e.to_string(),
            })
        }
    }
    Ok(was_enabled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datapacks::level_dat;
    use crate::datapacks::level_dat::test_support::seed;
    use crate::datapacks::world_link::test_util::*;
    use crate::datapacks::world_link::{add_to_world_at, set_enabled_in_world_at};
    use crate::datapacks::WorldMigration;

    #[tokio::test]
    async fn migrate_preserves_a_disabled_pack_and_removes_the_old_file() {
        let _lock = hardlink_lock();
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "vm-1.zip", 48).await;
        let saves = td.path().join(".minecraft").join("saves");
        game_world(td.path(), "Alpha");
        add_to_world_at(td.path(), "Alpha", "vm-1.zip")
            .await
            .unwrap();
        // The user turns it OFF. Preserving this is the whole point.
        set_enabled_in_world_at(td.path(), "Alpha", "vm-1.zip", false)
            .await
            .unwrap();
        seed_library(td.path(), "vm-2.zip", 57).await;

        let report = migrate_placements(td.path(), "vm-1.zip", "vm-2.zip")
            .await
            .unwrap();

        assert_eq!(
            report,
            vec![crate::datapacks::WorldMigration::Migrated {
                world: "Alpha".to_string(),
                was_enabled: false,
            }]
        );

        let dp = saves.join("Alpha").join("datapacks");
        assert!(
            !dp.join("vm-1.zip").exists(),
            "the OLD world file must be gone: present-and-unlisted is auto-enabled by Minecraft, \
             so leaving it loads BOTH versions"
        );
        assert!(dp.join("vm-2.zip").exists());

        let (root, _) = level_dat::read_at(&world_dir(td.path(), "Alpha")).unwrap();
        let (enabled, disabled) = level_dat::lists(&root);
        assert!(
            disabled.iter().any(|s| s == "file/vm-2.zip"),
            "must still be disabled: {disabled:?}"
        );
        assert!(
            !enabled.iter().any(|s| s.contains("vm-")),
            "nothing may have been enabled: {enabled:?}"
        );
    }

    #[tokio::test]
    async fn migrate_keeps_an_unlisted_pack_enabled() {
        // A pack present on disk but named in NEITHER list is enabled —
        // Minecraft auto-enables it on the next load (state::derive's
        // `(true, _, false) => Enabled`). Reading enabled-ness as `in_enabled`
        // instead of `!in_disabled` silently turns such a pack off.
        let _lock = hardlink_lock();
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "vm-1.zip", 48).await;
        let saves = td.path().join(".minecraft").join("saves");
        let dp = saves.join("Beta").join("datapacks");
        game_world(td.path(), "Beta");
        std::fs::create_dir_all(&dp).unwrap();
        // Copy the library bytes so identity matches; the world's real
        // level.dat names the pack in neither list.
        std::fs::write(dp.join("vm-1.zip"), datapack_zip(48)).unwrap();
        seed_library(td.path(), "vm-2.zip", 57).await;

        let report = migrate_placements(td.path(), "vm-1.zip", "vm-2.zip")
            .await
            .unwrap();

        assert_eq!(
            report,
            vec![crate::datapacks::WorldMigration::Migrated {
                world: "Beta".to_string(),
                was_enabled: true,
            }]
        );
    }

    #[tokio::test]
    async fn migrate_in_a_world_without_a_datapacks_compound_creates_no_compound() {
        // The forget, then `set_enabled(.., true)`. If the forget created an
        // empty compound, the enable would write `[file/vm-2.zip]` with no
        // vanilla below it: the order bug §0.5 A15 removes.
        let _lock = hardlink_lock();
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "vm-1.zip", 48).await;
        let wd = world_dir(td.path(), "Old");
        level_dat::test_support::seed_root(
            &wd,
            &level_dat::test_support::game_root_without_datapacks(),
        );
        std::fs::create_dir_all(wd.join("datapacks")).unwrap();
        std::fs::write(wd.join("datapacks/vm-1.zip"), datapack_zip(48)).unwrap();
        let before = std::fs::read(wd.join("level.dat")).unwrap();
        seed_library(td.path(), "vm-2.zip", 57).await;

        let report = migrate_placements(td.path(), "vm-1.zip", "vm-2.zip")
            .await
            .unwrap();

        assert_eq!(
            report,
            vec![crate::datapacks::WorldMigration::Migrated {
                world: "Old".to_string(),
                was_enabled: true,
            }]
        );
        assert_eq!(std::fs::read(wd.join("level.dat")).unwrap(), before);
        assert!(wd.join("datapacks/vm-2.zip").exists());
        assert!(!wd.join("datapacks/vm-1.zip").exists());
    }

    #[tokio::test]
    async fn migrate_reports_failed_for_an_only_old_world_and_touches_nothing() {
        let _lock = hardlink_lock();
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "vm-1.zip", 48).await;
        let wd = world_dir(td.path(), "Restoring");
        level_dat::test_support::seed_old(&wd, &[], &["file/vm-1.zip"]);
        let old_before = std::fs::read(wd.join("level.dat_old")).unwrap();
        std::fs::create_dir_all(wd.join("datapacks")).unwrap();
        std::fs::write(wd.join("datapacks/vm-1.zip"), datapack_zip(48)).unwrap();
        seed_library(td.path(), "vm-2.zip", 57).await;

        let report = migrate_placements(td.path(), "vm-1.zip", "vm-2.zip")
            .await
            .unwrap();

        assert!(
            matches!(
                report.as_slice(),
                [crate::datapacks::WorldMigration::Failed { world, .. }] if world == "Restoring"
            ),
            "got {report:?}"
        );
        assert!(wd.join("datapacks/vm-1.zip").exists());
        assert!(!wd.join("datapacks/vm-2.zip").exists());
        assert!(!wd.join("level.dat").exists());
        assert_eq!(std::fs::read(wd.join("level.dat_old")).unwrap(), old_before);
    }

    #[tokio::test]
    async fn an_unreadable_new_library_file_fails_every_placement_and_keeps_the_old_file() {
        // Fallback discipline Q1: "the new file could not be read" is not
        // "nothing to migrate". An empty report let `datapacks::update` call
        // the update complete and delete the OLD library file, after which the
        // worlds' copies no longer count as ours and no later update reaches
        // them.
        let _lock = hardlink_lock();
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "vm-1.zip", 48).await;
        game_world(td.path(), "Alpha");
        add_to_world_at(td.path(), "Alpha", "vm-1.zip")
            .await
            .unwrap();
        // A same-named pack that is not ours stays reported as not ours.
        let foreign = world_dir(td.path(), "Gamma").join("datapacks");
        std::fs::create_dir_all(&foreign).unwrap();
        std::fs::write(foreign.join("vm-1.zip"), datapack_zip(57)).unwrap();
        // A directory where the new library file should be: it cannot be read.
        std::fs::create_dir_all(library_dir_at(td.path()).join("vm-2.zip")).unwrap();
        let wd = world_dir(td.path(), "Alpha");
        let level_before = std::fs::read(wd.join("level.dat")).unwrap();

        let report = migrate_placements(td.path(), "vm-1.zip", "vm-2.zip")
            .await
            .unwrap();

        assert_eq!(report.len(), 2, "got {report:?}");
        assert!(
            report.iter().any(|m| matches!(
                m,
                crate::datapacks::WorldMigration::Failed { world, .. } if world == "Alpha"
            )),
            "got {report:?}"
        );
        assert!(
            report.contains(&crate::datapacks::WorldMigration::SkippedNotOurs {
                world: "Gamma".into()
            }),
            "got {report:?}"
        );
        assert!(
            wd.join("datapacks/vm-1.zip").exists(),
            "the old world file stays"
        );
        assert!(!wd.join("datapacks/vm-2.zip").exists());
        assert_eq!(std::fs::read(wd.join("level.dat")).unwrap(), level_before);
    }

    #[tokio::test]
    async fn migrate_leaves_a_foreign_same_named_file_alone() {
        let _lock = hardlink_lock();
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "vm-1.zip", 48).await;
        let saves = td.path().join(".minecraft").join("saves");
        let dp = saves.join("Gamma").join("datapacks");
        std::fs::create_dir_all(&dp).unwrap();
        let foreign = datapack_zip(57);
        std::fs::write(dp.join("vm-1.zip"), &foreign).unwrap();
        seed_library(td.path(), "vm-2.zip", 61).await;

        let report = migrate_placements(td.path(), "vm-1.zip", "vm-2.zip")
            .await
            .unwrap();

        assert_eq!(
            report,
            vec![crate::datapacks::WorldMigration::SkippedNotOurs {
                world: "Gamma".to_string(),
            }]
        );
        assert_eq!(
            std::fs::read(dp.join("vm-1.zip")).unwrap(),
            foreign,
            "a same-named pack the user installed must survive untouched"
        );
        assert!(!dp.join("vm-2.zip").exists());
    }

    /// Engine (N.0): both lists ⇒ loaded ⇒ the update carries "enabled".
    #[tokio::test]
    async fn an_update_keeps_a_both_lists_pack_enabled() {
        let _lock = hardlink_lock();
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "vm-1.zip", 48).await;
        let wd = game_world(td.path(), "Alpha");
        add_to_world_at(td.path(), "Alpha", "vm-1.zip")
            .await
            .unwrap();
        seed(&wd, &["file/vm-1.zip"], &["file/vm-1.zip"]);
        seed_library(td.path(), "vm-2.zip", 57).await;
        let report = migrate_placements(td.path(), "vm-1.zip", "vm-2.zip")
            .await
            .unwrap();
        assert_eq!(
            report,
            vec![WorldMigration::Migrated {
                world: "Alpha".into(),
                was_enabled: true
            }]
        );
    }

    #[tokio::test]
    async fn an_update_ignores_a_drifted_disabled_entry() {
        let _lock = hardlink_lock();
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "vm-1.zip", 48).await;
        let wd = game_world(td.path(), "Alpha");
        add_to_world_at(td.path(), "Alpha", "vm-1.zip")
            .await
            .unwrap();
        seed(&wd, &[], &["file/VM-1.zip"]);
        seed_library(td.path(), "vm-2.zip", 57).await;
        let report = migrate_placements(td.path(), "vm-1.zip", "vm-2.zip")
            .await
            .unwrap();
        assert_eq!(
            report,
            vec![WorldMigration::Migrated {
                world: "Alpha".into(),
                was_enabled: true
            }]
        );
    }
}
