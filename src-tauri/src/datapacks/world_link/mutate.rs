//! The three locked single-world entry points (add / remove / toggle) and
//! their conflict gate. Each takes `level_dat_lock` itself — see the parent
//! module doc for why they must never be composed under it.

use std::path::Path;

use crate::datapacks::{level_dat, level_dat_entry, library_dir_at};
use crate::error::{Error, Result};
use crate::mods::store::{materialize, LinkPolicy, Placement};

use super::{level_dat_lock, map_removal_err, read_level_dat_or_empty, world_dirs_checked};

/// `Some(err)` when `dest` already holds something that is NOT the library
/// file at `src`, so placing over it would destroy a pack Lucerna did not put
/// there. `None` when the destination is provably free, or already holds
/// exactly these bytes. Anything this function could not SEE — an unstatable
/// or unreadable entry — is an `Err`, never a verdict: `materialize` replaces
/// its destination unconditionally, so answering "free" out of ignorance
/// would destroy a file that was never identified.
///
/// A DIRECTORY is always a conflict: Minecraft loads folder datapacks, and a
/// folder has no file sha1 to compare, so it can never be proven ours. Doing
/// otherwise would let `materialize` rename a zip over a whole pack folder.
///
/// Sizes are compared before hashing so a large pack costs one `metadata` call
/// in the common "different pack" case rather than two full reads.
async fn conflicting_world_entry(src: &Path, dest: &Path) -> Result<Option<Error>> {
    let dest_meta = match tokio::fs::metadata(dest).await {
        Ok(meta) => meta,
        // Absent is a fact — nothing there, free to place. Any other error is
        // ignorance. Mirrors `migrate_one`'s discrimination at its own
        // destination check in this module's `migrate.rs`.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(Error::ModsInstancePath {
                path: dest.display().to_string(),
                details: e.to_string(),
            })
        }
    };
    let filename = dest
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    if dest_meta.is_dir() {
        return Ok(Some(Error::ModsFilenameConflict {
            filename,
            existing_sha: String::new(),
            incoming_sha: String::new(),
        }));
    }
    let src_meta = tokio::fs::metadata(src)
        .await
        .map_err(|e| Error::io(src.display().to_string(), e))?;
    if src_meta.len() == dest_meta.len() {
        let a = tokio::fs::read(src)
            .await
            .map_err(|e| Error::io(src.display().to_string(), e))?;
        let b = match tokio::fs::read(dest).await {
            Ok(bytes) => bytes,
            // Vanished between the stat above and this read: the slot really
            // is free now — the same fact the NotFound arm above records.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(Error::ModsInstancePath {
                    path: dest.display().to_string(),
                    details: e.to_string(),
                })
            }
        };
        let (sa, sb) = (
            crate::datapacks::library::sha1_hex(&a),
            crate::datapacks::library::sha1_hex(&b),
        );
        if sa == sb {
            return Ok(None); // already ours — fall through so a disabled pack re-enables
        }
        return Ok(Some(Error::ModsFilenameConflict {
            filename,
            existing_sha: sb,
            incoming_sha: sa,
        }));
    }
    // Different sizes ⇒ different content; no need to hash either side. The
    // shas below are only for the conflict report — but a fabricated blank
    // hash is not a report. A failed read propagates exactly as in the
    // equal-size branch above; the two branches must not disagree about what
    // an unreadable entry means.
    let existing_sha = match tokio::fs::read(dest).await {
        Ok(bytes) => crate::datapacks::library::sha1_hex(&bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(Error::ModsInstancePath {
                path: dest.display().to_string(),
                details: e.to_string(),
            })
        }
    };
    let incoming_sha = tokio::fs::read(src)
        .await
        .map(|bytes| crate::datapacks::library::sha1_hex(&bytes))
        .map_err(|e| Error::io(src.display().to_string(), e))?;
    Ok(Some(Error::ModsFilenameConflict {
        filename,
        existing_sha,
        incoming_sha,
    }))
}

/// Link a library pack into a world's `datapacks/` folder and mark it
/// enabled in level.dat.
pub async fn add_to_world_at(
    instance_root: &Path,
    world: &str,
    filename: &str,
) -> Result<Placement> {
    if !crate::pathsafe::is_safe_filename(filename) {
        return Err(Error::ModsUnsafeFilename {
            filename: filename.to_string(),
        });
    }
    let (world_dir, dp_dir) = world_dirs_checked(instance_root, world)?;

    // Check the source before handing it to `materialize`: without this, a
    // missing library file reaches `materialize`, which logs a misleading
    // "hardlink failed; falling back to a copy" diagnostic and then fails
    // `NotFound` against the DESTINATION path — telling the user the file
    // that is supposed not to exist yet cannot be found.
    let src = library_dir_at(instance_root).join(filename);
    match tokio::fs::metadata(&src).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(Error::ModsInstancePath {
                path: src.display().to_string(),
                details: format!("{filename} is not in this instance's datapack library"),
            });
        }
        Err(e) => {
            return Err(Error::ModsInstancePath {
                path: src.display().to_string(),
                details: e.to_string(),
            });
        }
    }

    let _guard = level_dat_lock().lock().await;

    tokio::fs::create_dir_all(&dp_dir)
        .await
        .map_err(|e| Error::ModsInstancePath {
            path: dp_dir.display().to_string(),
            details: e.to_string(),
        })?;

    let dest = dp_dir.join(filename);

    // Refuse to replace a DIFFERENT entry that already holds this name.
    // `materialize` commits by unconditional rename over the destination, so
    // without this a pack the user dropped into the world folder themselves —
    // or one that arrived with an imported world — is destroyed silently.
    //
    // Matching content deliberately falls THROUGH rather than returning early:
    // re-adding a pack that is present but sits in the world's `Disabled` list
    // must re-enable it, which the `set_enabled(.., true)` below does. That is
    // exactly what the library screen's world picker relies on when a user
    // ticks a world where the pack is currently off, so turning this into a
    // no-op would delete a behaviour the UI depends on.
    if let Some(conflict) = conflicting_world_entry(&src, &dest).await? {
        return Err(conflict);
    }

    // `LinkIfPossible`, not `ForceCopy`: deduplicating one physical pack
    // across every world that installs it is worth keeping. But `store.rs`'s
    // stated justification for `LinkIfPossible` — "corruption is a
    // re-download, never data loss" — does NOT hold here: a datapack's
    // `source` is always `None` in this slice, so this library copy is the
    // only one Lucerna has. The accepted consequence is the mod-jar hazard
    // this feature inherits on purpose: a user opening
    // `saves/<world>/datapacks/<file>.zip` in an archive tool and saving
    // edits the library copy and every other world linking it, in place.
    // That is the user acting on their own file, not a hazard Lucerna
    // introduces, so the link stays.
    let placement = materialize(&src, &dest, LinkPolicy::LinkIfPossible)
        .await
        .map_err(|e| Error::ModsInstancePath {
            path: e.path.display().to_string(),
            details: e.details(),
        })?;

    let (mut root, framing) = read_level_dat_or_empty(&world_dir)?;
    let entry = level_dat_entry(filename);
    // Write only when the toggle actually changed something: `write_at` rolls
    // the pre-edit backup forward on every call, so a redundant write would
    // replace the last pristine copy with a copy of the state we're already
    // in.
    if level_dat::set_enabled(&mut root, &entry, true)? {
        level_dat::write_at(&world_dir, &root, framing).await?;
    }

    Ok(placement)
}

/// Unlink a datapack from a world and drop its level.dat entry from both
/// lists. Idempotent: a missing file is `Ok`, and this doubles as the repair
/// path for an `Orphaned` row — a level.dat name with no file — since it
/// still clears the name even when there is nothing to unlink.
pub async fn remove_from_world_at(instance_root: &Path, world: &str, filename: &str) -> Result<()> {
    if !crate::pathsafe::is_safe_filename(filename) {
        return Err(Error::ModsUnsafeFilename {
            filename: filename.to_string(),
        });
    }
    let (world_dir, dp_dir) = world_dirs_checked(instance_root, world)?;

    let _guard = level_dat_lock().lock().await;

    // `filename` was already validated above by `is_safe_filename`, which
    // requires exactly one `Normal` path component — no separator, no `..`,
    // no absolute prefix — and `dp_dir` only ever resolves under a validated
    // world segment (`world_dirs_checked`). So `path` can never point above
    // `<world>/datapacks/`, which is what makes the unconditional
    // `remove_dir_all` below safe to call.
    let path = dp_dir.join(filename);
    match tokio::fs::metadata(&path).await {
        Ok(meta) => {
            // Minecraft loads DIRECTORIES from `datapacks/` too, not just
            // `.zip` files, and records them in level.dat the same way a zip
            // is recorded. Picking the removal call by the entry's real type
            // is what the old code got wrong: `remove_file` on a directory
            // fails with OS error 5 on Windows, which the mapping below used
            // to turn into a false "quit Minecraft and try again" even with
            // Minecraft closed.
            let removal = if meta.is_dir() {
                tokio::fs::remove_dir_all(&path).await
            } else {
                tokio::fs::remove_file(&path).await
            };
            if let Err(e) = removal {
                return Err(map_removal_err(&path, e, world));
            }
        }
        // Idempotent: no entry at all is exactly the orphan-repair case.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(Error::ModsInstancePath {
                path: path.display().to_string(),
                details: e.to_string(),
            })
        }
    }

    let (mut root, framing) = read_level_dat_or_empty(&world_dir)?;
    let entry = level_dat_entry(filename);
    // `forget_ci`, not `forget`: the name arrives from a UI row or — on the
    // cascade path — from the library registry, and level.dat may spell the
    // same file with different case (NTFS is case-insensitive; the drift is
    // documented and encountered — see `contains_ci`). An exact match would
    // delete the file but keep the name: a permanent Orphaned row and
    // Minecraft's "data packs are no longer present" screen.
    if level_dat::forget_ci(&mut root, &entry)? {
        level_dat::write_at(&world_dir, &root, framing).await?;
    }
    Ok(())
}

/// Toggle a datapack's enabled/disabled state for one world. level.dat only —
/// the file itself is never touched.
pub async fn set_enabled_in_world_at(
    instance_root: &Path,
    world: &str,
    filename: &str,
    enabled: bool,
) -> Result<()> {
    if !crate::pathsafe::is_safe_filename(filename) {
        return Err(Error::ModsUnsafeFilename {
            filename: filename.to_string(),
        });
    }
    let (world_dir, _dp_dir) = world_dirs_checked(instance_root, world)?;

    let _guard = level_dat_lock().lock().await;

    let (mut root, framing) = read_level_dat_or_empty(&world_dir)?;
    let entry = level_dat_entry(filename);
    if level_dat::set_enabled(&mut root, &entry, enabled)? {
        level_dat::write_at(&world_dir, &root, framing).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datapacks::level_dat;
    use crate::datapacks::world_link::test_util::*;
    use crate::mods::store::Placement;
    use fastnbt::Value;
    use std::collections::HashMap;

    #[tokio::test]
    async fn add_to_world_refuses_a_different_same_named_file() {
        let _lock = hardlink_lock();
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "vm.zip", 48).await;
        let saves = td.path().join(".minecraft").join("saves");
        let dp = saves.join("Alpha").join("datapacks");
        std::fs::create_dir_all(&dp).unwrap();
        let theirs = datapack_zip(57);
        std::fs::write(dp.join("vm.zip"), &theirs).unwrap();

        let err = add_to_world_at(td.path(), "Alpha", "vm.zip")
            .await
            .unwrap_err();

        assert!(
            matches!(err, Error::ModsFilenameConflict { .. }),
            "got {err:?}"
        );
        assert_eq!(
            std::fs::read(dp.join("vm.zip")).unwrap(),
            theirs,
            "the user's own pack must survive"
        );
    }

    #[tokio::test]
    async fn re_adding_a_disabled_pack_re_enables_it() {
        // NOT a no-op. `add_to_world_at` ends with `set_enabled(.., true)`, and
        // the library screen's world picker is exactly where a user ticks a
        // world in which the pack is currently off. A literal
        // file-and-level.dat no-op would silently delete that behaviour.
        let _lock = hardlink_lock();
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "vm.zip", 48).await;
        let saves = td.path().join(".minecraft").join("saves");
        std::fs::create_dir_all(saves.join("Alpha")).unwrap();
        add_to_world_at(td.path(), "Alpha", "vm.zip").await.unwrap();
        set_enabled_in_world_at(td.path(), "Alpha", "vm.zip", false)
            .await
            .unwrap();

        add_to_world_at(td.path(), "Alpha", "vm.zip").await.unwrap();

        let (root, _) = level_dat::read_at(&world_dir(td.path(), "Alpha")).unwrap();
        let (enabled, disabled) = level_dat::lists(&root);
        assert!(
            enabled.iter().any(|s| s == "file/vm.zip"),
            "re-adding must re-enable: {enabled:?}"
        );
        assert!(!disabled.iter().any(|s| s == "file/vm.zip"));
    }

    #[tokio::test]
    async fn a_colliding_directory_is_a_conflict() {
        // Minecraft loads folder datapacks, and this module already models them
        // elsewhere. A folder has no file sha1, so it can never be proven ours
        // — letting it through would rename a zip over a whole pack folder.
        let _lock = hardlink_lock();
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "vm.zip", 48).await;
        let saves = td.path().join(".minecraft").join("saves");
        let dp = saves.join("Alpha").join("datapacks");
        std::fs::create_dir_all(dp.join("vm.zip").join("data")).unwrap();

        let err = add_to_world_at(td.path(), "Alpha", "vm.zip")
            .await
            .unwrap_err();

        assert!(
            matches!(err, Error::ModsFilenameConflict { .. }),
            "got {err:?}"
        );
        assert!(
            dp.join("vm.zip").join("data").is_dir(),
            "the folder pack must survive"
        );
    }

    #[tokio::test]
    async fn add_places_the_file_and_enables_it_in_level_dat() {
        let _lock = hardlink_lock();
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "vm.zip", 48).await;
        std::fs::create_dir_all(world_dir(td.path(), "Survival")).unwrap();

        let placement = add_to_world_at(td.path(), "Survival", "vm.zip")
            .await
            .unwrap();

        assert_eq!(placement, Placement::Linked);
        let wd = world_dir(td.path(), "Survival");
        assert!(wd.join("datapacks/vm.zip").exists());
        let (root, _framing) = level_dat::read_at(&wd).unwrap();
        let (enabled, disabled) = level_dat::lists(&root);
        assert_eq!(enabled, vec!["file/vm.zip".to_string()]);
        assert!(disabled.is_empty());
    }

    #[tokio::test]
    async fn remove_clears_the_file_and_both_level_dat_lists() {
        let _lock = hardlink_lock();
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "vm.zip", 48).await;
        std::fs::create_dir_all(world_dir(td.path(), "Survival")).unwrap();
        add_to_world_at(td.path(), "Survival", "vm.zip")
            .await
            .unwrap();

        remove_from_world_at(td.path(), "Survival", "vm.zip")
            .await
            .unwrap();

        let wd = world_dir(td.path(), "Survival");
        assert!(!wd.join("datapacks/vm.zip").exists());
        let (root, _framing) = level_dat::read_at(&wd).unwrap();
        let (enabled, disabled) = level_dat::lists(&root);
        assert!(enabled.is_empty());
        assert!(disabled.is_empty());
    }

    #[tokio::test]
    async fn remove_clears_a_case_drifted_level_dat_entry() {
        // NTFS is case-insensitive: a level.dat entry spelled
        // `file/VeinMiner.zip` and a removal request for `veinminer.zip` name
        // the SAME file — the drift `contains_ci` exists for. An exact
        // `forget` would delete the file but keep the name: a permanent
        // Orphaned row and Minecraft's "data packs are no longer present"
        // screen. The cascade removal path hands this function the LIBRARY's
        // spelling, so the mismatch is reachable, not hypothetical.
        let _lock = hardlink_lock();
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "VeinMiner.zip", 48).await;
        std::fs::create_dir_all(world_dir(td.path(), "Survival")).unwrap();
        add_to_world_at(td.path(), "Survival", "VeinMiner.zip")
            .await
            .unwrap();

        remove_from_world_at(td.path(), "Survival", "veinminer.zip")
            .await
            .unwrap();

        // Only level.dat is asserted: on a case-sensitive filesystem the
        // lowercase path legitimately misses the file (idempotent Ok), but the
        // name must be gone from the lists on every platform.
        let (root, _framing) = level_dat::read_at(&world_dir(td.path(), "Survival")).unwrap();
        let (enabled, disabled) = level_dat::lists(&root);
        assert!(
            enabled.is_empty(),
            "level.dat still names the pack: {enabled:?}"
        );
        assert!(disabled.is_empty());
    }

    #[tokio::test]
    async fn remove_clears_an_orphan_with_no_file() {
        let td = tempfile::tempdir().unwrap();
        let wd = world_dir(td.path(), "Survival");
        std::fs::create_dir_all(&wd).unwrap();
        let mut root = Value::Compound(HashMap::new());
        level_dat::set_enabled(&mut root, "file/ghost.zip", true).unwrap();
        level_dat::write_at(&wd, &root, level_dat::Framing::Gzip)
            .await
            .unwrap();
        assert!(!wd.join("datapacks/ghost.zip").exists());

        remove_from_world_at(td.path(), "Survival", "ghost.zip")
            .await
            .unwrap();

        let (after, _framing) = level_dat::read_at(&wd).unwrap();
        let (enabled, disabled) = level_dat::lists(&after);
        assert!(enabled.is_empty());
        assert!(disabled.is_empty());
    }

    #[tokio::test]
    async fn toggling_disabled_leaves_the_file_in_place() {
        let _lock = hardlink_lock();
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "vm.zip", 48).await;
        std::fs::create_dir_all(world_dir(td.path(), "Survival")).unwrap();
        add_to_world_at(td.path(), "Survival", "vm.zip")
            .await
            .unwrap();

        set_enabled_in_world_at(td.path(), "Survival", "vm.zip", false)
            .await
            .unwrap();

        let wd = world_dir(td.path(), "Survival");
        assert!(
            wd.join("datapacks/vm.zip").exists(),
            "disabling must not touch the file"
        );
        let (root, _framing) = level_dat::read_at(&wd).unwrap();
        let (enabled, disabled) = level_dat::lists(&root);
        assert!(enabled.is_empty());
        assert_eq!(disabled, vec!["file/vm.zip".to_string()]);
    }

    #[tokio::test]
    async fn a_world_segment_with_a_path_separator_is_rejected() {
        let td = tempfile::tempdir().unwrap();
        let err = add_to_world_at(td.path(), "../evil", "vm.zip")
            .await
            .unwrap_err();
        assert!(matches!(err, Error::WorldPathInvalid { .. }));
    }

    #[tokio::test]
    async fn removing_a_folder_datapack_succeeds_and_clears_both_lists() {
        let td = tempfile::tempdir().unwrap();
        let wd = world_dir(td.path(), "Survival");
        std::fs::create_dir_all(wd.join("datapacks/MyFolderPack/data")).unwrap();
        let mut root = Value::Compound(HashMap::new());
        level_dat::set_enabled(&mut root, "file/MyFolderPack", true).unwrap();
        level_dat::write_at(&wd, &root, level_dat::Framing::Gzip)
            .await
            .unwrap();

        // Before the fix this called `remove_file` on a directory, which
        // fails with OS error 5 on Windows — mapped to a false `WorldInUse`
        // ("quit Minecraft and try again") even with Minecraft closed.
        remove_from_world_at(td.path(), "Survival", "MyFolderPack")
            .await
            .unwrap();

        assert!(
            !wd.join("datapacks/MyFolderPack").exists(),
            "the folder itself must be gone, not just its level.dat entry"
        );
        let (after, _framing) = level_dat::read_at(&wd).unwrap();
        let (enabled, disabled) = level_dat::lists(&after);
        assert!(enabled.is_empty());
        assert!(disabled.is_empty());
    }

    #[tokio::test]
    async fn add_to_world_at_rejects_a_nonexistent_world_and_creates_nothing() {
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "vm.zip", 48).await;

        let err = add_to_world_at(td.path(), "GhostWorld", "vm.zip")
            .await
            .unwrap_err();

        assert!(matches!(err, Error::WorldNotFound { .. }));
        assert!(
            !td.path().join(".minecraft/saves/GhostWorld").exists(),
            "a rejected write must not create the phantom world directory"
        );
    }

    #[tokio::test]
    async fn add_to_world_at_names_the_missing_library_file() {
        let td = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(world_dir(td.path(), "Survival")).unwrap();
        // No `seed_library` call: "vm.zip" was never installed into the
        // library, so `materialize` would otherwise fail against the
        // DESTINATION path with a misleading message.

        let err = add_to_world_at(td.path(), "Survival", "vm.zip")
            .await
            .unwrap_err();

        let Error::ModsInstancePath { path, details } = err else {
            panic!("expected Error::ModsInstancePath, got {err:?}");
        };
        let expected_src = library_dir_at(td.path()).join("vm.zip");
        assert_eq!(
            path,
            expected_src.display().to_string(),
            "must name the LIBRARY source path, not the world destination"
        );
        assert!(details.contains("vm.zip"), "details was: {details}");
    }

    /// Regression for the level.dat lost-update window described in the
    /// datapacks batch-2 review: two concurrent mutations on the SAME world
    /// used to interleave their read → mutate → write, letting the later
    /// write silently discard the earlier edit. With `level_dat_lock`
    /// serializing every call, both edits survive deterministically,
    /// regardless of scheduling.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_disable_and_add_do_not_lose_either_update() {
        let _lock = hardlink_lock();
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "a.zip", 48).await;
        seed_library(td.path(), "b.zip", 48).await;
        std::fs::create_dir_all(world_dir(td.path(), "Survival")).unwrap();
        add_to_world_at(td.path(), "Survival", "a.zip")
            .await
            .unwrap();

        let root1 = td.path().to_path_buf();
        let t1 = tokio::spawn(async move {
            set_enabled_in_world_at(&root1, "Survival", "a.zip", false).await
        });
        let root2 = td.path().to_path_buf();
        let t2 = tokio::spawn(async move { add_to_world_at(&root2, "Survival", "b.zip").await });

        t1.await.unwrap().unwrap();
        t2.await.unwrap().unwrap();

        let wd = world_dir(td.path(), "Survival");
        let (root, _framing) = level_dat::read_at(&wd).unwrap();
        let (enabled, disabled) = level_dat::lists(&root);
        assert!(
            disabled.contains(&"file/a.zip".to_string()),
            "a.zip's disable must survive a concurrent add of b.zip"
        );
        assert!(
            enabled.contains(&"file/b.zip".to_string()),
            "b.zip's enable must survive a concurrent disable of a.zip"
        );
    }

    /// Windows: '<' cannot appear in a filename, so the opening stat fails
    /// with ERROR_INVALID_NAME — a non-NotFound failure, exactly the class
    /// the old `let Ok(..) else {{ return Ok(None) }}` collapsed into "free
    /// to place". Absent stays a fact (NotFound → free); ignorance must not.
    #[cfg(windows)]
    #[tokio::test]
    async fn an_unstatable_dest_is_an_error_not_a_free_slot() {
        let td = tempfile::tempdir().unwrap();
        let src = td.path().join("lib-vm.zip");
        std::fs::write(&src, b"library bytes").unwrap();
        let dest = td.path().join("vm<invalid>.zip");

        let verdict = conflicting_world_entry(&src, &dest).await;

        assert!(
            verdict.is_err(),
            "an unstatable dest must be an error, not a free slot: {verdict:?}"
        );
    }

    /// Windows: a handle held with no sharing makes a later open-for-read
    /// fail with a sharing violation while `metadata` (attribute-only access)
    /// still succeeds. That is what a running game holding the pack open
    /// looks like. Equal sizes steer the gate into its hash-compare branch,
    /// whose read failure used to collapse to "free to place".
    #[cfg(windows)]
    #[tokio::test]
    async fn a_share_locked_equal_size_dest_is_an_error_not_a_free_slot() {
        use std::os::windows::fs::OpenOptionsExt;

        let td = tempfile::tempdir().unwrap();
        let src = td.path().join("lib-vm.zip");
        let dest = td.path().join("world-vm.zip");
        std::fs::write(&src, b"library-bytes").unwrap();
        // Same length as the library bytes, different content.
        std::fs::write(&dest, b"foreign-bytes").unwrap();
        let _held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0) // no sharing: any later open-for-read fails
            .open(&dest)
            .unwrap();

        let verdict = conflicting_world_entry(&src, &dest).await;

        assert!(
            verdict.is_err(),
            "an unreadable equal-size dest must be an error, not a free slot: {verdict:?}"
        );
    }

    /// Windows twin for the DIFFERENT-size branch. Sizes differing proves the
    /// contents differ, but the old code answered "conflict" carrying a
    /// fabricated blank `existing_sha` when the dest could not be read. Both
    /// branches must agree on what an unreadable entry means: propagate.
    #[cfg(windows)]
    #[tokio::test]
    async fn a_share_locked_different_size_dest_propagates_instead_of_a_blank_sha() {
        use std::os::windows::fs::OpenOptionsExt;

        let td = tempfile::tempdir().unwrap();
        let src = td.path().join("lib-vm.zip");
        let dest = td.path().join("world-vm.zip");
        std::fs::write(&src, b"library bytes").unwrap();
        std::fs::write(&dest, b"a much longer foreign payload").unwrap();
        let _held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&dest)
            .unwrap();

        let verdict = conflicting_world_entry(&src, &dest).await;

        assert!(
            matches!(verdict, Err(Error::ModsInstancePath { .. })),
            "an unreadable different-size dest must propagate, not report a conflict \
             with a fabricated blank sha: {verdict:?}"
        );
    }
}
