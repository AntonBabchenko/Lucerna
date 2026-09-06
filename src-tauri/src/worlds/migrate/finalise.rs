//! Phase 4 of a world migration — the rename that makes the staged world
//! appear under its final name — and, for a Move that had to copy, the removal
//! of the source world afterwards.
//!
//! `finalise_at`'s successful rename is the POINT OF NO RETURN (spec §4.2, A4).
//! Before it, every failure rolls the stage back and is an error; after it the
//! world is complete in the target and nothing here ever undoes that, which is
//! why `remove_source_after_copy_at` returns a `SourceState` and never an error.
//!
//! No write primitive is named here. `rename` and `remove_dir_all` go through
//! the injected seams (the `restore::swap_in_place` shape), so every rollback
//! branch is a plain `#[test]` on Windows, Linux and macOS alike.

use super::{MigrationLocations, MigrationPath, MigrationSeams, SourceState, Staged};
use crate::error::{Error, Result};
use std::path::Path;

/// Bound on final-rename attempts. `pick_free_world_name` stops at ` (999)` by
/// itself; this bound only matters if the filesystem keeps calling a name taken
/// that the probe called free, and turns that into `WorldNameUnresolvable`
/// instead of a spin.
const MAX_FINALISE_ATTEMPTS: usize = 999;

/// Rename the stage to its final name under `dst_saves` and return that name.
///
/// The name is chosen by `pick_free_world_name` immediately before each
/// attempt; a rename refused because the name appeared in the probe → rename
/// gap moves on to the next suffix. Any other failure rolls the stage back
/// (`roll_back_stage`) and is returned — errno 5/32/33 as `WorldInUse`, whose
/// "try again" is true because the source slot is whole again, anything else
/// as `Io`. On Linux `rename(2)` replaces an EMPTY directory of the target
/// name; an empty `saves/<name>` is not a world and the gap is microseconds —
/// stated, not closed, as `import.rs` states its own.
pub(crate) fn finalise_at(
    loc: &MigrationLocations,
    staged: &Staged,
    seams: &MigrationSeams,
) -> Result<String> {
    match place(loc, &staged.stage, seams) {
        Ok(final_name) => Ok(final_name),
        Err(cause) => Err(roll_back_stage(loc, staged, seams, cause)),
    }
}

fn place(loc: &MigrationLocations, stage: &Path, seams: &MigrationSeams) -> Result<String> {
    for _ in 0..MAX_FINALISE_ATTEMPTS {
        let final_name =
            crate::worlds::import::pick_free_world_name(&loc.dst_saves, &loc.world_folder)?;
        let dest = loc.dst_saves.join(&final_name);
        match (seams.rename)(stage, &dest) {
            Ok(()) => return Ok(final_name),
            // The name appeared between the probe and the rename: the next
            // probe sees it as taken and moves on to the next suffix.
            Err(e)
                if is_name_taken(&e) || (e.raw_os_error() == Some(5) && name_appeared(&dest)) =>
            {
                continue;
            }
            Err(e) => return Err(map_rename_failure(&loc.world_folder, &dest, e)),
        }
    }
    Err(Error::WorldNameUnresolvable {
        folder_name: loc.world_folder.clone(),
    })
}

/// `rename` refused because something already sits at the destination. The
/// kind is `AlreadyExists` for a file on every platform; a NON-EMPTY directory
/// reports `ENOTEMPTY` (39 Linux, 66 macOS/BSD) or `EEXIST` (17) on unix and
/// `ERROR_ALREADY_EXISTS` (183) / `ERROR_FILE_EXISTS` (80) on Windows, which
/// std does not all map to a kind. Anything else is a real failure.
fn is_name_taken(e: &std::io::Error) -> bool {
    if e.kind() == std::io::ErrorKind::AlreadyExists {
        return true;
    }
    let Some(code) = e.raw_os_error() else {
        return false;
    };
    if cfg!(windows) {
        matches!(code, 80 | 183)
    } else if cfg!(target_os = "linux") {
        matches!(code, 17 | 39)
    } else {
        matches!(code, 17 | 66)
    }
}

/// Windows reports a rename onto an existing directory as `ERROR_ACCESS_DENIED`
/// (5) — the same code a held-open world produces. The destination tells them
/// apart: if it now exists, the name was taken in the gap. Fallback direction:
/// "could not tell" reads as NOT taken, so a stat failure can never spin the
/// loop; the error then surfaces as `WorldInUse` and the rollback runs.
fn name_appeared(dest: &Path) -> bool {
    dest.try_exists().unwrap_or(false)
}

/// errno 5/32/33 is a running Minecraft holding the world open — the mapping
/// `worlds::remove_world_dir_at` uses. Valid here only because the caller rolls
/// the stage back first: `WorldInUse`'s copy says "try again".
fn map_rename_failure(world_folder: &str, dest: &Path, e: std::io::Error) -> Error {
    if matches!(e.raw_os_error(), Some(5) | Some(32) | Some(33)) {
        Error::WorldInUse {
            folder_name: world_folder.to_string(),
        }
    } else {
        Error::io(dest.display().to_string(), e)
    }
}

/// Undo a stage before the point of no return and return the error the caller
/// surfaces. `Renamed` ⇒ the stage holds the user's ONLY copy and is renamed
/// back into the source slot (free: the maintenance claim refuses every other
/// writer meanwhile). `Copied` ⇒ the source is intact; the stage is removed.
///
/// The rollback's own result is checked (CLAUDE.md, Fallback discipline, q. 4).
/// When it fails the returned error is `WorldMigratePartialLeft`, naming the
/// stage directory and the target instance, with `only_copy` telling the UI
/// whether the directory is garbage (copy path) or the world itself (rename
/// path — "do not delete it"). Both causes are `diag!`-logged here, where the
/// full context exists; the variant carries neither.
///
/// A `NotFound` from the rename-back is deliberately NOT read as "nothing to
/// do": on the rename path a missing stage is the user's only copy missing,
/// and pointing the user at the stranded-worlds list (where it may yet appear
/// once the filesystem settles) is the one answer that destroys nothing.
pub(crate) fn roll_back_stage(
    loc: &MigrationLocations,
    staged: &Staged,
    seams: &MigrationSeams,
    cause: Error,
) -> Error {
    let stage_name = staged
        .stage
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let only_copy = matches!(staged.path, MigrationPath::Renamed);
    let rollback = if only_copy {
        (seams.rename)(&staged.stage, &loc.src_saves.join(&loc.world_folder))
    } else {
        (seams.remove)(&staged.stage)
    };
    match rollback {
        Ok(()) => {
            crate::diag!("world migrate: failed before the point of no return ({cause}); stage {stage_name} rolled back");
            cause
        }
        // Copy path only: the stage is already gone, so nothing is left behind
        // — the discrimination `import::copy_world_with_rollback` makes.
        Err(rb) if !only_copy && rb.kind() == std::io::ErrorKind::NotFound => {
            crate::diag!("world migrate: failed before the point of no return ({cause}); stage {stage_name} was already gone ({rb})");
            cause
        }
        Err(rb) => {
            crate::diag!(
                "world migrate: failed before the point of no return ({cause}) AND rollback of stage {stage_name} failed ({rb}); only_copy={only_copy}"
            );
            Error::WorldMigratePartialLeft {
                folder_name: stage_name,
                target_instance: loc.target_instance_name.clone(),
                only_copy,
            }
        }
    }
}

/// Move, copy fallback only: remove the source world now that the target holds
/// the complete copy under `final_name`. Runs AFTER the point of no return, so
/// it never returns an error — every failure is a `SourceState` the toast can
/// state honestly ("the world is in <target>; <source> still holds …").
///
/// Re-verifies first (defence in depth behind the maintenance claim) that the
/// target's `instance.json`, the target world directory and — when the SOURCE
/// world has one — the target world's `level.dat` still exist, with
/// `try_exists`: absent AND "could not tell" both keep the source
/// (`LeftIntact`) — the restrictive direction, because a Move that keeps its
/// source is a Copy the user can finish by hand, while a removed source with
/// no verified target is a lost world. `level.dat` is demanded of the target
/// only when the source has one (the source is intact on this path): §6
/// accepts a world without it (delete and backup accept such worlds), and
/// demanding it unconditionally would keep that world's source forever — a
/// Move that can never complete. The reason names the file that was missing.
///
/// The removal goes through `seams.remove` rather than `remove_world_dir_at`:
/// the errno → `WorldInUse` mapping is exactly what must NOT be reported after
/// the point of no return, and the seam makes the failure injectable.
///
/// After a failed removal the state is decided from the tree itself, never
/// from the error text: gone ⇒ `Removed`; still there with as many entries as
/// before ⇒ `LeftIntact`; fewer entries, or uncountable ⇒ `LeftPartial`, the
/// pessimistic reading. `reason` is io text for Logs, not user copy.
pub(crate) fn remove_source_after_copy_at(
    loc: &MigrationLocations,
    final_name: &str,
    seams: &MigrationSeams,
) -> SourceState {
    let source = loc.src_saves.join(&loc.world_folder);
    let world_dir = loc.dst_saves.join(final_name);
    let mut required = vec![loc.dst_root.join("instance.json"), world_dir.clone()];
    // The target must hold a `level.dat` exactly when the source does: a
    // world without one is verified by its directory alone.
    match source.join("level.dat").try_exists() {
        Ok(true) => required.push(world_dir.join("level.dat")),
        Ok(false) => {}
        // Could not tell what the source holds: keep it (restrictive), and
        // say why.
        Err(e) => {
            return SourceState::LeftIntact {
                reason: format!(
                    "{} could not be checked ({e}); source kept",
                    source.display()
                ),
            };
        }
    }
    for path in &required {
        match path.try_exists() {
            Ok(true) => {}
            Ok(false) => {
                return SourceState::LeftIntact {
                    reason: format!("{} is missing; source kept", path.display()),
                };
            }
            Err(e) => {
                return SourceState::LeftIntact {
                    reason: format!("{} could not be checked ({e}); source kept", path.display()),
                };
            }
        }
    }
    let before = count_entries(&source);
    match (seams.remove)(&source) {
        Ok(()) => SourceState::Removed,
        Err(e) => state_after_failed_removal(&source, before, &e),
    }
}

fn state_after_failed_removal(
    source: &Path,
    before: Option<u64>,
    cause: &std::io::Error,
) -> SourceState {
    let reason = format!("{}: {cause}", source.display());
    match source.try_exists() {
        // Gone after all (removed underneath the failing call): the tree, not
        // the error, decides.
        Ok(false) => {
            crate::diag!(
                "world migrate: source removal reported {cause} but the directory is gone"
            );
            SourceState::Removed
        }
        Ok(true) => match (before, count_entries(source)) {
            (Some(b), Some(a)) if a == b => SourceState::LeftIntact { reason },
            _ => SourceState::LeftPartial { reason },
        },
        Err(e) => SourceState::LeftPartial {
            reason: format!("{reason}; state unknown: {e}"),
        },
    }
}

/// Entries (files and directories, recursively, symlinks not followed) under
/// `dir`. `None` when any listing fails: "could not count" is kept apart from
/// "zero" so the caller can pick the pessimistic state.
fn count_entries(dir: &Path) -> Option<u64> {
    let mut n = 0u64;
    let mut pending = vec![dir.to_path_buf()];
    while let Some(d) = pending.pop() {
        for entry in std::fs::read_dir(&d).ok()? {
            let entry = entry.ok()?;
            n += 1;
            if entry.file_type().ok()?.is_dir() {
                pending.push(entry.path());
            }
        }
    }
    Some(n)
}

#[cfg(test)]
pub mod fixture {
    //! Shared by this file's tests and by `migrate::orchestrator_tests`.
    //! `pub mod`, not `pub(crate) mod`: `structural_no_inplace_mods_write`
    //! recognises a test region only as `#[cfg(test)]` + `mod ` / `pub mod `.

    use crate::worlds::migrate::{MigrationLocations, MigrationSeams};
    use std::fs;
    use std::io;
    use std::path::Path;
    use std::sync::Arc;

    /// Two instance roots under one tempdir, a source world `W` carrying a
    /// marker file, and the locations a migration between them uses.
    pub struct Fx {
        pub td: tempfile::TempDir,
        pub loc: MigrationLocations,
    }

    pub fn two_instances() -> Fx {
        let td = tempfile::tempdir().unwrap();
        let src_root = td.path().join("instances").join("src");
        let dst_root = td.path().join("instances").join("dst");
        let loc = MigrationLocations {
            src_saves: src_root.join(".minecraft").join("saves"),
            src_backups_root: src_root.join("backups"),
            src_root: src_root.clone(),
            dst_saves: dst_root.join(".minecraft").join("saves"),
            dst_backups_root: dst_root.join("backups"),
            dst_root: dst_root.clone(),
            world_folder: "W".to_string(),
            target_instance_name: "Target".to_string(),
        };
        fs::create_dir_all(&loc.src_saves).unwrap();
        fs::create_dir_all(&loc.dst_saves).unwrap();
        fs::write(src_root.join("instance.json"), b"{}").unwrap();
        fs::write(dst_root.join("instance.json"), b"{}").unwrap();
        make_world(&loc.src_saves.join("W"), b"original");
        Fx { td, loc }
    }

    /// `level.dat` + `region/r.0.0.mca` holding `marker`.
    pub fn make_world(dir: &Path, marker: &[u8]) {
        fs::create_dir_all(dir.join("region")).unwrap();
        fs::write(dir.join("level.dat"), b"level").unwrap();
        fs::write(dir.join("region").join("r.0.0.mca"), marker).unwrap();
    }

    pub fn marker_of(world_dir: &Path) -> Vec<u8> {
        fs::read(world_dir.join("region").join("r.0.0.mca")).unwrap()
    }

    pub fn add_backup(backups_root: &Path, world: &str, name: &str, bytes: &[u8]) {
        let dir = backups_root.join(world);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(name), bytes).unwrap();
    }

    pub fn seams(
        rename: impl Fn(&Path, &Path) -> io::Result<()> + Send + Sync + 'static,
        remove: impl Fn(&Path) -> io::Result<()> + Send + Sync + 'static,
    ) -> MigrationSeams {
        MigrationSeams {
            rename: Arc::new(rename),
            remove: Arc::new(remove),
        }
    }

    /// Names under `saves` that start with `.tmp-migrate-`, sorted.
    pub fn tmp_dirs(saves: &Path) -> Vec<String> {
        let mut out: Vec<String> = fs::read_dir(saves)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(".tmp-migrate-"))
            .collect();
        out.sort();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::fixture::*;
    use super::*;
    use std::fs;
    use std::io;

    fn stage_with_world(fx: &Fx, path: MigrationPath) -> Staged {
        let name = match path {
            MigrationPath::Copied => ".tmp-migrate-copy-W-0",
            MigrationPath::Renamed => ".tmp-migrate-moved-W-0",
        };
        let stage = fx.loc.dst_saves.join(name);
        make_world(&stage, b"staged");
        Staged {
            stage,
            path,
            links_skipped: 0,
        }
    }

    #[test]
    fn a_name_taken_in_the_gap_retries_the_next_suffix() {
        let fx = two_instances();
        let staged = stage_with_world(&fx, MigrationPath::Copied);
        let taken = fx.loc.dst_saves.join("W");
        let seams = seams(
            move |from, to| {
                if to == taken.as_path() {
                    // Another writer took the name between probe and rename.
                    fs::create_dir_all(to)?;
                    return Err(io::Error::from(io::ErrorKind::AlreadyExists));
                }
                fs::rename(from, to)
            },
            |p| fs::remove_dir_all(p),
        );

        let got = finalise_at(&fx.loc, &staged, &seams).unwrap();

        assert_eq!(got, "W (2)");
        assert_eq!(marker_of(&fx.loc.dst_saves.join("W (2)")), b"staged");
        assert!(
            tmp_dirs(&fx.loc.dst_saves).is_empty(),
            "the stage was consumed"
        );
    }

    #[test]
    fn a_persistent_refusal_ends_in_name_unresolvable_with_the_stage_rolled_back() {
        let fx = two_instances();
        let staged = stage_with_world(&fx, MigrationPath::Copied);
        let seams = seams(
            |_, _| Err(io::Error::from(io::ErrorKind::AlreadyExists)),
            |p| fs::remove_dir_all(p),
        );

        let r = finalise_at(&fx.loc, &staged, &seams);

        assert!(
            matches!(r, Err(Error::WorldNameUnresolvable { .. })),
            "{r:?}"
        );
        assert!(
            !staged.stage.try_exists().unwrap(),
            "copy-path rollback removes the stage; the source is intact"
        );
        assert_eq!(marker_of(&fx.loc.src_saves.join("W")), b"original");
    }

    #[test]
    fn a_real_rename_failure_on_the_rename_path_puts_the_world_back() {
        let fx = two_instances();
        // The rename path: the source slot is empty, the stage IS the world.
        fs::remove_dir_all(fx.loc.src_saves.join("W")).unwrap();
        let staged = stage_with_world(&fx, MigrationPath::Renamed);
        let dst_saves = fx.loc.dst_saves.clone();
        let seams = seams(
            move |from, to| {
                if to.parent() == Some(dst_saves.as_path()) {
                    return Err(io::Error::new(io::ErrorKind::PermissionDenied, "injected"));
                }
                fs::rename(from, to)
            },
            |p| fs::remove_dir_all(p),
        );

        let r = finalise_at(&fx.loc, &staged, &seams);

        assert!(matches!(r, Err(Error::Io { .. })), "{r:?}");
        assert_eq!(marker_of(&fx.loc.src_saves.join("W")), b"staged");
        assert!(tmp_dirs(&fx.loc.dst_saves).is_empty());
    }

    #[test]
    fn a_vanished_target_world_keeps_the_source() {
        // §9.16: finalise placed dst/W, then it disappeared before the source
        // removal ran. The source stays, whatever the seam would have done.
        let fx = two_instances();
        let seams = seams(|f, t| fs::rename(f, t), |p| fs::remove_dir_all(p));

        let state = remove_source_after_copy_at(&fx.loc, "W", &seams);

        assert!(matches!(state, SourceState::LeftIntact { .. }), "{state:?}");
        assert_eq!(marker_of(&fx.loc.src_saves.join("W")), b"original");
    }

    #[test]
    fn a_missing_target_instance_file_keeps_the_source() {
        let fx = two_instances();
        make_world(&fx.loc.dst_saves.join("W"), b"original");
        fs::remove_file(fx.loc.dst_root.join("instance.json")).unwrap();
        let seams = seams(|f, t| fs::rename(f, t), |p| fs::remove_dir_all(p));

        let state = remove_source_after_copy_at(&fx.loc, "W", &seams);

        assert!(matches!(state, SourceState::LeftIntact { .. }), "{state:?}");
        assert_eq!(marker_of(&fx.loc.src_saves.join("W")), b"original");
    }

    #[test]
    fn a_verified_target_lets_the_source_go() {
        let fx = two_instances();
        make_world(&fx.loc.dst_saves.join("W"), b"original");
        let seams = seams(|f, t| fs::rename(f, t), |p| fs::remove_dir_all(p));

        let state = remove_source_after_copy_at(&fx.loc, "W", &seams);

        assert_eq!(state, SourceState::Removed);
        assert!(!fx.loc.src_saves.join("W").try_exists().unwrap());
    }

    /// §6 accepts a world without `level.dat`. Re-verification must not
    /// demand of the target what the source never had, or such a world moved
    /// across volumes keeps its source forever — a Move that never completes.
    #[test]
    fn a_world_without_level_dat_on_either_side_lets_the_source_go() {
        let fx = two_instances();
        fs::remove_file(fx.loc.src_saves.join("W").join("level.dat")).unwrap();
        make_world(&fx.loc.dst_saves.join("W"), b"original");
        fs::remove_file(fx.loc.dst_saves.join("W").join("level.dat")).unwrap();
        let seams = seams(|f, t| fs::rename(f, t), |p| fs::remove_dir_all(p));

        let state = remove_source_after_copy_at(&fx.loc, "W", &seams);

        assert_eq!(state, SourceState::Removed);
        assert!(!fx.loc.src_saves.join("W").try_exists().unwrap());
    }

    /// The other half of the conditional: a source WITH `level.dat` still
    /// requires it in the target — a directory alone is not that world.
    #[test]
    fn a_target_missing_the_level_dat_the_source_has_keeps_the_source() {
        let fx = two_instances();
        make_world(&fx.loc.dst_saves.join("W"), b"original");
        fs::remove_file(fx.loc.dst_saves.join("W").join("level.dat")).unwrap();
        let seams = seams(|f, t| fs::rename(f, t), |p| fs::remove_dir_all(p));

        let state = remove_source_after_copy_at(&fx.loc, "W", &seams);

        assert!(matches!(state, SourceState::LeftIntact { .. }), "{state:?}");
        assert_eq!(marker_of(&fx.loc.src_saves.join("W")), b"original");
    }

    #[test]
    fn a_failed_removal_is_intact_if_it_took_nothing_and_partial_if_it_took_some() {
        let fx = two_instances();
        make_world(&fx.loc.dst_saves.join("W"), b"original");

        let intact = remove_source_after_copy_at(
            &fx.loc,
            "W",
            &seams(
                |f, t| fs::rename(f, t),
                |_| Err(io::Error::new(io::ErrorKind::PermissionDenied, "injected")),
            ),
        );
        assert!(
            matches!(intact, SourceState::LeftIntact { .. }),
            "{intact:?}"
        );
        assert_eq!(marker_of(&fx.loc.src_saves.join("W")), b"original");

        let partial = remove_source_after_copy_at(
            &fx.loc,
            "W",
            &seams(
                |f, t| fs::rename(f, t),
                |p| {
                    fs::remove_dir_all(p.join("region"))?;
                    Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "injected after region/ went",
                    ))
                },
            ),
        );
        assert!(
            matches!(partial, SourceState::LeftPartial { .. }),
            "{partial:?}"
        );
        assert!(
            fx.loc.src_saves.join("W").join("level.dat").is_file(),
            "what the failed removal left is still there"
        );
    }
}
