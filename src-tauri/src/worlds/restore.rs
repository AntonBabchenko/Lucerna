//! Restore-side operations.

use crate::error::{Error, Result};
use crate::worlds::{
    backups_root, fs as wfs, saves_dir, world_dir_at, zip as wzip, RestoreMode, RestoredWorld,
};
use chrono::Utc;

/// Prefix of the directory that holds the LIVE world while a Replace restore
/// swaps the extract into its place.
///
/// The world folder is embedded in the name — `.tmp-restoring-<world>-<n>` — so
/// a directory left behind by a restore that could not put the world back can be
/// recognised and offered for recovery. Without it the bytes would be
/// unattributable. A sidecar file would have been the obvious alternative and is
/// the wrong one: writing into a world's own tree is what
/// `structural_no_inplace_mods_write` exists to stop.
pub(crate) const TMP_RESTORING_PREFIX: &str = ".tmp-restoring-";

/// Prefix of the hidden stage a world migration (`worlds::migrate`) parks a
/// world in while it is moved or copied between instances —
/// `.tmp-migrate-moved-<world>-<n>`, the same `<world>-<n>` shape as
/// `TMP_RESTORING_PREFIX`, read back by the same `world_folder_of_tmp_dir`.
///
/// The stage lives in the TARGET instance's `saves/`. On the migration's
/// rename path it is the user's ONLY copy of the world from the moment the
/// source folder is renamed into it until the final rename lands under the
/// real name; process death in that window leaves this directory and nothing
/// else. That is why `worlds::orphans` must list it as a stranded world and be
/// able to put it under `saves/<world>`, and why nothing may ever delete a
/// candidate on sight — the rule `claim_stage` states for `.tmp-restoring-*`.
///
/// Invisible to every world listing for the same reason `.tmp-restoring-*` is:
/// `pathsafe::validate_segment` rejects a leading `.`, and `list_worlds` /
/// `list_world_names_in` skip every `saves/` entry that fails it.
///
/// Declared next to its sibling rather than in `worlds::migrate` so the reader
/// and both writers share one definition.
pub(crate) const TMP_MIGRATE_MOVED_PREFIX: &str = ".tmp-migrate-moved-";
/// The stage a migration is COPYING a world into (spec §4.1). Never a stranded
/// world: the source is intact and the stage may be partial. Named here only so
/// the recogniser can state, in one place, which dot-prefixed directories under
/// `saves/` are ours and which of them hold a user's only copy.
pub(crate) const TMP_MIGRATE_COPY_PREFIX: &str = ".tmp-migrate-copy-";
const STAGE_PREFIX: &str = ".tmp-restore-stage-";

/// How many `-<n>` candidates to try before giving up. Reaching this means ~64
/// leaked directories under one `saves/`, i.e. something is deeply wrong.
const MAX_STAGE_CANDIDATES: usize = 64;

/// A claimed staging directory, plus the name reserved to park the live world.
pub(crate) struct Stage {
    /// Created and owned by this call.
    pub stage: std::path::PathBuf,
    /// Reserved by name only — consumed later by a rename.
    pub tmp: std::path::PathBuf,
}

/// Claim a staging directory and reserve the matching name for the live world.
///
/// The stage is claimed with `create_dir`, NOT `create_dir_all`: a colliding name
/// fails `AlreadyExists`, which is this loop's "try the next candidate". That is
/// atomic. `create_dir_all` *succeeds* on an existing directory, so two
/// concurrent restores would extract into one stage and the extra-root check
/// would then accuse both users' perfectly good backups of having an unexpected
/// root.
///
/// The tmp name can only be probed, because a rename consumes it four steps
/// later. `try_exists` is used rather than `exists` because `exists()` reports
/// `false` for ANY stat failure — reading "could not tell" as "free", in the
/// permissive direction. That race is narrowed, not closed; closing it needs a
/// lock held over `saves/` for the whole operation.
///
/// Nothing here ever deletes a candidate. A `.tmp-restoring-*` may be a previous
/// run's stranded world — the user's only copy.
pub(crate) fn claim_stage(saves: &std::path::Path, world_folder: &str) -> Result<Stage> {
    for n in 0..MAX_STAGE_CANDIDATES {
        let stage = saves.join(format!("{STAGE_PREFIX}{world_folder}-{n}"));
        let tmp = saves.join(format!("{TMP_RESTORING_PREFIX}{world_folder}-{n}"));
        if tmp.try_exists().unwrap_or(true) {
            continue;
        }
        match std::fs::create_dir(&stage) {
            Ok(()) => return Ok(Stage { stage, tmp }),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(Error::io(stage.display().to_string(), e)),
        }
    }
    Err(Error::io(
        saves.display().to_string(),
        format!("could not allocate a staging name after {MAX_STAGE_CANDIDATES} attempts"),
    ))
}

/// Recover the world folder name from a `.tmp-restoring-<world>-<n>` or a
/// `.tmp-migrate-moved-<world>-<n>` directory name. `None` for anything that is not
/// one of ours — including a restore staging directory
/// (`.tmp-restore-stage-*`), which holds extracted backup bytes rather than a
/// world.
///
/// Both prefixes answer the same question — whose bytes are parked here? — and
/// `worlds::orphans` uses the answer the same way for both: list the directory,
/// and on request rename it back to `saves/<world>`. For a migration stage that
/// is the right destination: the stage sits in the target instance's `saves/`,
/// so recovery lands the world where the migration was taking it. The tail
/// rule is shared too: a non-empty world, then `-`, then one or more ASCII
/// digits; the world keeps everything before the LAST `-`.
///
/// The two prefixes are pairwise disjoint (and disjoint from `STAGE_PREFIX`),
/// which the tests pin; the order of the two `strip_prefix` calls is therefore
/// immaterial.
///
/// This is the *reader* for the naming decision `claim_stage` and
/// `worlds::migrate` make: embedding the world folder is only justified if it
/// can be read back. `worlds::orphans` is the consumer.
pub(crate) fn world_folder_of_tmp_dir(dir_name: &str) -> Option<String> {
    parked_world_of_tmp_dir(dir_name).map(|(world, _)| world)
}

/// The world folder AND which operation parked it. `Restore` for
/// `.tmp-restoring-<world>-<n>`, `Migration` for
/// `.tmp-migrate-moved-<world>-<n>` — the stage a migration MOVED a world
/// into by rename, i.e. the user's only copy (spec §4.2). A copy-path stage
/// (`.tmp-migrate-copy-<world>-<n>`, spec §4.1) is deliberately `None`: the
/// source world is intact, the stage may be a partial tree, and putting a
/// partial tree back under a real name would list an incomplete world as a
/// normal one — the exact hazard `WorldMigratePartialLeft` exists to name.
pub(crate) fn parked_world_of_tmp_dir(
    dir_name: &str,
) -> Option<(String, crate::worlds::orphans::StrandedKind)> {
    use crate::worlds::orphans::StrandedKind;
    let (rest, kind) = if let Some(rest) = dir_name.strip_prefix(TMP_RESTORING_PREFIX) {
        (rest, StrandedKind::Restore)
    } else if let Some(rest) = dir_name.strip_prefix(TMP_MIGRATE_MOVED_PREFIX) {
        (rest, StrandedKind::Migration)
    } else if dir_name.starts_with(TMP_MIGRATE_COPY_PREFIX) {
        // A copy-path stage is never a parked world: the source is intact and
        // the tree may be partial (spec §4.1). Spelled out rather than left to
        // "no prefix matched" so the exclusion is code, not an accident.
        return None;
    } else {
        return None;
    };
    let (world, n) = rest.rsplit_once('-')?;
    if world.is_empty() || n.is_empty() || !n.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((world.to_string(), kind))
}

/// Outcome of the two-rename window. Every variant says exactly where the
/// user's world is — that is the whole point of the type.
#[derive(Debug)]
pub(crate) enum SwapOutcome {
    /// The extract is in place; the old world has been removed.
    Ok,
    /// The live world could not be moved aside. Nothing changed.
    NotStarted(std::io::Error),
    /// The extract could not be moved in, and the world was put back.
    RolledBack(std::io::Error),
    /// The extract could not be moved in AND the world could not be put back.
    /// `at` is the BARE DIRECTORY NAME holding the world, never a full path:
    /// it reaches the user inside a fully translated sentence, and a filesystem
    /// path pasted into one is exactly what the typed variant exists to avoid.
    ///
    /// Neither cause travels in this variant. Both are `diag!`-logged at the
    /// point of failure, where the full context still exists, and the user-facing
    /// copy points at Logs. Carrying them here as well would be a second copy
    /// nobody reads.
    Stranded { at: String },
}

/// The entire destructive window of a Replace restore: move the live world
/// aside, move the verified extract into its place, roll back if that fails.
///
/// `rename` is injected rather than called directly so a test can fail one
/// specific move. No filesystem trick fails the *second* rename deterministically
/// on Windows, Linux AND macOS — all three of which must run this — and an
/// environment-variable seam is worse than useless here: `test_seam::resolve`
/// falls through to `std::env::var` in production, where forcing these failures
/// IS the data loss this function exists to prevent. The file already
/// establishes this shape: `restore_backup_at_saves` exists solely because it
/// takes its paths as arguments instead of reaching for an `AppHandle`.
///
/// Note what is deliberately absent: the old code ran
/// `let _ = remove_dir_all(&world_path)` before the rollback rename. Under this
/// ordering `world_path` was just vacated by the first rename and the only
/// statement that could recreate it is the one that just failed — so there is
/// nothing to remove, and removing blindly would delete whatever else happened
/// to appear there.
pub(crate) fn swap_in_place(
    world_path: &std::path::Path,
    tmp_path: &std::path::Path,
    inner: &std::path::Path,
    rename: &dyn Fn(&std::path::Path, &std::path::Path) -> std::io::Result<()>,
) -> SwapOutcome {
    if let Err(e) = rename(world_path, tmp_path) {
        return SwapOutcome::NotStarted(e);
    }
    let cause = match rename(inner, world_path) {
        Ok(()) => {
            // Best-effort: the live world is healthy, so a surviving tmp dir is
            // clutter rather than a failure. Logged because the user pays for it
            // in disk space until they find it, and nothing else ever will.
            if let Err(e) = std::fs::remove_dir_all(tmp_path) {
                crate::diag!("restore: leftover tmp dir {}: {e}", tmp_path.display());
            }
            return SwapOutcome::Ok;
        }
        Err(e) => e,
    };
    match rename(tmp_path, world_path) {
        Ok(()) => {
            crate::diag!(
                "restore: swap-in failed ({cause}); world rolled back to {}",
                world_path.display()
            );
            SwapOutcome::RolledBack(cause)
        }
        Err(rollback_cause) => {
            crate::diag!(
                "restore: swap-in failed ({cause}) AND rollback failed ({rollback_cause}); world stranded at {}",
                tmp_path.display()
            );
            SwapOutcome::Stranded {
                at: tmp_path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            }
        }
    }
}

/// Public entrypoint. Resolves paths from the AppHandle then
/// delegates to `restore_backup_at_saves`.
pub async fn restore_backup(
    app: &tauri::AppHandle,
    instance_id: &str,
    world_folder_name: &str,
    backup_filename: &str,
    mode: RestoreMode,
) -> Result<RestoredWorld> {
    wfs::validate_segment(world_folder_name)?;
    wfs::validate_segment(backup_filename)?;
    let saves = saves_dir(app, instance_id)?;
    let backups_dir = backups_root(app, instance_id)?.join(world_folder_name);
    let backup_path = backups_dir.join(backup_filename);
    restore_backup_at_saves(&saves, &backups_dir, &backup_path, world_folder_name, mode).await
}

/// Test-friendly variant that takes the saves dir + backups dir
/// directly (skips the AppHandle path-resolution). Public API uses
/// this internally; integration tests use it too.
pub async fn restore_backup_at_saves(
    saves: &std::path::Path,
    backups_dir: &std::path::Path,
    backup_path: &std::path::Path,
    world_folder_name: &str,
    mode: RestoreMode,
) -> Result<RestoredWorld> {
    wfs::validate_segment(world_folder_name)?;
    if !backup_path.is_file() {
        return Err(Error::BackupNotFound {
            instance_id: "<test>".into(),
            world_folder: world_folder_name.into(),
            filename: backup_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("?")
                .into(),
        });
    }
    match mode {
        RestoreMode::Replace => {
            restore_replace(saves, backups_dir, backup_path, world_folder_name).await
        }
        RestoreMode::AsCopy => restore_as_copy(saves, backup_path, world_folder_name).await,
    }
}

async fn restore_replace(
    saves: &std::path::Path,
    backups_dir: &std::path::Path,
    backup_path: &std::path::Path,
    world_folder: &str,
) -> Result<RestoredWorld> {
    let world_path = world_dir_at(saves, world_folder)?;
    let corrupt = |details: String| Error::BackupCorrupt {
        filename: backup_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .into(),
        details,
    };

    // 1. Claim a staging dir and reserve the name that will park the live world.
    let staged = claim_stage(saves, world_folder)?;

    // 2-4. Extract and verify BEFORE anything destructive happens. Every exit
    //      from here to the swap drops the stage and leaves the world untouched
    //      — a corrupt or foreign backup, by far the most likely failure, no
    //      longer moves the user's world at all, and no longer costs a
    //      world-sized snapshot before it is discovered.
    let verified = async {
        let backup_clone = backup_path.to_path_buf();
        let stage_clone = staged.stage.clone();
        // spawn_blocking has TWO results: the join and the extraction itself.
        // Both route through this one `?` chain so neither can skip the cleanup
        // below; `restore_as_copy` uses the same shape.
        tokio::task::spawn_blocking(move || wzip::extract_zip(&backup_clone, &stage_clone))
            .await
            .map_err(|e| Error::io(staged.stage.display().to_string(), format!("join: {e}")))??;

        // The zip's root is the world's folder name AT BACKUP TIME (put there
        // by backup_world / zip_dir), which need not be its name now — see
        // `reroot_single_root`. Exactly one root directory is accepted,
        // whatever it is called, and re-rooted to `world_folder`; any other
        // shape must ERROR, never silently leave an empty world.
        reroot_single_root(&staged.stage, world_folder, &corrupt)
    }
    .await;

    let inner = match verified {
        Ok(i) => i,
        Err(e) => {
            drop_stage(&staged.stage);
            return Err(e);
        }
    };

    // 5. Snapshot the live world, now that the backup is known good.
    //    `pick_unused_filename` because `zip_dir` opens its destination with
    //    `File::create`, which truncates: two restores of the same world in the
    //    same wall-clock second used to destroy the first snapshot — the very
    //    artefact the recovery story rests on.
    let (_, pre_path) = crate::worlds::backup::pick_unused_filename(
        backups_dir,
        &format!("pre-restore-{}", Utc::now().format("%Y-%m-%dT%H-%M-%S")),
    )?;
    let world_clone = world_path.clone();
    let pre_clone = pre_path.clone();
    let world_folder_owned = world_folder.to_string();
    let snapshot = tokio::task::spawn_blocking(move || {
        wzip::zip_dir(&world_clone, &pre_clone, &world_folder_owned)
    })
    .await
    .map_err(|e| Error::io(pre_path.display().to_string(), format!("join: {e}")));
    match snapshot {
        Ok(Ok(())) => {}
        Ok(Err(e)) | Err(e) => {
            drop_stage(&staged.stage);
            return Err(e);
        }
    }

    // 6-7. The destructive window: two adjacent renames plus the rollback.
    let outcome = swap_in_place(&world_path, &staged.tmp, &inner, &|a, b| {
        std::fs::rename(a, b)
    });
    drop_stage(&staged.stage);

    match outcome {
        SwapOutcome::Ok => Ok(RestoredWorld {
            final_folder_name: world_folder.into(),
        }),
        SwapOutcome::NotStarted(e) => Err(map_move_aside_error(
            &world_path,
            &staged.tmp,
            world_folder,
            e,
        )),
        SwapOutcome::RolledBack(e) => Err(Error::io(world_path.display().to_string(), e)),
        SwapOutcome::Stranded { at, .. } => Err(Error::WorldRestoreStranded {
            world_folder: world_folder.into(),
            recovered_at: at,
        }),
    }
}

/// Drop a staging directory. Best-effort by nature — it holds only extracted
/// bytes we can always re-extract — but never silently: a stage that survives is
/// a world-sized directory no launcher listing will ever show, because
/// `validate_segment` rejects its leading dot.
fn drop_stage(stage: &std::path::Path) {
    if let Err(e) = std::fs::remove_dir_all(stage) {
        crate::diag!("restore: leftover staging dir {}: {e}", stage.display());
    }
}

/// Reduce what `extract_zip` left at the top level of a staging directory to
/// the one shape a world backup can have — exactly one directory — and make
/// that directory answer to `world_folder`.
///
/// **Invariant: the archive's root name is the world's folder name AT BACKUP
/// TIME; it is informational, not identity.** `backup_world` zips with
/// `root_name = <world folder>`, and worlds get renamed afterwards: a
/// migration into an instance where the name is taken suffixes it
/// (`Survival` → `Survival (2)`), and nothing stops the user renaming the
/// folder in Explorer. Either used to make every earlier backup of that world
/// unrestorable (`BackupCorrupt`, "unexpected root"). So a single root of ANY
/// name is the world and is re-rooted here — renamed to
/// `<stage>/<world_folder>` — so every later step sees the name it expects.
/// What stays `BackupCorrupt` is a shape that cannot be one world: no entry at
/// all, several entries, or a lone file. Each rejection says what was found.
///
/// Fallback direction (restrictive): an unreadable listing or entry type is
/// `Io`, never "nothing extra here" — the check this replaces `flatten()`ed
/// per-entry errors, reading "could not tell" as "no extra root". The entry
/// type is read without following links: `extract_zip` never creates one, so
/// a symlink here is not something a backup produced and is refused as
/// "not a folder".
fn reroot_single_root(
    stage: &std::path::Path,
    world_folder: &str,
    corrupt: &dyn Fn(String) -> Error,
) -> Result<std::path::PathBuf> {
    let mut entries: Vec<(std::ffi::OsString, bool)> = Vec::new();
    let listing =
        std::fs::read_dir(stage).map_err(|e| Error::io(stage.display().to_string(), e))?;
    for entry in listing {
        let entry = entry.map_err(|e| Error::io(stage.display().to_string(), e))?;
        let is_dir = entry
            .file_type()
            .map_err(|e| Error::io(entry.path().display().to_string(), e))?
            .is_dir();
        entries.push((entry.file_name(), is_dir));
    }
    let (root, is_dir) = match entries.as_slice() {
        [] => return Err(corrupt("archive is empty: it holds no root folder".into())),
        [single] => single,
        many => {
            let names: Vec<String> = many
                .iter()
                .map(|(name, _)| name.to_string_lossy().into_owned())
                .collect();
            return Err(corrupt(format!(
                "archive has {} top-level entries ({}); a world backup has exactly one root folder",
                names.len(),
                names.join(", ")
            )));
        }
    };
    let root_name = root.to_string_lossy();
    if !*is_dir {
        return Err(corrupt(format!(
            "archive's only root '{root_name}' is a file, not a folder"
        )));
    }
    let inner = stage.join(world_folder);
    if root.as_os_str() == std::ffi::OsStr::new(world_folder) {
        return Ok(inner);
    }
    let found = stage.join(root);
    std::fs::rename(&found, &inner).map_err(|e| {
        Error::io(
            inner.display().to_string(),
            format!("re-root '{root_name}/' as '{world_folder}/': {e}"),
        )
    })?;
    crate::diag!(
        "restore: archive root '{root_name}' re-rooted as '{world_folder}' (the world was renamed after this backup was taken)"
    );
    Ok(inner)
}

/// Map a failure to move the live world aside.
///
/// A running Minecraft holds the world's lock file open, which Windows reports
/// as access denied (5) / sharing violation (32) / lock violation (33). But an
/// EXISTING destination directory is *also* reported as 5 — `MoveFileExW`
/// ignores `MOVEFILE_REPLACE_EXISTING` when the destination is a directory — so
/// mapping 5 straight to `WorldInUse` would tell a user with Minecraft closed to
/// quit Minecraft, permanently and falsely. Check the destination first.
fn map_move_aside_error(
    world_path: &std::path::Path,
    tmp_path: &std::path::Path,
    world_folder: &str,
    e: std::io::Error,
) -> Error {
    if tmp_path.try_exists().unwrap_or(true) {
        return Error::io(
            tmp_path.display().to_string(),
            format!("staging name was taken between reservation and use: {e}"),
        );
    }
    if matches!(e.raw_os_error(), Some(5) | Some(32) | Some(33)) {
        return Error::WorldInUse {
            folder_name: world_folder.to_string(),
        };
    }
    Error::io(world_path.display().to_string(), e)
}

async fn restore_as_copy(
    saves: &std::path::Path,
    backup_path: &std::path::Path,
    world_folder: &str,
) -> Result<RestoredWorld> {
    // Pick a free `<world> (restored)` name (suffix on collision).
    let mut chosen = format!("{world_folder} (restored)");
    if saves.join(&chosen).exists() {
        let mut ok = false;
        for i in 2..=999 {
            chosen = format!("{world_folder} (restored {i})");
            if !saves.join(&chosen).exists() {
                ok = true;
                break;
            }
        }
        if !ok {
            return Err(Error::WorldNameUnresolvable {
                folder_name: world_folder.into(),
            });
        }
    }
    let corrupt = |details: String| Error::BackupCorrupt {
        filename: backup_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .into(),
        details,
    };

    // Extract the zip into a temp staging dir, re-root its single top-level
    // folder to `world_folder` (the archive's root is the name the world had
    // when it was backed up — see `reroot_single_root`), then rename that
    // folder to `saves/<chosen>/`.
    let tmp_extract = saves.join(format!(".tmp-as-copy-{}", &chosen.replace(' ', "_")));
    let _ = std::fs::remove_dir_all(&tmp_extract); // stale from earlier failure
    std::fs::create_dir_all(&tmp_extract)
        .map_err(|e| Error::io(tmp_extract.display().to_string(), e))?;
    let extracted = async {
        let backup_clone = backup_path.to_path_buf();
        let tmp_clone = tmp_extract.clone();
        // Both spawn_blocking results — the join and the extraction — route
        // through this one `?` chain so neither can skip the cleanup below
        // (the `restore_replace` shape; a join error used to leak this dir).
        tokio::task::spawn_blocking(move || wzip::extract_zip(&backup_clone, &tmp_clone))
            .await
            .map_err(|e| Error::io(tmp_extract.display().to_string(), format!("join: {e}")))??;
        reroot_single_root(&tmp_extract, world_folder, &corrupt)
    }
    .await;
    let inner = match extracted {
        Ok(i) => i,
        Err(e) => {
            drop_stage(&tmp_extract);
            return Err(e);
        }
    };
    let final_path = saves.join(&chosen);
    if let Err(e) = std::fs::rename(&inner, &final_path) {
        // The extract is only bytes the backup zip still holds, so dropping it
        // costs nothing; `drop_stage` logs its own failure (Fallback Q4).
        drop_stage(&tmp_extract);
        return Err(Error::io(final_path.display().to_string(), e));
    }
    drop_stage(&tmp_extract);
    Ok(RestoredWorld {
        final_folder_name: chosen,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worlds::zip as wzip;
    use std::fs;
    use std::path::{Path, PathBuf};
    use tempfile::tempdir;

    fn make_world_with_files(
        world_name: &str,
        files: &[(&str, &[u8])],
    ) -> (tempfile::TempDir, PathBuf, PathBuf) {
        let td = tempdir().unwrap();
        let saves = td.path().join("saves");
        let backups = td.path().join("backups").join(world_name);
        fs::create_dir_all(saves.join(world_name)).unwrap();
        fs::create_dir_all(&backups).unwrap();
        for (rel, bytes) in files {
            let p = saves.join(world_name).join(rel);
            if let Some(parent) = p.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&p, *bytes).unwrap();
        }
        (td, saves, backups)
    }

    /// A rename that fails on the given 1-based call numbers and otherwise
    /// delegates to the real one. This is the entire test seam — an argument,
    /// not a process-global override that production would also honour.
    fn failing_rename(fail_on: &'static [usize]) -> impl Fn(&Path, &Path) -> std::io::Result<()> {
        let calls = std::cell::Cell::new(0usize);
        move |from: &Path, to: &Path| {
            calls.set(calls.get() + 1);
            if fail_on.contains(&calls.get()) {
                return Err(std::io::Error::other("forced rename failure"));
            }
            std::fs::rename(from, to)
        }
    }

    /// `saves/W` holding a marker, plus a staged extract at `<stage>/W` holding
    /// a different one, so every assertion can tell which tree ended up where.
    fn make_swap_fixture() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
        let td = tempdir().unwrap();
        let saves = td.path().join("saves");
        let world = saves.join("W");
        fs::create_dir_all(&world).unwrap();
        fs::write(world.join("marker.txt"), b"original").unwrap();
        let s = claim_stage(&saves, "W").unwrap();
        let inner = s.stage.join("W");
        fs::create_dir_all(&inner).unwrap();
        fs::write(inner.join("marker.txt"), b"restored").unwrap();
        (td, world, s.tmp, inner)
    }

    #[test]
    fn swap_in_place_replaces_the_world_and_leaves_no_tmp() {
        let (_td, world, tmp, inner) = make_swap_fixture();
        let out = swap_in_place(&world, &tmp, &inner, &|a, b| std::fs::rename(a, b));
        assert!(matches!(out, SwapOutcome::Ok), "got {out:?}");
        assert_eq!(fs::read(world.join("marker.txt")).unwrap(), b"restored");
        assert!(!tmp.exists(), "tmp must be gone on success");
    }

    #[test]
    fn swap_in_place_reports_not_started_when_the_world_cannot_move() {
        let (_td, world, tmp, inner) = make_swap_fixture();
        let out = swap_in_place(&world, &tmp, &inner, &failing_rename(&[1]));
        assert!(matches!(out, SwapOutcome::NotStarted(_)), "got {out:?}");
        assert_eq!(
            fs::read(world.join("marker.txt")).unwrap(),
            b"original",
            "nothing may move when the first rename fails"
        );
    }

    #[test]
    fn swap_in_place_rolls_back_when_the_swap_in_fails() {
        let (_td, world, tmp, inner) = make_swap_fixture();
        let out = swap_in_place(&world, &tmp, &inner, &failing_rename(&[2]));
        assert!(matches!(out, SwapOutcome::RolledBack(_)), "got {out:?}");
        assert_eq!(
            fs::read(world.join("marker.txt")).unwrap(),
            b"original",
            "the original world must be back in place"
        );
        assert!(!tmp.exists(), "the rollback consumed the tmp dir");
    }

    #[test]
    fn swap_in_place_reports_stranded_when_the_rollback_also_fails() {
        let (_td, world, tmp, inner) = make_swap_fixture();
        let out = swap_in_place(&world, &tmp, &inner, &failing_rename(&[2, 3]));
        let SwapOutcome::Stranded { at, .. } = out else {
            panic!("expected Stranded, got {out:?}");
        };
        assert_eq!(at, tmp.file_name().unwrap().to_string_lossy());
        assert!(
            !at.contains(std::path::MAIN_SEPARATOR),
            "`at` must be a bare segment, never a path: {at}"
        );
        assert!(tmp.is_dir(), "the world's bytes must still be at `at`");
        assert_eq!(
            fs::read(tmp.join("marker.txt")).unwrap(),
            b"original",
            "`at` must hold the ORIGINAL world, not the extract"
        );
        assert!(
            !world.exists(),
            "saves/W really is gone - that is the state being reported"
        );
    }

    #[test]
    fn claim_stage_skips_taken_candidates() {
        let td = tempdir().unwrap();
        let saves = td.path().to_path_buf();
        fs::create_dir_all(saves.join(".tmp-restore-stage-W-0")).unwrap();

        let s = claim_stage(&saves, "W").unwrap();

        assert!(
            s.stage.ends_with(".tmp-restore-stage-W-1"),
            "got {:?}",
            s.stage
        );
        assert!(s.tmp.ends_with(".tmp-restoring-W-1"), "got {:?}", s.tmp);
        assert!(
            s.stage.is_dir(),
            "the stage must be created, not just named"
        );
        assert!(
            saves.join(".tmp-restore-stage-W-0").is_dir(),
            "an existing candidate must never be deleted"
        );
    }

    #[test]
    fn claim_stage_skips_a_candidate_whose_tmp_name_is_taken() {
        let td = tempdir().unwrap();
        let saves = td.path().to_path_buf();
        // A stranded world from an earlier run occupies the tmp name for n=0.
        fs::create_dir_all(saves.join(".tmp-restoring-W-0")).unwrap();

        let s = claim_stage(&saves, "W").unwrap();

        assert!(s.tmp.ends_with(".tmp-restoring-W-1"), "got {:?}", s.tmp);
        assert!(
            saves.join(".tmp-restoring-W-0").is_dir(),
            "a stranded world must never be deleted to free its name"
        );
    }

    #[test]
    fn claim_stage_gives_up_when_every_candidate_is_taken() {
        let td = tempdir().unwrap();
        let saves = td.path().to_path_buf();
        for n in 0..MAX_STAGE_CANDIDATES {
            fs::create_dir_all(saves.join(format!(".tmp-restore-stage-W-{n}"))).unwrap();
        }
        assert!(matches!(claim_stage(&saves, "W"), Err(Error::Io { .. })));
    }

    #[test]
    fn tmp_dir_name_round_trips_the_world_folder() {
        let td = tempdir().unwrap();
        let saves = td.path().to_path_buf();
        let s = claim_stage(&saves, "My-World").unwrap();
        let name = s.tmp.file_name().unwrap().to_string_lossy().to_string();
        assert_eq!(world_folder_of_tmp_dir(&name).as_deref(), Some("My-World"));
    }

    #[test]
    fn world_folder_of_tmp_dir_rejects_names_that_are_not_ours() {
        assert_eq!(world_folder_of_tmp_dir("Survival"), None);
        assert_eq!(world_folder_of_tmp_dir(".tmp-restore-stage-W-0"), None);
        assert_eq!(world_folder_of_tmp_dir(".tmp-restoring-W"), None);
        assert_eq!(world_folder_of_tmp_dir(".tmp-restoring--0"), None);
    }

    #[test]
    fn migrate_stage_name_round_trips_the_world_folder() {
        // `worlds::migrate` names its stage with the same `<prefix><world>-<n>`
        // shape `claim_stage` uses, so the one reader serves both. Built from
        // the constant so a renamed prefix cannot silently orphan every stage
        // already on disk.
        let name = format!("{TMP_MIGRATE_MOVED_PREFIX}Survival-1");
        assert_eq!(world_folder_of_tmp_dir(&name).as_deref(), Some("Survival"));
        assert_eq!(
            world_folder_of_tmp_dir(".tmp-migrate-moved-W-0").as_deref(),
            Some("W")
        );
        // A world name containing '-' keeps everything before the LAST '-'.
        assert_eq!(
            world_folder_of_tmp_dir(".tmp-migrate-moved-My-World-12").as_deref(),
            Some("My-World")
        );
        // Spaces are ordinary world-name characters.
        assert_eq!(
            world_folder_of_tmp_dir(".tmp-migrate-moved-My World-0").as_deref(),
            Some("My World")
        );
    }

    #[test]
    fn world_folder_of_tmp_dir_rejects_migrate_names_without_a_counter() {
        // Same tail rule as `.tmp-restoring-`: the name must end in `-<digits>`
        // with a non-empty world before it. A bare `.tmp-migrate-moved-W` is not a
        // stage this launcher ever creates and must not be offered as one.
        assert_eq!(world_folder_of_tmp_dir(".tmp-migrate-moved-W"), None);
        assert_eq!(world_folder_of_tmp_dir(".tmp-migrate-moved-"), None);
        assert_eq!(world_folder_of_tmp_dir(".tmp-migrate-moved--0"), None);
        assert_eq!(world_folder_of_tmp_dir(".tmp-migrate-moved-W-x"), None);
        assert_eq!(world_folder_of_tmp_dir(".tmp-migrate-moved-W-"), None);
    }

    #[test]
    fn the_parked_prefixes_are_pairwise_disjoint() {
        // `strip_prefix` is tried in order. If one prefix were a prefix of
        // another, a directory of the longer kind would parse as a world of the
        // shorter kind with prefix garbage glued onto its name — and recovery
        // would rename it to that garbage.
        assert!(!TMP_MIGRATE_MOVED_PREFIX.starts_with(TMP_RESTORING_PREFIX));
        assert!(!TMP_RESTORING_PREFIX.starts_with(TMP_MIGRATE_MOVED_PREFIX));
        assert!(!TMP_MIGRATE_MOVED_PREFIX.starts_with(STAGE_PREFIX));
        assert!(!STAGE_PREFIX.starts_with(TMP_MIGRATE_MOVED_PREFIX));
    }

    /// Collect `pre-restore-*.zip` names in a backups dir.
    fn pre_restore_zips(backups_dir: &Path) -> Vec<String> {
        fs::read_dir(backups_dir)
            .unwrap()
            .filter_map(|e| {
                let n = e.ok()?.file_name().into_string().ok()?;
                n.starts_with("pre-restore-").then_some(n)
            })
            .collect()
    }

    /// Collect leftover staging / parked-world directories in a saves dir.
    fn staging_leftovers(saves: &Path) -> Vec<String> {
        fs::read_dir(saves)
            .unwrap()
            .filter_map(|e| {
                let n = e.ok()?.file_name().into_string().ok()?;
                (n.starts_with(".tmp-restore-stage-") || n.starts_with(".tmp-restoring-"))
                    .then_some(n)
            })
            .collect()
    }

    /// Handcraft a zip whose entries are exactly `entries` (`name`, bytes).
    /// `zip_dir` can only produce a single-root archive, and the shape tests
    /// need archives it would never write: two roots, or a lone file.
    fn write_zip_with_entries(dest: &Path, entries: &[(&str, &[u8])]) {
        use std::io::Write as _;
        let file = std::fs::File::create(dest).unwrap();
        let mut zw = zip::ZipWriter::new(std::io::BufWriter::new(file));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, bytes) in entries {
            zw.start_file(*name, options).unwrap();
            zw.write_all(bytes).unwrap();
        }
        zw.finish().unwrap();
    }

    /// Collect leftover `.tmp-as-copy-*` extraction directories in a saves dir.
    fn as_copy_leftovers(saves: &Path) -> Vec<String> {
        fs::read_dir(saves)
            .unwrap()
            .filter_map(|e| {
                let n = e.ok()?.file_name().into_string().ok()?;
                n.starts_with(".tmp-as-copy-").then_some(n)
            })
            .collect()
    }

    #[tokio::test]
    async fn restore_replace_rejects_a_corrupt_backup_without_moving_the_world() {
        // World "W" with marker file inside.
        let (_td, saves, backups_dir) = make_world_with_files("W", &[("marker.txt", b"original")]);

        // Place a CORRUPT backup zip in backups_dir — extract will fail.
        let bad_backup = backups_dir.join("2026-05-24T10-00-00.zip");
        fs::write(&bad_backup, b"NOT A ZIP").unwrap();

        let r = restore_replace(&saves, &backups_dir, &bad_backup, "W").await;
        assert!(
            matches!(r, Err(Error::BackupCorrupt { .. })),
            "expected BackupCorrupt, got: {r:?}"
        );

        // The world never moved — not "was rolled back", never moved.
        let marker = saves.join("W").join("marker.txt");
        assert!(marker.is_file(), "the world must not have moved at all");
        assert_eq!(fs::read(&marker).unwrap(), b"original");

        // And no snapshot was written. The pre-restore zip is taken only once
        // the backup is known good, so the most likely failure - an unreadable
        // backup - costs nothing. Before the reorder this was exactly 1.
        let pre_restore = pre_restore_zips(&backups_dir);
        assert!(
            pre_restore.is_empty(),
            "no snapshot should be written for a backup that never verified, found {pre_restore:?}"
        );

        let leftovers = staging_leftovers(&saves);
        assert!(leftovers.is_empty(), "leftovers: {leftovers:?}");
    }

    #[tokio::test]
    async fn restore_replace_happy_path_swaps_contents() {
        // World "W" with v1 content.
        let (_td, saves, backups_dir) = make_world_with_files("W", &[("file.txt", b"v1")]);
        // Make a real backup of v1.
        let backup_path = backups_dir.join("2026-05-24T10-00-00.zip");
        wzip::zip_dir(&saves.join("W"), &backup_path, "W").unwrap();
        // Now mutate the world to v2.
        fs::write(saves.join("W").join("file.txt"), b"v2").unwrap();

        let r = restore_replace(&saves, &backups_dir, &backup_path, "W")
            .await
            .unwrap();
        assert_eq!(r.final_folder_name, "W");
        // World now contains v1 again.
        assert_eq!(fs::read(saves.join("W").join("file.txt")).unwrap(), b"v1");
        // Pre-restore captured v2 (so user can roll back the rollback).
        let pre_restore: Vec<_> = fs::read_dir(&backups_dir)
            .unwrap()
            .filter_map(|e| {
                let n = e.ok()?.file_name().into_string().ok()?;
                if n.starts_with("pre-restore-") {
                    Some(n)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(pre_restore.len(), 1);
    }

    #[tokio::test]
    async fn restore_as_copy_suffixes_on_conflict() {
        let (_td, saves, backups_dir) = make_world_with_files("W", &[("file.txt", b"v1")]);
        let backup_path = backups_dir.join("2026-05-24T10-00-00.zip");
        wzip::zip_dir(&saves.join("W"), &backup_path, "W").unwrap();

        // Pre-create "W (restored)" so the first attempt collides.
        fs::create_dir_all(saves.join("W (restored)")).unwrap();
        fs::write(saves.join("W (restored)").join("placeholder"), b"x").unwrap();

        let r = restore_as_copy(&saves, &backup_path, "W").await.unwrap();
        assert_eq!(r.final_folder_name, "W (restored 2)");
        // Original and the pre-existing collider untouched.
        assert!(saves.join("W").is_dir());
        assert_eq!(
            fs::read(saves.join("W (restored)").join("placeholder")).unwrap(),
            b"x"
        );
        // New copy contains v1.
        assert_eq!(
            fs::read(saves.join("W (restored 2)").join("file.txt")).unwrap(),
            b"v1"
        );
    }

    #[tokio::test]
    async fn restore_as_copy_basic_no_conflict() {
        let (_td, saves, backups_dir) = make_world_with_files("W", &[("file.txt", b"v1")]);
        let backup_path = backups_dir.join("2026-05-24T10-00-00.zip");
        wzip::zip_dir(&saves.join("W"), &backup_path, "W").unwrap();

        let r = restore_as_copy(&saves, &backup_path, "W").await.unwrap();
        assert_eq!(r.final_folder_name, "W (restored)");
        assert_eq!(
            fs::read(saves.join("W (restored)").join("file.txt")).unwrap(),
            b"v1"
        );
    }

    #[test]
    fn reroot_single_root_renames_a_foreign_root_and_keeps_a_matching_one() {
        let td = tempdir().unwrap();
        let corrupt = |details: String| Error::BackupCorrupt {
            filename: "t.zip".into(),
            details,
        };

        // A root under the world's OLD name is renamed to its current one.
        let stage = td.path().join("stage-a");
        fs::create_dir_all(stage.join("Survival")).unwrap();
        fs::write(stage.join("Survival").join("level.dat"), b"x").unwrap();
        let inner = reroot_single_root(&stage, "Survival (2)", &corrupt).unwrap();
        assert_eq!(inner, stage.join("Survival (2)"));
        assert_eq!(fs::read(inner.join("level.dat")).unwrap(), b"x");
        assert!(
            !stage.join("Survival").try_exists().unwrap(),
            "the old root name must not survive next to the new one"
        );

        // A root already under the current name is left exactly where it is.
        let stage = td.path().join("stage-b");
        fs::create_dir_all(stage.join("W")).unwrap();
        let inner = reroot_single_root(&stage, "W", &corrupt).unwrap();
        assert_eq!(inner, stage.join("W"));
        assert!(stage.join("W").is_dir());

        // An empty stage is a corrupt archive, never an empty world.
        let stage = td.path().join("stage-c");
        fs::create_dir_all(&stage).unwrap();
        assert!(matches!(
            reroot_single_root(&stage, "W", &corrupt),
            Err(Error::BackupCorrupt { .. })
        ));
    }

    #[tokio::test]
    async fn restore_replace_re_roots_a_backup_taken_under_the_worlds_old_name() {
        // The world was backed up as "Survival" and has since been renamed to
        // "Survival (2)" — a migration into an instance where the name was
        // taken, or a rename in Explorer. The backup's root is the OLD name.
        let (_td, saves, backups_dir) =
            make_world_with_files("Survival (2)", &[("file.txt", b"v1")]);
        let backup_path = backups_dir.join("2026-05-24T10-00-00.zip");
        wzip::zip_dir(&saves.join("Survival (2)"), &backup_path, "Survival").unwrap();
        fs::write(saves.join("Survival (2)").join("file.txt"), b"v2").unwrap();

        let r = restore_replace(&saves, &backups_dir, &backup_path, "Survival (2)")
            .await
            .unwrap();

        assert_eq!(r.final_folder_name, "Survival (2)");
        assert_eq!(
            fs::read(saves.join("Survival (2)").join("file.txt")).unwrap(),
            b"v1",
            "the world under its CURRENT name holds the backup's bytes"
        );
        assert!(
            !saves.join("Survival").try_exists().unwrap(),
            "re-rooting must not resurrect the old name as a second world"
        );
        assert_eq!(pre_restore_zips(&backups_dir).len(), 1);
        let leftovers = staging_leftovers(&saves);
        assert!(leftovers.is_empty(), "leftovers: {leftovers:?}");
    }

    #[tokio::test]
    async fn restore_replace_rejects_an_archive_with_two_roots_naming_both() {
        let (_td, saves, backups_dir) = make_world_with_files("W", &[("marker.txt", b"original")]);
        let bad_backup = backups_dir.join("2026-05-24T10-00-00.zip");
        write_zip_with_entries(
            &bad_backup,
            &[("Alpha/level.dat", b"a"), ("Beta/level.dat", b"b")],
        );

        let r = restore_replace(&saves, &backups_dir, &bad_backup, "W").await;

        let Err(Error::BackupCorrupt { details, .. }) = r else {
            panic!("two roots must be BackupCorrupt, got: {r:?}");
        };
        assert!(
            details.contains("Alpha") && details.contains("Beta"),
            "the rejection must say what was found: {details}"
        );
        // The world never moved, no snapshot was taken, nothing was left behind
        // — the guarantees the old mismatched-root test pinned, kept here.
        assert_eq!(
            fs::read(saves.join("W").join("marker.txt")).unwrap(),
            b"original"
        );
        assert!(pre_restore_zips(&backups_dir).is_empty());
        assert!(staging_leftovers(&saves).is_empty());
    }

    #[tokio::test]
    async fn restore_replace_rejects_an_archive_whose_only_root_is_a_file() {
        let (_td, saves, backups_dir) = make_world_with_files("W", &[("marker.txt", b"original")]);
        let bad_backup = backups_dir.join("2026-05-24T10-00-00.zip");
        write_zip_with_entries(&bad_backup, &[("level.dat", b"loose")]);

        let r = restore_replace(&saves, &backups_dir, &bad_backup, "W").await;

        let Err(Error::BackupCorrupt { details, .. }) = r else {
            panic!("a lone file root must be BackupCorrupt, got: {r:?}");
        };
        assert!(
            details.contains("level.dat"),
            "the rejection must name what was found: {details}"
        );
        assert_eq!(
            fs::read(saves.join("W").join("marker.txt")).unwrap(),
            b"original"
        );
        assert!(pre_restore_zips(&backups_dir).is_empty());
        assert!(staging_leftovers(&saves).is_empty());
    }

    #[tokio::test]
    async fn restore_as_copy_re_roots_a_backup_taken_under_the_worlds_old_name() {
        let (_td, saves, backups_dir) =
            make_world_with_files("Survival (2)", &[("file.txt", b"v1")]);
        let backup_path = backups_dir.join("2026-05-24T10-00-00.zip");
        wzip::zip_dir(&saves.join("Survival (2)"), &backup_path, "Survival").unwrap();
        fs::write(saves.join("Survival (2)").join("file.txt"), b"v2").unwrap();

        let r = restore_as_copy(&saves, &backup_path, "Survival (2)")
            .await
            .unwrap();

        assert_eq!(r.final_folder_name, "Survival (2) (restored)");
        assert_eq!(
            fs::read(saves.join("Survival (2) (restored)").join("file.txt")).unwrap(),
            b"v1"
        );
        assert_eq!(
            fs::read(saves.join("Survival (2)").join("file.txt")).unwrap(),
            b"v2",
            "the original is untouched by an as-copy restore"
        );
        assert!(
            !saves.join("Survival").try_exists().unwrap(),
            "re-rooting must not resurrect the old name as a second world"
        );
        let leftovers = as_copy_leftovers(&saves);
        assert!(leftovers.is_empty(), "leftovers: {leftovers:?}");
    }

    #[tokio::test]
    async fn restore_as_copy_rejects_an_archive_with_two_roots_and_leaves_nothing_behind() {
        let (_td, saves, backups_dir) = make_world_with_files("W", &[("marker.txt", b"original")]);
        let bad_backup = backups_dir.join("2026-05-24T10-00-00.zip");
        write_zip_with_entries(
            &bad_backup,
            &[("Alpha/level.dat", b"a"), ("Beta/level.dat", b"b")],
        );

        let r = restore_as_copy(&saves, &bad_backup, "W").await;

        let Err(Error::BackupCorrupt { details, .. }) = r else {
            panic!("two roots must be BackupCorrupt, got: {r:?}");
        };
        assert!(
            details.contains("Alpha") && details.contains("Beta"),
            "the rejection must say what was found: {details}"
        );
        assert!(
            !saves.join("W (restored)").try_exists().unwrap(),
            "no world may appear from a rejected archive"
        );
        assert_eq!(
            fs::read(saves.join("W").join("marker.txt")).unwrap(),
            b"original"
        );
        assert!(as_copy_leftovers(&saves).is_empty());
    }

    #[test]
    fn parked_world_of_tmp_dir_tells_a_restore_from_a_move() {
        use crate::worlds::orphans::StrandedKind;
        assert_eq!(
            parked_world_of_tmp_dir(".tmp-restoring-W-0"),
            Some(("W".to_string(), StrandedKind::Restore))
        );
        assert_eq!(
            parked_world_of_tmp_dir(".tmp-migrate-moved-W-3"),
            Some(("W".to_string(), StrandedKind::Migration))
        );
    }

    #[test]
    fn a_copy_path_stage_is_never_a_parked_world() {
        // Spec §4.1: the source is intact and the stage may be a partial tree.
        assert_eq!(parked_world_of_tmp_dir(".tmp-migrate-copy-W-0"), None);
        assert_eq!(world_folder_of_tmp_dir(".tmp-migrate-copy-W-0"), None);
    }

    #[test]
    fn the_three_prefixes_are_pairwise_disjoint() {
        for (a, b) in [
            (TMP_RESTORING_PREFIX, TMP_MIGRATE_MOVED_PREFIX),
            (TMP_RESTORING_PREFIX, TMP_MIGRATE_COPY_PREFIX),
            (TMP_MIGRATE_MOVED_PREFIX, TMP_MIGRATE_COPY_PREFIX),
        ] {
            assert!(!a.starts_with(b) && !b.starts_with(a), "{a} vs {b}");
        }
    }
}
