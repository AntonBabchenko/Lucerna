//! The locked single-world entry points (add / remove / toggle, plus the
//! cascade's removal) and their conflict gate. Each takes `level_dat_lock`
//! itself — see the parent module doc for why they must never be composed
//! under it.

use std::path::Path;

use crate::datapacks::detect;
use crate::datapacks::presence::{self, LevelDatPresence};
use crate::datapacks::{level_dat, level_dat_entry, library_dir_at};
use crate::error::{DatapackRejection, Error, Result};
use crate::mods::store::{materialize, LinkPolicy, Placement};

use super::{
    level_dat_lock, level_dat_missing, map_removal_err, only_old, require_level_dat,
    world_dirs_checked,
};

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
///
/// Refuses before any write (D2): a world with only `level.dat_old`
/// ([`Error::WorldLevelDatOnlyOld`]), a folder with neither level file
/// ([`Error::WorldLevelDatMissing`]), and a `level.dat` that cannot be read
/// or edited. A world with no `DataPacks` compound keeps its `level.dat`
/// unchanged: the game adds the pack itself, after `vanilla` and the mod
/// packs (spec §0.5 A15).
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

    // A legacy `X.ZIP` library row: the game only loads `*.zip` in lower
    // case (N.4). Input validation, so it comes first (§0.5 A7).
    if !detect::has_zip_suffix(filename) {
        return Err(Error::DatapackInvalid {
            filename: filename.to_string(),
            reason: DatapackRejection::NotAZip,
        });
    }

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

    // D1 (N.4): a library file with no root `pack.mcmeta` is refused. This
    // closes the vouch rule's only hole (`registry::reconcile` never
    // classifies). The zip is read here, before the lock, so no other world's
    // change waits on it; the verdict is reported after the level.dat presence
    // check below (§0.5 A7 step 5).
    let src_check = src.clone();
    let has_root: Result<bool> =
        tokio::task::spawn_blocking(move || detect::zip_has_root_pack_mcmeta(&src_check))
            .await
            .map_err(|e| Error::io(src.display().to_string(), format!("join: {e}")))
            .and_then(|r| r.map_err(|e| Error::io(src.display().to_string(), e)));

    let _guard = level_dat_lock().lock().await;

    // D2 (spec §3 L.4): only a world whose level.dat is a regular file. This
    // comes after the input checks above (§0.5 A7) and before this call's
    // first write.
    require_level_dat(&world_dir, world)?;
    if !has_root? {
        return Err(Error::DatapackInvalid {
            filename: filename.to_string(),
            reason: DatapackRejection::NotAPack,
        });
    }
    // Read level.dat and check its DataPacks shape BEFORE touching the folder,
    // so an unreadable or malformed level.dat refuses here instead of after
    // the pack is linked. The id to write is only known after `materialize`
    // (N.4, below); `set_enabled`'s refusals depend on the compound's shape,
    // never on the id, so a trial edit on a copy proves the real one succeeds.
    let (mut root, framing) = level_dat::read_at(&world_dir)?;
    level_dat::set_enabled(&mut root.clone(), &level_dat_entry(filename), true)?;

    // N.4: the entry `filename` denotes in this world now. Where the file
    // system folds case, linking over an entry spelled `X.ZIP` may keep that
    // spelling, an id the game ignores: refuse before anything is linked.
    // (`linked_entry_name` below checks the result again.)
    let (_, existing) = super::resolve_on_disk(&dp_dir, filename)
        .await
        .map_err(|e| Error::ModsInstancePath {
            path: dp_dir.display().to_string(),
            details: e.to_string(),
        })?;
    match existing {
        detect::Resolved::Exact(n) | detect::Resolved::Folded(n) if !detect::has_zip_suffix(&n) => {
            return Err(Error::DatapackInvalid {
                filename: n,
                reason: DatapackRejection::NotAZip,
            });
        }
        detect::Resolved::Unknown(e) => {
            return Err(Error::ModsInstancePath {
                path: dp_dir.join(filename).display().to_string(),
                details: e.to_string(),
            });
        }
        detect::Resolved::Exact(_) | detect::Resolved::Folded(_) | detect::Resolved::Absent => {}
    }

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

    // N.4: write the engine's id, `file/` + the entry's own spelling. Write
    // only when the toggle actually changed something: `write_at` rolls the
    // pre-edit backup forward on every call. A world with no DataPacks
    // compound reports no change; the game adds the pack itself.
    let on_disk = linked_entry_name(&dp_dir, filename).await?;
    if level_dat::set_enabled(&mut root, &level_dat_entry(&on_disk), true)? {
        level_dat::write_at(&world_dir, &root, framing).await?;
    }
    Ok(placement)
}

/// The on-disk spelling of the entry `materialize` just placed as
/// `filename` (R2). On NTFS/APFS, a link over an entry differing only in case
/// leaves an unspecified spelling, so it is read back. A spelling the game
/// does not load (an existing `X.ZIP` kept by the file system) is refused
/// with `DatapackInvalid { NotAZip }` rather than recorded as its id. Not
/// seeing the entry just placed is "could not tell".
async fn linked_entry_name(dp_dir: &Path, filename: &str) -> Result<String> {
    let dest = dp_dir.join(filename);
    let (_, resolved) = super::resolve_on_disk(dp_dir, filename)
        .await
        .map_err(|e| Error::ModsInstancePath {
            path: dp_dir.display().to_string(),
            details: e.to_string(),
        })?;
    let on_disk = match resolved {
        detect::Resolved::Exact(n) | detect::Resolved::Folded(n) => n,
        detect::Resolved::Absent => {
            return Err(Error::ModsInstancePath {
                path: dest.display().to_string(),
                details: "the linked pack is not listed in the world's datapacks folder".into(),
            })
        }
        detect::Resolved::Unknown(e) => {
            return Err(Error::ModsInstancePath {
                path: dest.display().to_string(),
                details: e.to_string(),
            })
        }
    };
    if !detect::has_zip_suffix(&on_disk) {
        return Err(Error::DatapackInvalid {
            filename: on_disk,
            reason: DatapackRejection::NotAZip,
        });
    }
    Ok(on_disk)
}

/// Unlink a datapack from a world and drop its level.dat entry from both
/// lists. Idempotent: a missing file is `Ok`. This doubles as the repair path
/// for an `Orphaned` row (a level.dat name with no file), because it still
/// clears the name when there is nothing to unlink.
///
/// Refuses, before touching anything, a world with only `level.dat_old` and
/// a folder with neither file (D2). The library cascade uses
/// [`remove_for_cascade_at`] instead, which unlinks the file in the latter.
pub async fn remove_from_world_at(instance_root: &Path, world: &str, filename: &str) -> Result<()> {
    remove_in_world(instance_root, world, filename, OnAbsent::Refuse).await
}

/// [`remove_from_world_at`] for the library's cascade removal (spec §0.5
/// A3). In a folder with neither `level.dat` nor `level.dat_old`, it unlinks
/// the file and neither reads nor writes a level.dat: the game loads nothing
/// from such a folder, and refusing there would leave the library row
/// impossible to remove. A world with only `level.dat_old` is still refused.
pub(crate) async fn remove_for_cascade_at(
    instance_root: &Path,
    world: &str,
    filename: &str,
) -> Result<()> {
    remove_in_world(instance_root, world, filename, OnAbsent::UnlinkOnly).await
}

/// The cascade's orphan sweep: forget `filename`'s ids in a world whose
/// level.dat names the pack but where no file of ours was verified. It never
/// deletes a file. An entry the name resolves to now was never checked as the
/// library's copy, so the world is refused instead (it reports `Failed`, and
/// the library copy stays for a retry); so is one whose `datapacks/` cannot be
/// listed. The ids go with R3 (spec §2 N.3). A world with only
/// `level.dat_old` is refused (D2); a folder with neither file has no list.
pub(crate) async fn forget_for_cascade_at(
    instance_root: &Path,
    world: &str,
    filename: &str,
) -> Result<()> {
    if !crate::pathsafe::is_safe_filename(filename) {
        return Err(Error::ModsUnsafeFilename {
            filename: filename.to_string(),
        });
    }
    let (world_dir, dp_dir) = world_dirs_checked(instance_root, world)?;

    let _guard = level_dat_lock().lock().await;

    match presence::of(&world_dir)? {
        LevelDatPresence::Present => {}
        LevelDatPresence::OnlyOld => return Err(only_old(world)),
        // No level file: no list to name the pack.
        LevelDatPresence::Absent => return Ok(()),
    }
    let (names, resolved) = super::resolve_on_disk(&dp_dir, filename)
        .await
        .map_err(|e| Error::ModsInstancePath {
            path: dp_dir.display().to_string(),
            details: e.to_string(),
        })?;
    match resolved {
        detect::Resolved::Absent => {}
        detect::Resolved::Exact(n) | detect::Resolved::Folded(n) => {
            return Err(Error::ModsInstancePath {
                path: dp_dir.join(n).display().to_string(),
                details: "an entry by this name was not checked as the library's copy, so \
                          nothing was removed"
                    .into(),
            })
        }
        detect::Resolved::Unknown(e) => {
            return Err(Error::ModsInstancePath {
                path: dp_dir.join(filename).display().to_string(),
                details: e.to_string(),
            })
        }
    }
    let (mut root, framing) = level_dat::read_at(&world_dir)?;
    if level_dat::forget_with_case_ghosts(&mut root, &level_dat_entry(filename), Some(&names))? {
        level_dat::write_at(&world_dir, &root, framing).await?;
    }
    Ok(())
}

/// What a removal does in a folder with neither level file.
#[derive(Clone, Copy)]
enum OnAbsent {
    /// The world tab. D2 makes no change at all.
    Refuse,
    /// The library cascade: the file only (§0.5 A3).
    UnlinkOnly,
}

async fn remove_in_world(
    instance_root: &Path,
    world: &str,
    filename: &str,
    on_absent: OnAbsent,
) -> Result<()> {
    if !crate::pathsafe::is_safe_filename(filename) {
        return Err(Error::ModsUnsafeFilename {
            filename: filename.to_string(),
        });
    }
    let (world_dir, dp_dir) = world_dirs_checked(instance_root, world)?;

    let _guard = level_dat_lock().lock().await;

    // D2 first (§0.5 A7): an only-old world and a refused folder leave the
    // file where it is.
    let level_dat_present = match presence::of(&world_dir)? {
        LevelDatPresence::Present => true,
        LevelDatPresence::OnlyOld => return Err(only_old(world)),
        LevelDatPresence::Absent => match on_absent {
            OnAbsent::Refuse => return Err(level_dat_missing(world)),
            OnAbsent::UnlinkOnly => false,
        },
    };

    // R2 (N.4): delete only the entry `filename` actually denotes. On a
    // case-sensitive file system a case variant is a different pack.
    let (names, resolved) = super::resolve_on_disk(&dp_dir, filename)
        .await
        .map_err(|e| Error::ModsInstancePath {
            path: dp_dir.display().to_string(),
            details: e.to_string(),
        })?;
    let on_disk = match resolved {
        detect::Resolved::Exact(n) | detect::Resolved::Folded(n) => Some(n),
        detect::Resolved::Absent => None,
        detect::Resolved::Unknown(e) => {
            return Err(Error::ModsInstancePath {
                path: dp_dir.join(filename).display().to_string(),
                details: e.to_string(),
            })
        }
    };

    // The level.dat edit, before the removal, so an unreadable or malformed
    // level.dat leaves the file where it is. R3's `present` is what is left
    // after the removal: the one `read_dir` above, minus the entry removed.
    let edit = if level_dat_present {
        let (mut root, framing) = level_dat::read_at(&world_dir)?;
        let present: Vec<String> = names
            .iter()
            .filter(|n| Some(*n) != on_disk.as_ref())
            .cloned()
            .collect();
        let entry = level_dat_entry(on_disk.as_deref().unwrap_or(filename));
        let changed = level_dat::forget_with_case_ghosts(&mut root, &entry, Some(&present))?;
        changed.then_some((root, framing))
    } else {
        None
    };

    // `name` is either `filename`, already validated above by
    // `is_safe_filename` (exactly one `Normal` path component — no separator,
    // no `..`, no absolute prefix), or a name `read_dir` returned for
    // `dp_dir` itself, which is one component by construction. `dp_dir` only
    // ever resolves under a validated world segment (`world_dirs_checked`). So
    // `path` can never point above `<world>/datapacks/`, which is what makes
    // the unconditional `remove_dir_all` below safe to call.
    if let Some(name) = &on_disk {
        remove_entry(&dp_dir.join(name), world).await?;
    }

    if let Some((root, framing)) = edit {
        level_dat::write_at(&world_dir, &root, framing).await?;
    }
    Ok(())
}

/// Remove one world entry by its real type. Idempotent: an entry that is gone
/// by now is `Ok`.
async fn remove_entry(path: &Path, world: &str) -> Result<()> {
    match tokio::fs::metadata(path).await {
        Ok(meta) => {
            // Minecraft loads DIRECTORIES from `datapacks/` too, not just
            // `.zip` files, and records them in level.dat the same way a zip
            // is recorded. Picking the removal call by the entry's real type
            // is what the old code got wrong: `remove_file` on a directory
            // fails with OS error 5 on Windows, which the mapping below used
            // to turn into a false "quit Minecraft and try again" even with
            // Minecraft closed.
            let removal = if meta.is_dir() {
                tokio::fs::remove_dir_all(path).await
            } else {
                tokio::fs::remove_file(path).await
            };
            if let Err(e) = removal {
                return Err(map_removal_err(path, e, world));
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
    Ok(())
}

/// Toggle a datapack's enabled/disabled state for one world. level.dat only —
/// the file itself is never touched.
///
/// Refuses (D2) a world with only `level.dat_old`
/// ([`Error::WorldLevelDatOnlyOld`]) and a folder with neither level file
/// ([`Error::WorldLevelDatMissing`]). In a world with no `DataPacks`
/// compound, enabling leaves `level.dat` unchanged (the game enables a
/// present, unlisted pack itself), and disabling seeds the engine default
/// with this entry switched off (spec §0.5 A15).
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
    let (world_dir, dp_dir) = world_dirs_checked(instance_root, world)?;

    let _guard = level_dat_lock().lock().await;

    require_level_dat(&world_dir, world)?;
    // §0.5 A7 step 5: R2 + D1, after the level.dat presence check.
    let on_disk = super::pack_name_for_write(&dp_dir, filename).await?;
    let (mut root, framing) = level_dat::read_at(&world_dir)?;
    let entry = level_dat_entry(on_disk.as_deref().unwrap_or(filename));
    if level_dat::set_enabled(&mut root, &entry, enabled)? {
        level_dat::write_at(&world_dir, &root, framing).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datapacks::detect::test_support::{fs_folds_case, pack_zip, zip_of};
    use crate::datapacks::level_dat;
    use crate::datapacks::level_dat::test_support::seed;
    use crate::datapacks::world_link::test_util::*;
    use crate::datapacks::WorldPackState;
    use crate::error::DatapackRejection;
    use crate::mods::store::Placement;

    #[tokio::test]
    async fn add_to_world_refuses_a_different_same_named_file() {
        let _lock = hardlink_lock();
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "vm.zip", 48).await;
        let saves = td.path().join(".minecraft").join("saves");
        let dp = saves.join("Alpha").join("datapacks");
        game_world(td.path(), "Alpha");
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
        game_world(td.path(), "Alpha");
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
        game_world(td.path(), "Alpha");
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
        game_world(td.path(), "Survival");

        let placement = add_to_world_at(td.path(), "Survival", "vm.zip")
            .await
            .unwrap();

        assert_eq!(placement, Placement::Linked);
        let wd = world_dir(td.path(), "Survival");
        assert!(wd.join("datapacks/vm.zip").exists());
        let (root, _framing) = level_dat::read_at(&wd).unwrap();
        let (enabled, disabled) = level_dat::lists(&root);
        assert_eq!(
            enabled,
            vec!["vanilla".to_string(), "file/vm.zip".to_string()]
        );
        assert!(disabled.is_empty());
    }

    #[tokio::test]
    async fn remove_clears_the_file_and_both_level_dat_lists() {
        let _lock = hardlink_lock();
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "vm.zip", 48).await;
        game_world(td.path(), "Survival");
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
        assert_eq!(enabled, vec!["vanilla".to_string()]);
        assert!(disabled.is_empty());
    }

    /// R2/R3 (N.3). On NTFS/APFS `veinminer.zip` and `VeinMiner.zip` are one
    /// entry: the file goes, and so does every spelling of its id. On a
    /// case-sensitive file system they are two packs: the lower-case request
    /// names nothing on disk, so the file AND its id must stay.
    #[tokio::test]
    async fn remove_clears_a_case_drifted_level_dat_entry() {
        let _lock = hardlink_lock();
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "VeinMiner.zip", 48).await;
        let wd = game_world(td.path(), "Survival");
        add_to_world_at(td.path(), "Survival", "VeinMiner.zip")
            .await
            .unwrap();
        remove_from_world_at(td.path(), "Survival", "veinminer.zip")
            .await
            .unwrap();
        let (root, _) = level_dat::read_at(&wd).unwrap();
        let (enabled, _) = level_dat::lists(&root);
        let file_ids: Vec<&String> = enabled.iter().filter(|s| s.starts_with("file/")).collect();
        if fs_folds_case(td.path()) {
            assert!(!wd.join("datapacks/VeinMiner.zip").exists());
            assert!(file_ids.is_empty(), "{enabled:?}");
        } else {
            assert!(
                wd.join("datapacks/VeinMiner.zip").exists(),
                "a case variant is a different pack"
            );
            assert_eq!(file_ids, vec!["file/VeinMiner.zip"]);
        }
    }

    #[tokio::test]
    async fn remove_clears_an_orphan_with_no_file() {
        let td = tempfile::tempdir().unwrap();
        let wd = world_dir(td.path(), "Survival");
        level_dat::test_support::seed(&wd, &["file/ghost.zip"], &[]);
        assert!(!wd.join("datapacks/ghost.zip").exists());

        remove_from_world_at(td.path(), "Survival", "ghost.zip")
            .await
            .unwrap();

        let (after, _framing) = level_dat::read_at(&wd).unwrap();
        let (enabled, disabled) = level_dat::lists(&after);
        assert_eq!(enabled, vec!["vanilla".to_string()]);
        assert!(disabled.is_empty());
    }

    #[tokio::test]
    async fn toggling_disabled_leaves_the_file_in_place() {
        let _lock = hardlink_lock();
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "vm.zip", 48).await;
        game_world(td.path(), "Survival");
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
        assert_eq!(enabled, vec!["vanilla".to_string()]);
        assert_eq!(disabled, vec!["file/vm.zip".to_string()]);
    }

    #[tokio::test]
    async fn add_to_a_world_without_a_datapacks_compound_creates_no_compound() {
        // §0.5 A15. An old world moved to a newer Minecraft has no DataPacks
        // compound until the game opens it; the game then adds the pack after
        // vanilla and every mod pack itself.
        let _lock = hardlink_lock();
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "vm.zip", 48).await;
        let wd = world_dir(td.path(), "Old");
        level_dat::test_support::seed_root(
            &wd,
            &level_dat::test_support::game_root_without_datapacks(),
        );
        let before = std::fs::read(wd.join("level.dat")).unwrap();

        add_to_world_at(td.path(), "Old", "vm.zip").await.unwrap();

        assert!(wd.join("datapacks/vm.zip").exists());
        assert_eq!(std::fs::read(wd.join("level.dat")).unwrap(), before);
        assert!(
            !wd.join("level.dat_lucerna.bak").exists(),
            "nothing was written, so nothing was backed up"
        );
    }

    #[tokio::test]
    async fn enabling_without_a_datapacks_compound_writes_nothing_and_lists_enabled() {
        let td = tempfile::tempdir().unwrap();
        let wd = world_dir(td.path(), "Old");
        level_dat::test_support::seed_root(
            &wd,
            &level_dat::test_support::game_root_without_datapacks(),
        );
        std::fs::create_dir_all(wd.join("datapacks")).unwrap();
        std::fs::write(wd.join("datapacks/vm.zip"), datapack_zip(48)).unwrap();
        let before = std::fs::read(wd.join("level.dat")).unwrap();

        set_enabled_in_world_at(td.path(), "Old", "vm.zip", true)
            .await
            .unwrap();

        assert_eq!(std::fs::read(wd.join("level.dat")).unwrap(), before);
        let listed = crate::datapacks::world_link::list_for_world_at(td.path(), "Old", None)
            .await
            .unwrap()
            .packs;
        assert_eq!(listed.len(), 1);
        assert_eq!(
            listed[0].state,
            crate::datapacks::WorldPackState::Enabled,
            "present and unlisted: the game loads it"
        );
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
        level_dat::test_support::seed(&wd, &["file/MyFolderPack"], &[]);

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
        assert_eq!(enabled, vec!["vanilla".to_string()]);
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
        game_world(td.path(), "Survival");
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
        game_world(td.path(), "Survival");
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

    #[tokio::test]
    async fn add_refuses_a_world_with_only_level_dat_old_and_writes_nothing() {
        // The game restores level.dat from level.dat_old when it opens this
        // world. A level.dat written first parses cleanly and switches that
        // recovery off (spec §3 L.1).
        let _lock = hardlink_lock();
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "vm.zip", 48).await;
        let wd = world_dir(td.path(), "Restoring");
        level_dat::test_support::seed_old(&wd, &[], &[]);
        let old_before = std::fs::read(wd.join("level.dat_old")).unwrap();

        let err = add_to_world_at(td.path(), "Restoring", "vm.zip")
            .await
            .unwrap_err();

        assert!(
            matches!(
                &err,
                Error::WorldLevelDatOnlyOld { folder_name } if folder_name == "Restoring"
            ),
            "got {err:?}"
        );
        assert!(
            !wd.join("datapacks").exists(),
            "no datapacks/ may be created"
        );
        assert!(
            !wd.join("level.dat").exists(),
            "no level.dat may be created"
        );
        assert_eq!(std::fs::read(wd.join("level.dat_old")).unwrap(), old_before);
    }

    #[tokio::test]
    async fn add_refuses_a_folder_without_level_dat_and_writes_nothing() {
        // Neither file: the game does not list this folder as a world.
        let _lock = hardlink_lock();
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "vm.zip", 48).await;
        let wd = world_dir(td.path(), "NotAWorld");
        std::fs::create_dir_all(&wd).unwrap();

        let err = add_to_world_at(td.path(), "NotAWorld", "vm.zip")
            .await
            .unwrap_err();

        assert!(
            matches!(
                &err,
                Error::WorldLevelDatMissing { folder_name } if folder_name == "NotAWorld"
            ),
            "got {err:?}"
        );
        assert!(!wd.join("datapacks").exists());
        assert!(!wd.join("level.dat").exists());
    }

    #[tokio::test]
    async fn remove_refuses_a_world_with_only_level_dat_old_and_keeps_the_file() {
        let td = tempfile::tempdir().unwrap();
        let wd = world_dir(td.path(), "Restoring");
        level_dat::test_support::seed_old(&wd, &["file/vm.zip"], &[]);
        let old_before = std::fs::read(wd.join("level.dat_old")).unwrap();
        std::fs::create_dir_all(wd.join("datapacks")).unwrap();
        std::fs::write(wd.join("datapacks/vm.zip"), datapack_zip(48)).unwrap();

        let err = remove_from_world_at(td.path(), "Restoring", "vm.zip")
            .await
            .unwrap_err();

        assert!(
            matches!(err, Error::WorldLevelDatOnlyOld { .. }),
            "got {err:?}"
        );
        assert!(
            wd.join("datapacks/vm.zip").exists(),
            "the pack stays until the world is restored"
        );
        assert!(!wd.join("level.dat").exists());
        assert_eq!(
            std::fs::read(wd.join("level.dat_old")).unwrap(),
            old_before,
            "Minecraft's backup must be byte-identical"
        );
    }

    #[tokio::test]
    async fn remove_refuses_a_folder_without_level_dat_and_keeps_the_file() {
        // The world tab's removal. The library cascade uses
        // `remove_for_cascade_at`, which unlinks here (§0.5 A3).
        let td = tempfile::tempdir().unwrap();
        let wd = world_dir(td.path(), "NotAWorld");
        std::fs::create_dir_all(wd.join("datapacks")).unwrap();
        std::fs::write(wd.join("datapacks/vm.zip"), datapack_zip(48)).unwrap();

        let err = remove_from_world_at(td.path(), "NotAWorld", "vm.zip")
            .await
            .unwrap_err();

        assert!(
            matches!(err, Error::WorldLevelDatMissing { .. }),
            "got {err:?}"
        );
        assert!(wd.join("datapacks/vm.zip").exists());
        assert!(!wd.join("level.dat").exists());
    }

    #[tokio::test]
    async fn toggle_refuses_a_world_with_only_level_dat_old() {
        let td = tempfile::tempdir().unwrap();
        let wd = world_dir(td.path(), "Restoring");
        level_dat::test_support::seed_old(&wd, &[], &[]);

        let err = set_enabled_in_world_at(td.path(), "Restoring", "vm.zip", false)
            .await
            .unwrap_err();

        assert!(
            matches!(err, Error::WorldLevelDatOnlyOld { .. }),
            "got {err:?}"
        );
        assert!(
            !wd.join("level.dat").exists(),
            "a stub would switch off the game's restore"
        );
    }

    #[tokio::test]
    async fn toggle_refuses_a_folder_without_level_dat() {
        let td = tempfile::tempdir().unwrap();
        let wd = world_dir(td.path(), "NotAWorld");
        std::fs::create_dir_all(&wd).unwrap();

        let err = set_enabled_in_world_at(td.path(), "NotAWorld", "vm.zip", false)
            .await
            .unwrap_err();

        assert!(
            matches!(err, Error::WorldLevelDatMissing { .. }),
            "got {err:?}"
        );
        assert!(!wd.join("level.dat").exists());
    }

    #[tokio::test]
    async fn add_with_an_unreadable_level_dat_links_nothing() {
        let _lock = hardlink_lock();
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "vm.zip", 48).await;
        let wd = world_dir(td.path(), "Survival");
        std::fs::create_dir_all(&wd).unwrap();
        std::fs::write(wd.join("level.dat"), b"not nbt at all").unwrap();

        let err = add_to_world_at(td.path(), "Survival", "vm.zip")
            .await
            .unwrap_err();

        assert!(matches!(err, Error::LevelDatParse { .. }), "got {err:?}");
        assert!(
            !wd.join("datapacks/vm.zip").exists(),
            "no pack may be linked when level.dat cannot follow"
        );
        assert!(
            !wd.join("datapacks").exists(),
            "level.dat is read before datapacks/ is created"
        );
    }

    #[tokio::test]
    async fn remove_with_an_unreadable_level_dat_keeps_the_file() {
        let td = tempfile::tempdir().unwrap();
        let wd = world_dir(td.path(), "Survival");
        std::fs::create_dir_all(wd.join("datapacks")).unwrap();
        std::fs::write(wd.join("datapacks/vm.zip"), datapack_zip(48)).unwrap();
        std::fs::write(wd.join("level.dat"), b"not nbt at all").unwrap();

        let err = remove_from_world_at(td.path(), "Survival", "vm.zip")
            .await
            .unwrap_err();

        assert!(matches!(err, Error::LevelDatParse { .. }), "got {err:?}");
        assert!(wd.join("datapacks/vm.zip").exists());
    }

    #[tokio::test]
    async fn a_toggle_on_a_world_whose_level_dat_is_a_directory_is_not_world_in_use() {
        // `Path::exists` called this "present", the read failed with Windows
        // error 5, and `map_read_err` told the user to quit Minecraft while
        // Minecraft was closed. RED on Windows only: on POSIX, reading a
        // directory was already a plain I/O error.
        let td = tempfile::tempdir().unwrap();
        let wd = world_dir(td.path(), "Odd");
        std::fs::create_dir_all(wd.join("level.dat")).unwrap();

        let err = set_enabled_in_world_at(td.path(), "Odd", "vm.zip", false)
            .await
            .unwrap_err();

        assert!(matches!(err, Error::Io { .. }), "got {err:?}");
    }

    #[tokio::test]
    async fn add_names_a_missing_library_file_before_the_level_dat_state() {
        // §0.5 A7: input validation, library membership included, comes before
        // the level.dat presence check.
        let td = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(world_dir(td.path(), "NotAWorld")).unwrap();

        let err = add_to_world_at(td.path(), "NotAWorld", "vm.zip")
            .await
            .unwrap_err();

        assert!(matches!(err, Error::ModsInstancePath { .. }), "got {err:?}");
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

    #[tokio::test]
    async fn the_toggle_refuses_an_entry_the_game_ignores() {
        let td = tempfile::tempdir().unwrap();
        let wd = game_world(td.path(), "Survival");
        std::fs::create_dir_all(wd.join("datapacks/Loose/data")).unwrap();
        let before = std::fs::read(wd.join("level.dat")).unwrap();
        let err = set_enabled_in_world_at(td.path(), "Survival", "Loose", false)
            .await
            .unwrap_err();
        assert!(
            matches!(
                err,
                Error::DatapackInvalid {
                    reason: DatapackRejection::NotAPack,
                    ..
                }
            ),
            "got {err:?}"
        );
        assert_eq!(
            std::fs::read(wd.join("level.dat")).unwrap(),
            before,
            "nothing may be written"
        );
    }

    /// N.4: the toggle writes `file/<on-disk spelling>`. This fixes the stuck
    /// toggle when the library picker passes the library spelling.
    #[tokio::test]
    async fn the_toggle_writes_the_entrys_own_spelling() {
        let td = tempfile::tempdir().unwrap();
        if !fs_folds_case(td.path()) {
            return; // case-sensitive: `VeinMiner.zip` names nothing on disk (N.10)
        }
        let wd = game_world(td.path(), "Survival");
        std::fs::create_dir_all(wd.join("datapacks")).unwrap();
        std::fs::write(wd.join("datapacks/veinminer.zip"), pack_zip()).unwrap();
        set_enabled_in_world_at(td.path(), "Survival", "VeinMiner.zip", false)
            .await
            .unwrap();
        let (root, _) = level_dat::read_at(&wd).unwrap();
        assert_eq!(
            level_dat::lists(&root).1,
            vec!["file/veinminer.zip".to_string()]
        );
    }

    #[tokio::test]
    async fn add_refuses_a_library_file_whose_name_the_game_ignores() {
        // A legacy `X.ZIP` library row, from before N.5 normalised install names.
        let td = tempfile::tempdir().unwrap();
        let lib = library_dir_at(td.path());
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("Legacy.ZIP"), pack_zip()).unwrap();
        let wd = game_world(td.path(), "Survival");
        let err = add_to_world_at(td.path(), "Survival", "Legacy.ZIP")
            .await
            .unwrap_err();
        assert!(
            matches!(
                err,
                Error::DatapackInvalid {
                    reason: DatapackRejection::NotAZip,
                    ..
                }
            ),
            "got {err:?}"
        );
        assert!(!wd.join("datapacks").exists(), "nothing may be linked");
    }

    #[tokio::test]
    async fn add_refuses_a_library_file_without_a_root_pack_mcmeta() {
        // Registry adoption never classifies; `add` closes the vouch rule's only hole (N.4).
        let td = tempfile::tempdir().unwrap();
        let lib = library_dir_at(td.path());
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("rootless.zip"), zip_of(&[("readme.txt", b"x")])).unwrap();
        let wd = game_world(td.path(), "Survival");
        let err = add_to_world_at(td.path(), "Survival", "rootless.zip")
            .await
            .unwrap_err();
        assert!(
            matches!(
                err,
                Error::DatapackInvalid {
                    reason: DatapackRejection::NotAPack,
                    ..
                }
            ),
            "got {err:?}"
        );
        assert!(!wd.join("datapacks").exists());
    }

    /// §0.5 A7: an only-old world reports D2's reason ahead of any §2 error.
    #[tokio::test]
    async fn an_only_old_world_refuses_a_rootless_zip_add_with_the_level_dat_reason() {
        let td = tempfile::tempdir().unwrap();
        let lib = library_dir_at(td.path());
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("rootless.zip"), zip_of(&[("readme.txt", b"x")])).unwrap();
        let wd = game_world(td.path(), "Survival");
        std::fs::rename(wd.join("level.dat"), wd.join("level.dat_old")).unwrap();
        let err = add_to_world_at(td.path(), "Survival", "rootless.zip")
            .await
            .unwrap_err();
        assert!(
            matches!(err, Error::WorldLevelDatOnlyOld { .. }),
            "got {err:?}"
        );
    }

    /// N.4 / N.10: after `materialize`, the id is the entry's own spelling.
    /// NTFS may keep the existing entry's case.
    #[tokio::test]
    async fn add_writes_the_world_entrys_own_spelling() {
        let _lock = hardlink_lock();
        let td = tempfile::tempdir().unwrap();
        if !fs_folds_case(td.path()) {
            return; // case-sensitive: two spellings are two packs
        }
        seed_library(td.path(), "VeinMiner.zip", 48).await;
        let wd = game_world(td.path(), "Survival");
        let dp = wd.join("datapacks");
        std::fs::create_dir_all(&dp).unwrap();
        let lib_bytes = std::fs::read(library_dir_at(td.path()).join("VeinMiner.zip")).unwrap();
        std::fs::write(dp.join("veinminer.zip"), &lib_bytes).unwrap(); // same bytes: ours
        seed(&wd, &[], &["file/veinminer.zip"]);
        add_to_world_at(td.path(), "Survival", "VeinMiner.zip")
            .await
            .unwrap();
        let names: Vec<String> = std::fs::read_dir(&dp)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), 1, "{names:?}");
        let (root, _) = level_dat::read_at(&wd).unwrap();
        let (en, dis) = level_dat::lists(&root);
        let id = level_dat_entry(&names[0]);
        assert!(en.contains(&id) && !dis.contains(&id), "{en:?} {dis:?}");
        let listed = crate::datapacks::world_link::list_for_world_at(td.path(), "Survival", None)
            .await
            .unwrap()
            .packs;
        assert_eq!(listed[0].state, WorldPackState::Enabled);
    }

    /// The cascade's orphan sweep never deletes a file: it reaches worlds with
    /// no verified file of ours. An id with no file behind it is forgotten.
    #[tokio::test]
    async fn the_sweep_forgets_an_orphaned_id() {
        let td = tempfile::tempdir().unwrap();
        let wd = game_world(td.path(), "Alpha");
        seed(&wd, &["file/vm.zip"], &[]);
        forget_for_cascade_at(td.path(), "Alpha", "vm.zip")
            .await
            .unwrap();
        let (root, _) = level_dat::read_at(&wd).unwrap();
        assert_eq!(level_dat::lists(&root).0, vec!["vanilla".to_string()]);
    }

    /// A file the sweep finds under the name was never verified as ours: it is
    /// not deleted, its id stays, and the world reports a failure.
    #[tokio::test]
    async fn the_sweep_never_deletes_a_file() {
        let td = tempfile::tempdir().unwrap();
        let wd = game_world(td.path(), "Alpha");
        std::fs::create_dir_all(wd.join("datapacks")).unwrap();
        std::fs::write(wd.join("datapacks/vm.zip"), pack_zip()).unwrap();
        seed(&wd, &["file/vm.zip"], &[]);
        let level_before = std::fs::read(wd.join("level.dat")).unwrap();
        assert!(forget_for_cascade_at(td.path(), "Alpha", "vm.zip")
            .await
            .is_err());
        assert!(wd.join("datapacks/vm.zip").exists());
        assert_eq!(std::fs::read(wd.join("level.dat")).unwrap(), level_before);
    }

    /// N.4 / N.10: NTFS may keep an existing entry's spelling when a link
    /// lands over it. The id written must be one the game loads, so a linked
    /// entry spelled `X.ZIP` is refused rather than recorded as `file/X.ZIP`.
    #[tokio::test]
    async fn a_linked_entry_whose_spelling_the_game_ignores_is_refused() {
        let td = tempfile::tempdir().unwrap();
        std::fs::write(td.path().join("X.ZIP"), b"x").unwrap();
        let got = linked_entry_name(td.path(), "X.zip").await;
        if fs_folds_case(td.path()) {
            assert!(
                matches!(
                    got,
                    Err(Error::DatapackInvalid {
                        reason: DatapackRejection::NotAZip,
                        ..
                    })
                ),
                "{got:?}"
            );
        } else {
            // Case-sensitive: `X.zip` is not there at all.
            assert!(
                matches!(got, Err(Error::ModsInstancePath { .. })),
                "{got:?}"
            );
        }
        std::fs::write(td.path().join("ok.zip"), b"x").unwrap();
        assert_eq!(
            linked_entry_name(td.path(), "ok.zip").await.unwrap(),
            "ok.zip"
        );
    }

    /// N.4: where the file system folds case, adding `VM.zip` to a world that
    /// holds `VM.ZIP` would link over that entry, and NTFS may keep its
    /// spelling — an id the game ignores. It is refused before anything is
    /// linked, and the world entry is left exactly as it was.
    #[tokio::test]
    async fn add_refuses_before_linking_over_an_entry_spelled_x_zip_upper() {
        let _lock = hardlink_lock();
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "VM.zip", 48).await;
        let wd = game_world(td.path(), "Survival");
        let dp = wd.join("datapacks");
        std::fs::create_dir_all(&dp).unwrap();
        let lib_bytes = std::fs::read(library_dir_at(td.path()).join("VM.zip")).unwrap();
        std::fs::write(dp.join("VM.ZIP"), &lib_bytes).unwrap();
        let level_before = std::fs::read(wd.join("level.dat")).unwrap();

        let got = add_to_world_at(td.path(), "Survival", "VM.zip").await;

        let names: Vec<String> = std::fs::read_dir(&dp)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        if fs_folds_case(td.path()) {
            assert!(
                matches!(
                    got,
                    Err(Error::DatapackInvalid {
                        reason: DatapackRejection::NotAZip,
                        ..
                    })
                ),
                "{got:?}"
            );
            assert_eq!(names, vec!["VM.ZIP".to_string()], "nothing was linked");
            assert_eq!(std::fs::read(wd.join("level.dat")).unwrap(), level_before);
        } else {
            // Case-sensitive: `VM.zip` is a different entry from `VM.ZIP`.
            assert!(got.is_ok(), "{got:?}");
        }
    }
}
