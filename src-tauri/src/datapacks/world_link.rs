//! Per-world datapack placement: link/unlink a library file into one world's
//! `datapacks/` folder, toggle its level.dat state, and list the merged view a
//! world's datapack tab renders.
//!
//! A world's `datapacks/` folder holds hardlinks to library files — a second
//! NAME for the library's physical file, not a copy. Creating and deleting a
//! name is safe; writing through one is not (see `mods::store`'s module doc).
//! Every placement here goes through `mods::store::materialize`, so this
//! module holds no raw write primitive and the structural guard passes over
//! it with no exemption.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

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
    let (world_dir, dp_dir) = world_dirs(instance_root, world)?;

    tokio::fs::create_dir_all(&dp_dir)
        .await
        .map_err(|e| Error::ModsInstancePath {
            path: dp_dir.display().to_string(),
            details: e.to_string(),
        })?;

    let src = library_dir_at(instance_root).join(filename);
    let dest = dp_dir.join(filename);
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
    let (world_dir, dp_dir) = world_dirs(instance_root, world)?;

    let path = dp_dir.join(filename);
    match tokio::fs::remove_file(&path).await {
        Ok(()) => {}
        // Idempotent: no file is exactly the orphan-repair case.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        // A running Minecraft holds the world's directory entries open on
        // Windows — surface that as the friendly typed error rather than a
        // raw IO string. Same codes `level_dat::map_read_err` maps.
        Err(e) if matches!(e.raw_os_error(), Some(5) | Some(32) | Some(33)) => {
            return Err(Error::WorldInUse {
                folder_name: world.to_string(),
            });
        }
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
    let (world_dir, _dp_dir) = world_dirs(instance_root, world)?;

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
            if name.to_ascii_lowercase().ends_with(".zip") {
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
}
