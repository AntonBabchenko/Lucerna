//! Stage phase (§4.1 steps 1–3, §4.2 step 3): claim a hidden
//! `.tmp-migrate-{moved,copy}-<world>-<n>` directory in the target's `saves/` and put the
//! world in it — by one rename on the move path, or by a verified copy.
//!
//! Synchronous on purpose: the orchestrator runs it under `spawn_blocking`,
//! and every rollback branch is deterministic in a plain `#[test]` through
//! the injected seams (the `restore::swap_in_place` shape). The tree copy is
//! a third seam (`CopyFn`) for the same reason: no filesystem trick fails a
//! copy mid-tree on Windows, Linux AND macOS.
//!
//! No write primitive is named here: `fs::copy` lives in
//! `worlds::import::copy_tree` (the guard-allowlisted caller). This file only
//! creates, renames, removes and reads.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::worlds::import::copy_tree;
use crate::worlds::restore::{TMP_MIGRATE_COPY_PREFIX, TMP_MIGRATE_MOVED_PREFIX};

use super::{MigrationLocations, MigrationMode, MigrationPath, MigrationSeams, Staged};

/// Same bound as `restore::claim_stage`: reaching it means ~64 leaked stages
/// under one `saves/`, i.e. something is deeply wrong.
const MAX_STAGE_CANDIDATES: usize = 64;

/// A tree copy: `copy(src, dst, on_file)` copies `src`'s contents into the
/// EXISTING directory `dst`, calling `on_file(size)` once per regular file.
/// Production passes `copy_tree`; tests inject failures.
pub(crate) type CopyFn<'a> = &'a dyn Fn(&Path, &Path, &mut dyn FnMut(u64)) -> Result<()>;

/// Stage `loc.world_folder` from `loc.src_saves` into a hidden directory under
/// `loc.dst_saves`. `on_bytes(copied_so_far, total)` fires on the copy path
/// only. On every error nothing is left in the target, except the one case
/// that says so: `WorldMigratePartialLeft`.
pub(crate) fn stage_world_at(
    loc: &MigrationLocations,
    mode: MigrationMode,
    on_bytes: &mut dyn FnMut(u64, u64),
    seams: &MigrationSeams,
) -> Result<Staged> {
    stage_world_at_with(loc, mode, on_bytes, seams, &|src, dst, on_file| {
        let mut aggregate = 0u64;
        // No caps (§11.3): the source is a world this launcher already
        // manages. With `u64::MAX` for both, `copy_tree`'s two cap checks
        // are unreachable, so `map_copy_error`'s `WorldImportTooLarge` arm
        // can never fire in production.
        copy_tree(src, dst, &mut aggregate, u64::MAX, u64::MAX, on_file)
    })
}

/// [`stage_world_at`] with the tree copy injected.
pub(crate) fn stage_world_at_with(
    loc: &MigrationLocations,
    mode: MigrationMode,
    on_bytes: &mut dyn FnMut(u64, u64),
    seams: &MigrationSeams,
    copy: CopyFn<'_>,
) -> Result<Staged> {
    let world = loc.src_saves.join(&loc.world_folder);
    // §4.1 step 1: only a readable directory enters staging, so a later
    // `WorldNameUnresolvable` stays true and a copy never lands somewhere
    // `list_worlds` cannot read.
    std::fs::create_dir_all(&loc.dst_saves)
        .map_err(|e| Error::io(loc.dst_saves.display().to_string(), e))?;
    std::fs::read_dir(&loc.dst_saves)
        .map_err(|e| Error::io(loc.dst_saves.display().to_string(), e))?;
    let stage = claim_stage(&loc.dst_saves, &loc.world_folder, stage_prefix(mode))?;

    match mode {
        MigrationMode::Move => {
            // The claim was made with `create_dir`, and Windows refuses to
            // rename a directory ONTO an existing directory (errno 5 — see
            // `restore::map_move_aside_error`). So the empty claim is dropped
            // an instant before the rename consumes the name. That gap is the
            // probe-then-rename gap `restore::claim_stage` already states; A5's
            // maintenance claim keeps every other launcher writer off this
            // `saves/` meanwhile.
            if let Err(e) = std::fs::remove_dir(&stage) {
                return Err(Error::io(
                    stage.display().to_string(),
                    format!("could not release the claimed stage name before the move: {e}"),
                ));
            }
            match (seams.rename)(&world, &stage) {
                Ok(()) => Ok(Staged {
                    stage,
                    path: MigrationPath::Renamed,
                    links_skipped: 0,
                }),
                Err(e) => move_rename_failed(loc, &world, stage, e, on_bytes, seams, copy),
            }
        }
        MigrationMode::Copy => {
            let links_skipped = match count_symlinks(&world) {
                Ok(n) => n,
                Err(e) => return Err(discard_empty_stage(&stage, e)),
            };
            copy_into_stage(loc, &world, stage, links_skipped, on_bytes, seams, copy)
        }
    }
}

/// Which stage prefix a mode claims: a moved world is the user's ONLY copy and
/// must be recoverable (`.tmp-migrate-moved-`, listed by `stranded_worlds_at`);
/// a copy-path stage may be a partial tree and must never be offered back
/// (`.tmp-migrate-copy-`, never listed). Reader: `restore::parked_world_of_tmp_dir`.
fn stage_prefix(mode: MigrationMode) -> &'static str {
    match mode {
        MigrationMode::Move => TMP_MIGRATE_MOVED_PREFIX,
        MigrationMode::Copy => TMP_MIGRATE_COPY_PREFIX,
    }
}

/// `create_dir`, not `create_dir_all`: a colliding name fails `AlreadyExists`,
/// which is this loop's "next candidate". Nothing here ever deletes a
/// candidate — a `.tmp-migrate-*` may be a previous run's stranded world.
fn claim_stage(saves: &Path, world_folder: &str, prefix: &str) -> Result<PathBuf> {
    for n in 0..MAX_STAGE_CANDIDATES {
        let stage = saves.join(format!("{prefix}{world_folder}-{n}"));
        match std::fs::create_dir(&stage) {
            Ok(()) => return Ok(stage),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(Error::io(stage.display().to_string(), e)),
        }
    }
    Err(Error::io(
        saves.display().to_string(),
        format!("could not allocate a migration stage after {MAX_STAGE_CANDIDATES} attempts"),
    ))
}

/// The move's rename failed and NOTHING was renamed (the stage name was
/// released just before). Cross-device ⇒ the copy fallback; a held-open world
/// ⇒ `WorldInUse`; anything else ⇒ `Io`.
fn move_rename_failed(
    loc: &MigrationLocations,
    world: &Path,
    stage: PathBuf,
    e: std::io::Error,
    on_bytes: &mut dyn FnMut(u64, u64),
    seams: &MigrationSeams,
    copy: CopyFn<'_>,
) -> Result<Staged> {
    if is_cross_device(&e) {
        crate::diag!(
            "world migration: rename {} -> {} crossed a device ({e}); falling back to copy",
            world.display(),
            stage.display()
        );
        // The moved-prefix name was released just before the rename attempt
        // and stays free. The fallback fills a COPY-prefixed stage instead: a
        // copy-path stage may be partial and must never be listed as a
        // recoverable world, whatever mode the user chose.
        let copy_stage = claim_stage(&loc.dst_saves, &loc.world_folder, TMP_MIGRATE_COPY_PREFIX)?;
        return copy_fallback(loc, world, copy_stage, on_bytes, seams, copy);
    }
    // Discriminate an occupied destination from a held-open world before
    // blaming Minecraft — `restore::map_move_aside_error`'s rule. `try_exists`:
    // a stat failure reads as occupied, the restrictive direction.
    if stage.try_exists().unwrap_or(true) {
        return Err(Error::io(
            stage.display().to_string(),
            format!("staging name was taken between reservation and use: {e}"),
        ));
    }
    if is_in_use(&e) {
        return Err(Error::WorldInUse {
            folder_name: loc.world_folder.clone(),
        });
    }
    Err(Error::io(world.display().to_string(), e))
}

/// §4.2 step 3 fallback: a world containing a link is refused BEFORE any byte
/// is copied — a cross-volume move must not silently drop a link's content.
fn copy_fallback(
    loc: &MigrationLocations,
    world: &Path,
    stage: PathBuf,
    on_bytes: &mut dyn FnMut(u64, u64),
    seams: &MigrationSeams,
    copy: CopyFn<'_>,
) -> Result<Staged> {
    match crate::data_root::migrate::contains_reparse_point(world) {
        Ok(false) => copy_into_stage(loc, world, stage, 0, on_bytes, seams, copy),
        Ok(true) => Err(discard_empty_stage(
            &stage,
            Error::io(
                world.display().to_string(),
                "the world contains a symbolic link or junction; a move across volumes would drop its content — move it by hand",
            ),
        )),
        // Could not tell whether the world contains a link: refuse. A silent
        // drop is the failure this check exists to prevent (Fallback Q1/Q2).
        Err(scan) => Err(discard_empty_stage(&stage, scan)),
    }
}

/// Copy the tree into the (empty, claimed) stage, verify it, and roll the
/// stage back on any failure.
fn copy_into_stage(
    loc: &MigrationLocations,
    world: &Path,
    stage: PathBuf,
    links_skipped: u32,
    on_bytes: &mut dyn FnMut(u64, u64),
    seams: &MigrationSeams,
    copy: CopyFn<'_>,
) -> Result<Staged> {
    let (total, _mtime) = match crate::worlds::fs::dir_size_and_mtime(world) {
        Ok(t) => t,
        Err(e) => return Err(discard_empty_stage(&stage, e)),
    };
    let mut copied: u64 = 0;
    let copied_result = copy(world, &stage, &mut |size| {
        copied = copied.saturating_add(size);
        on_bytes(copied, total);
    });
    match copied_result.and_then(|()| verify_stage(world, &stage)) {
        Ok(()) => Ok(Staged {
            stage,
            path: MigrationPath::Copied,
            links_skipped,
        }),
        Err(e) => Err(roll_back_copy(
            loc,
            &stage,
            map_copy_error(e, world, &loc.world_folder),
            seams,
        )),
    }
}

/// Remove the partial stage through the injected seam. The removal's own
/// result is checked (Fallback discipline Q4): a stage that survives is a
/// `.tmp-migrate-copy-*` nothing lists (a copy-path stage is never offered back),
/// so the user must be told where it is and that it is incomplete — `WorldMigratePartialLeft`,
/// naming the stage and the TARGET instance, never the source.
fn roll_back_copy(
    loc: &MigrationLocations,
    stage: &Path,
    cause: Error,
    seams: &MigrationSeams,
) -> Error {
    match (seams.remove)(stage) {
        Ok(()) => cause,
        // Already gone (removed externally between the failed copy and the
        // rollback): nothing is left behind — same discrimination as
        // `import::copy_world_with_rollback`.
        Err(rb) if rb.kind() == std::io::ErrorKind::NotFound => cause,
        Err(rb) => {
            crate::diag!(
                "world migration: copy into {} failed ({cause}) AND rollback failed ({rb}); partial copy left",
                stage.display()
            );
            Error::WorldMigratePartialLeft {
                folder_name: dir_name_of(stage),
                target_instance: loc.target_instance_name.clone(),
                only_copy: false, // copy path: the source is intact
            }
        }
    }
}

/// Drop a stage that is still EMPTY (nothing was copied into it) after a
/// refusal, keeping `cause` as the answer. Checked, not best-effort: an empty
/// `.tmp-migrate-moved-*` left behind would be listed by `stranded_worlds_at` as a
/// recoverable world, which it is not — so a failed removal is folded into an
/// `Io` that names the folder, and both causes go to Logs.
fn discard_empty_stage(stage: &Path, cause: Error) -> Error {
    match std::fs::remove_dir(stage) {
        Ok(()) => cause,
        // Already gone: nothing is left behind to tell the user about.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => cause,
        Err(e) => {
            crate::diag!(
                "world migration: refused ({cause}) AND the empty stage {} could not be removed ({e})",
                stage.display()
            );
            Error::io(
                stage.display().to_string(),
                format!(
                    "{cause}; the empty staging folder {} could not be removed ({e}) — delete it by hand",
                    dir_name_of(stage)
                ),
            )
        }
    }
}

/// Read-only size verification of the copy, mirroring `copy_tree`'s own
/// classification: symlinks are skipped at EVERY depth (`copy_tree` skips
/// them at every depth, and they are reported in `links_skipped`), only
/// regular files are compared. `data_root::migrate::verify_copy` was not
/// reused because its `skip` applies at the tree root only.
fn verify_stage(src: &Path, dst: &Path) -> Result<()> {
    for entry in std::fs::read_dir(src).map_err(|e| Error::io(src.display().to_string(), e))? {
        let entry = entry.map_err(|e| Error::io(src.display().to_string(), e))?;
        let ft = entry
            .file_type()
            .map_err(|e| Error::io(entry.path().display().to_string(), e))?;
        if ft.is_symlink() {
            continue;
        }
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if ft.is_dir() {
            verify_stage(&from, &to)?;
            continue;
        }
        if !ft.is_file() {
            continue;
        }
        let src_len = std::fs::metadata(&from)
            .map_err(|e| Error::io(from.display().to_string(), e))?
            .len();
        let dst_len = std::fs::metadata(&to)
            .map_err(|e| Error::io(to.display().to_string(), e))?
            .len();
        if src_len != dst_len {
            return Err(Error::io(
                to.display().to_string(),
                format!("copy verification failed: the source is {src_len} bytes but the copy is {dst_len} bytes"),
            ));
        }
    }
    Ok(())
}

/// Number of symlink entries under `root` at any depth — exactly the entries
/// `copy_tree` will skip (it tests `file_type().is_symlink()` per entry and
/// never follows one). A read-only walk of the SOURCE, taken before the copy.
fn count_symlinks(root: &Path) -> Result<u32> {
    let mut n: u32 = 0;
    for entry in std::fs::read_dir(root).map_err(|e| Error::io(root.display().to_string(), e))? {
        let entry = entry.map_err(|e| Error::io(root.display().to_string(), e))?;
        let ft = entry
            .file_type()
            .map_err(|e| Error::io(entry.path().display().to_string(), e))?;
        if ft.is_symlink() {
            n = n.saturating_add(1);
        } else if ft.is_dir() {
            n = n.saturating_add(count_symlinks(&entry.path())?);
        }
    }
    Ok(n)
}

/// `copy_tree` stringifies the io error into `Error::Io { details }`, so the
/// errno is recovered from std's stable `(os error N)` suffix. Windows only:
/// 5/32/33 are the codes a held-open file surfaces there (the `delete_world`
/// / `level_dat::map_read_err` mapping); on POSIX errno 5 is EIO and
/// "quit Minecraft and try again" would be a false statement.
fn map_copy_error(e: Error, world: &Path, world_folder: &str) -> Error {
    match e {
        Error::Io { ref details, .. } if cfg!(windows) && has_os_error(details, &[5, 32, 33]) => {
            Error::WorldInUse {
                folder_name: world_folder.to_string(),
            }
        }
        // Unreachable with `u64::MAX` caps (see `stage_world_at`); mapped
        // rather than `unreachable!` so an injected copy seam that returns
        // it still yields an honest error — "too large to import" is not.
        Error::WorldImportTooLarge { .. } => Error::io(
            world.display().to_string(),
            "the tree copy reported a size cap although migration sets none",
        ),
        other => other,
    }
}

fn has_os_error(details: &str, codes: &[i32]) -> bool {
    codes
        .iter()
        .any(|c| details.ends_with(&format!("(os error {c})")))
}

/// EXDEV (18) on POSIX, ERROR_NOT_SAME_DEVICE (17) on Windows — both decode
/// to `CrossesDevices`; the raw codes are kept as a belt-and-braces check.
fn is_cross_device(e: &std::io::Error) -> bool {
    if e.kind() == std::io::ErrorKind::CrossesDevices {
        return true;
    }
    let raw = e.raw_os_error();
    if cfg!(windows) {
        raw == Some(17)
    } else {
        raw == Some(18)
    }
}

/// Windows: access denied (5) / sharing violation (32) / lock violation (33)
/// — a running Minecraft holding the world's files open. Same codes as
/// `worlds::delete_world` and `restore::map_move_aside_error`.
fn is_in_use(e: &std::io::Error) -> bool {
    matches!(e.raw_os_error(), Some(5) | Some(32) | Some(33))
}

/// The stage's bare directory NAME — a segment inside a translated sentence,
/// never a full path (the `WorldRestoreStranded` rule).
fn dir_name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Arc;
    use tempfile::tempdir;

    struct Fx {
        _td: tempfile::TempDir,
        loc: MigrationLocations,
        world: PathBuf,
    }

    fn fixture() -> Fx {
        let td = tempdir().unwrap();
        let src_root = td.path().join("Src");
        let dst_root = td.path().join("Dst");
        let loc = MigrationLocations {
            src_saves: src_root.join(".minecraft").join("saves"),
            src_backups_root: src_root.join("backups"),
            src_root,
            dst_saves: dst_root.join(".minecraft").join("saves"),
            dst_backups_root: dst_root.join("backups"),
            dst_root,
            world_folder: "Survival".into(),
            target_instance_name: "Target".into(),
        };
        let world = loc.src_saves.join("Survival");
        fs::create_dir_all(world.join("region")).unwrap();
        fs::write(world.join("level.dat"), b"level").unwrap();
        fs::write(world.join("region").join("r.0.0.mca"), b"region-bytes").unwrap();
        Fx {
            _td: td,
            loc,
            world,
        }
    }

    /// Sorted (relative path, bytes) of every regular file under `root`.
    fn tree(root: &Path) -> Vec<(String, Vec<u8>)> {
        fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
            for e in fs::read_dir(dir).unwrap() {
                let p = e.unwrap().path();
                let ft = fs::symlink_metadata(&p).unwrap().file_type();
                if ft.is_dir() {
                    walk(root, &p, out);
                } else if ft.is_file() {
                    let rel = p
                        .strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/");
                    out.push((rel, fs::read(&p).unwrap()));
                }
            }
        }
        let mut out = Vec::new();
        walk(root, root, &mut out);
        out.sort();
        out
    }

    fn stages_in(saves: &Path) -> Vec<String> {
        let Ok(rd) = fs::read_dir(saves) else {
            return Vec::new();
        };
        let mut v: Vec<String> = rd
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| n.starts_with(".tmp-migrate-"))
            .collect();
        v.sort();
        v
    }

    fn seams(
        rename: impl Fn(&Path, &Path) -> std::io::Result<()> + Send + Sync + 'static,
        remove: impl Fn(&Path) -> std::io::Result<()> + Send + Sync + 'static,
    ) -> MigrationSeams {
        MigrationSeams {
            rename: Arc::new(rename),
            remove: Arc::new(remove),
        }
    }

    fn real_copy(src: &Path, dst: &Path, on_file: &mut dyn FnMut(u64)) -> Result<()> {
        let mut aggregate = 0u64;
        copy_tree(src, dst, &mut aggregate, u64::MAX, u64::MAX, on_file)
    }

    #[test]
    fn copy_stages_the_tree_and_leaves_the_source_intact() {
        let fx = fixture();
        let before = tree(&fx.world);
        let mut calls: Vec<(u64, u64)> = Vec::new();
        let staged = stage_world_at(
            &fx.loc,
            MigrationMode::Copy,
            &mut |c, t| calls.push((c, t)),
            &MigrationSeams::real(),
        )
        .unwrap();
        assert_eq!(staged.path, MigrationPath::Copied);
        assert_eq!(staged.links_skipped, 0);
        assert!(staged.stage.starts_with(&fx.loc.dst_saves));
        assert!(dir_name_of(&staged.stage).starts_with(TMP_MIGRATE_COPY_PREFIX));
        assert_eq!(tree(&staged.stage), before);
        assert_eq!(tree(&fx.world), before, "the source must be untouched");
        let total = (b"level".len() + b"region-bytes".len()) as u64;
        assert_eq!(calls.last(), Some(&(total, total)), "calls: {calls:?}");
    }

    #[test]
    fn move_renames_into_the_stage_on_one_volume() {
        let fx = fixture();
        let before = tree(&fx.world);
        let mut calls = 0u32;
        let staged = stage_world_at(
            &fx.loc,
            MigrationMode::Move,
            &mut |_, _| calls += 1,
            &MigrationSeams::real(),
        )
        .unwrap();
        assert_eq!(staged.path, MigrationPath::Renamed);
        assert!(
            !fx.world.exists(),
            "the source slot must be empty after a rename"
        );
        assert_eq!(tree(&staged.stage), before);
        assert_eq!(calls, 0, "the rename path reports no bytes");
    }

    #[test]
    fn move_falls_back_to_copy_across_devices() {
        let fx = fixture();
        let before = tree(&fx.world);
        let s = seams(
            |_, _| Err(std::io::Error::from(std::io::ErrorKind::CrossesDevices)),
            |p| fs::remove_dir_all(p),
        );
        let staged = stage_world_at(&fx.loc, MigrationMode::Move, &mut |_, _| {}, &s).unwrap();
        assert_eq!(staged.path, MigrationPath::Copied);
        assert_eq!(tree(&staged.stage), before);
        assert_eq!(
            tree(&fx.world),
            before,
            "the caller removes the source later"
        );
    }

    #[cfg(unix)]
    #[test]
    fn move_refuses_the_copy_fallback_for_a_world_containing_a_link() {
        let fx = fixture();
        std::os::unix::fs::symlink(fx.world.join("level.dat"), fx.world.join("link")).unwrap();
        let s = seams(
            |_, _| Err(std::io::Error::from_raw_os_error(18)),
            |p| fs::remove_dir_all(p),
        );
        let err = stage_world_at(&fx.loc, MigrationMode::Move, &mut |_, _| {}, &s).unwrap_err();
        match err {
            Error::Io { details, .. } => assert!(details.contains("symbolic link"), "{details}"),
            other => panic!("expected Io, got {other:?}"),
        }
        assert!(
            stages_in(&fx.loc.dst_saves).is_empty(),
            "no stage may survive a refusal"
        );
        assert!(fx.world.join("level.dat").is_file());
    }

    #[test]
    fn move_reports_world_in_use_and_leaves_nothing_behind() {
        let fx = fixture();
        let before = tree(&fx.world);
        let s = seams(
            |_, _| Err(std::io::Error::from_raw_os_error(32)),
            |p| fs::remove_dir_all(p),
        );
        let err = stage_world_at(&fx.loc, MigrationMode::Move, &mut |_, _| {}, &s).unwrap_err();
        match err {
            Error::WorldInUse { folder_name } => assert_eq!(folder_name, "Survival"),
            other => panic!("expected WorldInUse, got {other:?}"),
        }
        assert!(stages_in(&fx.loc.dst_saves).is_empty());
        assert_eq!(tree(&fx.world), before);
    }

    #[test]
    fn move_maps_any_other_rename_failure_to_io_with_nothing_changed() {
        let fx = fixture();
        let s = seams(
            |_, _| Err(std::io::Error::other("forced rename failure")),
            |p| fs::remove_dir_all(p),
        );
        let err = stage_world_at(&fx.loc, MigrationMode::Move, &mut |_, _| {}, &s).unwrap_err();
        assert!(matches!(err, Error::Io { .. }), "got {err:?}");
        assert!(stages_in(&fx.loc.dst_saves).is_empty());
        assert!(fx.world.join("level.dat").is_file());
    }

    #[test]
    fn claim_skips_a_taken_candidate_and_never_deletes_it() {
        let fx = fixture();
        let taken = fx
            .loc
            .dst_saves
            .join(format!("{TMP_MIGRATE_COPY_PREFIX}Survival-0"));
        fs::create_dir_all(&taken).unwrap();
        fs::write(taken.join("marker"), b"someone else's").unwrap();
        let staged = stage_world_at(
            &fx.loc,
            MigrationMode::Copy,
            &mut |_, _| {},
            &MigrationSeams::real(),
        )
        .unwrap();
        assert_eq!(
            dir_name_of(&staged.stage),
            format!("{TMP_MIGRATE_COPY_PREFIX}Survival-1")
        );
        assert_eq!(fs::read(taken.join("marker")).unwrap(), b"someone else's");
    }

    #[test]
    fn a_copy_failure_rolls_the_stage_back_and_returns_the_copy_error() {
        let fx = fixture();
        let err = stage_world_at_with(
            &fx.loc,
            MigrationMode::Copy,
            &mut |_, _| {},
            &MigrationSeams::real(),
            &|_, _, _| Err(Error::io("injected", "copy failure injected by the test")),
        )
        .unwrap_err();
        assert!(matches!(err, Error::Io { .. }), "got {err:?}");
        assert!(
            stages_in(&fx.loc.dst_saves).is_empty(),
            "the stage must be rolled back"
        );
        assert!(fx.world.join("level.dat").is_file());
    }

    #[test]
    fn a_copy_failure_whose_rollback_fails_is_partial_left_naming_the_stage_and_target() {
        let fx = fixture();
        let s = seams(
            |a, b| fs::rename(a, b),
            |_| Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
        );
        let err = stage_world_at_with(
            &fx.loc,
            MigrationMode::Copy,
            &mut |_, _| {},
            &s,
            &|_, _, _| Err(Error::io("injected", "copy failure injected by the test")),
        )
        .unwrap_err();
        match err {
            Error::WorldMigratePartialLeft {
                folder_name,
                target_instance,
                only_copy,
            } => {
                assert_eq!(folder_name, format!("{TMP_MIGRATE_COPY_PREFIX}Survival-0"));
                assert_eq!(target_instance, "Target");
                assert!(!only_copy, "a copy-path stage is never the only copy");
                assert!(!folder_name.contains(std::path::MAIN_SEPARATOR));
            }
            other => panic!("expected WorldMigratePartialLeft, got {other:?}"),
        }
        assert_eq!(
            stages_in(&fx.loc.dst_saves),
            vec![format!("{TMP_MIGRATE_COPY_PREFIX}Survival-0")],
            "the stage really is still there — that is the state being reported"
        );
    }

    #[test]
    fn a_rollback_that_finds_the_stage_already_gone_keeps_the_copy_error() {
        let fx = fixture();
        let s = seams(
            |a, b| fs::rename(a, b),
            |_| Err(std::io::Error::from(std::io::ErrorKind::NotFound)),
        );
        let err = stage_world_at_with(
            &fx.loc,
            MigrationMode::Copy,
            &mut |_, _| {},
            &s,
            &|_, _, _| Err(Error::io("injected", "copy failure injected by the test")),
        )
        .unwrap_err();
        assert!(matches!(err, Error::Io { .. }), "got {err:?}");
    }

    #[test]
    fn an_incomplete_copy_fails_verification_and_rolls_back() {
        let fx = fixture();
        // A copy that reports success without writing a byte.
        let err = stage_world_at_with(
            &fx.loc,
            MigrationMode::Copy,
            &mut |_, _| {},
            &MigrationSeams::real(),
            &|_, _, _| Ok(()),
        )
        .unwrap_err();
        assert!(matches!(err, Error::Io { .. }), "got {err:?}");
        assert!(stages_in(&fx.loc.dst_saves).is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn a_locked_file_during_the_copy_reads_as_world_in_use() {
        let fx = fixture();
        let err = stage_world_at_with(
            &fx.loc,
            MigrationMode::Copy,
            &mut |_, _| {},
            &MigrationSeams::real(),
            &|_, _, _| Err(Error::io("region", std::io::Error::from_raw_os_error(32))),
        )
        .unwrap_err();
        assert!(matches!(err, Error::WorldInUse { .. }), "got {err:?}");
        assert!(stages_in(&fx.loc.dst_saves).is_empty());
    }

    #[test]
    fn the_import_size_cap_can_never_escape_a_migration() {
        let fx = fixture();
        let err = stage_world_at_with(
            &fx.loc,
            MigrationMode::Copy,
            &mut |_, _| {},
            &MigrationSeams::real(),
            &|_, _, _| {
                Err(Error::WorldImportTooLarge {
                    size: 2.0,
                    cap: 1.0,
                })
            },
        )
        .unwrap_err();
        assert!(matches!(err, Error::Io { .. }), "got {err:?}");
    }

    #[cfg(unix)]
    #[test]
    fn copy_counts_the_symlinks_it_skips_at_every_depth() {
        let fx = fixture();
        std::os::unix::fs::symlink(fx.world.join("level.dat"), fx.world.join("top-link")).unwrap();
        std::os::unix::fs::symlink(
            fx.world.join("level.dat"),
            fx.world.join("region").join("nested-link"),
        )
        .unwrap();
        let staged = stage_world_at_with(
            &fx.loc,
            MigrationMode::Copy,
            &mut |_, _| {},
            &MigrationSeams::real(),
            &real_copy,
        )
        .unwrap();
        assert_eq!(staged.links_skipped, 2);
        assert!(!staged.stage.join("top-link").exists());
        assert!(!staged.stage.join("region").join("nested-link").exists());
        assert!(staged.stage.join("region").join("r.0.0.mca").is_file());
    }
}
