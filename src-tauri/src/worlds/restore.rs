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

/// Recover the world folder name from a `.tmp-restoring-<world>-<n>` directory
/// name. `None` for anything that is not one of ours — including a staging
/// directory, which holds extracted backup bytes rather than a world.
///
/// Unused in production until the follow-up PR surfaces stranded worlds in the
/// UI. It lives here, now, because it is the *reader* for the naming decision
/// `claim_stage` makes: embedding the world folder is only justified if it can
/// be read back, and the round-trip test below is what proves it. Splitting the
/// two across PRs would leave the decision unpinned in this one.
#[allow(dead_code)]
pub(crate) fn world_folder_of_tmp_dir(dir_name: &str) -> Option<String> {
    let rest = dir_name.strip_prefix(TMP_RESTORING_PREFIX)?;
    let (world, n) = rest.rsplit_once('-')?;
    if world.is_empty() || n.is_empty() || !n.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(world.to_string())
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
        // below — `restore_as_copy` gets this wrong and leaks its staging dir on
        // a join error.
        tokio::task::spawn_blocking(move || wzip::extract_zip(&backup_clone, &stage_clone))
            .await
            .map_err(|e| Error::io(staged.stage.display().to_string(), format!("join: {e}")))??;

        // The zip's root is named after the world (put there by backup_world /
        // zip_dir). Verify the expected root is present and is the ONLY
        // top-level entry — a mismatched root must ERROR, never silently leave
        // an empty world.
        let inner = staged.stage.join(world_folder);
        if !inner.is_dir() {
            return Err(corrupt(format!(
                "extract did not produce expected folder '{world_folder}/'"
            )));
        }
        let want = std::ffi::OsStr::new(world_folder);
        let extra = std::fs::read_dir(&staged.stage)
            .map_err(|e| Error::io(staged.stage.display().to_string(), e))?
            .flatten()
            .find(|e| e.file_name() != want);
        if let Some(extra) = extra {
            return Err(corrupt(format!(
                "backup has unexpected root '{}' (expected only '{world_folder}/')",
                extra.file_name().to_string_lossy()
            )));
        }
        Ok(inner)
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

    // Extract zip into a temp staging dir, then rename the inner
    // folder to the chosen name. The zip's root is `<world>/` so the
    // staging dir contains `staging/<world>/...` — we rename that to
    // `saves/<chosen>/`.
    let tmp_extract = saves.join(format!(".tmp-as-copy-{}", &chosen.replace(' ', "_")));
    let _ = std::fs::remove_dir_all(&tmp_extract); // stale from earlier failure
    std::fs::create_dir_all(&tmp_extract)
        .map_err(|e| Error::io(tmp_extract.display().to_string(), e))?;
    let backup_clone = backup_path.to_path_buf();
    let tmp_clone = tmp_extract.clone();
    let result = tokio::task::spawn_blocking(move || wzip::extract_zip(&backup_clone, &tmp_clone))
        .await
        .map_err(|e| Error::io(tmp_extract.display().to_string(), format!("join: {e}")))?;
    if let Err(e) = result {
        let _ = std::fs::remove_dir_all(&tmp_extract);
        return Err(e);
    }
    let inner = tmp_extract.join(world_folder);
    if !inner.is_dir() {
        let _ = std::fs::remove_dir_all(&tmp_extract);
        return Err(Error::BackupCorrupt {
            filename: backup_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("?")
                .into(),
            details: format!("zip root was not '{world_folder}/'"),
        });
    }
    let final_path = saves.join(&chosen);
    std::fs::rename(&inner, &final_path)
        .map_err(|e| Error::io(final_path.display().to_string(), e))?;
    let _ = std::fs::remove_dir_all(&tmp_extract);
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
    async fn restore_replace_rejects_a_mismatched_root_without_moving_the_world() {
        // A backup whose zip root is NOT the world folder must ERROR and leave
        // the original world intact — never silently empty it.
        let (_td, saves, backups_dir) = make_world_with_files("W", &[("marker.txt", b"original")]);
        // Build a backup whose single root folder is "WRONG", not "W".
        let bad_backup = backups_dir.join("2026-05-24T10-00-00.zip");
        let wrong_world = _td.path().join("WRONG");
        fs::create_dir_all(&wrong_world).unwrap();
        fs::write(wrong_world.join("level.dat"), b"x").unwrap();
        wzip::zip_dir(&wrong_world, &bad_backup, "WRONG").unwrap();

        let r = restore_replace(&saves, &backups_dir, &bad_backup, "W").await;
        assert!(
            matches!(r, Err(Error::BackupCorrupt { .. })),
            "mismatched root must error, got: {r:?}"
        );
        // The world never moved — verification happens before anything
        // destructive, so there is no rollback to depend on.
        let marker = saves.join("W").join("marker.txt");
        assert!(marker.is_file(), "the world must not have moved at all");
        assert_eq!(fs::read(&marker).unwrap(), b"original");

        let pre_restore = pre_restore_zips(&backups_dir);
        assert!(
            pre_restore.is_empty(),
            "a backup that never verified must not cost a snapshot, found {pre_restore:?}"
        );

        let leftovers = staging_leftovers(&saves);
        assert!(leftovers.is_empty(), "staging dirs must be cleaned up");
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
}
