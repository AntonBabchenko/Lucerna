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
//!   * [`mutate`] — the three locked single-world entry points;
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
pub(crate) use listing::list_on_disk_entries;
pub(crate) use migrate::migrate_placements;
pub use mutate::{add_to_world_at, remove_from_world_at, set_enabled_in_world_at};
pub(crate) use placements::{placements_of, refresh_placements};

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use fastnbt::Value;

use crate::datapacks::level_dat;
use crate::error::{Error, Result};

/// Resolve a world's own directory and its `datapacks/` subdirectory,
/// validating `world` exactly once (via `world_datapacks_dir_at`, which
/// rejects a path separator or traversal). Existence is deliberately NOT
/// checked here — see `read_level_dat_or_empty` for why a world with no
/// on-disk level.dat yet is a supported, non-error state.
///
/// LENIENT lookup: reserved for [`list_for_world_at`], the one caller that
/// must not fail just because a world hasn't been played yet. Every WRITE
/// path uses [`world_dirs_checked`] instead — see its doc for why.
fn world_dirs(instance_root: &Path, world: &str) -> Result<(PathBuf, PathBuf)> {
    let dp_dir = crate::datapacks::world_datapacks_dir_at(instance_root, world)?;
    let world_dir = dp_dir
        .parent()
        // `world_datapacks_dir_at` always joins
        // `.../.minecraft/saves/<world>/datapacks`, which always has a parent.
        .expect("world_datapacks_dir_at always returns a path with a parent")
        .to_path_buf();
    Ok((world_dir, dp_dir))
}

/// Resolve a world's own directory and its `datapacks/` subdirectory for a
/// WRITE path, requiring the world to already exist on disk.
///
/// Unlike [`world_dirs`], a stale or mistyped `world` here must fail rather
/// than silently create `saves/<world>/` — `worlds::list_worlds` treats any
/// directory under `saves/` as a real world, so a phantom created by a typo
/// would immediately show up in the worlds list as a real, unopenable world.
/// `crate::worlds::world_dir_at` is the existing validate → join → `is_dir`
/// helper every other world-mutating path already uses for exactly this
/// reason.
fn world_dirs_checked(instance_root: &Path, world: &str) -> Result<(PathBuf, PathBuf)> {
    let saves_dir = instance_root.join(".minecraft").join("saves");
    let world_dir = crate::worlds::world_dir_at(&saves_dir, world)?;
    let dp_dir = world_dir.join("datapacks");
    Ok((world_dir, dp_dir))
}

/// Serializes the three level.dat mutations below against each other and
/// against themselves.
///
/// Tauri runs each command as its own task on a multi-threaded runtime, so
/// (for example) `set_enabled_in_world_at` disabling pack A and
/// `add_to_world_at` linking pack B can interleave their read → mutate →
/// write of the SAME world's level.dat: both read the same on-disk root,
/// both compute an edit against that stale snapshot, and whichever writes
/// last wins — silently reverting the other's change. Because
/// `state::derive(true, false, false)` is `Enabled` ("present and unlisted"
/// is correct — Minecraft auto-enables it), the reverted disable is not just
/// lost, it is invisible: the next refresh shows the pack Enabled with no
/// error, and it loads in game.
///
/// This closes that lost-update window. It does NOT close the separate
/// TOCTOU against a running Minecraft process rewriting level.dat itself —
/// that window is accepted; see `guard`'s module doc and `map_read_err`'s
/// note on POSIX rename semantics.
///
/// A `tokio::sync::Mutex`, not `std::sync::Mutex`: the critical section spans
/// `.await` points (`materialize`, `read_level_dat_or_empty`'s NOT being
/// async is incidental — `level_dat::write_at` is), and holding a std mutex
/// guard across an `.await` does not compile (the guard is not `Send`).
///
/// Only the three public entry points below take this lock; every helper
/// they call (`world_dirs_checked`, `read_level_dat_or_empty`) stays
/// lock-free, and none of the three ever calls into `registry::*` (which
/// takes its OWN, separate lock — see `registry::registry_lock`'s doc) — so
/// the two locks are never nested and cannot deadlock each other.
fn level_dat_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

/// `level_dat::read_at`, except a world that has never been loaded (or was
/// imported without a level.dat) reads as an empty root in `Gzip` framing —
/// the framing every real level.dat uses — rather than erroring. Every
/// caller here only needs the DataPacks lists, which are simply absent on
/// such a world; `write_at` already treats "no pre-existing file" as the one
/// case where it skips the backup, so writing this empty root back out (when
/// an edit actually changes something) is exactly as safe as it looks.
pub(crate) fn read_level_dat_or_empty(world_dir: &Path) -> Result<(Value, level_dat::Framing)> {
    if !world_dir.join("level.dat").exists() {
        return Ok((Value::Compound(HashMap::new()), level_dat::Framing::Gzip));
    }
    level_dat::read_at(world_dir)
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

    /// Every test that exercises a real hardlink must hold this — a sibling
    /// test's `LUCERNA_TEST_FORCE_LINK_FAILURE` seam is process-global. See
    /// `mods::store`'s own test module for the full explanation of why a test
    /// that installs no scope still needs the lock. Never combine this with
    /// `test_seam::scope()` in the same test — the mutex is not reentrant.
    pub(crate) fn hardlink_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::test_env_lock()
    }
}
