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
pub(crate) fn world_folder_of_tmp_dir(dir_name: &str) -> Option<String> {
    let rest = dir_name.strip_prefix(TMP_RESTORING_PREFIX)?;
    let (world, n) = rest.rsplit_once('-')?;
    if world.is_empty() || n.is_empty() || !n.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(world.to_string())
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

    // 1. Auto-pre-restore zip BEFORE any destructive step. Failure
    //    here aborts cleanly — original world untouched.
    let pre_restore_name = format!("pre-restore-{}.zip", Utc::now().format("%Y-%m-%dT%H-%M-%S"));
    let pre_restore_path = backups_dir.join(&pre_restore_name);
    let world_clone = world_path.clone();
    let pre_clone = pre_restore_path.clone();
    let world_folder_owned = world_folder.to_string();
    tokio::task::spawn_blocking(move || {
        wzip::zip_dir(&world_clone, &pre_clone, &world_folder_owned)
    })
    .await
    .map_err(|e| Error::io(pre_restore_path.display().to_string(), format!("join: {e}")))??;

    // 2. Rename world to .tmp-restoring-<random>. Atomic on same vol.
    let tmp_suffix: String = (0..8)
        .map(|_| {
            let n = (Utc::now().timestamp_nanos_opt().unwrap_or(0) as u32) % 16;
            // n % 16 is always a valid base-16 digit. Per CLAUDE.md `.unwrap()` rule.
            std::char::from_digit(n, 16).unwrap()
        })
        .collect();
    let tmp_path = saves.join(format!(".tmp-restoring-{tmp_suffix}"));
    std::fs::rename(&world_path, &tmp_path).map_err(|e| {
        // A running Minecraft holds the world's lock file open — surface the
        // friendly typed WorldInUse instead of a raw IO error. Windows:
        // sharing violation (32) / lock violation (33) / access denied (5).
        if matches!(e.raw_os_error(), Some(5) | Some(32) | Some(33)) {
            Error::WorldInUse {
                folder_name: world_folder.to_string(),
            }
        } else {
            Error::io(world_path.display().to_string(), e)
        }
    })?;

    // 3. Extract the backup into a SEPARATE staging dir, verify it contains
    //    exactly the expected `<world_folder>/` root, then rename that inner
    //    folder over world_path. Mirrors restore_as_copy: a backup whose root
    //    doesn't match must ERROR (and roll back) — never silently leave an
    //    empty world. The old flow extracted straight into saves/ and guarded
    //    with `!world_path.is_dir()`, which was dead because create_dir_all
    //    had just created it, so a mismatched-root backup produced an EMPTY
    //    world instead of an error.
    let stage_path = saves.join(format!(".tmp-restore-stage-{tmp_suffix}"));
    let result = (|| -> Result<()> {
        let _ = std::fs::remove_dir_all(&stage_path); // stale from earlier failure
        std::fs::create_dir_all(&stage_path)
            .map_err(|e| Error::io(stage_path.display().to_string(), e))?;
        // The zip's root is named after the world (put there by
        // backup_world / zip_dir). Extract into staging, then verify the
        // expected root is present and is the ONLY top-level entry.
        wzip::extract_zip(backup_path, &stage_path)?;
        let inner = stage_path.join(world_folder);
        if !inner.is_dir() {
            return Err(Error::BackupCorrupt {
                filename: backup_path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("?")
                    .into(),
                details: format!("extract did not produce expected folder '{world_folder}/'"),
            });
        }
        // Reject a backup whose staging dir carries extra top-level roots — a
        // sign of a malformed/foreign archive; we would otherwise drop them.
        let want = std::ffi::OsStr::new(world_folder);
        let extra = std::fs::read_dir(&stage_path)
            .map_err(|e| Error::io(stage_path.display().to_string(), e))?
            .flatten()
            .find(|e| e.file_name() != want);
        if let Some(extra) = extra {
            return Err(Error::BackupCorrupt {
                filename: backup_path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("?")
                    .into(),
                details: format!(
                    "backup has unexpected root '{}' (expected only '{world_folder}/')",
                    extra.file_name().to_string_lossy()
                ),
            });
        }
        // Move the verified inner folder into place at saves/<world_folder>.
        std::fs::rename(&inner, &world_path)
            .map_err(|e| Error::io(world_path.display().to_string(), e))?;
        Ok(())
    })();

    // The staging dir is scratch: drop it either way.
    let _ = std::fs::remove_dir_all(&stage_path);

    match result {
        Ok(()) => {
            // 4. Drop the tmp. Best-effort: if remove_dir_all errors,
            //    log but don't fail the restore — the live world is
            //    healthy; the tmp dir is cosmetic clutter.
            let _ = std::fs::remove_dir_all(&tmp_path);
            Ok(RestoredWorld {
                final_folder_name: world_folder.into(),
            })
        }
        Err(e) => {
            // Roll back: nuke whatever the move left at world_path, put the
            // original back, bubble the original error.
            let _ = std::fs::remove_dir_all(&world_path);
            let _ = std::fs::rename(&tmp_path, &world_path);
            Err(e)
        }
    }
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
    use std::path::PathBuf;
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
        assert!(s.stage.is_dir(), "the stage must be created, not just named");
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

    #[tokio::test]
    async fn restore_replace_rolls_back_on_extract_failure() {
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

        // Critical assertion: the ORIGINAL world is still intact.
        let marker = saves.join("W").join("marker.txt");
        assert!(marker.is_file(), "world rollback failed");
        assert_eq!(fs::read(&marker).unwrap(), b"original");

        // And the auto-pre-restore zip was created (safety net visible).
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
        assert_eq!(
            pre_restore.len(),
            1,
            "expected exactly one pre-restore zip, found {pre_restore:?}"
        );
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
    async fn restore_replace_errors_on_mismatched_root_and_preserves_world() {
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
        // Original world preserved by rollback.
        let marker = saves.join("W").join("marker.txt");
        assert!(marker.is_file(), "world must be rolled back intact");
        assert_eq!(fs::read(&marker).unwrap(), b"original");
        // No leftover staging dirs in saves/.
        let leftovers: Vec<_> = fs::read_dir(&saves)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                let n = e.file_name().to_string_lossy().to_string();
                n.starts_with(".tmp-restore-stage-") || n.starts_with(".tmp-restoring-")
            })
            .collect();
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
