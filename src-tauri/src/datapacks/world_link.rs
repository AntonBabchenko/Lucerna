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

use std::collections::{BTreeMap, HashMap};
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

/// `Some(err)` when `dest` already holds something that is NOT the library
/// file at `src`, so placing over it would destroy a pack Lucerna did not put
/// there. `None` when the destination is free, or already holds exactly these
/// bytes.
///
/// A DIRECTORY is always a conflict: Minecraft loads folder datapacks, and a
/// folder has no file sha1 to compare, so it can never be proven ours. Doing
/// otherwise would let `materialize` rename a zip over a whole pack folder.
///
/// Sizes are compared before hashing so a large pack costs one `metadata` call
/// in the common "different pack" case rather than two full reads.
async fn conflicting_world_entry(src: &Path, dest: &Path) -> Result<Option<Error>> {
    let Ok(dest_meta) = tokio::fs::metadata(dest).await else {
        return Ok(None); // nothing there — free to place
    };
    let filename = dest
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    if dest_meta.is_dir() {
        return Ok(Some(Error::ModsFilenameConflict {
            filename,
            existing_sha: String::new(),
            incoming_sha: String::new(),
        }));
    }
    let src_meta = tokio::fs::metadata(src)
        .await
        .map_err(|e| Error::io(src.display().to_string(), e))?;
    if src_meta.len() == dest_meta.len() {
        let (Ok(a), Ok(b)) = (tokio::fs::read(src).await, tokio::fs::read(dest).await) else {
            return Ok(None);
        };
        let (sa, sb) = (
            crate::datapacks::library::sha1_hex(&a),
            crate::datapacks::library::sha1_hex(&b),
        );
        if sa == sb {
            return Ok(None); // already ours — fall through so a disabled pack re-enables
        }
        return Ok(Some(Error::ModsFilenameConflict {
            filename,
            existing_sha: sb,
            incoming_sha: sa,
        }));
    }
    // Different sizes ⇒ different content; no need to hash either side.
    let existing_sha = match tokio::fs::read(dest).await {
        Ok(b) => crate::datapacks::library::sha1_hex(&b),
        Err(_) => String::new(),
    };
    let incoming_sha = match tokio::fs::read(src).await {
        Ok(b) => crate::datapacks::library::sha1_hex(&b),
        Err(_) => String::new(),
    };
    Ok(Some(Error::ModsFilenameConflict {
        filename,
        existing_sha,
        incoming_sha,
    }))
}

/// One world holding an entry by a given name, and whether that entry is
/// provably the library's own content.
pub(crate) struct WorldPlacement {
    pub world: String,
    pub path: PathBuf,
    /// The world-side entry's sha1 equals the library entry's. `false` means a
    /// same-named entry the user (or a world import) put there — replacing it
    /// would be data loss, so every MUTATING caller must skip it and say so.
    pub is_ours: bool,
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
pub(crate) async fn placements_of(instance_root: &Path, filename: &str) -> Vec<WorldPlacement> {
    let lib_sha = match tokio::fs::read(library_dir_at(instance_root).join(filename)).await {
        Ok(bytes) => Some(crate::datapacks::library::sha1_hex(&bytes)),
        Err(_) => None,
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
) -> Vec<WorldPlacement> {
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
        let candidate = entry.path().join("datapacks").join(filename);
        let Ok(cand_meta) = tokio::fs::metadata(&candidate).await else {
            continue;
        };
        let is_ours = if cand_meta.is_dir() {
            false
        } else {
            match (lib_sha, tokio::fs::read(&candidate).await) {
                (Some(lib), Ok(bytes)) => crate::datapacks::library::sha1_hex(&bytes) == lib,
                _ => false,
            }
        };
        out.push(WorldPlacement {
            world,
            path: candidate,
            is_ours,
        });
    }
    out
}

/// The same-name reinstall fan-out: push the library's (already replaced)
/// bytes into every world whose `datapacks/` holds this filename AND whose
/// current content is provably the pack being replaced — `expected_sha` is the
/// OLD library file's hash, captured by the caller before `place_bytes` ran.
/// A same-named entry that does not match is a pack the user (or a world
/// import) put there themselves; replacing it would be the F5 data loss, so it
/// is skipped and reported. `None` (fresh install, or the old file was
/// unreadable) skips every candidate — the safe direction.
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
pub(crate) async fn refresh_placements(
    instance_root: &Path,
    filename: &str,
    expected_sha: Option<&str>,
) -> Vec<crate::datapacks::WorldMigration> {
    use crate::datapacks::WorldMigration;

    let src = library_dir_at(instance_root).join(filename);

    let _guard = level_dat_lock().lock().await;

    let placements = placements_against(instance_root, filename, expected_sha).await;
    let mut report = Vec::with_capacity(placements.len());
    for p in placements {
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
    report
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

    // Refuse to replace a DIFFERENT entry that already holds this name.
    // `materialize` commits by unconditional rename over the destination, so
    // without this a pack the user dropped into the world folder themselves —
    // or one that arrived with an imported world — is destroyed silently.
    //
    // Matching content deliberately falls THROUGH rather than returning early:
    // re-adding a pack that is present but sits in the world's `Disabled` list
    // must re-enable it, which the `set_enabled(.., true)` below does. That is
    // exactly what the library screen's world picker relies on when a user
    // ticks a world where the pack is currently off, so turning this into a
    // no-op would delete a behaviour the UI depends on.
    if let Some(conflict) = conflicting_world_entry(&src, &dest).await? {
        return Err(conflict);
    }

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
    // `forget_ci`, not `forget`: the name arrives from a UI row or — on the
    // cascade path — from the library registry, and level.dat may spell the
    // same file with different case (NTFS is case-insensitive; the drift is
    // documented and encountered — see `contains_ci`). An exact match would
    // delete the file but keep the name: a permanent Orphaned row and
    // Minecraft's "data packs are no longer present" screen.
    if level_dat::forget_ci(&mut root, &entry)? {
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

/// Move every world holding `old_filename` onto `new_filename`, preserving each
/// world's own enabled/disabled choice.
///
/// Both library files must already exist: the caller installs the new one
/// first and deletes the old one afterwards. This function owns only the world
/// side.
///
/// **It lives here, and takes [`level_dat_lock`] itself, for a reason.** The
/// lock is a non-reentrant `tokio::sync::Mutex` and all three public entry
/// points above take it internally, so a caller outside this module cannot
/// compose them into a read → unlink → relink → rewrite sequence: doing so
/// deadlocks with no error, no timeout and no log line. The lock is taken ONCE
/// here, around every world, and the module-private helpers are called
/// directly.
///
/// Never fails as a whole — every world's outcome is reported so the caller can
/// tell the user exactly which worlds moved. Re-running converges, because a
/// migrated world no longer holds `old_filename`.
pub(crate) async fn migrate_placements(
    instance_root: &Path,
    old_filename: &str,
    new_filename: &str,
) -> Vec<crate::datapacks::WorldMigration> {
    use crate::datapacks::WorldMigration;

    let src = library_dir_at(instance_root).join(new_filename);
    // The NEW library file's hash, for the foreign-under-the-new-name check in
    // `migrate_one`. Unreadable ⟹ nothing can migrate anyway — every
    // materialize would fail — so report nothing rather than guessing.
    let src_sha = match tokio::fs::read(&src).await {
        Ok(bytes) => crate::datapacks::library::sha1_hex(&bytes),
        Err(_) => return Vec::new(),
    };

    let _guard = level_dat_lock().lock().await;

    // Snapshot INSIDE the lock: a concurrent locked removal committing between
    // an outside-the-lock snapshot and the per-world writes would hand
    // `migrate_one` a world the user just emptied, and it would re-add the new
    // pack there, enabled. Spec §8.5 places identity verification under the
    // lock for exactly this reason.
    let placements = placements_of(instance_root, old_filename).await;

    let mut report = Vec::with_capacity(placements.len());
    for p in placements {
        if !p.is_ours {
            report.push(WorldMigration::SkippedNotOurs { world: p.world });
            continue;
        }
        match migrate_one(
            instance_root,
            &src,
            &src_sha,
            &p,
            old_filename,
            new_filename,
        )
        .await
        {
            Ok(was_enabled) => report.push(WorldMigration::Migrated {
                world: p.world,
                was_enabled,
            }),
            Err(e) => report.push(WorldMigration::Failed {
                world: p.world,
                details: e.to_string(),
            }),
        }
    }
    report
}

/// One world's half of [`migrate_placements`]. Assumes `level_dat_lock` is
/// ALREADY held by the caller — it takes no lock of its own, and must never
/// call the three public entry points above.
///
/// Step order is link-new → rewrite-level.dat → delete-old-LAST, and the
/// order is load-bearing for retry convergence. The retry finds a world by
/// its still-present OLD file (`placements_of`), so the old file must be the
/// last thing to go: with delete-first, a failure in the middle left the
/// world holding neither a findable old file nor a listed new one — invisible
/// to the re-run, which then reported success and deleted the old library
/// file, permanently. With delete-last, every failure position leaves the old
/// file in place and the re-run picks the world up again. The cost is a
/// transient both-files window in the failed state (the new file may sit
/// unlisted, which Minecraft auto-enables), but that state is REPORTED as
/// Failed and converges on retry — the opposite trade of silent permanent
/// loss.
async fn migrate_one(
    instance_root: &Path,
    src: &Path,
    src_sha: &str,
    placement: &WorldPlacement,
    old_filename: &str,
    new_filename: &str,
) -> Result<bool> {
    let (world_dir, dp_dir) = world_dirs_checked(instance_root, &placement.world)?;

    // Read the CURRENT state before changing anything.
    //
    // Enabled-ness is `!in_disabled`, NOT `in_enabled`. A pack present on disk
    // and named in neither list is ENABLED — Minecraft auto-enables a
    // present-but-unlisted pack, which is what `state::derive`'s
    // `(true, _, false) => Enabled` arm encodes. Reading this as two-valued
    // silently writes such a pack into `Disabled`, turning off a pack the user
    // had on.
    //
    // But the NEW entry wins when it is already listed: a previous partial run
    // may have written it (and forgotten the old entry) before failing at the
    // old-file removal. Deriving from the old entry on that retry would read
    // "in neither list" = enabled and flip a disabled pack back on — the exact
    // reversal this whole function exists to prevent.
    let (mut root, framing) = read_level_dat_or_empty(&world_dir)?;
    let (enabled, disabled) = level_dat::lists(&root);
    let old_entry = level_dat_entry(old_filename);
    let new_entry = level_dat_entry(new_filename);
    let was_enabled = if contains_ci(&disabled, &new_entry) {
        false
    } else if contains_ci(&enabled, &new_entry) {
        true
    } else {
        !contains_ci(&disabled, &old_entry)
    };

    tokio::fs::create_dir_all(&dp_dir)
        .await
        .map_err(|e| Error::ModsInstancePath {
            path: dp_dir.display().to_string(),
            details: e.to_string(),
        })?;
    let dest = dp_dir.join(new_filename);

    // The NEW name's slot in this world may already be occupied by something
    // that is not the new library file — the same F5 hazard as everywhere
    // else, one name over. `materialize` replaces its destination
    // unconditionally, so check first. Matching content falls through (a
    // previous partial run already linked it).
    match tokio::fs::metadata(&dest).await {
        Ok(meta) if meta.is_dir() => {
            return Err(Error::ModsFilenameConflict {
                filename: new_filename.to_string(),
                existing_sha: String::new(),
                incoming_sha: src_sha.to_string(),
            });
        }
        Ok(_) => {
            let existing_sha = match tokio::fs::read(&dest).await {
                Ok(bytes) => crate::datapacks::library::sha1_hex(&bytes),
                Err(_) => String::new(),
            };
            if existing_sha != src_sha {
                return Err(Error::ModsFilenameConflict {
                    filename: new_filename.to_string(),
                    existing_sha,
                    incoming_sha: src_sha.to_string(),
                });
            }
        }
        Err(_) => {}
    }

    materialize(src, &dest, LinkPolicy::LinkIfPossible)
        .await
        .map_err(|e| Error::ModsInstancePath {
            path: e.path.display().to_string(),
            details: e.details(),
        })?;

    // `forget_ci`, not `forget`: the old entry is known to us only by the
    // library's filename, and level.dat may hold a different case for the same
    // file. An exact match would remove nothing and leave a permanent orphan.
    let changed_forget = level_dat::forget_ci(&mut root, &old_entry)?;
    let changed_set = level_dat::set_enabled(&mut root, &new_entry, was_enabled)?;
    if changed_forget || changed_set {
        level_dat::write_at(&world_dir, &root, framing).await?;
    }

    // Delete the OLD world-side entry, LAST (see the fn doc for why). Not a
    // tidiness step: left permanently, the stale file is present-and-unlisted
    // after the `forget_ci` above, which Minecraft auto-enables — the world
    // would load BOTH versions.
    match tokio::fs::metadata(&placement.path).await {
        Ok(meta) => {
            // Same type-directed removal `remove_from_world_at` uses: Minecraft
            // loads folder datapacks too, and `remove_file` on a directory
            // fails with OS error 5 on Windows, which `map_removal_err` would
            // then mistranslate into "quit Minecraft and try again".
            let removal = if meta.is_dir() {
                tokio::fs::remove_dir_all(&placement.path).await
            } else {
                tokio::fs::remove_file(&placement.path).await
            };
            if let Err(e) = removal {
                return Err(map_removal_err(&placement.path, e, &placement.world));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(Error::ModsInstancePath {
                path: placement.path.display().to_string(),
                details: e.to_string(),
            })
        }
    }
    Ok(was_enabled)
}

/// The `.zip` files and directories Minecraft's own `datapacks/` scanner
/// would load out of one world's folder: any directory, regardless of what
/// it's called, plus any file whose name ends `.zip`. Without the directory
/// branch, a hand-installed folder datapack never appears "present" here,
/// which `state::derive` then turns into a false `Orphaned`.
///
/// A directory that cannot be read (missing `datapacks/` folder, a world
/// that has never had a pack added) yields an empty list, never an error —
/// mirrors `read_level_dat_or_empty`'s "absent is a supported state" policy.
pub(crate) async fn list_on_disk_entries(dp_dir: &Path) -> Vec<String> {
    let mut on_disk = Vec::new();
    let Ok(mut rd) = tokio::fs::read_dir(dp_dir).await else {
        return on_disk;
    };
    while let Ok(Some(e)) = rd.next_entry().await {
        let name = e.file_name().to_string_lossy().to_string();
        let is_datapack_entry = match e.file_type().await {
            Ok(ft) if ft.is_dir() => true,
            Ok(_) => name.to_ascii_lowercase().ends_with(".zip"),
            Err(_) => false,
        };
        if is_datapack_entry {
            on_disk.push(name);
        }
    }
    on_disk
}

/// Merge one world's three datapack name sources — the library registry, the
/// files/folders actually present in the world's `datapacks/` folder, and
/// the names level.dat references (its own `file/` prefix already stripped
/// by the caller) — into one deduplicated, sorted list.
///
/// NTFS is case-insensitive: a level.dat entry spelled `file/VeinMiner.zip`
/// and an on-disk file named `veinminer.zip` name the SAME file. Deduping on
/// exact string equality (the bug this replaces) kept both spellings as two
/// separate rows — a phantom `Orphaned` for the level.dat spelling (nothing
/// matched it byte-for-byte) alongside a genuine `Enabled` for the on-disk
/// spelling. One physical pack must never render as two contradictory rows,
/// so dedup keys on a case-folded name instead.
///
/// The kept SPELLING for a case-folded collision prefers, in order: on-disk
/// (that's what the file is actually named), then the registry's, then
/// level.dat's. On-disk wins because [`list_for_world_at`]'s own
/// `file_present` check is a plain, un-folded `on_disk.contains(&filename)`
/// — that only stays correct if, whenever a match exists on disk, the
/// chosen spelling IS the on-disk one.
///
/// Display/merge only. Nothing returned from here is written back to the
/// filesystem or level.dat; every write path keeps using the exact filename
/// its own caller gave it.
fn union_names(registry: &[String], on_disk: &[String], level_dat: &[String]) -> Vec<String> {
    let mut by_key: BTreeMap<String, String> = BTreeMap::new();
    // Insertion order is the priority order, LOWEST first: a later insert
    // for the same case-folded key overwrites the earlier one, so the
    // desired winner (on-disk) has to go last.
    for n in level_dat {
        by_key.insert(n.to_lowercase(), n.clone());
    }
    for n in registry {
        by_key.insert(n.to_lowercase(), n.clone());
    }
    for n in on_disk {
        by_key.insert(n.to_lowercase(), n.clone());
    }
    // `BTreeMap` iterates in key order, and the key IS the case-folded name —
    // already sorted the same way the old `sort_by(|a, b|
    // a.to_lowercase().cmp(&b.to_lowercase()))` produced.
    by_key.into_values().collect()
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

/// List every datapack relevant to one world: the union of the library's
/// filenames, the `.zip` files actually present in the world's `datapacks/`
/// folder, and the names level.dat references (its own `file/` prefix
/// stripped). Sorted case-insensitively by filename, deduplicated
/// case-insensitively too — see [`union_names`] for why.
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
    let on_disk = list_on_disk_entries(&dp_dir).await;

    let registry_names: Vec<String> = registry_entries
        .iter()
        .map(|e| e.filename.clone())
        .collect();
    let level_dat_names: Vec<String> = enabled
        .iter()
        .chain(disabled.iter())
        .filter_map(|n| n.strip_prefix("file/").map(str::to_string))
        .collect();
    let names = union_names(&registry_names, &on_disk, &level_dat_names);

    let out = names
        .into_iter()
        .map(|filename| {
            let file_present = on_disk.contains(&filename);
            let entry = level_dat_entry(&filename);
            let in_enabled = contains_ci(&enabled, &entry);
            let in_disabled = contains_ci(&disabled, &entry);
            let pack_state = state::derive(file_present, in_enabled, in_disabled);

            let reg = registry_entries
                .iter()
                .find(|e| e.filename.to_lowercase() == filename.to_lowercase());
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
///
/// Private: the only call site is [`list_for_world_at`] above, in this same
/// file; nothing else in the crate needs it.
#[must_use]
fn compat_of(pack_format: Option<u32>, expected: Option<u32>) -> PackCompat {
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
        library::install_named_at(root, filename, &datapack_zip(pack_format), None)
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
    async fn add_to_world_refuses_a_different_same_named_file() {
        let _lock = hardlink_lock();
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "vm.zip", 48).await;
        let saves = td.path().join(".minecraft").join("saves");
        let dp = saves.join("Alpha").join("datapacks");
        std::fs::create_dir_all(&dp).unwrap();
        let theirs = datapack_zip(57);
        std::fs::write(dp.join("vm.zip"), &theirs).unwrap();

        let err = add_to_world_at(td.path(), "Alpha", "vm.zip")
            .await
            .unwrap_err();

        assert!(
            matches!(err, Error::ModsFilenameConflict { .. }),
            "got {err:?}"
        );
        assert_eq!(
            std::fs::read(dp.join("vm.zip")).unwrap(),
            theirs,
            "the user's own pack must survive"
        );
    }

    #[tokio::test]
    async fn re_adding_a_disabled_pack_re_enables_it() {
        // NOT a no-op. `add_to_world_at` ends with `set_enabled(.., true)`, and
        // the library screen's world picker is exactly where a user ticks a
        // world in which the pack is currently off. A literal
        // file-and-level.dat no-op would silently delete that behaviour.
        let _lock = hardlink_lock();
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "vm.zip", 48).await;
        let saves = td.path().join(".minecraft").join("saves");
        std::fs::create_dir_all(saves.join("Alpha")).unwrap();
        add_to_world_at(td.path(), "Alpha", "vm.zip").await.unwrap();
        set_enabled_in_world_at(td.path(), "Alpha", "vm.zip", false)
            .await
            .unwrap();

        add_to_world_at(td.path(), "Alpha", "vm.zip").await.unwrap();

        let (root, _) = level_dat::read_at(&world_dir(td.path(), "Alpha")).unwrap();
        let (enabled, disabled) = level_dat::lists(&root);
        assert!(
            enabled.iter().any(|s| s == "file/vm.zip"),
            "re-adding must re-enable: {enabled:?}"
        );
        assert!(!disabled.iter().any(|s| s == "file/vm.zip"));
    }

    #[tokio::test]
    async fn a_colliding_directory_is_a_conflict() {
        // Minecraft loads folder datapacks, and this module already models them
        // elsewhere. A folder has no file sha1, so it can never be proven ours
        // — letting it through would rename a zip over a whole pack folder.
        let _lock = hardlink_lock();
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "vm.zip", 48).await;
        let saves = td.path().join(".minecraft").join("saves");
        let dp = saves.join("Alpha").join("datapacks");
        std::fs::create_dir_all(dp.join("vm.zip").join("data")).unwrap();

        let err = add_to_world_at(td.path(), "Alpha", "vm.zip")
            .await
            .unwrap_err();

        assert!(
            matches!(err, Error::ModsFilenameConflict { .. }),
            "got {err:?}"
        );
        assert!(
            dp.join("vm.zip").join("data").is_dir(),
            "the folder pack must survive"
        );
    }

    #[tokio::test]
    async fn placements_of_marks_only_the_library_content_as_ours() {
        let _lock = hardlink_lock();
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "vm.zip", 48).await;
        let saves = td.path().join(".minecraft").join("saves");
        std::fs::create_dir_all(saves.join("Ours")).unwrap();
        std::fs::create_dir_all(saves.join("Theirs").join("datapacks")).unwrap();
        add_to_world_at(td.path(), "Ours", "vm.zip").await.unwrap();
        // Same NAME, different content — a pack the user dropped in by hand.
        std::fs::write(
            saves.join("Theirs").join("datapacks").join("vm.zip"),
            datapack_zip(57),
        )
        .unwrap();

        let found = placements_of(td.path(), "vm.zip").await;

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

    #[tokio::test]
    async fn migrate_preserves_a_disabled_pack_and_removes_the_old_file() {
        let _lock = hardlink_lock();
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "vm-1.zip", 48).await;
        let saves = td.path().join(".minecraft").join("saves");
        std::fs::create_dir_all(saves.join("Alpha")).unwrap();
        add_to_world_at(td.path(), "Alpha", "vm-1.zip")
            .await
            .unwrap();
        // The user turns it OFF. Preserving this is the whole point.
        set_enabled_in_world_at(td.path(), "Alpha", "vm-1.zip", false)
            .await
            .unwrap();
        seed_library(td.path(), "vm-2.zip", 57).await;

        let report = migrate_placements(td.path(), "vm-1.zip", "vm-2.zip").await;

        assert_eq!(
            report,
            vec![crate::datapacks::WorldMigration::Migrated {
                world: "Alpha".to_string(),
                was_enabled: false,
            }]
        );

        let dp = saves.join("Alpha").join("datapacks");
        assert!(
            !dp.join("vm-1.zip").exists(),
            "the OLD world file must be gone: present-and-unlisted is auto-enabled by Minecraft, \
             so leaving it loads BOTH versions"
        );
        assert!(dp.join("vm-2.zip").exists());

        let (root, _) = level_dat::read_at(&world_dir(td.path(), "Alpha")).unwrap();
        let (enabled, disabled) = level_dat::lists(&root);
        assert!(
            disabled.iter().any(|s| s == "file/vm-2.zip"),
            "must still be disabled: {disabled:?}"
        );
        assert!(
            !enabled.iter().any(|s| s.contains("vm-")),
            "nothing may have been enabled: {enabled:?}"
        );
    }

    #[tokio::test]
    async fn migrate_keeps_an_unlisted_pack_enabled() {
        // A pack present on disk but named in NEITHER list is enabled —
        // Minecraft auto-enables it on the next load (state::derive's
        // `(true, _, false) => Enabled`). Reading enabled-ness as `in_enabled`
        // instead of `!in_disabled` silently turns such a pack off.
        let _lock = hardlink_lock();
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "vm-1.zip", 48).await;
        let saves = td.path().join(".minecraft").join("saves");
        let dp = saves.join("Beta").join("datapacks");
        std::fs::create_dir_all(&dp).unwrap();
        // Copy the library bytes so identity matches, but write NO level.dat:
        // both lists are absent.
        std::fs::write(dp.join("vm-1.zip"), datapack_zip(48)).unwrap();
        seed_library(td.path(), "vm-2.zip", 57).await;

        let report = migrate_placements(td.path(), "vm-1.zip", "vm-2.zip").await;

        assert_eq!(
            report,
            vec![crate::datapacks::WorldMigration::Migrated {
                world: "Beta".to_string(),
                was_enabled: true,
            }]
        );
    }

    #[tokio::test]
    async fn migrate_leaves_a_foreign_same_named_file_alone() {
        let _lock = hardlink_lock();
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "vm-1.zip", 48).await;
        let saves = td.path().join(".minecraft").join("saves");
        let dp = saves.join("Gamma").join("datapacks");
        std::fs::create_dir_all(&dp).unwrap();
        let foreign = datapack_zip(57);
        std::fs::write(dp.join("vm-1.zip"), &foreign).unwrap();
        seed_library(td.path(), "vm-2.zip", 61).await;

        let report = migrate_placements(td.path(), "vm-1.zip", "vm-2.zip").await;

        assert_eq!(
            report,
            vec![crate::datapacks::WorldMigration::SkippedNotOurs {
                world: "Gamma".to_string(),
            }]
        );
        assert_eq!(
            std::fs::read(dp.join("vm-1.zip")).unwrap(),
            foreign,
            "a same-named pack the user installed must survive untouched"
        );
        assert!(!dp.join("vm-2.zip").exists());
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
    async fn remove_clears_a_case_drifted_level_dat_entry() {
        // NTFS is case-insensitive: a level.dat entry spelled
        // `file/VeinMiner.zip` and a removal request for `veinminer.zip` name
        // the SAME file — the drift `contains_ci` exists for. An exact
        // `forget` would delete the file but keep the name: a permanent
        // Orphaned row and Minecraft's "data packs are no longer present"
        // screen. The cascade removal path hands this function the LIBRARY's
        // spelling, so the mismatch is reachable, not hypothetical.
        let _lock = hardlink_lock();
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "VeinMiner.zip", 48).await;
        std::fs::create_dir_all(world_dir(td.path(), "Survival")).unwrap();
        add_to_world_at(td.path(), "Survival", "VeinMiner.zip")
            .await
            .unwrap();

        remove_from_world_at(td.path(), "Survival", "veinminer.zip")
            .await
            .unwrap();

        // Only level.dat is asserted: on a case-sensitive filesystem the
        // lowercase path legitimately misses the file (idempotent Ok), but the
        // name must be gone from the lists on every platform.
        let (root, _framing) = level_dat::read_at(&world_dir(td.path(), "Survival")).unwrap();
        let (enabled, disabled) = level_dat::lists(&root);
        assert!(
            enabled.is_empty(),
            "level.dat still names the pack: {enabled:?}"
        );
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

    /// NTFS is case-insensitive: `veinminer.zip` on disk and level.dat's own
    /// `file/VeinMiner.zip` entry name the SAME file. Before the case-folded
    /// union in `union_names`, exact-string dedup kept both spellings as two
    /// separate rows — a phantom `Orphaned` for the level.dat spelling (no
    /// file matched it byte-for-byte) alongside a genuine `Enabled` for the
    /// on-disk spelling. One physical pack must render as exactly one row.
    #[tokio::test]
    async fn a_case_mismatched_name_between_level_dat_and_disk_merges_into_one_row() {
        let td = tempfile::tempdir().unwrap();
        let wd = world_dir(td.path(), "Survival");
        std::fs::create_dir_all(wd.join("datapacks")).unwrap();
        std::fs::write(wd.join("datapacks/veinminer.zip"), b"stub").unwrap();
        let mut root = Value::Compound(HashMap::new());
        level_dat::set_enabled(&mut root, "file/VeinMiner.zip", true).unwrap();
        level_dat::write_at(&wd, &root, level_dat::Framing::Gzip)
            .await
            .unwrap();

        let listed = list_for_world_at(td.path(), "Survival", None)
            .await
            .unwrap();

        assert_eq!(listed.len(), 1, "one physical pack must be one row");
        assert_eq!(
            listed[0].filename, "veinminer.zip",
            "the on-disk spelling must win over level.dat's"
        );
        assert_eq!(listed[0].state, WorldPackState::Enabled);
    }

    /// Regression for the membership check, not just the union: merging
    /// names case-insensitively is not enough if `in_enabled`/`in_disabled`
    /// still compare exact strings against level.dat's own spelling — that
    /// would report this pack `Enabled` (present and unlisted, per
    /// `state::derive`) when level.dat actually disabled it, which is WORSE
    /// than the phantom-row bug the union fixes, because it silently
    /// re-enables a pack the user turned off.
    #[tokio::test]
    async fn a_case_mismatched_disabled_entry_still_reports_disabled() {
        let td = tempfile::tempdir().unwrap();
        let wd = world_dir(td.path(), "Survival");
        std::fs::create_dir_all(wd.join("datapacks")).unwrap();
        std::fs::write(wd.join("datapacks/veinminer.zip"), b"stub").unwrap();
        let mut root = Value::Compound(HashMap::new());
        level_dat::set_enabled(&mut root, "file/VeinMiner.zip", false).unwrap();
        level_dat::write_at(&wd, &root, level_dat::Framing::Gzip)
            .await
            .unwrap();

        let listed = list_for_world_at(td.path(), "Survival", None)
            .await
            .unwrap();

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].filename, "veinminer.zip");
        assert_eq!(
            listed[0].state,
            WorldPackState::Disabled,
            "an exact-string membership check would miss level.dat's \
             differently-cased entry and report this Enabled instead"
        );
    }

    /// Direct unit coverage for the three-way spelling collision
    /// `union_names` itself resolves: same case-folded key, three different
    /// spellings, one from each source. On-disk must win regardless of
    /// insertion order because [`list_for_world_at`]'s `file_present` check
    /// is an un-folded `on_disk.contains(&filename)` — only correct if the
    /// chosen spelling equals the on-disk one whenever a match exists there.
    #[test]
    fn union_names_prefers_on_disk_over_registry_and_level_dat() {
        let registry = vec!["VEINMINER.zip".to_string()];
        let on_disk = vec!["veinminer.zip".to_string()];
        let level_dat = vec!["VeinMiner.zip".to_string()];

        let names = union_names(&registry, &on_disk, &level_dat);

        assert_eq!(names, vec!["veinminer.zip".to_string()]);
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
