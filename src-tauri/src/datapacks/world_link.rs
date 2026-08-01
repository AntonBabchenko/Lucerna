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

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use fastnbt::Value;

use crate::datapacks::{
    level_dat, level_dat_entry, library_dir_at, registry, state, InstalledDatapack, PackCompat,
    WorldDatapack,
};
use crate::error::{Error, Result};
use crate::mods::store::{materialize, LinkPolicy, Placement};

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

/// Every world under `<instance>/.minecraft/saves/` that already has a file
/// or folder named `filename` in its own `datapacks/` folder. Used to fan a
/// library reinstall out to every world that already links the old bytes —
/// see [`crate::datapacks::library::install_named_at`].
///
/// One `read_dir` of `saves/`; a missing `saves/` dir yields an empty vec,
/// never an error — mirrors `worlds::list_worlds`'s own "missing saves ⇒
/// empty" policy. Deliberately synchronous (not `tokio::fs`): this is a
/// single shallow directory listing plus one `exists()` per world, called
/// from `install_named_at` which is not on a latency-sensitive path.
pub(crate) fn worlds_linking(instance_root: &Path, filename: &str) -> Vec<PathBuf> {
    let saves_dir = instance_root.join(".minecraft").join("saves");
    let Ok(rd) = std::fs::read_dir(&saves_dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in rd.flatten() {
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_dir() {
            continue;
        }
        let candidate = entry.path().join("datapacks").join(filename);
        if candidate.exists() {
            out.push(candidate);
        }
    }
    out
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
fn read_level_dat_or_empty(world_dir: &Path) -> Result<(Value, level_dat::Framing)> {
    if !world_dir.join("level.dat").exists() {
        return Ok((Value::Compound(HashMap::new()), level_dat::Framing::Gzip));
    }
    level_dat::read_at(world_dir)
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
    if level_dat::forget(&mut root, &entry)? {
        level_dat::write_at(&world_dir, &root, framing).await?;
    }
    Ok(())
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

/// Append `n` to `names` unless an equal string is already present.
fn push_unique(names: &mut Vec<String>, n: String) {
    if !names.contains(&n) {
        names.push(n);
    }
}

/// List every datapack relevant to one world: the union of the library's
/// filenames, the `.zip` files actually present in the world's `datapacks/`
/// folder, and the names level.dat references (its own `file/` prefix
/// stripped). Sorted case-insensitively by filename.
///
/// `compat` is computed from the pack_format the REGISTRY recorded at
/// install time, never by opening a zip here — listing a world must cost no
/// zip reads. A world file with no registry entry (e.g. hand-dropped
/// straight into the world folder, or imported with a world) reports
/// `Unknown` deliberately, rather than opening every zip on every render.
pub async fn list_for_world_at(
    instance_root: &Path,
    world: &str,
    expected: Option<u32>,
) -> Result<Vec<WorldDatapack>> {
    let (world_dir, dp_dir) = world_dirs(instance_root, world)?;

    let registry_entries: Vec<InstalledDatapack> = registry::list(instance_root).await?;
    let (root, _framing) = read_level_dat_or_empty(&world_dir)?;
    let (enabled, disabled) = level_dat::lists(&root);

    let mut on_disk: Vec<String> = Vec::new();
    if let Ok(mut rd) = tokio::fs::read_dir(&dp_dir).await {
        while let Ok(Some(e)) = rd.next_entry().await {
            let name = e.file_name().to_string_lossy().to_string();
            // Minecraft's own datapacks/ scanner loads directories as well as
            // `.zip` files, under any name — so a directory counts regardless
            // of what it's called, and only a FILE is additionally gated on
            // the `.zip` extension. Without the directory branch, a
            // hand-installed folder datapack never appears "present" here,
            // which `state::derive` then turns into a false `Orphaned`.
            let is_datapack_entry = match e.file_type().await {
                Ok(ft) if ft.is_dir() => true,
                Ok(_) => name.to_ascii_lowercase().ends_with(".zip"),
                Err(_) => false,
            };
            if is_datapack_entry {
                on_disk.push(name);
            }
        }
    }

    let mut names: Vec<String> = Vec::new();
    for e in &registry_entries {
        push_unique(&mut names, e.filename.clone());
    }
    for n in &on_disk {
        push_unique(&mut names, n.clone());
    }
    for n in enabled.iter().chain(disabled.iter()) {
        if let Some(stripped) = n.strip_prefix("file/") {
            push_unique(&mut names, stripped.to_string());
        }
    }
    names.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));

    let out = names
        .into_iter()
        .map(|filename| {
            let file_present = on_disk.contains(&filename);
            let entry = level_dat_entry(&filename);
            let in_enabled = enabled.contains(&entry);
            let in_disabled = disabled.contains(&entry);
            let pack_state = state::derive(file_present, in_enabled, in_disabled);

            let reg = registry_entries.iter().find(|e| e.filename == filename);
            let in_library = reg.is_some();
            let compat = compat_of(reg.and_then(|e| e.pack_format), expected);

            WorldDatapack {
                filename,
                state: pack_state,
                in_library,
                compat,
            }
        })
        .collect();

    Ok(out)
}

/// Compare a pack's own `pack_format` against what the instance's Minecraft
/// expects. `Unknown` when either side is unavailable — an unreadable pack,
/// or (see `compat` module) a client jar that hasn't been installed yet.
#[must_use]
pub fn compat_of(pack_format: Option<u32>, expected: Option<u32>) -> PackCompat {
    match (pack_format, expected) {
        (Some(p), Some(e)) if p == e => PackCompat::Compatible,
        (Some(p), Some(e)) => PackCompat::Mismatch {
            pack_format: p,
            expected: e,
        },
        _ => PackCompat::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datapacks::{library, WorldPackState};
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn datapack_zip(pack_format: u32) -> Vec<u8> {
        let mut zw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zw.start_file("pack.mcmeta", opts).unwrap();
        zw.write_all(format!(r#"{{"pack":{{"pack_format":{pack_format}}}}}"#).as_bytes())
            .unwrap();
        zw.start_file("data/x/function/a.mcfunction", opts).unwrap();
        zw.write_all(b"say hi").unwrap();
        zw.finish().unwrap().into_inner()
    }

    async fn seed_library(root: &Path, filename: &str, pack_format: u32) {
        library::install_named_at(root, filename, &datapack_zip(pack_format))
            .await
            .unwrap();
    }

    fn world_dir(root: &Path, world: &str) -> PathBuf {
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
    fn hardlink_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::test_env_lock()
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
    async fn list_reports_orphaned_for_a_level_dat_name_with_no_file() {
        let td = tempfile::tempdir().unwrap();
        let wd = world_dir(td.path(), "Survival");
        std::fs::create_dir_all(&wd).unwrap();
        let mut root = Value::Compound(HashMap::new());
        level_dat::set_enabled(&mut root, "file/ghost.zip", true).unwrap();
        level_dat::write_at(&wd, &root, level_dat::Framing::Gzip)
            .await
            .unwrap();

        let listed = list_for_world_at(td.path(), "Survival", None)
            .await
            .unwrap();

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].filename, "ghost.zip");
        assert_eq!(listed[0].state, WorldPackState::Orphaned);
        assert!(!listed[0].in_library);
    }

    #[tokio::test]
    async fn list_reports_not_added_for_a_library_pack_not_in_the_world() {
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "vm.zip", 48).await;

        let listed = list_for_world_at(td.path(), "Survival", None)
            .await
            .unwrap();

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].filename, "vm.zip");
        assert_eq!(listed[0].state, WorldPackState::NotAdded);
        assert!(listed[0].in_library);
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
    async fn a_mismatched_pack_format_reports_mismatch() {
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "vm.zip", 48).await;

        let listed = list_for_world_at(td.path(), "Survival", Some(10))
            .await
            .unwrap();

        assert_eq!(listed.len(), 1);
        assert_eq!(
            listed[0].compat,
            PackCompat::Mismatch {
                pack_format: 48,
                expected: 10
            }
        );
    }

    #[test]
    fn compat_of_matches_reports_compatible() {
        assert_eq!(compat_of(Some(48), Some(48)), PackCompat::Compatible);
    }

    #[test]
    fn compat_of_missing_either_side_is_unknown() {
        assert_eq!(compat_of(None, Some(48)), PackCompat::Unknown);
        assert_eq!(compat_of(Some(48), None), PackCompat::Unknown);
        assert_eq!(compat_of(None, None), PackCompat::Unknown);
    }

    #[tokio::test]
    async fn a_folder_datapack_is_reported_enabled_not_orphaned() {
        let td = tempfile::tempdir().unwrap();
        let wd = world_dir(td.path(), "Survival");
        // A hand-installed FOLDER datapack, not a `.zip` — Minecraft's own
        // datapacks/ scanner loads directories under any name, and so must
        // this listing. Before the fix this reported `file_present: false`
        // (only `.zip`-named files were scanned) and, once level.dat records
        // it, that becomes a false `Orphaned`.
        std::fs::create_dir_all(wd.join("datapacks/MyFolderPack/data")).unwrap();

        let listed = list_for_world_at(td.path(), "Survival", None)
            .await
            .unwrap();

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].filename, "MyFolderPack");
        assert_eq!(
            listed[0].state,
            WorldPackState::Enabled,
            "present and unlisted in level.dat ⇒ Minecraft auto-enables it"
        );
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
}
