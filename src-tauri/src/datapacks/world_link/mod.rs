//! Per-world datapack placement: link/unlink a library file into one world's
//! `datapacks/` folder, toggle its level.dat state, and list the merged view a
//! world's datapack tab renders.
//!
//! A world's `datapacks/` folder holds hardlinks to library files — a second
//! NAME for the library's physical file, not a copy. Creating and deleting a
//! name is safe; writing through one is not (see `mods::store`'s module doc).
//! Every placement here goes through `mods::store::materialize`, so this
//! module holds no raw write primitive — enforced by the structural guard
//! (`tests/structural_no_inplace_mods_write.rs`), which scans `src/datapacks/`
//! alongside `src/mods/` and `src/worlds/`.
//!
//! Split by responsibility — the public paths (`world_link::X`) are unchanged,
//! re-exported below:
//!   * [`mutate`] — the locked single-world entry points (add / remove /
//!     toggle, plus the library cascade's removal);
//!   * [`migrate`] — the update's cross-filename world migration;
//!   * [`placements`] — identity-verified placement enumeration + the
//!     same-name refresh fan-out;
//!   * [`listing`] — the read-only merged per-world view.
//! `level_dat_lock` stays PRIVATE to this module tree: only entry points
//! defined inside `world_link` may take it, which is what makes "composing
//! the public entry points under the lock deadlocks" structurally impossible
//! for outside callers.

mod listing;
mod migrate;
mod mutate;
mod placements;

pub use listing::list_for_world_at;
pub(crate) use listing::vouched_by_library;
pub(crate) use migrate::migrate_placements;
pub(crate) use mutate::remove_for_cascade_at;
pub use mutate::{add_to_world_at, remove_from_world_at, set_enabled_in_world_at};
pub(crate) use placements::{placements_of, refresh_placements};

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::datapacks::presence::{self, LevelDatPresence};
use crate::error::{Error, Result};

/// Resolve a world's own directory and its `datapacks/` subdirectory,
/// requiring the world folder to already exist on disk. Every entry point
/// that is handed a world by name — the writers and the read-only listing —
/// resolves it here.
///
/// A stale or mistyped `world` must fail with `WorldNotFound` rather than be
/// read as a folder that merely lacks a `level.dat`:
///   * a WRITE would otherwise create `saves/<world>/` — and
///     `worlds::list_worlds` treats any directory under `saves/` as a real
///     world, so a phantom created by a typo would immediately show up in the
///     worlds list as a real, unopenable world;
///   * the LISTING would otherwise report a world that is simply gone (deleted
///     while its tab was open) as `Absent` — "not a world" — since
///     `presence::of` reads a missing directory as `Absent`, which is right
///     only for a server that has never booted.
///
/// `crate::worlds::world_dir_at` is the existing validate → join → `is_dir`
/// helper every other world-mutating path already uses for exactly this
/// reason. A failed stat of the world folder is currently also reported as
/// `WorldNotFound` (`world_dir_at` uses `is_dir`): a known limitation,
/// tracked separately.
fn world_dirs_checked(instance_root: &Path, world: &str) -> Result<(PathBuf, PathBuf)> {
    let saves_dir = instance_root.join(".minecraft").join("saves");
    let world_dir = crate::worlds::world_dir_at(&saves_dir, world)?;
    let dp_dir = world_dir.join("datapacks");
    Ok((world_dir, dp_dir))
}

/// D2 (spec §0.1): only a world whose `level.dat` is a regular file gets a
/// data-pack change. `OnlyOld` is a world the game restores from
/// `level.dat_old`, and a `level.dat` written first switches that recovery off.
/// `Absent` is a folder the game does not treat as a world. `Err` means the
/// state could not be told, and the change is refused.
fn require_level_dat(world_dir: &Path, world: &str) -> Result<()> {
    match presence::of(world_dir)? {
        LevelDatPresence::Present => Ok(()),
        LevelDatPresence::OnlyOld => Err(only_old(world)),
        LevelDatPresence::Absent => Err(level_dat_missing(world)),
    }
}

fn only_old(world: &str) -> Error {
    Error::WorldLevelDatOnlyOld {
        folder_name: world.to_string(),
    }
}

fn level_dat_missing(world: &str) -> Error {
    Error::WorldLevelDatMissing {
        folder_name: world.to_string(),
    }
}

/// Serializes every level.dat mutation in this module tree against the others
/// and against itself.
///
/// Tauri runs each command as its own task on a multi-threaded runtime, so
/// (for example) `set_enabled_in_world_at` disabling pack A and
/// `add_to_world_at` linking pack B can interleave their read → mutate →
/// write of the SAME world's level.dat: both read the same on-disk root,
/// both compute an edit against that stale snapshot, and whichever writes
/// last wins — silently reverting the other's change. Because
/// `state::derive_listed` reads a present, unlisted pack as `Enabled`
/// (Minecraft auto-enables it), the reverted disable is not just
/// lost, it is invisible: the next refresh shows the pack Enabled with no
/// error, and it loads in game.
///
/// This closes that lost-update window. It does NOT close the separate
/// TOCTOU against a running Minecraft process rewriting level.dat itself —
/// that window is accepted; see `guard`'s module doc and `map_read_err`'s
/// note on POSIX rename semantics.
///
/// A `tokio::sync::Mutex`, not `std::sync::Mutex`: the critical section spans
/// `.await` points (`materialize`, `level_dat::write_at`), and holding a std
/// mutex guard across an `.await` does not compile (the guard is not `Send`).
///
/// Only the entry points of this module tree take this lock; every helper
/// they call (`world_dirs_checked`, `require_level_dat`, `presence::of`,
/// `level_dat::read_at`) stays lock-free, and none of them ever calls into
/// `registry::*` (which takes its OWN, separate lock — see
/// `registry::registry_lock`'s doc) — so the two locks are never nested and
/// cannot deadlock each other.
fn level_dat_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

/// Maps a failed directory- or file-removal to the friendly typed
/// `WorldInUse` for the OS codes a held-open entry surfaces on Windows — same
/// codes `level_dat::map_read_err` maps. Reached only once the entry's own
/// type has already selected the matching removal call above
/// (`remove_dir_all` for a directory, `remove_file` for a file), so this can
/// no longer misfire for "wrong verb used on this type" — the bug this
/// branch replaces — only for a genuine lock held by a running Minecraft.
fn map_removal_err(path: &Path, e: std::io::Error, world: &str) -> Error {
    if matches!(e.raw_os_error(), Some(5) | Some(32) | Some(33)) {
        Error::WorldInUse {
            folder_name: world.to_string(),
        }
    } else {
        Error::ModsInstancePath {
            path: path.display().to_string(),
            details: e.to_string(),
        }
    }
}

/// R2 + D1 for a toggle (spec §2 N.4, §0.5 A19). Resolves `filename` against
/// ONE `read_dir` of `dp_dir` and classifies only the entry it resolves to,
/// off the executor, with no vouch. `Ok(Some(name))` = a pack, spelled as on
/// disk. `Ok(None)` = nothing on disk by that name; the caller writes its own
/// spelling, as before. A non-pack is refused with the matching
/// `DatapackInvalid` reason, and an unreadable entry is `Error::io` with its
/// cause. Never touches the registry, so a caller holding a `level_dat_lock`
/// may call it.
pub(crate) async fn pack_name_for_write(dp_dir: &Path, filename: &str) -> Result<Option<String>> {
    use crate::datapacks::detect::{self, Presence, WriteTarget};
    let (dp, name) = (dp_dir.to_path_buf(), filename.to_string());
    let target = tokio::task::spawn_blocking(move || detect::target_for_write(&dp, &name))
        .await
        .map_err(|e| Error::io(dp_dir.display().to_string(), format!("join: {e}")))?
        .map_err(|e| Error::io(dp_dir.join(filename).display().to_string(), e))?;
    match target {
        WriteTarget::Absent => Ok(None),
        WriteTarget::Present {
            name,
            presence: Some(Presence::Pack { .. }),
        } => Ok(Some(name)),
        WriteTarget::Present {
            presence: Some(Presence::Unusable { reason, .. }),
            ..
        } => Err(Error::DatapackInvalid {
            filename: filename.to_string(),
            reason: detect::rejection_for(reason),
        }),
        // A plain file that is not a zip: the game never scans it as a pack.
        WriteTarget::Present { presence: None, .. } => Err(Error::DatapackInvalid {
            filename: filename.to_string(),
            reason: crate::error::DatapackRejection::NotAZip,
        }),
    }
}

/// Case-insensitive membership check against a level.dat name list (the
/// `Enabled`/`Disabled` lists `level_dat::lists` returns). The spelling
/// [`union_names`] picked for a pack may differ in case from what level.dat
/// actually holds for that same file — an exact `contains` here would then
/// miss the match, turning a `Disabled` pack into a reported `Enabled` (or
/// vice versa), which is worse than the phantom-row bug `union_names` fixes.
/// Query-only: never compare a value here that is about to be written back
/// to level.dat — those writes must keep the caller's exact filename.
#[must_use]
pub(crate) fn contains_ci(haystack: &[String], needle: &str) -> bool {
    let needle = needle.to_lowercase();
    haystack.iter().any(|h| h.to_lowercase() == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_level_dat_lock_excludes_a_second_holder() {
        let guard = level_dat_lock().lock().await;
        // Proves this is a real mutex, not a no-op: while we hold it, ANY
        // other attempt — from this test or a concurrently-running one —
        // must fail immediately rather than silently succeed.
        assert!(
            level_dat_lock().try_lock().is_err(),
            "a held lock must block a second acquisition attempt"
        );
        drop(guard);
    }
}

/// Shared fixtures for this module tree's tests. `pub(crate)` under
/// `#[cfg(test)]`: the per-file test modules below this directory are
/// grandchildren of `world_link`, so plain `pub(super)` would not reach them.
#[cfg(test)]
pub(crate) mod test_util {
    use std::io::Write;
    use std::path::{Path, PathBuf};

    use zip::write::SimpleFileOptions;

    use crate::datapacks::library;

    pub(crate) fn datapack_zip(pack_format: u32) -> Vec<u8> {
        let mut zw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zw.start_file("pack.mcmeta", opts).unwrap();
        zw.write_all(format!(r#"{{"pack":{{"pack_format":{pack_format}}}}}"#).as_bytes())
            .unwrap();
        zw.start_file("data/x/function/a.mcfunction", opts).unwrap();
        zw.write_all(b"say hi").unwrap();
        zw.finish().unwrap().into_inner()
    }

    pub(crate) async fn seed_library(root: &Path, filename: &str, pack_format: u32) {
        library::install_named_at(root, filename, &datapack_zip(pack_format), None)
            .await
            .unwrap();
    }

    pub(crate) fn world_dir(root: &Path, world: &str) -> PathBuf {
        crate::datapacks::world_datapacks_dir_at(root, world)
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf()
    }

    /// `saves/<world>/` of a world Minecraft has played, with a real
    /// `level.dat` (`Enabled:["vanilla"]`, nothing else listed). Returns the
    /// world directory.
    ///
    /// It writes through `level_dat::test_support::seed` and never with a
    /// write primitive of its own. This module is `pub(crate) mod`, which
    /// `structural_no_inplace_mods_write` does NOT mask as a test region
    /// (spec §0.5 A17).
    pub(crate) fn game_world(root: &Path, world: &str) -> PathBuf {
        let wd = world_dir(root, world);
        crate::datapacks::level_dat::test_support::seed(&wd, &[], &[]);
        wd
    }

    /// Every test that exercises a real hardlink must hold this — a sibling
    /// test's `LUCERNA_TEST_FORCE_LINK_FAILURE` seam is process-global. See
    /// `mods::store`'s own test module for the full explanation of why a test
    /// that installs no scope still needs the lock. Never combine this with
    /// `test_seam::scope()` in the same test — the mutex is not reentrant.
    pub(crate) fn hardlink_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::test_env_lock()
    }
}
