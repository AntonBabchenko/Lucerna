//! Which worlds hold a given library pack, with an identity verdict per
//! world, plus the same-name reinstall fan-out built on top of it. See the
//! parent module doc for the lock rules.

use std::path::{Path, PathBuf};

use crate::datapacks::detect::Resolved;
use crate::datapacks::{library_dir_at, WorldEntryKind};
use crate::error::{Error, Result};
use crate::mods::store::{materialize, LinkPolicy};

use super::{level_dat_lock, world_dirs_checked};

/// One world holding an entry by a given name, and whether that entry is
/// provably the library's own content.
pub(crate) struct WorldPlacement {
    pub world: String,
    pub path: PathBuf,
    /// The on-disk spelling (R2). The engine's id is `file/` + this, not the
    /// library's spelling.
    pub entry_name: String,
    /// The world-side entry's sha1 equals the library entry's. `false` means a
    /// same-named entry the user (or a world import) put there — replacing it
    /// would be data loss, so every MUTATING caller must skip it and say so.
    pub is_ours: bool,
}

/// What a scan of every world found for one name.
pub(crate) struct Placements {
    /// Worlds holding an entry by the name, each with its identity verdict.
    pub found: Vec<WorldPlacement>,
    /// Worlds Lucerna could not check for the name, each with the reason: its
    /// `datapacks/` could not be listed, R2 could not tell (a stat error), or
    /// the entry could not be read to compare. "Could not check" is not "does
    /// not hold it" (Fallback discipline Q1): every caller reports these as a
    /// failure, which keeps the library copy so a retry converges.
    pub unchecked: Vec<(String, String)>,
}

/// THE rule for "is this world entry the library's own copy" — shared by
/// the placement scan (every mutating caller) and the removal confirmation
/// ([`world_entry_kind_at`]), so the dialog can never promise "stays in the
/// library" for an entry a cascade would call foreign. A directory is never
/// the library's copy (library entries are zip files; a folder has no file
/// sha1). A file is iff its sha1 equals the library file's. An unknown side
/// (`None`) never matches — the restrictive answer.
pub(crate) fn is_library_copy(
    entry_is_dir: bool,
    entry_sha: Option<&str>,
    lib_sha: Option<&str>,
) -> bool {
    !entry_is_dir && matches!((entry_sha, lib_sha), (Some(e), Some(l)) if e == l)
}

/// Every world whose `datapacks/` folder holds an entry named `filename`, with
/// world NAMES rather than paths, and an identity verdict per world.
///
/// This replaced a `worlds_linking` helper that answered only "does an entry
/// with this name exist there". That was enough for a same-name reinstall
/// (same name, same intent, and `materialize` writes the same bytes either
/// way), but NOT for an update, which deletes the old entry and links a
/// differently-named one: a same-named entry that is not ours must be left
/// alone. `materialize` commits by unconditional rename over the destination,
/// so a name-only check lets the update path silently destroy a pack the user
/// installed by hand — the F5 defect, reintroduced on the update path. The old
/// helper was deleted rather than kept, so no future caller can reach for the
/// unsafe one by mistake.
///
/// A directory entry is never `is_ours`: a folder datapack has no file sha1 to
/// compare against the library's zip.
///
/// `Err` = the library copy, or `saves/` itself, could not be read: no world
/// can be verified then, and there is no world to name in a report. A missing
/// library copy or `saves/` is a fact, not an error.
pub(crate) async fn placements_of(instance_root: &Path, filename: &str) -> Result<Placements> {
    let lib = library_dir_at(instance_root).join(filename);
    let lib_sha = match tokio::fs::read(&lib).await {
        Ok(bytes) => Some(crate::datapacks::library::sha1_hex(&bytes)),
        // No library copy (a pack left only in worlds): nothing can be ours.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(Error::io(lib.display().to_string(), e)),
    };
    placements_against(instance_root, filename, lib_sha.as_deref()).await
}

/// [`placements_of`]'s body, with the identity reference supplied by the
/// caller instead of read from the library file. The distinction matters on
/// the reinstall path: by the time the fan-out runs, the library file already
/// holds the NEW bytes, so hashing against it would classify every
/// legitimately-linked-but-stale world as foreign. The caller captures the
/// OLD file's sha1 before replacing it and passes it here.
async fn placements_against(
    instance_root: &Path,
    filename: &str,
    lib_sha: Option<&str>,
) -> Result<Placements> {
    let saves_dir = instance_root.join(".minecraft").join("saves");
    let rd = match std::fs::read_dir(&saves_dir) {
        Ok(rd) => rd,
        // No saves/ at all: no world can hold the pack.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Placements {
                found: Vec::new(),
                unchecked: Vec::new(),
            })
        }
        Err(e) => return Err(Error::io(saves_dir.display().to_string(), e)),
    };
    let mut out = Placements {
        found: Vec::new(),
        unchecked: Vec::new(),
    };
    for entry in rd {
        // An entry that cannot be read names no world to report.
        let entry = entry.map_err(|e| Error::io(saves_dir.display().to_string(), e))?;
        let Some(world) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        // Match `worlds::list_worlds`' own filter: without it a dot-directory
        // under `saves/` would be treated as a world, and `world_dirs_checked`
        // would then reject it downstream with a confusing path error instead
        // of it never having been offered.
        if crate::worlds::fs::validate_segment(&world).is_err() {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(meta) => meta,
            Err(e) => {
                out.unchecked
                    .push((world, format!("could not read its folder: {e}")));
                continue;
            }
        };
        if !meta.is_dir() {
            continue;
        }
        let dp = entry.path().join("datapacks");
        // R2 (N.3): the entry this library name denotes in this world.
        let entry_name = match super::resolve_on_disk(&dp, filename).await {
            Ok((_, Resolved::Exact(n) | Resolved::Folded(n))) => n,
            Ok((_, Resolved::Absent)) => continue,
            Ok((_, Resolved::Unknown(e))) => {
                out.unchecked.push((
                    world,
                    format!(
                        "could not tell whether {} holds {filename}: {e}",
                        dp.display()
                    ),
                ));
                continue;
            }
            Err(e) => {
                out.unchecked
                    .push((world, format!("could not list {}: {e}", dp.display())));
                continue;
            }
        };
        let candidate = dp.join(&entry_name);
        let cand_meta = match tokio::fs::metadata(&candidate).await {
            Ok(meta) => meta,
            // Gone since the listing: the world no longer holds it.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                out.unchecked.push((
                    world,
                    format!("could not read {}: {e}", candidate.display()),
                ));
                continue;
            }
        };
        // A folder is never the library's zip, and with no library copy
        // nothing can be proven ours: neither needs the entry's bytes. Nor
        // does anything else that is not a regular file (a FIFO, a socket, a
        // device; followed, so a link to a file is a file): it is not the
        // library's zip, and opening a FIFO would block until a writer
        // appears — under `level_dat_lock` for every mutating caller.
        let entry_sha = if !cand_meta.is_file() || lib_sha.is_none() {
            None
        } else {
            match tokio::fs::read(&candidate).await {
                Ok(bytes) => Some(crate::datapacks::library::sha1_hex(&bytes)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                // Could not compare: not "not ours", which a caller would
                // leave alone and then report the job done.
                Err(e) => {
                    out.unchecked.push((
                        world,
                        format!("could not read {}: {e}", candidate.display()),
                    ));
                    continue;
                }
            }
        };
        let is_ours = is_library_copy(cand_meta.is_dir(), entry_sha.as_deref(), lib_sha);
        out.found.push(WorldPlacement {
            world,
            path: candidate,
            entry_name,
            is_ours,
        });
    }
    Ok(out)
}

/// What removing `filename` from `world` would do, for the "remove from this
/// world" confirmation (spec 2026-09-24 §4 U1, A20). Read-only. The name is
/// resolved exactly as `remove_from_world_at` resolves it (R2), so the answer
/// is about the entry the removal would delete:
/// - nothing on disk under that name → `Missing` (only the level.dat entry goes);
/// - a directory → `OwnFolder`;
/// - a file → [`is_library_copy`] against `library/<filename>`: `LibraryCopy`,
///   else `OwnFile`; no library file at all is `OwnFile`.
///
/// Fallback discipline: every listing, stat or read failure other than a
/// named NotFound is `Err` ("could not tell"). It is never guessed into
/// `LibraryCopy`, the one answer that promises nothing is lost; the dialog
/// words an `Err` as "couldn't check — if it isn't the library's copy it is
/// deleted permanently". Nothing is written, so there is no recovery path.
/// Both reads and the hashes run in ONE `spawn_blocking`.
pub(crate) async fn world_entry_kind_at(
    instance_root: &Path,
    world: &str,
    filename: &str,
) -> Result<WorldEntryKind> {
    if !crate::pathsafe::is_safe_filename(filename) {
        return Err(Error::ModsUnsafeFilename {
            filename: filename.to_string(),
        });
    }
    let (_world_dir, dp_dir) = world_dirs_checked(instance_root, world)?;
    // R2, as the removal does: on a case-sensitive file system a case
    // variant is a different pack, and nothing of it would be deleted.
    let (_names, resolved) = super::resolve_on_disk(&dp_dir, filename)
        .await
        .map_err(|e| Error::io(dp_dir.display().to_string(), e))?;
    let entry_name = match resolved {
        Resolved::Exact(n) | Resolved::Folded(n) => n,
        Resolved::Absent => return Ok(WorldEntryKind::Missing),
        Resolved::Unknown(e) => {
            return Err(Error::io(dp_dir.join(filename).display().to_string(), e))
        }
    };
    let entry = dp_dir.join(&entry_name);
    // Followed, as `remove_entry` follows it to pick its removal call.
    let entry_meta = match tokio::fs::metadata(&entry).await {
        Ok(meta) => meta,
        // Gone since the listing: the removal would delete nothing either.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(WorldEntryKind::Missing),
        Err(e) => return Err(Error::io(entry.display().to_string(), e)),
    };
    if entry_meta.is_dir() {
        return Ok(WorldEntryKind::OwnFolder);
    }
    // Not a regular file (a FIFO, a socket, a device): never the library's
    // zip, and reading a FIFO would block the dialog's check forever.
    if !entry_meta.is_file() {
        return Ok(WorldEntryKind::OwnFile);
    }
    let lib = library_dir_at(instance_root).join(filename);
    let label = entry.display().to_string();
    tokio::task::spawn_blocking(move || identify_world_file(&entry, entry_meta.len(), &lib))
        .await
        .map_err(|e| Error::io(label, format!("join: {e}")))?
}

/// [`world_entry_kind_at`]'s blocking half, for a world entry that is a
/// regular file of `entry_len` bytes. Lengths are compared before hashing:
/// different lengths cannot be the same bytes, so a large foreign file
/// costs one stat, not two full reads.
fn identify_world_file(entry: &Path, entry_len: u64, lib: &Path) -> Result<WorldEntryKind> {
    let lib_len = match std::fs::metadata(lib) {
        Ok(meta) => meta.len(),
        // No library copy for this file to be: it is the only copy.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(WorldEntryKind::OwnFile),
        Err(e) => return Err(Error::io(lib.display().to_string(), e)),
    };
    if lib_len != entry_len {
        return Ok(WorldEntryKind::OwnFile);
    }
    let world_sha = std::fs::read(entry)
        .map(|b| crate::datapacks::library::sha1_hex(&b))
        .map_err(|e| Error::io(entry.display().to_string(), e))?;
    let lib_sha = std::fs::read(lib)
        .map(|b| crate::datapacks::library::sha1_hex(&b))
        .map_err(|e| Error::io(lib.display().to_string(), e))?;
    Ok(
        if is_library_copy(false, Some(&world_sha), Some(&lib_sha)) {
            WorldEntryKind::LibraryCopy
        } else {
            WorldEntryKind::OwnFile
        },
    )
}

/// The same-name reinstall fan-out: push the library's (already replaced)
/// bytes into every world whose `datapacks/` holds this filename AND whose
/// current content is provably the pack being replaced — `expected_sha` is the
/// OLD library file's hash, captured by the caller before `place_bytes` ran.
/// A same-named entry that does not match is a pack the user (or a world
/// import) put there themselves; replacing it would be the F5 data loss, so it
/// is skipped and reported. `None` (fresh install, or the old file was
/// unreadable) skips every candidate — the safe direction. A world that could
/// not be checked is reported `Failed`: it may still hold the stale bytes.
///
/// level.dat is deliberately never touched: each world's own enabled/disabled
/// choice stands, which is why the per-world outcome is
/// [`WorldMigration::Refreshed`], not `Migrated`.
///
/// Takes [`level_dat_lock`] once, and snapshots the placements INSIDE it, so
/// a concurrent locked removal cannot interleave between the snapshot and the
/// writes — without the lock, `remove_from_world_at` could delete a world's
/// file and level.dat entry and this fan-out would then re-materialize the
/// file from its stale snapshot, leaving it present-and-unlisted, which
/// Minecraft auto-enables: a silently resurrected, just-removed pack.
///
/// A FRESH install (`expected_sha: None`) has no old library copy to compare
/// a world's file with, so it refreshes no world. A world can still hold a
/// file under the name — one a library removal without cascade left behind —
/// but that file is the world's own copy now, reported `SkippedNotOurs`. So a
/// world it could not check loses nothing, and it reports nothing for it. A
/// reinstall that cannot even list `saves/` reports one `Failed` entry naming
/// that folder: the install itself went through, and the worlds may still be
/// on the old bytes.
pub(crate) async fn refresh_placements(
    instance_root: &Path,
    filename: &str,
    expected_sha: Option<&str>,
) -> Vec<crate::datapacks::WorldMigration> {
    use crate::datapacks::WorldMigration;

    let src = library_dir_at(instance_root).join(filename);

    let _guard = level_dat_lock().lock().await;

    let placements = match placements_against(instance_root, filename, expected_sha).await {
        Ok(placements) => placements,
        Err(e) if expected_sha.is_none() => {
            crate::diag!(
                "datapacks: fresh install of {filename} could not list the worlds; none was \
                 refreshed, and none would have been (there is no old library copy to refresh \
                 from): {e}"
            );
            return Vec::new();
        }
        Err(e) => {
            let saves = instance_root.join(".minecraft").join("saves");
            return vec![WorldMigration::Failed {
                world: saves.display().to_string(),
                details: format!("no world was refreshed: {e}"),
            }];
        }
    };
    let mut report = Vec::with_capacity(placements.found.len() + placements.unchecked.len());
    for p in placements.found {
        if !p.is_ours {
            report.push(WorldMigration::SkippedNotOurs { world: p.world });
            continue;
        }
        match materialize(&src, &p.path, LinkPolicy::LinkIfPossible).await {
            Ok(_) => report.push(WorldMigration::Refreshed { world: p.world }),
            Err(e) => report.push(WorldMigration::Failed {
                world: p.world,
                details: e.details(),
            }),
        }
    }
    // Only a reinstall refreshes, so only a reinstall can leave a world on the
    // old library bytes.
    if expected_sha.is_some() {
        for (world, details) in placements.unchecked {
            report.push(WorldMigration::Failed { world, details });
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datapacks::world_link::add_to_world_at;
    use crate::datapacks::world_link::test_util::*;

    #[tokio::test]
    async fn placements_of_marks_only_the_library_content_as_ours() {
        let _lock = hardlink_lock();
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "vm.zip", 48).await;
        let saves = td.path().join(".minecraft").join("saves");
        game_world(td.path(), "Ours");
        std::fs::create_dir_all(saves.join("Theirs").join("datapacks")).unwrap();
        add_to_world_at(td.path(), "Ours", "vm.zip").await.unwrap();
        // Same NAME, different content — a pack the user dropped in by hand.
        std::fs::write(
            saves.join("Theirs").join("datapacks").join("vm.zip"),
            datapack_zip(57),
        )
        .unwrap();

        let found = placements_of(td.path(), "vm.zip").await.unwrap().found;

        let mut ours: Vec<&str> = found
            .iter()
            .filter(|p| p.is_ours)
            .map(|p| p.world.as_str())
            .collect();
        ours.sort_unstable();
        assert_eq!(ours, vec!["Ours"]);
        let theirs = found
            .iter()
            .find(|p| p.world == "Theirs")
            .expect("Theirs must still be listed, just not claimed");
        assert!(!theirs.is_ours);
    }

    use crate::datapacks::WorldEntryKind;

    #[test]
    fn the_identity_rule_never_matches_a_folder_or_an_unknown_side() {
        assert!(is_library_copy(false, Some("a"), Some("a")));
        assert!(!is_library_copy(true, Some("a"), Some("a")));
        assert!(!is_library_copy(false, None, Some("a")));
        assert!(!is_library_copy(false, Some("a"), None));
        assert!(!is_library_copy(false, Some("a"), Some("b")));
    }

    #[tokio::test]
    async fn a_same_named_world_file_with_other_bytes_is_own_file() {
        let _lock = hardlink_lock();
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "vm.zip", 48).await;
        let dp = world_dir(td.path(), "W").join("datapacks");
        std::fs::create_dir_all(&dp).unwrap();
        // Same NAME and — two-digit formats — same LENGTH: only the hash can
        // tell them apart, so a size-only shortcut would fail this test.
        std::fs::write(dp.join("vm.zip"), datapack_zip(57)).unwrap();
        assert_eq!(
            world_entry_kind_at(td.path(), "W", "vm.zip").await.unwrap(),
            WorldEntryKind::OwnFile
        );
    }

    #[tokio::test]
    async fn a_hardlinked_library_copy_is_library_copy() {
        let _lock = hardlink_lock();
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "vm.zip", 48).await;
        let dp = world_dir(td.path(), "W").join("datapacks");
        std::fs::create_dir_all(&dp).unwrap();
        std::fs::hard_link(library_dir_at(td.path()).join("vm.zip"), dp.join("vm.zip")).unwrap();
        assert_eq!(
            world_entry_kind_at(td.path(), "W", "vm.zip").await.unwrap(),
            WorldEntryKind::LibraryCopy
        );
    }

    #[tokio::test]
    async fn a_folder_is_own_folder() {
        let td = tempfile::tempdir().unwrap();
        let pack = world_dir(td.path(), "W").join("datapacks").join("MyPack");
        std::fs::create_dir_all(&pack).unwrap();
        std::fs::write(
            pack.join("pack.mcmeta"),
            br#"{"pack":{"pack_format":48,"description":"x"}}"#,
        )
        .unwrap();
        assert_eq!(
            world_entry_kind_at(td.path(), "W", "MyPack").await.unwrap(),
            WorldEntryKind::OwnFolder
        );
    }

    #[tokio::test]
    async fn absent_is_missing() {
        let td = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(world_dir(td.path(), "W")).unwrap();
        assert_eq!(
            world_entry_kind_at(td.path(), "W", "vm.zip").await.unwrap(),
            WorldEntryKind::Missing
        );
    }

    /// A FIFO under the pack's name is not the library's copy, and reading it
    /// to hash it would block until a writer appears — under `level_dat_lock`
    /// for every mutating caller of `placements_of`, and forever in the
    /// removal dialog's check. Both judge it by its type, never opening it.
    /// `cfg(unix)`: Windows has no FIFO in a folder, so this runs in CI only.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_fifo_named_like_the_pack_is_not_ours_and_is_never_opened() {
        use std::os::unix::ffi::OsStrExt;
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "vm.zip", 48).await;
        let dp = game_world(td.path(), "W").join("datapacks");
        std::fs::create_dir_all(&dp).unwrap();
        let fifo = dp.join("vm.zip");
        let c = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        // SAFETY: `c` is a valid NUL-terminated path that outlives the call.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        let unblock = || {
            // Release a blocked reader so the runtime can shut down.
            drop(
                std::fs::OpenOptions::new()
                    .write(true)
                    .open(&fifo)
                    .expect("open the FIFO's write end"),
            );
        };
        let budget = std::time::Duration::from_secs(5);

        let Ok(found) = tokio::time::timeout(budget, placements_of(td.path(), "vm.zip")).await
        else {
            unblock();
            panic!("the placement scan opened the FIFO and blocked");
        };
        let found = found.unwrap();
        assert!(found.unchecked.is_empty(), "{:?}", found.unchecked);
        assert_eq!(found.found.len(), 1);
        assert!(!found.found[0].is_ours);

        // A FIFO's length is 0. Against the seeded library zip, the removal
        // check's length comparison would answer OwnFile before any read, and
        // this half would pass with the type check gone. An empty library
        // file makes the lengths equal, so only the type check keeps the FIFO
        // from being read.
        std::fs::write(library_dir_at(td.path()).join("vm.zip"), b"").unwrap();
        let Ok(kind) =
            tokio::time::timeout(budget, world_entry_kind_at(td.path(), "W", "vm.zip")).await
        else {
            unblock();
            panic!("the removal check opened the FIFO and blocked");
        };
        assert_eq!(kind.unwrap(), WorldEntryKind::OwnFile);
    }

    #[tokio::test]
    async fn a_world_file_with_no_library_copy_is_own_file() {
        let td = tempfile::tempdir().unwrap();
        let dp = world_dir(td.path(), "W").join("datapacks");
        std::fs::create_dir_all(&dp).unwrap();
        std::fs::write(dp.join("vm.zip"), datapack_zip(48)).unwrap();
        assert_eq!(
            world_entry_kind_at(td.path(), "W", "vm.zip").await.unwrap(),
            WorldEntryKind::OwnFile
        );
    }
}
