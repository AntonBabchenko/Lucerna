//! The datapack update's world half: move every world holding the old
//! filename onto the new one, preserving each world's own enabled/disabled
//! choice. Takes `level_dat_lock` exactly once — see the parent module doc.

use std::path::Path;

use crate::datapacks::{level_dat, level_dat_entry, library_dir_at};
use crate::error::{Error, Result};
use crate::mods::store::{materialize, LinkPolicy};

use super::placements::{placements_of, WorldPlacement};
use super::{
    contains_ci, level_dat_lock, map_removal_err, read_level_dat_or_empty, world_dirs_checked,
};

/// Move every world holding `old_filename` onto `new_filename`, preserving each
/// world's own enabled/disabled choice.
///
/// Both library files must already exist: the caller installs the new one
/// first and deletes the old one afterwards. This function owns only the world
/// side.
///
/// **It lives here, and takes [`level_dat_lock`] itself, for a reason.** The
/// lock is a non-reentrant `tokio::sync::Mutex` and all three public entry
/// points above take it internally, so a caller outside this module cannot
/// compose them into a read → unlink → relink → rewrite sequence: doing so
/// deadlocks with no error, no timeout and no log line. The lock is taken ONCE
/// here, around every world, and the module-private helpers are called
/// directly.
///
/// Never fails as a whole — every world's outcome is reported so the caller can
/// tell the user exactly which worlds moved. Re-running converges, because a
/// migrated world no longer holds `old_filename`.
pub(crate) async fn migrate_placements(
    instance_root: &Path,
    old_filename: &str,
    new_filename: &str,
) -> Vec<crate::datapacks::WorldMigration> {
    use crate::datapacks::WorldMigration;

    let src = library_dir_at(instance_root).join(new_filename);
    // The NEW library file's hash, for the foreign-under-the-new-name check in
    // `migrate_one`. Unreadable ⟹ nothing can migrate anyway — every
    // materialize would fail — so report nothing rather than guessing.
    let src_sha = match tokio::fs::read(&src).await {
        Ok(bytes) => crate::datapacks::library::sha1_hex(&bytes),
        Err(_) => return Vec::new(),
    };

    let _guard = level_dat_lock().lock().await;

    // Snapshot INSIDE the lock: a concurrent locked removal committing between
    // an outside-the-lock snapshot and the per-world writes would hand
    // `migrate_one` a world the user just emptied, and it would re-add the new
    // pack there, enabled. Spec §8.5 places identity verification under the
    // lock for exactly this reason.
    let placements = placements_of(instance_root, old_filename).await;

    let mut report = Vec::with_capacity(placements.len());
    for p in placements {
        if !p.is_ours {
            report.push(WorldMigration::SkippedNotOurs { world: p.world });
            continue;
        }
        match migrate_one(
            instance_root,
            &src,
            &src_sha,
            &p,
            old_filename,
            new_filename,
        )
        .await
        {
            Ok(was_enabled) => report.push(WorldMigration::Migrated {
                world: p.world,
                was_enabled,
            }),
            Err(e) => report.push(WorldMigration::Failed {
                world: p.world,
                details: e.to_string(),
            }),
        }
    }
    report
}

/// One world's half of [`migrate_placements`]. Assumes `level_dat_lock` is
/// ALREADY held by the caller — it takes no lock of its own, and must never
/// call the three public entry points above.
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
async fn migrate_one(
    instance_root: &Path,
    src: &Path,
    src_sha: &str,
    placement: &WorldPlacement,
    old_filename: &str,
    new_filename: &str,
) -> Result<bool> {
    let (world_dir, dp_dir) = world_dirs_checked(instance_root, &placement.world)?;

    // Read the CURRENT state before changing anything.
    //
    // Enabled-ness is `!in_disabled`, NOT `in_enabled`. A pack present on disk
    // and named in neither list is ENABLED — Minecraft auto-enables a
    // present-but-unlisted pack, which is what `state::derive`'s
    // `(true, _, false) => Enabled` arm encodes. Reading this as two-valued
    // silently writes such a pack into `Disabled`, turning off a pack the user
    // had on.
    //
    // But the NEW entry wins when it is already listed: a previous partial run
    // may have written it (and forgotten the old entry) before failing at the
    // old-file removal. Deriving from the old entry on that retry would read
    // "in neither list" = enabled and flip a disabled pack back on — the exact
    // reversal this whole function exists to prevent.
    let (mut root, framing) = read_level_dat_or_empty(&world_dir)?;
    let (enabled, disabled) = level_dat::lists(&root);
    let old_entry = level_dat_entry(old_filename);
    let new_entry = level_dat_entry(new_filename);
    let was_enabled = if contains_ci(&disabled, &new_entry) {
        false
    } else if contains_ci(&enabled, &new_entry) {
        true
    } else {
        !contains_ci(&disabled, &old_entry)
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

    // `forget_ci`, not `forget`: the old entry is known to us only by the
    // library's filename, and level.dat may hold a different case for the same
    // file. An exact match would remove nothing and leave a permanent orphan.
    let changed_forget = level_dat::forget_ci(&mut root, &old_entry)?;
    let changed_set = level_dat::set_enabled(&mut root, &new_entry, was_enabled)?;
    if changed_forget || changed_set {
        level_dat::write_at(&world_dir, &root, framing).await?;
    }

    // Delete the OLD world-side entry, LAST (see the fn doc for why). Not a
    // tidiness step: left permanently, the stale file is present-and-unlisted
    // after the `forget_ci` above, which Minecraft auto-enables — the world
    // would load BOTH versions.
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
    use crate::datapacks::world_link::test_util::*;
    use crate::datapacks::world_link::{add_to_world_at, set_enabled_in_world_at};

    #[tokio::test]
    async fn migrate_preserves_a_disabled_pack_and_removes_the_old_file() {
        let _lock = hardlink_lock();
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "vm-1.zip", 48).await;
        let saves = td.path().join(".minecraft").join("saves");
        std::fs::create_dir_all(saves.join("Alpha")).unwrap();
        add_to_world_at(td.path(), "Alpha", "vm-1.zip")
            .await
            .unwrap();
        // The user turns it OFF. Preserving this is the whole point.
        set_enabled_in_world_at(td.path(), "Alpha", "vm-1.zip", false)
            .await
            .unwrap();
        seed_library(td.path(), "vm-2.zip", 57).await;

        let report = migrate_placements(td.path(), "vm-1.zip", "vm-2.zip").await;

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
        std::fs::create_dir_all(&dp).unwrap();
        // Copy the library bytes so identity matches, but write NO level.dat:
        // both lists are absent.
        std::fs::write(dp.join("vm-1.zip"), datapack_zip(48)).unwrap();
        seed_library(td.path(), "vm-2.zip", 57).await;

        let report = migrate_placements(td.path(), "vm-1.zip", "vm-2.zip").await;

        assert_eq!(
            report,
            vec![crate::datapacks::WorldMigration::Migrated {
                world: "Beta".to_string(),
                was_enabled: true,
            }]
        );
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

        let report = migrate_placements(td.path(), "vm-1.zip", "vm-2.zip").await;

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
}
