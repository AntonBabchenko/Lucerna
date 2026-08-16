//! Local world import: bring a Minecraft save into `saves/` from a `.zip`
//! or a folder. Reuses the worlds module's zip-slip-safe extraction and the
//! path-safety gate; never follows symlinks.

use crate::error::{Error, Result};
use crate::worlds::{fs as wfs, zip as wzip, World};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

/// Per-file / aggregate caps. Larger than modpack overrides because a real
/// world (esp. a folder import of the user's own save) is legitimately
/// multi-GB; still bounds pathological zip bombs.
const PER_FILE_CAP: u64 = 2 * 1024 * 1024 * 1024;
const AGGREGATE_CAP: u64 = 8 * 1024 * 1024 * 1024;

/// Test-friendly core. `source` is a `.zip` file or a directory containing a
/// world (possibly nested). Writes the world under `saves/<name>` (suffixing
/// on collision) and returns the imported `World`.
pub fn import_into_saves(saves: &Path, source: &Path) -> Result<World> {
    if source.is_dir() {
        import_from_dir(saves, source, None)
    } else if source.is_file() {
        import_from_zip(saves, source)
    } else {
        Err(Error::WorldImportUnsupportedSource)
    }
}

fn import_from_zip(saves: &Path, zip_path: &Path) -> Result<World> {
    check_zip_size(zip_path, PER_FILE_CAP, AGGREGATE_CAP)?;
    std::fs::create_dir_all(saves).map_err(|e| Error::io(saves.display().to_string(), e))?;
    let staging = unique_staging_dir(saves);
    std::fs::create_dir_all(&staging).map_err(|e| Error::io(staging.display().to_string(), e))?;

    let result = (|| -> Result<World> {
        // `check_zip_size` above is only a cheap early reject on the archive's
        // DECLARED sizes (attacker-controlled). `extract_zip_capped` is the
        // real defense: it counts the ACTUAL bytes written and aborts the
        // moment a per-file or aggregate cap is exceeded, so a zip bomb whose
        // central directory lies about its sizes still cannot land oversized
        // data on disk.
        wzip::extract_zip_capped(zip_path, &staging, PER_FILE_CAP, AGGREGATE_CAP)
            .map_err(map_archive_err)?;
        let fallback = zip_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("World")
            .to_string();
        place_world(saves, &staging, Some(&fallback), Move::Rename)
    })();

    // NotFound is the COMMON success case, not debris: when level.dat sits at
    // the archive root, `find_world_root(staging) == staging` and the
    // `Move::Rename` inside `place_world` moves the staging dir itself into
    // saves/ — there is nothing left to sweep. Only a real removal failure
    // (something genuinely left behind that could not be cleared) is worth a
    // log line.
    if let Err(e) = std::fs::remove_dir_all(&staging) {
        if e.kind() != std::io::ErrorKind::NotFound {
            crate::diag!(
                "world import: leftover staging dir {}: {e}",
                staging.display()
            );
        }
    }
    result
}

fn import_from_dir(saves: &Path, source_dir: &Path, fallback_name: Option<&str>) -> Result<World> {
    std::fs::create_dir_all(saves).map_err(|e| Error::io(saves.display().to_string(), e))?;
    place_world(saves, source_dir, fallback_name, Move::Copy)
}

#[derive(Clone, Copy)]
enum Move {
    Rename,
    Copy,
}

/// Detect the world root, derive + validate the name, resolve the collision,
/// then move/copy the world subtree into `saves/<chosen>`.
fn place_world(
    saves: &Path,
    source_dir: &Path,
    fallback_name: Option<&str>,
    mode: Move,
) -> Result<World> {
    let world_root = find_world_root(source_dir).ok_or(Error::WorldImportNotAWorld)?;

    let name = if world_root == source_dir {
        match fallback_name {
            Some(f) => f.to_string(),
            None => source_dir
                .file_name()
                .and_then(|s| s.to_str())
                .map(String::from)
                .ok_or(Error::WorldImportNotAWorld)?,
        }
    } else {
        world_root
            .file_name()
            .and_then(|s| s.to_str())
            .map(String::from)
            .ok_or(Error::WorldImportNotAWorld)?
    };
    wfs::validate_segment(&name)?;

    // The free-name check and the move below are not atomic, but imports run
    // serially (spawn_blocking + the UI's sequential loop), so the only race is
    // two same-named imports in the microsecond gap — which can at worst merge
    // into a freshly created dir, never clobber a pre-existing save. Not worth a
    // lock.
    let chosen = pick_free_world_name(saves, &name)?;
    let dest = saves.join(&chosen);

    match mode {
        Move::Rename => {
            std::fs::rename(&world_root, &dest)
                .map_err(|e| Error::io(dest.display().to_string(), e))?;
        }
        Move::Copy => {
            std::fs::create_dir_all(&dest).map_err(|e| Error::io(dest.display().to_string(), e))?;
            copy_world_with_rollback(&world_root, &dest, &chosen, &|p| std::fs::remove_dir_all(p))?;
        }
    }

    let (size_bytes, modified_unix_ms) = wfs::dir_size_and_mtime(&dest)?;
    Ok(World {
        folder_name: chosen,
        size_bytes: size_bytes as f64,
        modified_unix_ms: modified_unix_ms as f64,
        backup_count: 0,
    })
}

/// Map `extract_zip`'s backup-flavored error onto the import surface; pass IO
/// errors through unchanged.
fn map_archive_err(e: Error) -> Error {
    match e {
        Error::BackupCorrupt { details, .. } => Error::WorldImportInvalidArchive { details },
        other => other,
    }
}

/// `saves/.tmp-import-<ts>` — same volume as the destination so the post-extract
/// move is a cheap rename. Hidden (leading dot) so `list_worlds` skips it.
fn unique_staging_dir(saves: &Path) -> PathBuf {
    let suffix = chrono::Utc::now().format("%Y%m%d%H%M%S%f").to_string();
    saves.join(format!(".tmp-import-{suffix}"))
}

/// Directory containing the SHALLOWEST `level.dat` at or beneath `root`.
/// `None` if no `level.dat` exists. Breadth-first so a world nested one level
/// wins over a deeper sub-world. Symlinks are never followed.
fn find_world_root(root: &Path) -> Option<PathBuf> {
    let mut queue: VecDeque<PathBuf> = VecDeque::new();
    queue.push_back(root.to_path_buf());
    while let Some(dir) = queue.pop_front() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        let mut subdirs: Vec<PathBuf> = Vec::new();
        let mut has_level_dat = false;
        for entry in entries.flatten() {
            let ft = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };
            if ft.is_symlink() {
                continue;
            }
            let path = entry.path();
            if ft.is_dir() {
                subdirs.push(path);
            } else if ft.is_file() && path.file_name().and_then(|s| s.to_str()) == Some("level.dat")
            {
                has_level_dat = true;
            }
        }
        if has_level_dat {
            return Some(dir);
        }
        subdirs.sort();
        for s in subdirs {
            queue.push_back(s);
        }
    }
    None
}

/// First free `saves/<name>` slot: `name`, then `name (2)`, `name (3)`, … up
/// to `(999)`. `WorldNameUnresolvable` if all are taken.
fn pick_free_world_name(saves: &Path, base: &str) -> Result<String> {
    // `try_exists`, not `exists`: `exists()` answers false for ANY stat
    // failure, so a transient error would report an occupied slot as free —
    // and `Move::Copy` then merges the import INTO that existing world
    // (`create_dir_all` + `copy_tree` do not object to a populated dest),
    // while `Move::Rename` renames onto it. Unread ⇒ occupied is the
    // restrictive direction (CLAUDE.md, Fallback discipline, question 1); a
    // skipped slot at worst costs a " (2)" suffix.
    if !saves.join(base).try_exists().unwrap_or(true) {
        return Ok(base.to_string());
    }
    for i in 2..=999 {
        let candidate = format!("{base} ({i})");
        if !saves.join(&candidate).try_exists().unwrap_or(true) {
            return Ok(candidate);
        }
    }
    Err(Error::WorldNameUnresolvable {
        folder_name: base.into(),
    })
}

/// Recursively copy `src` into `dst`. Skips symlinks (never follows them).
/// Enforces per-file and running-aggregate byte caps. Caller owns cleanup of
/// `dst` on error.
fn copy_tree(
    src: &Path,
    dst: &Path,
    aggregate: &mut u64,
    per_file_cap: u64,
    aggregate_cap: u64,
) -> Result<()> {
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
            std::fs::create_dir_all(&to).map_err(|e| Error::io(to.display().to_string(), e))?;
            copy_tree(&from, &to, aggregate, per_file_cap, aggregate_cap)?;
        } else if ft.is_file() {
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            if size > per_file_cap {
                return Err(Error::WorldImportTooLarge {
                    size: size as f64,
                    cap: per_file_cap as f64,
                });
            }
            *aggregate = aggregate.saturating_add(size);
            if *aggregate > aggregate_cap {
                return Err(Error::WorldImportTooLarge {
                    size: *aggregate as f64,
                    cap: aggregate_cap as f64,
                });
            }
            std::fs::copy(&from, &to).map_err(|e| Error::io(to.display().to_string(), e))?;
        }
    }
    Ok(())
}

/// Copy the world subtree into `dest`, removing `dest` again if the copy
/// fails. The rollback's own result is checked (CLAUDE.md, Fallback
/// discipline, question 4): `list_worlds` treats ANY direct subdirectory of
/// `saves/` as a world, so a partial `saves/<chosen>` surviving a swallowed
/// rollback shows up in the user's world list looking real, while the UI
/// claims only "import failed".
///
/// `remove` is injected rather than called directly so a test can fail the
/// rollback deterministically on Windows, Linux AND macOS — the seam shape
/// `restore::swap_in_place` established for its renames. Production passes
/// `std::fs::remove_dir_all`.
///
/// On rollback failure both causes are `diag!`-logged here, where the full
/// context still exists, and the returned `WorldImportPartialLeft` carries
/// only the folder NAME — a bare segment inside a fully translated sentence,
/// the same rule `WorldRestoreStranded` documents.
fn copy_world_with_rollback(
    world_root: &Path,
    dest: &Path,
    chosen: &str,
    remove: &dyn Fn(&Path) -> std::io::Result<()>,
) -> Result<()> {
    let mut aggregate = 0u64;
    let Err(e) = copy_tree(
        world_root,
        dest,
        &mut aggregate,
        PER_FILE_CAP,
        AGGREGATE_CAP,
    ) else {
        return Ok(());
    };
    match remove(dest) {
        Ok(()) => Err(e),
        // Already gone (external deletion between the failed copy and the
        // rollback): nothing is left behind, so nothing to report beyond the
        // copy failure itself — same discrimination as `rollback_partial_instance`.
        Err(rb) if rb.kind() == std::io::ErrorKind::NotFound => Err(e),
        Err(rb) => {
            crate::diag!(
                "world import: copy into {} failed ({e}) AND rollback failed ({rb}); partial copy left",
                dest.display()
            );
            Err(Error::WorldImportPartialLeft {
                folder_name: chosen.to_string(),
            })
        }
    }
}

/// Reject a zip whose declared (uncompressed) sizes exceed the caps — the
/// zip-bomb defense (trust central-directory sizes, not on-disk bytes). A
/// non-zip / unreadable archive surfaces as `WorldImportInvalidArchive`.
fn check_zip_size(zip_path: &Path, per_file_cap: u64, aggregate_cap: u64) -> Result<()> {
    let file =
        std::fs::File::open(zip_path).map_err(|e| Error::io(zip_path.display().to_string(), e))?;
    let mut archive = zip::ZipArchive::new(std::io::BufReader::new(file)).map_err(|e| {
        Error::WorldImportInvalidArchive {
            details: format!("open: {e}"),
        }
    })?;
    let mut total: u64 = 0;
    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .map_err(|e| Error::WorldImportInvalidArchive {
                details: format!("entry {i}: {e}"),
            })?;
        let size = entry.size();
        if size > per_file_cap {
            return Err(Error::WorldImportTooLarge {
                size: size as f64,
                cap: per_file_cap as f64,
            });
        }
        total = total.saturating_add(size);
        if total > aggregate_cap {
            return Err(Error::WorldImportTooLarge {
                size: total as f64,
                cap: aggregate_cap as f64,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write as _;
    use tempfile::tempdir;
    use zip::write::SimpleFileOptions;

    fn touch(path: &Path) {
        if let Some(p) = path.parent() {
            fs::create_dir_all(p).unwrap();
        }
        fs::write(path, b"x").unwrap();
    }

    fn make_world_dir(parent: &Path, world_name: &str) -> PathBuf {
        let w = parent.join(world_name);
        touch(&w.join("level.dat"));
        touch(&w.join("region").join("r.0.0.mca"));
        w
    }

    /// Build a zip; return the TempDir guard (keep it alive for the test) and
    /// the zip path inside it.
    fn make_zip(entries: &[(&str, &[u8])]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("in.zip");
        let f = fs::File::create(&path).unwrap();
        let mut w = zip::ZipWriter::new(f);
        let opts = SimpleFileOptions::default();
        for (name, body) in entries {
            if name.ends_with('/') {
                w.add_directory(*name, opts).unwrap();
            } else {
                w.start_file(*name, opts).unwrap();
                w.write_all(body).unwrap();
            }
        }
        w.finish().unwrap();
        (dir, path)
    }

    fn rename_zip(src: PathBuf, new_name: &str) -> PathBuf {
        let dest = src.parent().unwrap().join(new_name);
        fs::rename(&src, &dest).unwrap();
        dest
    }

    #[test]
    fn find_world_root_at_top() {
        let td = tempdir().unwrap();
        touch(&td.path().join("level.dat"));
        assert_eq!(find_world_root(td.path()), Some(td.path().to_path_buf()));
    }

    #[test]
    fn find_world_root_nested_one_level() {
        let td = tempdir().unwrap();
        touch(&td.path().join("MyMap").join("level.dat"));
        assert_eq!(find_world_root(td.path()), Some(td.path().join("MyMap")));
    }

    #[test]
    fn find_world_root_nested_two_levels() {
        let td = tempdir().unwrap();
        touch(&td.path().join("a").join("MyMap").join("level.dat"));
        assert_eq!(
            find_world_root(td.path()),
            Some(td.path().join("a").join("MyMap"))
        );
    }

    #[test]
    fn find_world_root_prefers_shallowest() {
        let td = tempdir().unwrap();
        touch(&td.path().join("Top").join("level.dat"));
        touch(
            &td.path()
                .join("Top")
                .join("deep")
                .join("Sub")
                .join("level.dat"),
        );
        assert_eq!(find_world_root(td.path()), Some(td.path().join("Top")));
    }

    #[test]
    fn find_world_root_none_without_level_dat() {
        let td = tempdir().unwrap();
        touch(&td.path().join("readme.txt"));
        assert_eq!(find_world_root(td.path()), None);
    }

    #[test]
    fn pick_free_world_name_unused_returns_base() {
        let td = tempdir().unwrap();
        assert_eq!(pick_free_world_name(td.path(), "World").unwrap(), "World");
    }

    #[test]
    fn pick_free_world_name_suffixes_on_collision() {
        let td = tempdir().unwrap();
        fs::create_dir_all(td.path().join("World")).unwrap();
        assert_eq!(
            pick_free_world_name(td.path(), "World").unwrap(),
            "World (2)"
        );
        fs::create_dir_all(td.path().join("World (2)")).unwrap();
        assert_eq!(
            pick_free_world_name(td.path(), "World").unwrap(),
            "World (3)"
        );
    }

    // NOTE (fallback discipline, Q2): a transient stat failure on a SINGLE
    // candidate inside an otherwise healthy saves dir cannot be forced
    // deterministically on Windows+Linux+macOS — the same impossibility
    // restore.rs documents (lines 110-117) for failing one specific rename.
    // What CAN be forced portably on unix is "every stat fails": a FILE where
    // the saves directory is expected makes each `saves.join(...)` lookup
    // fail with NotADirectory — "could not tell", not "absent". The
    // restrictive reading is: every slot occupied. The Windows-only remainder
    // is restrictive-by-construction after the fix (`unwrap_or(true)`).
    #[cfg(unix)]
    #[test]
    fn pick_free_world_name_unreadable_slots_read_as_occupied() {
        let td = tempdir().unwrap();
        let saves_is_a_file = td.path().join("saves");
        fs::write(&saves_is_a_file, b"not a directory").unwrap();
        let r = pick_free_world_name(&saves_is_a_file, "World");
        assert!(
            matches!(r, Err(Error::WorldNameUnresolvable { .. })),
            "a stat failure must read as occupied, not free — got: {r:?}"
        );
    }

    #[test]
    fn copy_tree_copies_files_and_subdirs() {
        let src = tempdir().unwrap();
        touch(&src.path().join("level.dat"));
        touch(&src.path().join("region").join("r.0.0.mca"));
        let dst = tempdir().unwrap();
        let mut agg = 0u64;
        copy_tree(
            src.path(),
            dst.path(),
            &mut agg,
            PER_FILE_CAP,
            AGGREGATE_CAP,
        )
        .unwrap();
        assert!(dst.path().join("level.dat").is_file());
        assert!(dst.path().join("region").join("r.0.0.mca").is_file());
    }

    #[test]
    fn copy_tree_rejects_over_aggregate_cap() {
        let src = tempdir().unwrap();
        fs::write(src.path().join("a.bin"), vec![0u8; 100]).unwrap();
        fs::write(src.path().join("b.bin"), vec![0u8; 100]).unwrap();
        let dst = tempdir().unwrap();
        let mut agg = 0u64;
        let r = copy_tree(src.path(), dst.path(), &mut agg, PER_FILE_CAP, 150);
        assert!(
            matches!(r, Err(Error::WorldImportTooLarge { .. })),
            "got: {r:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn copy_tree_skips_symlinks() {
        use std::os::unix::fs::symlink;
        let src = tempdir().unwrap();
        touch(&src.path().join("level.dat"));
        let outside = tempdir().unwrap();
        fs::write(outside.path().join("secret"), b"nope").unwrap();
        symlink(outside.path().join("secret"), src.path().join("link")).unwrap();
        let dst = tempdir().unwrap();
        let mut agg = 0u64;
        copy_tree(
            src.path(),
            dst.path(),
            &mut agg,
            PER_FILE_CAP,
            AGGREGATE_CAP,
        )
        .unwrap();
        assert!(dst.path().join("level.dat").is_file());
        assert!(!dst.path().join("link").exists(), "symlink must be skipped");
    }

    #[test]
    fn copy_rollback_removes_partial_copy_and_returns_the_copy_error() {
        let src = tempdir().unwrap();
        touch(&src.path().join("level.dat"));
        touch(&src.path().join("data.bin"));
        let saves = tempdir().unwrap();
        let dest = saves.path().join("World");
        fs::create_dir_all(&dest).unwrap();
        // Poison the copy: a DIRECTORY where `fs::copy` must write a file
        // fails on every platform (the poison-the-destination trick
        // l10n/prefill/run/state.rs:518 uses against a store write).
        fs::create_dir_all(dest.join("data.bin")).unwrap();
        let r =
            copy_world_with_rollback(src.path(), &dest, "World", &|p| std::fs::remove_dir_all(p));
        assert!(matches!(r, Err(Error::Io { .. })), "got: {r:?}");
        assert!(
            !dest.exists(),
            "the partial copy must be rolled back when removal succeeds"
        );
    }

    #[test]
    fn copy_rollback_failure_returns_partial_left_with_folder_name() {
        let src = tempdir().unwrap();
        touch(&src.path().join("level.dat"));
        touch(&src.path().join("data.bin"));
        let saves = tempdir().unwrap();
        let dest = saves.path().join("World (2)");
        fs::create_dir_all(&dest).unwrap();
        fs::create_dir_all(dest.join("data.bin")).unwrap();
        // Injected rollback failure — the deterministic cross-platform seam,
        // as in `restore::swap_in_place`.
        let r = copy_world_with_rollback(src.path(), &dest, "World (2)", &|_| {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "held open by another process",
            ))
        });
        match r {
            Err(Error::WorldImportPartialLeft { folder_name }) => {
                assert_eq!(folder_name, "World (2)");
            }
            other => panic!("expected WorldImportPartialLeft, got: {other:?}"),
        }
    }

    #[test]
    fn copy_rollback_already_gone_dest_keeps_the_copy_error() {
        let src = tempdir().unwrap();
        touch(&src.path().join("level.dat"));
        touch(&src.path().join("data.bin"));
        let saves = tempdir().unwrap();
        let dest = saves.path().join("World");
        fs::create_dir_all(&dest).unwrap();
        fs::create_dir_all(dest.join("data.bin")).unwrap();
        // Rollback finds nothing to remove (external deletion between the
        // failed copy and the rollback): the original copy error must come
        // back — never WorldImportPartialLeft about a path that is not there.
        let r = copy_world_with_rollback(src.path(), &dest, "World", &|_| {
            Err(std::io::Error::from(std::io::ErrorKind::NotFound))
        });
        assert!(matches!(r, Err(Error::Io { .. })), "got: {r:?}");
    }

    #[test]
    fn check_zip_size_ok_under_caps() {
        let (_z, zip) = make_zip(&[("level.dat", b"abc")]);
        assert!(check_zip_size(&zip, PER_FILE_CAP, AGGREGATE_CAP).is_ok());
    }

    #[test]
    fn check_zip_size_rejects_over_per_file_cap() {
        let (_z, zip) = make_zip(&[("level.dat", &[0u8; 64])]);
        let r = check_zip_size(&zip, 10, AGGREGATE_CAP);
        assert!(
            matches!(r, Err(Error::WorldImportTooLarge { .. })),
            "got: {r:?}"
        );
    }

    #[test]
    fn check_zip_size_rejects_over_aggregate_cap() {
        let (_z, zip) = make_zip(&[("a.bin", &[0u8; 40]), ("b.bin", &[0u8; 40])]);
        let r = check_zip_size(&zip, PER_FILE_CAP, 50);
        assert!(
            matches!(r, Err(Error::WorldImportTooLarge { .. })),
            "got: {r:?}"
        );
    }

    #[test]
    fn check_zip_size_rejects_non_zip() {
        let td = tempdir().unwrap();
        let path = td.path().join("not.zip");
        fs::write(&path, b"not a zip").unwrap();
        let r = check_zip_size(&path, PER_FILE_CAP, AGGREGATE_CAP);
        assert!(
            matches!(r, Err(Error::WorldImportInvalidArchive { .. })),
            "got: {r:?}"
        );
    }

    #[test]
    fn import_folder_root_is_world() {
        let saves = tempdir().unwrap();
        let src_parent = tempdir().unwrap();
        let src = make_world_dir(src_parent.path(), "Adventure");
        let w = import_into_saves(saves.path(), &src).unwrap();
        assert_eq!(w.folder_name, "Adventure");
        assert!(saves.path().join("Adventure").join("level.dat").is_file());
        assert!(src.join("level.dat").is_file());
    }

    #[test]
    fn import_folder_nested_world() {
        let saves = tempdir().unwrap();
        let src = tempdir().unwrap();
        make_world_dir(src.path(), "Inner");
        let w = import_into_saves(saves.path(), src.path()).unwrap();
        assert_eq!(w.folder_name, "Inner");
        assert!(saves.path().join("Inner").join("level.dat").is_file());
    }

    #[test]
    fn import_folder_without_level_dat_rejected() {
        let saves = tempdir().unwrap();
        let src = tempdir().unwrap();
        touch(&src.path().join("readme.txt"));
        let r = import_into_saves(saves.path(), src.path());
        assert!(matches!(r, Err(Error::WorldImportNotAWorld)), "got: {r:?}");
    }

    #[test]
    fn import_folder_collision_keeps_both() {
        let saves = tempdir().unwrap();
        fs::create_dir_all(saves.path().join("Adventure")).unwrap();
        let src_parent = tempdir().unwrap();
        let src = make_world_dir(src_parent.path(), "Adventure");
        let w = import_into_saves(saves.path(), &src).unwrap();
        assert_eq!(w.folder_name, "Adventure (2)");
        assert!(saves
            .path()
            .join("Adventure (2)")
            .join("level.dat")
            .is_file());
    }

    #[test]
    fn import_zip_world_at_root_uses_zip_stem() {
        let saves = tempdir().unwrap();
        let (_z, raw) = make_zip(&[("level.dat", b"x"), ("region/r.mca", b"y")]);
        let zip = rename_zip(raw, "Skyblock.zip");
        let w = import_into_saves(saves.path(), &zip).unwrap();
        assert_eq!(w.folder_name, "Skyblock");
        assert!(saves.path().join("Skyblock").join("level.dat").is_file());
        assert!(saves
            .path()
            .join("Skyblock")
            .join("region")
            .join("r.mca")
            .is_file());
    }

    #[test]
    fn import_zip_nested_world_uses_folder_name() {
        let saves = tempdir().unwrap();
        let (_z, zip) = make_zip(&[("MyMap/level.dat", b"x"), ("MyMap/data/x.dat", b"y")]);
        let w = import_into_saves(saves.path(), &zip).unwrap();
        assert_eq!(w.folder_name, "MyMap");
        assert!(saves.path().join("MyMap").join("level.dat").is_file());
        let leftover: Vec<_> = fs::read_dir(saves.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with(".tmp-import-"))
            .collect();
        assert!(leftover.is_empty(), "staging dir not cleaned up");
    }

    #[test]
    fn import_zip_without_level_dat_rejected() {
        let saves = tempdir().unwrap();
        let (_z, zip) = make_zip(&[("notes.txt", b"x")]);
        let r = import_into_saves(saves.path(), &zip);
        assert!(matches!(r, Err(Error::WorldImportNotAWorld)), "got: {r:?}");
    }

    #[test]
    fn import_zip_slip_rejected_nothing_escapes() {
        let saves = tempdir().unwrap();
        let (_z, zip) = make_zip(&[("../escape.txt", b"pwned")]);
        let r = import_into_saves(saves.path(), &zip);
        assert!(
            matches!(r, Err(Error::WorldImportInvalidArchive { .. })),
            "got: {r:?}"
        );
        assert!(!saves.path().parent().unwrap().join("escape.txt").exists());
    }

    #[test]
    fn import_corrupt_zip_rejected() {
        let saves = tempdir().unwrap();
        let td = tempdir().unwrap();
        let bad = td.path().join("bad.zip");
        fs::write(&bad, b"not a zip").unwrap();
        let r = import_into_saves(saves.path(), &bad);
        assert!(
            matches!(r, Err(Error::WorldImportInvalidArchive { .. })),
            "got: {r:?}"
        );
    }

    #[test]
    fn import_unsupported_source_rejected() {
        let saves = tempdir().unwrap();
        let r = import_into_saves(saves.path(), Path::new("/no/such/path/x.bin"));
        assert!(
            matches!(r, Err(Error::WorldImportUnsupportedSource)),
            "got: {r:?}"
        );
    }
}
