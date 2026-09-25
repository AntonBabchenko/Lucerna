//! The instance's datapack library: `<instance>/datapacks/`.
//!
//! This directory is launcher-owned and sits OUTSIDE `.minecraft/` on purpose —
//! `.minecraft/` mirrors what the game reads, the instance root holds what
//! Lucerna owns (`lucerna/`, `backups/`). The game never reads this folder;
//! worlds get hardlinks from it.
//!
//! Every write goes through `store::place_bytes`. A folder datapack is zipped
//! IN MEMORY and placed with the same call, so this module holds no raw write
//! primitive — enforced by the structural guard
//! (`tests/structural_no_inplace_mods_write.rs`), which scans `src/datapacks/`
//! alongside `src/mods/` and `src/worlds/`.

use std::io::{Cursor, Write};
use std::path::Path;

use chrono::Utc;
use sha1::{Digest, Sha1};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::datapacks::pack_meta::{self, PackKind};
use crate::datapacks::{library_dir_at, registry, InstalledDatapack};
use crate::error::{DatapackRejection, Error, Result};

/// Lowercase 40-char SHA-1 hex digest.
///
/// `pub(crate)`, not `pub`: the only caller outside this file is
/// `registry::reconcile` adopting a hand-dropped library file (same crate);
/// nothing outside `lucerna_lib` — including
/// `tests/datapacks_integration.rs` — reaches this directly.
#[must_use]
pub(crate) fn sha1_hex(bytes: &[u8]) -> String {
    hex::encode(Sha1::digest(bytes))
}

/// Install a `.zip` file or a folder datapack found at `src` into the
/// instance's library. A directory is zipped in memory — see
/// [`zip_folder_in_memory`] — and named `<foldername>.zip`; a file is read and
/// installed under its own name.
///
/// A local install carries no provenance, so it is never conflict-checked: a
/// name the library already holds is replaced, and the same-name fan-out
/// refreshes the worlds linked to the old copy. Its per-world report is
/// returned with the row, as a catalog install's is.
pub async fn install_local_at(
    instance_root: &Path,
    src: &Path,
) -> Result<crate::datapacks::LibraryInstall> {
    let meta = tokio::fs::metadata(src)
        .await
        .map_err(|e| Error::io(src.display().to_string(), e))?;
    let name = src
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::to_string)
        .ok_or_else(|| Error::io(src.display().to_string(), "source path has no file name"))?;

    if meta.is_dir() {
        let filename = format!("{name}.zip");
        let src_owned = src.to_path_buf();
        // CPU-bound zip build; offload so the IPC thread stays responsive —
        // mirrors `worlds::backup::backup_world`'s `spawn_blocking` around
        // `worlds::zip::zip_dir`, this module's sibling for whole world folders.
        let bytes = tokio::task::spawn_blocking(move || zip_folder_in_memory(&src_owned))
            .await
            .map_err(|e| Error::io(name.clone(), format!("join: {e}")))??;
        install_named_at(instance_root, &filename, &bytes, None).await
    } else {
        // `install_named_at` enforces this too, and is the authoritative gate
        // now that the catalog can reach it directly. Kept here as well because
        // this is the one path that can reject the name BEFORE spending a read
        // on the bytes — a 200 MB `.rar` should not be loaded into memory just
        // to be told its extension is wrong.
        if !name.to_ascii_lowercase().ends_with(".zip") {
            return Err(Error::DatapackInvalid {
                filename: name,
                reason: DatapackRejection::NotAZip,
            });
        }
        let bytes = tokio::fs::read(src)
            .await
            .map_err(|e| Error::io(src.display().to_string(), e))?;
        install_named_at(instance_root, &name, &bytes, None).await
    }
}

/// Recursively zip `src_dir`'s contents into an in-memory `.zip`, with entries
/// rooted at the zip's own top level rather than nested under `src_dir`'s
/// name — Minecraft requires `pack.mcmeta` at the zip root, so nesting the
/// folder name would produce an unloadable pack.
///
/// Runs synchronously; callers on the async IPC thread must offload it via
/// `spawn_blocking` (see [`install_local_at`]).
fn zip_folder_in_memory(src_dir: &Path) -> Result<Vec<u8>> {
    let mut zw = ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    add_dir_entries(&mut zw, src_dir, "", &opts)?;
    zw.finish()
        .map(|c| c.into_inner())
        .map_err(|e| Error::io(src_dir.display().to_string(), format!("zip finish: {e}")))
}

/// `zip_prefix` is the forward-slash zip-internal path built up so far (empty
/// at the root). `DirEntry::metadata` does not follow symlinks, so a symlinked
/// file or directory is neither `is_dir()` nor `is_file()` here and is
/// silently skipped — mirrors `worlds::zip::add_dir_contents`'s "special
/// files" handling; a datapack tree has no legitimate reason to contain one.
/// An empty directory (or one containing only skipped entries) simply
/// produces no matching `pack.mcmeta` entry, so `install_named_at`'s
/// classification step rejects it same as any other non-datapack zip.
fn add_dir_entries(
    zw: &mut ZipWriter<Cursor<Vec<u8>>>,
    fs_dir: &Path,
    zip_prefix: &str,
    opts: &SimpleFileOptions,
) -> Result<()> {
    let entries =
        std::fs::read_dir(fs_dir).map_err(|e| Error::io(fs_dir.display().to_string(), e))?;
    for entry in entries {
        let entry = entry.map_err(|e| Error::io(fs_dir.display().to_string(), e))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let zip_path = if zip_prefix.is_empty() {
            name
        } else {
            format!("{zip_prefix}/{name}")
        };
        let file_meta = entry
            .metadata()
            .map_err(|e| Error::io(path.display().to_string(), e))?;
        if file_meta.is_dir() {
            add_dir_entries(zw, &path, &zip_path, opts)?;
        } else if file_meta.is_file() {
            zw.start_file(&zip_path, *opts)
                .map_err(|e| Error::io(path.display().to_string(), format!("zip start: {e}")))?;
            let bytes =
                std::fs::read(&path).map_err(|e| Error::io(path.display().to_string(), e))?;
            zw.write_all(&bytes)
                .map_err(|e| Error::io(path.display().to_string(), e))?;
        }
        // Neither a file nor a directory (symlink or other special entry):
        // deliberately skipped, see the doc comment above.
    }
    Ok(())
}

/// Validate and place raw bytes as a named datapack in the instance's
/// library, recording it in the registry. This is the seam the catalog
/// install path (slice 2) will reuse once downloads land —
/// [`install_local_at`] is just this plus a filesystem read.
pub async fn install_named_at(
    instance_root: &Path,
    filename: &str,
    bytes: &[u8],
    provenance: Option<&crate::datapacks::DatapackProvenance>,
) -> Result<crate::datapacks::LibraryInstall> {
    // N.5: the library file is Lucerna's to name, and the game loads only
    // `*.zip` in lower case. Every later step and the returned row use this name.
    let normalised = crate::datapacks::detect::normalise_zip_extension(filename);
    let filename = normalised.as_str();
    if !crate::pathsafe::is_safe_filename(filename) {
        return Err(Error::ModsUnsafeFilename {
            filename: filename.to_string(),
        });
    }

    // Moved up from `install_local_at`, which used to be the only way in.
    // Minecraft's pack-folder scanner loads directories and `*.zip` only, and a
    // non-zip name written through THIS path is worse than one that never
    // loads: `registry::reconcile` adopts only `.zip` names, so the row is
    // dropped on the very next `list()` while the file stays on disk —
    // invisible in the UI and unremovable through it.
    if !crate::datapacks::detect::has_zip_suffix(filename) {
        return Err(Error::DatapackInvalid {
            filename: filename.to_string(),
            reason: DatapackRejection::NotAZip,
        });
    }

    if bytes.len() > crate::datapacks::MAX_DATAPACK_BYTES {
        return Err(Error::DatapackTooLarge {
            filename: filename.to_string(),
            size_bytes: bytes.len() as f64,
            limit_bytes: crate::datapacks::MAX_DATAPACK_BYTES as f64,
        });
    }

    // One registry read serves both the A21 check and the provenance gate below.
    let rows = registry::list(instance_root).await?;
    // §0.5 A21: a legacy `X.ZIP` row that this name differs from only in case
    // is refused, never installed over, and never renamed automatically.
    if let Some(legacy) = rows.iter().find(|r| {
        r.filename != filename
            && !crate::datapacks::detect::has_zip_suffix(&r.filename)
            && r.filename.to_lowercase() == filename.to_lowercase()
    }) {
        return Err(Error::DatapackLegacyCaseName {
            filename: filename.to_string(),
            legacy: legacy.filename.clone(),
        });
    }

    // Classification, metadata and hashing each walk the whole archive. Before
    // slice 2 every pack came from a file the user picked; the catalog makes
    // this an automated path where a large pack would stall the async
    // executor. Offload all three at once — same `spawn_blocking` shape
    // `install_local_at` already uses for folder zipping.
    let owned = bytes.to_vec();
    let (kind, meta, sha1) = tokio::task::spawn_blocking(move || {
        let kind = pack_meta::classify(&owned);
        let meta = pack_meta::read_meta(&owned);
        let sha1 = sha1_hex(&owned);
        (kind, meta, sha1)
    })
    .await
    .map_err(|e| Error::io(filename.to_string(), format!("join: {e}")))?;

    if kind != PackKind::Datapack {
        // Name the real kind when that's why it was rejected — mirrors
        // `mods::asset_local::validate_asset_zip`'s equivalent message for the
        // resource-pack side of the same discriminator. A typed reason, not a
        // message: see `DatapackRejection`'s doc comment for why.
        let reason = match kind {
            PackKind::ResourcePack => DatapackRejection::IsAResourcePack,
            PackKind::Neither => DatapackRejection::NotAPack,
            // Unreachable: this branch only runs when `kind != PackKind::Datapack`,
            // and `kind` was computed once above from the same `bytes` — it
            // cannot equal `Datapack` here.
            PackKind::Datapack => unreachable!("kind != PackKind::Datapack was just checked"),
        };
        return Err(Error::DatapackInvalid {
            filename: filename.to_string(),
            reason,
        });
    }

    let lib = library_dir_at(instance_root);
    let dest = lib.join(filename);

    // The OUTGOING library file's hash, captured BEFORE `place_bytes` replaces
    // it. It is the identity reference for the fan-out below: a world file
    // matching it is ours-but-stale (refresh it); anything else under this
    // name is a pack the user put there (leave it alone). Hashing after the
    // replace would compare worlds against the NEW bytes and classify every
    // legitimately linked world as foreign. Hashed off-executor like the
    // incoming bytes above, and for the same F18 reason.
    // Absent is a fact — a fresh install has nothing to hash. Any other read
    // failure is ignorance, and everything below leans on this hash: with
    // `None` the provenance-conflict gate silently waves the install through
    // and the fan-out classifies every legitimately linked world as foreign.
    // Mirrors `migrate_one`'s discrimination at its own destination check.
    let old_bytes = match tokio::fs::read(&dest).await {
        Ok(b) => Some(b),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            return Err(Error::ModsInstancePath {
                path: dest.display().to_string(),
                details: e.to_string(),
            })
        }
    };
    let old_sha = match old_bytes {
        Some(b) => Some(
            tokio::task::spawn_blocking(move || sha1_hex(&b))
                .await
                .map_err(|e| Error::io(filename.to_string(), format!("join: {e}")))?,
        ),
        None => None,
    };

    // Refuse to let the CATALOG silently replace a different pack that already
    // holds this name: `terralith.zip` from Modrinth landing on a
    // `terralith.zip` the user installed by hand would replace their pack in
    // the library AND — via the fan-out below — in every world using it, all
    // at once and unprompted.
    //
    // The test is provenance, NOT bytes. Slice 1 deliberately pinned
    // "reinstall the same name with newer bytes refreshes every world"
    // (`reinstalling_over_an_existing_pack_refreshes_every_world_using_it`) —
    // that is the ordinary "install a newer zip" workflow, and a blanket
    // differing-sha1 rejection would break it. So:
    //
    //   * local install (`provenance: None`) — the user picked this exact file
    //     under this exact name. Their intent is explicit; never block it.
    //   * catalog install onto a row from the SAME project — this is the update
    //     path. Allow.
    //   * catalog install onto a local pack, or onto a different project's
    //     pack — two different packs competing for one name. Conflict.
    if let Some(prov) = provenance {
        if let Some(ref existing_sha) = old_sha {
            // N.3 provenance lookup: the exact name first, else the single
            // case-insensitive match (full Unicode folding: NTFS folds Cyrillic
            // and friends too, so an ASCII-only match would miss the row for a
            // non-ASCII name the file system just resolved, and a same-project
            // update would fail as a spurious conflict).
            let same_project =
                crate::datapacks::detect::find_by_name(&rows, filename, |r| r.filename.as_str())
                    .is_some_and(|row| {
                        row.source == Some(prov.source)
                            && row.project_id.as_deref() == Some(&prov.project_id)
                    });
            if !same_project {
                return Err(Error::ModsFilenameConflict {
                    filename: filename.to_string(),
                    existing_sha: existing_sha.clone(),
                    incoming_sha: sha1,
                });
            }
        }
    }

    tokio::fs::create_dir_all(&lib)
        .await
        .map_err(|e| Error::ModsInstancePath {
            path: lib.display().to_string(),
            details: e.to_string(),
        })?;
    crate::mods::store::place_bytes(&dest, bytes)
        .await
        .map_err(|e| Error::ModsInstancePath {
            path: e.path.display().to_string(),
            details: e.details(),
        })?;

    // `place_bytes` is temp-then-rename, so reinstalling over an existing
    // library file gives the LIBRARY name a brand-new inode while every
    // world that already links this filename still points at the OLD one —
    // the ordinary "install a newer zip" workflow would otherwise leave
    // every such world stuck on stale bytes forever, with nothing able to
    // detect it (`registry::reconcile` only scans the library dir, never a
    // world's own `datapacks/`). Re-materialize onto every world whose copy
    // is identity-verified against the OLD file (`old_sha`), so they all pick
    // up the new bytes; a same-named world file that is NOT the outgoing pack
    // is skipped and reported — replacing it would be the F5 data loss. A
    // failure on one world must not abort the install — the library file is
    // already in place. Per-world outcomes are RETURNED, not swallowed into a
    // `diag!` line: the catalog paths promise a precise per-world report.
    //
    // A reinstall that cannot list `saves/` still installs: that no world
    // could be refreshed comes back as a `Failed` entry, not as an error
    // after the library file and its row were already written.
    let refreshed = crate::datapacks::world_link::refresh_placements(
        instance_root,
        filename,
        old_sha.as_deref(),
    )
    .await;

    let entry = InstalledDatapack {
        filename: filename.to_string(),
        sha1,
        size_bytes: bytes.len() as f64,
        name: meta
            .description
            .unwrap_or_else(|| filename.trim_end_matches(".zip").to_string()),
        source: provenance.map(|p| p.source),
        project_id: provenance.map(|p| p.project_id.clone()),
        version_id: provenance.map(|p| p.version_id.clone()),
        version_number: provenance.and_then(|p| p.version_number.clone()),
        installed_at: Utc::now().to_rfc3339(),
    };
    registry::add(instance_root, entry.clone(), meta.mcmeta).await?;
    Ok(crate::datapacks::LibraryInstall {
        pack: entry,
        refreshed,
    })
}

/// Delegates to [`registry::list`] — reconciled against the library dir on
/// every call.
pub async fn list_at(instance_root: &Path) -> Result<Vec<InstalledDatapack>> {
    registry::list(instance_root).await
}

/// Remove a datapack from the library, optionally cascading into every world
/// that holds it. This is the seam behind `datapacks_remove_from_library`.
///
/// Placements are computed BEFORE the library file goes away: `placements_of`'s
/// `is_ours` verdict is a sha1 comparison against the library copy, and after
/// deletion every world file would look foreign.
///
/// With `cascade`, each world holding OUR file goes through
/// `world_link::remove_for_cascade_at`, which is the world tab's removal
/// except in a folder with no level file at all, where it only unlinks (spec
/// §0.5 A3). A world with only `level.dat_old` is refused, reports `Failed`,
/// and so keeps the library copy. That entry point takes `level_dat_lock`
/// itself; the calls here are sequential, never nested under it, so this
/// cannot deadlock. A same-named file that is not ours is left alone either
/// way. A per-world failure does not abort the others, but it does keep the
/// library copy and registry row — see
/// [`crate::datapacks::LibraryRemoval::removed_from_library`]. A world the
/// orphan sweep cannot check ([`worlds_naming`]) counts as such a failure;
/// a `saves/` that cannot be listed fails the whole call before any world is
/// touched.
pub async fn remove_from_library_at(
    instance_root: &Path,
    filename: &str,
    cascade: bool,
) -> Result<crate::datapacks::LibraryRemoval> {
    use crate::datapacks::WorldRemoval;

    if !crate::pathsafe::is_safe_filename(filename) {
        return Err(Error::ModsUnsafeFilename {
            filename: filename.to_string(),
        });
    }

    // `Err`: the library copy or `saves/` could not be read, so no world can
    // be verified; the cascade fails as a whole with nothing touched.
    let placements = crate::datapacks::world_link::placements_of(instance_root, filename).await?;
    // The orphan sweep (below) runs before any world is touched: when it
    // cannot even list `saves/`, the cascade fails as a whole with nothing
    // changed, rather than after some worlds were already cleared.
    let sweep = if cascade {
        Some(worlds_naming(instance_root, filename).await?)
    } else {
        None
    };
    let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut worlds = Vec::with_capacity(placements.found.len() + placements.unchecked.len());
    let mut any_failed = false;
    // A world that could not be checked may hold the pack (Fallback
    // discipline Q1). A cascade reports it Failed, which keeps the library
    // copy; the orphan sweep below must not reach it either. Without a
    // cascade no world is touched, so there is nothing to report.
    for (world, details) in placements.unchecked {
        if cascade {
            visited.insert(world.clone());
            any_failed = true;
            worlds.push(WorldRemoval::Failed { world, details });
        } else {
            crate::diag!(
                "datapacks: library removal without cascade could not check world {world}: {details}"
            );
        }
    }
    for p in placements.found {
        // Exact: both sides come from the same `saves/` `read_dir` (N.3).
        visited.insert(p.world.clone());
        if !p.is_ours {
            worlds.push(WorldRemoval::KeptNotOurs { world: p.world });
            continue;
        }
        if !cascade {
            worlds.push(WorldRemoval::KeptNoCascade { world: p.world });
            continue;
        }
        match crate::datapacks::world_link::remove_for_cascade_at(instance_root, &p.world, filename)
            .await
        {
            Ok(()) => worlds.push(WorldRemoval::Removed { world: p.world }),
            Err(e) => {
                any_failed = true;
                worlds.push(WorldRemoval::Failed {
                    world: p.world,
                    details: e.to_string(),
                });
            }
        }
    }

    // `placements_of` requires an on-disk file, so a world whose level.dat
    // still NAMES the pack while its file is already gone — an orphan, e.g.
    // the user deleted the file by hand — was invisible above. A cascade
    // clears those names too: the game would only log "Missing data pack" and
    // drop the id at its next save, but a removal that says the pack is gone
    // should not leave the entry behind. These worlds had no verified file of
    // ours, so `forget_for_cascade_at` only forgets the ids and never deletes
    // a file: one that appeared under the name fails the world instead.
    if let Some(sweep) = sweep {
        // A world the sweep could not check may name the pack: "could not
        // check" is not "names nothing" (Fallback discipline Q1). It fails,
        // which keeps the library copy for a retry. A world the loop above
        // already handled has its own answer.
        for (world, details) in sweep.unchecked {
            if visited.contains(&world) {
                continue;
            }
            any_failed = true;
            worlds.push(WorldRemoval::Failed { world, details });
        }
        for world in sweep.naming {
            if visited.contains(&world) {
                continue;
            }
            match crate::datapacks::world_link::forget_for_cascade_at(
                instance_root,
                &world,
                filename,
            )
            .await
            {
                Ok(()) => worlds.push(WorldRemoval::Removed { world }),
                Err(e) => {
                    any_failed = true;
                    worlds.push(WorldRemoval::Failed {
                        world,
                        details: e.to_string(),
                    });
                }
            }
        }
    }

    if any_failed {
        return Ok(crate::datapacks::LibraryRemoval {
            worlds,
            removed_from_library: false,
        });
    }
    remove_at(instance_root, filename).await?;
    Ok(crate::datapacks::LibraryRemoval {
        worlds,
        removed_from_library: true,
    })
}

/// What the cascade's orphan sweep found in `saves/`.
struct Sweep {
    /// Worlds whose `level.dat` names the pack in either list.
    naming: Vec<String>,
    /// Worlds the sweep could not check, each with why. Any of them may name
    /// the pack, so the cascade reports each as `Failed` — which keeps the
    /// library copy — rather than as naming nothing (Fallback discipline Q1).
    unchecked: Vec<(String, String)>,
}

/// Worlds whose `level.dat` names `filename` in either list — regardless of
/// whether the file is on disk, which is exactly what `placements_of` cannot
/// answer — plus the worlds that could not be checked.
///
/// `Err` when `saves/` itself cannot be listed, or one of its entries cannot
/// be read: no world name is known there to report as `Failed`, so the
/// cascade fails as a whole. A missing `saves/` is not that: no world can
/// name the pack.
///
/// Only a world whose `level.dat` is a regular file holds a list Lucerna may
/// edit (D2). `Absent` has no list, and the game loads nothing from such a
/// folder. `OnlyOld` is skipped, as spec §3 L.4 has it, and logged: every
/// writer refuses it, and its names stay until the world is restored.
async fn worlds_naming(instance_root: &Path, filename: &str) -> Result<Sweep> {
    let entry = crate::datapacks::level_dat_entry(filename);
    let saves_dir = instance_root.join(".minecraft").join("saves");
    let mut sweep = Sweep {
        naming: Vec::new(),
        unchecked: Vec::new(),
    };
    let rd = match std::fs::read_dir(&saves_dir) {
        Ok(rd) => rd,
        // No saves/ at all: no world can name the pack.
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(sweep),
        Err(err) => return Err(Error::io(saves_dir.display().to_string(), err)),
    };
    for e in rd {
        let e = e.map_err(|err| Error::io(saves_dir.display().to_string(), err))?;
        let Some(world) = e.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if crate::worlds::fs::validate_segment(&world).is_err() {
            continue;
        }
        let path = e.path();
        match e.metadata() {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => continue,
            Err(err) => {
                let details = Error::io(path.display().to_string(), err).to_string();
                crate::diag!("datapacks: removal sweep cannot check world {world}: {details}");
                sweep.unchecked.push((world, details));
                continue;
            }
        }
        match crate::datapacks::presence::of(&path) {
            Ok(crate::datapacks::presence::LevelDatPresence::Present) => {}
            Ok(crate::datapacks::presence::LevelDatPresence::Absent) => continue,
            Ok(crate::datapacks::presence::LevelDatPresence::OnlyOld) => {
                crate::diag!(
                    "datapacks: removal sweep skips world {world}: only level.dat_old survives, \
                     so any {entry} name in it stays until the world is restored"
                );
                continue;
            }
            Err(err) => {
                let details = err.to_string();
                crate::diag!(
                    "datapacks: removal sweep cannot check world {world}: could not tell \
                     whether it has a level.dat: {details}"
                );
                sweep.unchecked.push((world, details));
                continue;
            }
        }
        let (root, _framing) = match crate::datapacks::level_dat::read_at(&path) {
            Ok(read) => read,
            Err(err) => {
                let details = err.to_string();
                crate::diag!(
                    "datapacks: removal sweep cannot check world {world}: level.dat could not \
                     be read: {details}"
                );
                sweep.unchecked.push((world, details));
                continue;
            }
        };
        let (enabled, disabled) = crate::datapacks::level_dat::lists(&root);
        if names_ci(&enabled, &entry) || names_ci(&disabled, &entry) {
            sweep.naming.push(world);
        }
    }
    Ok(sweep)
}

// Case-folded on purpose (§0.5 A18): it only NOMINATES worlds for the
/// cascade's orphan sweep, and the removal it calls applies R3, which never
/// drops an id whose exact spelling is a present entry.
fn names_ci(list: &[String], entry: &str) -> bool {
    let e = entry.to_lowercase();
    list.iter().any(|s| s.to_lowercase() == e)
}

/// Remove a datapack from the instance's library, then drop its registry
/// entry. Deleting one name provably cannot affect any other hardlink pointing
/// at the same physical file (see `mods::store`'s module doc), so a plain
/// `remove_file` is correct here — no `store::` routing needed. A missing file
/// is `Ok` (idempotent, matches `registry::remove`'s semantics).
pub async fn remove_at(instance_root: &Path, filename: &str) -> Result<()> {
    if !crate::pathsafe::is_safe_filename(filename) {
        return Err(Error::ModsUnsafeFilename {
            filename: filename.to_string(),
        });
    }
    let path = library_dir_at(instance_root).join(filename);
    match tokio::fs::remove_file(&path).await {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(Error::ModsInstancePath {
                path: path.display().to_string(),
                details: e.to_string(),
            })
        }
    }
    registry::remove(instance_root, filename).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zip_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut zw = ZipWriter::new(Cursor::new(Vec::new()));
        let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        for (name, bytes) in entries {
            zw.start_file(*name, opts).unwrap();
            zw.write_all(bytes).unwrap();
        }
        zw.finish().unwrap().into_inner()
    }

    const MCMETA: &[u8] = br#"{"pack":{"pack_format":48,"description":"Vein Miner"}}"#;

    fn datapack_zip() -> Vec<u8> {
        zip_with(&[
            ("pack.mcmeta", MCMETA),
            ("data/vm/function/tick.mcfunction", b"say hi"),
        ])
    }

    fn resource_pack_zip() -> Vec<u8> {
        zip_with(&[
            ("pack.mcmeta", br#"{"pack":{"pack_format":15}}"#),
            ("assets/minecraft/textures/x.png", b"\x89PNG"),
        ])
    }

    #[tokio::test]
    async fn installs_a_zip_and_records_its_declaration_and_name() {
        let td = tempfile::tempdir().unwrap();
        let entry = install_named_at(td.path(), "VeinMiner.zip", &datapack_zip(), None)
            .await
            .unwrap();

        let rows = registry::list_rows(td.path()).await.unwrap();
        let row = rows
            .iter()
            .find(|r| r.pack.filename == "VeinMiner.zip")
            .unwrap();
        assert!(
            matches!(&row.mcmeta, Some(crate::datapacks::format::PackMcmeta::Read(d))
                if d.pack_format == crate::datapacks::format::Fact::Present(48)),
            "{:?}",
            row.mcmeta
        );
        assert_eq!(entry.pack.name, "Vein Miner");
        assert!(
            entry.refreshed.is_empty(),
            "a fresh install has no worlds to refresh"
        );
        assert!(td.path().join("datapacks/VeinMiner.zip").exists());
        let listed = list_at(td.path()).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].filename, "VeinMiner.zip");
    }

    /// A DIRECTORY squatting on the pack's library name makes the identity
    /// read fail with a non-NotFound error on every platform. The install
    /// must refuse at the identity read, name the destination, and leave the
    /// directory alone — while a genuinely absent file (NotFound) keeps
    /// installing fine, which `installs_a_zip_and_records_its_declaration_and_name`
    /// already pins.
    ///
    /// NOTE: before the fix this scenario ALSO errored, but only by accident
    /// — `place_bytes`' commit rename happened to fail against the directory,
    /// with the same variant and path. The failing-first witness for this fix
    /// is `a_catalog_install_cannot_skip_the_conflict_gate_via_an_unreadable_pack`
    /// in tests/datapacks_integration.rs; this test pins the refusal
    /// happening at the gate, before anything touches the store.
    #[tokio::test]
    async fn a_directory_squatting_on_the_library_name_stops_the_install() {
        let td = tempfile::tempdir().unwrap();
        let dest = td.path().join("datapacks").join("vm.zip");
        std::fs::create_dir_all(dest.join("data")).unwrap();

        let err = install_named_at(td.path(), "vm.zip", &datapack_zip(), None)
            .await
            .unwrap_err();

        let Error::ModsInstancePath { path, .. } = err else {
            panic!("expected Error::ModsInstancePath, got {err:?}");
        };
        assert_eq!(path, dest.display().to_string());
        assert!(
            dest.join("data").is_dir(),
            "the squatting directory's content must survive the refusal"
        );
        assert!(
            list_at(td.path()).await.unwrap().is_empty(),
            "a refused install must not register the pack"
        );
    }

    #[tokio::test]
    async fn rejects_a_resource_pack_with_the_right_error() {
        let td = tempfile::tempdir().unwrap();
        let err = install_named_at(td.path(), "Faithful.zip", &resource_pack_zip(), None)
            .await
            .unwrap_err();

        let Error::DatapackInvalid { filename, reason } = err else {
            panic!("expected Error::DatapackInvalid");
        };
        assert_eq!(filename, "Faithful.zip");
        assert!(
            matches!(reason, DatapackRejection::IsAResourcePack),
            "reason was: {reason:?}"
        );
        assert!(!td.path().join("datapacks/Faithful.zip").exists());
    }

    #[tokio::test]
    async fn rejects_a_non_zip_extension_even_with_valid_datapack_content() {
        let td = tempfile::tempdir().unwrap();
        // The BYTES are a perfectly valid datapack zip; only the on-disk name
        // is wrong. Minecraft's scanner only loads directories and `*.zip`,
        // so a `.rar`-named pack would never load despite passing every
        // content check.
        let src = td.path().join("MyPack.rar");
        std::fs::write(&src, datapack_zip()).unwrap();

        let err = install_local_at(td.path(), &src).await.unwrap_err();

        let Error::DatapackInvalid { filename, reason } = err else {
            panic!("expected Error::DatapackInvalid, got {err:?}");
        };
        assert_eq!(filename, "MyPack.rar");
        assert!(
            matches!(reason, DatapackRejection::NotAZip),
            "reason was: {reason:?}"
        );
        assert!(!td.path().join("datapacks").exists());
    }

    #[tokio::test]
    async fn zips_a_folder_datapack_on_import() {
        let td = tempfile::tempdir().unwrap();
        let src = td.path().join("VeinMiner");
        std::fs::create_dir_all(src.join("data/vm/function")).unwrap();
        std::fs::write(src.join("pack.mcmeta"), MCMETA).unwrap();
        std::fs::write(src.join("data/vm/function/tick.mcfunction"), b"say hi").unwrap();

        let entry = install_local_at(td.path(), &src).await.unwrap().pack;

        assert_eq!(entry.filename, "VeinMiner.zip");
        assert_eq!(entry.name, "Vein Miner");
        let placed = std::fs::read(td.path().join("datapacks/VeinMiner.zip")).unwrap();
        assert_eq!(pack_meta::classify(&placed), PackKind::Datapack);
    }

    #[tokio::test]
    async fn rejects_an_unsafe_filename() {
        let td = tempfile::tempdir().unwrap();
        let err = install_named_at(td.path(), "../escape.zip", &datapack_zip(), None)
            .await
            .unwrap_err();

        assert!(matches!(err, Error::ModsUnsafeFilename { .. }));
        assert!(!td.path().join("datapacks").exists());
    }

    #[tokio::test]
    async fn remove_at_deletes_the_file_and_empties_the_listing() {
        let td = tempfile::tempdir().unwrap();
        install_named_at(td.path(), "VeinMiner.zip", &datapack_zip(), None)
            .await
            .unwrap();

        remove_at(td.path(), "VeinMiner.zip").await.unwrap();

        assert!(!td.path().join("datapacks/VeinMiner.zip").exists());
        assert!(list_at(td.path()).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn remove_at_is_idempotent_for_a_missing_file() {
        let td = tempfile::tempdir().unwrap();
        remove_at(td.path(), "never-existed.zip").await.unwrap();
    }

    #[tokio::test]
    async fn remove_at_rejects_an_unsafe_filename() {
        let td = tempfile::tempdir().unwrap();
        let err = remove_at(td.path(), "../escape.zip").await.unwrap_err();
        assert!(matches!(err, Error::ModsUnsafeFilename { .. }));
    }

    #[test]
    fn sha1_hex_is_lowercase_and_40_chars() {
        let hash = sha1_hex(b"hello world");
        assert_eq!(hash.len(), 40);
        assert_eq!(hash, hash.to_ascii_lowercase());
    }

    fn datapack_zip_v2() -> Vec<u8> {
        zip_with(&[
            ("pack.mcmeta", MCMETA),
            ("data/vm/function/tick.mcfunction", b"say updated"),
        ])
    }

    /// Regression for the reinstall fan-out: `place_bytes` is temp-then-
    /// rename, so before the fix a reinstalled library file got a brand-new
    /// inode while every world's own `datapacks/` link still pointed at the
    /// OLD one — the ordinary "install a newer zip" workflow silently left
    /// every world stuck on stale bytes forever.
    #[tokio::test]
    async fn reinstalling_over_an_existing_pack_refreshes_every_world_using_it() {
        // add_to_world_at performs a real hardlink; serialize against the
        // process-global FORCE_LINK_FAILURE seam other tests may set.
        let _lock = crate::test_env_lock();
        let td = tempfile::tempdir().unwrap();
        install_named_at(td.path(), "vm.zip", &datapack_zip(), None)
            .await
            .unwrap();

        // `add_to_world_at` requires a real, pre-existing world directory.
        let saves = td.path().join(".minecraft").join("saves");
        crate::datapacks::world_link::test_util::game_world(td.path(), "Alpha");
        crate::datapacks::world_link::test_util::game_world(td.path(), "Beta");
        crate::datapacks::world_link::add_to_world_at(td.path(), "Alpha", "vm.zip")
            .await
            .unwrap();
        crate::datapacks::world_link::add_to_world_at(td.path(), "Beta", "vm.zip")
            .await
            .unwrap();

        let new_bytes = datapack_zip_v2();
        assert_ne!(
            new_bytes,
            datapack_zip(),
            "the reinstall must use genuinely different bytes"
        );
        install_named_at(td.path(), "vm.zip", &new_bytes, None)
            .await
            .unwrap();

        let alpha = std::fs::read(saves.join("Alpha").join("datapacks").join("vm.zip")).unwrap();
        let beta = std::fs::read(saves.join("Beta").join("datapacks").join("vm.zip")).unwrap();
        assert_eq!(
            alpha, new_bytes,
            "Alpha's world-side file must see the reinstalled bytes"
        );
        assert_eq!(
            beta, new_bytes,
            "Beta's world-side file must see the reinstalled bytes"
        );
    }

    #[tokio::test]
    async fn a_reinstall_leaves_a_foreign_same_named_world_file_alone() {
        // The fan-out's identity check: a world file that does NOT match the
        // OUTGOING library file is a pack the user put there themselves, and
        // pushing the new bytes over it is the F5 data loss — the same rule
        // `migrate_placements` and the cascade removal already follow. The
        // legitimate stale-world refresh keeps working because identity is
        // checked against the OLD file's sha (captured before `place_bytes`),
        // not the new one.
        let _lock = crate::test_env_lock();
        let td = tempfile::tempdir().unwrap();
        install_named_at(td.path(), "vm.zip", &datapack_zip(), None)
            .await
            .unwrap();
        let saves = td.path().join(".minecraft").join("saves");
        crate::datapacks::world_link::test_util::game_world(td.path(), "Ours");
        crate::datapacks::world_link::add_to_world_at(td.path(), "Ours", "vm.zip")
            .await
            .unwrap();
        let foreign_dp = saves.join("Theirs").join("datapacks");
        std::fs::create_dir_all(&foreign_dp).unwrap();
        std::fs::write(foreign_dp.join("vm.zip"), b"the user's own pack").unwrap();

        let out = install_named_at(td.path(), "vm.zip", &datapack_zip_v2(), None)
            .await
            .unwrap();

        let mut kinds: Vec<&str> = out
            .refreshed
            .iter()
            .map(|m| match m {
                crate::datapacks::WorldMigration::Refreshed { world } => {
                    assert_eq!(world, "Ours");
                    "refreshed"
                }
                crate::datapacks::WorldMigration::SkippedNotOurs { world } => {
                    assert_eq!(world, "Theirs");
                    "skipped"
                }
                other => panic!("unexpected outcome {other:?}"),
            })
            .collect();
        kinds.sort_unstable();
        assert_eq!(kinds, vec!["refreshed", "skipped"]);
        assert_eq!(
            std::fs::read(saves.join("Ours/datapacks/vm.zip")).unwrap(),
            datapack_zip_v2(),
            "the legitimately linked world must still be refreshed"
        );
        assert_eq!(
            std::fs::read(foreign_dp.join("vm.zip")).unwrap(),
            b"the user's own pack",
            "a foreign same-named world file must never be overwritten by the fan-out"
        );
    }

    #[tokio::test]
    async fn cascading_removal_clears_an_orphaned_level_dat_name() {
        // A world whose level.dat still names the pack while the file is gone
        // (the user deleted it by hand) has no placement, but a cascade
        // clears the name anyway: the game would only log "Missing data pack"
        // and drop the id at its next save, and a removal that says the pack
        // is gone should not leave the entry behind.
        let _lock = crate::test_env_lock();
        let td = tempfile::tempdir().unwrap();
        install_named_at(td.path(), "vm.zip", &datapack_zip(), None)
            .await
            .unwrap();
        let saves = td.path().join(".minecraft").join("saves");
        crate::datapacks::world_link::test_util::game_world(td.path(), "Alpha");
        crate::datapacks::world_link::add_to_world_at(td.path(), "Alpha", "vm.zip")
            .await
            .unwrap();
        std::fs::remove_file(saves.join("Alpha/datapacks/vm.zip")).unwrap();

        let out = remove_from_library_at(td.path(), "vm.zip", true)
            .await
            .unwrap();

        assert!(out.removed_from_library);
        assert_eq!(
            out.worlds,
            vec![crate::datapacks::WorldRemoval::Removed {
                world: "Alpha".into()
            }]
        );
        let (root, _) = crate::datapacks::level_dat::read_at(&saves.join("Alpha")).unwrap();
        let (enabled, disabled) = crate::datapacks::level_dat::lists(&root);
        assert!(
            enabled == vec!["vanilla".to_string()] && disabled.is_empty(),
            "the orphaned name must be cleared: {enabled:?} {disabled:?}"
        );
    }

    #[tokio::test]
    async fn reinstalling_with_no_worlds_linking_it_yet_is_a_plain_reinstall() {
        let td = tempfile::tempdir().unwrap();
        install_named_at(td.path(), "vm.zip", &datapack_zip(), None)
            .await
            .unwrap();

        // No `.minecraft/saves/` at all — `placements_of`'s missing-saves
        // case must yield an empty fan-out, not an error.
        let entry = install_named_at(td.path(), "vm.zip", &datapack_zip_v2(), None)
            .await
            .unwrap();

        assert_eq!(entry.pack.filename, "vm.zip");
        assert!(entry.refreshed.is_empty());
        assert_eq!(
            std::fs::read(td.path().join("datapacks/vm.zip")).unwrap(),
            datapack_zip_v2()
        );
    }

    fn provenance(project: &str, version: &str) -> crate::datapacks::DatapackProvenance {
        crate::datapacks::DatapackProvenance {
            source: crate::mods::platform::ModSource::Modrinth,
            project_id: project.into(),
            version_id: version.into(),
            version_number: Some("2.1.0".into()),
        }
    }

    #[tokio::test]
    async fn records_provenance_so_update_checking_is_not_inert() {
        let td = tempfile::tempdir().unwrap();
        let out = install_named_at(
            td.path(),
            "vm.zip",
            &datapack_zip(),
            Some(&provenance("abc", "v9")),
        )
        .await
        .unwrap();

        // `classify_asset_update` answers UpToDate whenever `version_id` is
        // None, so without this the update check would report every catalog
        // pack current forever, with no error to notice.
        assert_eq!(out.pack.version_id.as_deref(), Some("v9"));
        assert_eq!(out.pack.project_id.as_deref(), Some("abc"));
        assert_eq!(out.pack.version_number.as_deref(), Some("2.1.0"));
        let listed = list_at(td.path()).await.unwrap();
        assert_eq!(listed[0].version_id.as_deref(), Some("v9"));
    }

    #[tokio::test]
    async fn rejects_a_non_zip_name_on_the_catalog_path_too() {
        // The gate used to live only in `install_local_at`. A non-zip name
        // written through here is dropped from the registry by the next
        // reconcile while the file stays on disk: invisible in the UI, and
        // unremovable through it.
        let td = tempfile::tempdir().unwrap();
        let err = install_named_at(td.path(), "pack.rar", &datapack_zip(), None)
            .await
            .unwrap_err();
        assert!(
            matches!(
                err,
                Error::DatapackInvalid {
                    reason: DatapackRejection::NotAZip,
                    ..
                }
            ),
            "got {err:?}"
        );
        assert!(!td.path().join("datapacks/pack.rar").exists());
    }

    #[tokio::test]
    async fn rejects_a_pack_over_the_size_cap() {
        let td = tempfile::tempdir().unwrap();
        let huge = vec![0u8; crate::datapacks::MAX_DATAPACK_BYTES + 1];
        let err = install_named_at(td.path(), "huge.zip", &huge, None)
            .await
            .unwrap_err();
        assert!(matches!(err, Error::DatapackTooLarge { .. }), "got {err:?}");
        assert!(!td.path().join("datapacks/huge.zip").exists());
    }

    #[tokio::test]
    async fn a_catalog_install_will_not_clobber_a_hand_installed_pack_of_the_same_name() {
        let td = tempfile::tempdir().unwrap();
        // The user's own pack, installed locally.
        install_named_at(td.path(), "terralith.zip", &datapack_zip(), None)
            .await
            .unwrap();

        let err = install_named_at(
            td.path(),
            "terralith.zip",
            &datapack_zip_v2(),
            Some(&provenance("terralith", "v1")),
        )
        .await
        .unwrap_err();

        assert!(
            matches!(err, Error::ModsFilenameConflict { .. }),
            "got {err:?}"
        );
        assert_eq!(
            std::fs::read(td.path().join("datapacks/terralith.zip")).unwrap(),
            datapack_zip(),
            "the user's pack must survive untouched"
        );
    }

    #[tokio::test]
    async fn cascading_removal_clears_every_world() {
        let _lock = crate::test_env_lock();
        let td = tempfile::tempdir().unwrap();
        install_named_at(td.path(), "vm.zip", &datapack_zip(), None)
            .await
            .unwrap();
        let saves = td.path().join(".minecraft").join("saves");
        crate::datapacks::world_link::test_util::game_world(td.path(), "Alpha");
        crate::datapacks::world_link::test_util::game_world(td.path(), "Beta");
        crate::datapacks::world_link::add_to_world_at(td.path(), "Alpha", "vm.zip")
            .await
            .unwrap();
        crate::datapacks::world_link::add_to_world_at(td.path(), "Beta", "vm.zip")
            .await
            .unwrap();
        crate::datapacks::world_link::set_enabled_in_world_at(td.path(), "Beta", "vm.zip", false)
            .await
            .unwrap();

        let out = remove_from_library_at(td.path(), "vm.zip", true)
            .await
            .unwrap();

        assert!(out.removed_from_library);
        let mut removed: Vec<&str> = out
            .worlds
            .iter()
            .map(|w| match w {
                crate::datapacks::WorldRemoval::Removed { world } => world.as_str(),
                other => panic!("expected Removed, got {other:?}"),
            })
            .collect();
        removed.sort_unstable();
        assert_eq!(removed, vec!["Alpha", "Beta"]);
        assert!(!td.path().join("datapacks/vm.zip").exists());
        assert!(list_at(td.path()).await.unwrap().is_empty());
        for w in ["Alpha", "Beta"] {
            let wd = saves.join(w);
            assert!(
                !wd.join("datapacks/vm.zip").exists(),
                "{w}'s link must be gone"
            );
            let (root, _) = crate::datapacks::level_dat::read_at(&wd).unwrap();
            let (enabled, disabled) = crate::datapacks::level_dat::lists(&root);
            assert!(
                enabled == vec!["vanilla".to_string()] && disabled.is_empty(),
                "{w}'s level.dat must not name the pack: {enabled:?} {disabled:?}"
            );
        }
    }

    #[tokio::test]
    async fn non_cascading_removal_leaves_world_links_and_names_them() {
        let _lock = crate::test_env_lock();
        let td = tempfile::tempdir().unwrap();
        install_named_at(td.path(), "vm.zip", &datapack_zip(), None)
            .await
            .unwrap();
        let saves = td.path().join(".minecraft").join("saves");
        crate::datapacks::world_link::test_util::game_world(td.path(), "Alpha");
        crate::datapacks::world_link::add_to_world_at(td.path(), "Alpha", "vm.zip")
            .await
            .unwrap();

        let out = remove_from_library_at(td.path(), "vm.zip", false)
            .await
            .unwrap();

        assert!(out.removed_from_library);
        assert_eq!(
            out.worlds,
            vec![crate::datapacks::WorldRemoval::KeptNoCascade {
                world: "Alpha".into()
            }],
            "F3: the removal must NAME the world still holding the pack"
        );
        assert!(!td.path().join("datapacks/vm.zip").exists());
        // The world's own hardlink survives and still reads the full content —
        // deleting one name of a hardlinked file cannot affect the others.
        assert_eq!(
            std::fs::read(saves.join("Alpha/datapacks/vm.zip")).unwrap(),
            datapack_zip()
        );
    }

    #[tokio::test]
    async fn cascade_skips_a_foreign_same_named_world_file() {
        let _lock = crate::test_env_lock();
        let td = tempfile::tempdir().unwrap();
        install_named_at(td.path(), "vm.zip", &datapack_zip(), None)
            .await
            .unwrap();
        let saves = td.path().join(".minecraft").join("saves");
        let dp = saves.join("Alpha").join("datapacks");
        std::fs::create_dir_all(&dp).unwrap();
        std::fs::write(dp.join("vm.zip"), b"the user's own pack").unwrap();

        let out = remove_from_library_at(td.path(), "vm.zip", true)
            .await
            .unwrap();

        assert_eq!(
            out.worlds,
            vec![crate::datapacks::WorldRemoval::KeptNotOurs {
                world: "Alpha".into()
            }]
        );
        assert!(out.removed_from_library, "the library copy still goes");
        assert_eq!(
            std::fs::read(dp.join("vm.zip")).unwrap(),
            b"the user's own pack",
            "a foreign same-named file must never be cascade-deleted"
        );
    }

    /// Windows-only: holding a handle without `FILE_SHARE_DELETE` is exactly
    /// what a running game does to a loaded pack, and it makes `remove_file`
    /// fail with a sharing violation (os error 32 → `WorldInUse`)
    /// deterministically. (A readonly attribute no longer works for this —
    /// std's Windows `remove_file` clears it itself.) On Unix, deletability is
    /// a property of the parent directory, so the same failure cannot be
    /// staged this way; the retention policy is platform-independent and CI
    /// runs this on Windows.
    #[cfg(windows)]
    #[tokio::test]
    async fn a_failed_world_keeps_the_library_copy_for_a_retry() {
        use std::os::windows::fs::OpenOptionsExt;

        let _lock = crate::test_env_lock();
        let td = tempfile::tempdir().unwrap();
        install_named_at(td.path(), "vm.zip", &datapack_zip(), None)
            .await
            .unwrap();
        let saves = td.path().join(".minecraft").join("saves");
        crate::datapacks::world_link::test_util::game_world(td.path(), "Alpha");
        crate::datapacks::world_link::add_to_world_at(td.path(), "Alpha", "vm.zip")
            .await
            .unwrap();
        let locked = saves.join("Alpha/datapacks/vm.zip");
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0x1 /* FILE_SHARE_READ: no delete sharing */)
            .open(&locked)
            .unwrap();

        let out = remove_from_library_at(td.path(), "vm.zip", true)
            .await
            .unwrap();

        assert!(
            matches!(
                out.worlds.as_slice(),
                [crate::datapacks::WorldRemoval::Failed { world, .. }] if world == "Alpha"
            ),
            "got {:?}",
            out.worlds
        );
        assert!(!out.removed_from_library);
        assert!(
            td.path().join("datapacks/vm.zip").exists(),
            "the library copy must survive a failed cascade so a retry can converge"
        );
        assert_eq!(list_at(td.path()).await.unwrap().len(), 1);

        // Release the handle and retry: the removal must now finish everywhere.
        drop(held);
        let out = remove_from_library_at(td.path(), "vm.zip", true)
            .await
            .unwrap();
        assert!(out.removed_from_library);
        assert!(!locked.exists());
        assert!(!td.path().join("datapacks/vm.zip").exists());
    }

    #[tokio::test]
    async fn a_catalog_update_of_the_same_project_is_allowed() {
        // The conflict test is provenance, not bytes: replacing a project's own
        // pack with a newer build of that same project is the update path, and
        // must not be mistaken for a name collision.
        let td = tempfile::tempdir().unwrap();
        install_named_at(
            td.path(),
            "terralith.zip",
            &datapack_zip(),
            Some(&provenance("terralith", "v1")),
        )
        .await
        .unwrap();

        let out = install_named_at(
            td.path(),
            "terralith.zip",
            &datapack_zip_v2(),
            Some(&provenance("terralith", "v2")),
        )
        .await
        .unwrap();

        assert_eq!(out.pack.version_id.as_deref(), Some("v2"));
        assert_eq!(
            std::fs::read(td.path().join("datapacks/terralith.zip")).unwrap(),
            datapack_zip_v2()
        );
    }

    #[tokio::test]
    async fn a_cascade_keeps_the_library_copy_when_a_world_has_only_level_dat_old() {
        // D2 refuses the world, which reports Failed. The library copy stays, so
        // a retry after the world is restored can still identity-check the
        // world's file and converge.
        let _lock = crate::test_env_lock();
        let td = tempfile::tempdir().unwrap();
        install_named_at(td.path(), "vm.zip", &datapack_zip(), None)
            .await
            .unwrap();
        let wd = td.path().join(".minecraft/saves/Restoring");
        crate::datapacks::level_dat::test_support::seed_old(&wd, &["file/vm.zip"], &[]);
        std::fs::create_dir_all(wd.join("datapacks")).unwrap();
        std::fs::write(wd.join("datapacks/vm.zip"), datapack_zip()).unwrap();

        let out = remove_from_library_at(td.path(), "vm.zip", true)
            .await
            .unwrap();

        assert!(
            matches!(
                out.worlds.as_slice(),
                [crate::datapacks::WorldRemoval::Failed { world, .. }] if world == "Restoring"
            ),
            "got {:?}",
            out.worlds
        );
        assert!(!out.removed_from_library);
        assert!(td.path().join("datapacks/vm.zip").exists());
        assert!(wd.join("datapacks/vm.zip").exists());
        assert!(!wd.join("level.dat").exists());
    }

    /// Fallback Q1/Q3: the orphan sweep could not tell whether this world
    /// names the pack (its level.dat is a directory). "Could not check" is not
    /// "names nothing": deleting the library copy here would leave the world
    /// to open with a missing pack if it did name it. The world reports
    /// `Failed`, and the library copy stays for a retry.
    #[tokio::test]
    async fn a_cascade_that_cannot_check_a_world_keeps_the_library_copy() {
        let _lock = crate::test_env_lock();
        let td = tempfile::tempdir().unwrap();
        install_named_at(td.path(), "vm.zip", &datapack_zip(), None)
            .await
            .unwrap();
        let odd = td.path().join(".minecraft/saves/Odd");
        std::fs::create_dir_all(odd.join("level.dat")).unwrap();

        let out = remove_from_library_at(td.path(), "vm.zip", true)
            .await
            .unwrap();

        assert!(
            matches!(
                out.worlds.as_slice(),
                [crate::datapacks::WorldRemoval::Failed { world, .. }] if world == "Odd"
            ),
            "got {:?}",
            out.worlds
        );
        assert!(!out.removed_from_library);
        assert!(td.path().join("datapacks/vm.zip").exists());
        assert_eq!(list_at(td.path()).await.unwrap().len(), 1);
    }

    /// An unreadable `saves/` listing names no world at all, so there is no
    /// world to report `Failed` for: the cascade fails as a whole, before it
    /// has touched anything.
    #[tokio::test]
    async fn a_cascade_that_cannot_list_saves_fails_and_keeps_the_library_copy() {
        let _lock = crate::test_env_lock();
        let td = tempfile::tempdir().unwrap();
        install_named_at(td.path(), "vm.zip", &datapack_zip(), None)
            .await
            .unwrap();
        // A file where the saves/ directory should be: listing it fails with
        // something other than NotFound on every platform.
        std::fs::create_dir_all(td.path().join(".minecraft")).unwrap();
        std::fs::write(td.path().join(".minecraft/saves"), b"not a directory").unwrap();

        let err = remove_from_library_at(td.path(), "vm.zip", true)
            .await
            .unwrap_err();

        assert!(matches!(err, Error::Io { .. }), "got {err:?}");
        assert!(td.path().join("datapacks/vm.zip").exists());
        assert_eq!(list_at(td.path()).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_cascade_is_not_blocked_by_a_folder_without_level_dat() {
        // §0.5 A3: the cascade unlinks the file and never reads or writes a
        // level.dat; refusing would leave the library row impossible to remove.
        let _lock = crate::test_env_lock();
        let td = tempfile::tempdir().unwrap();
        install_named_at(td.path(), "vm.zip", &datapack_zip(), None)
            .await
            .unwrap();
        let loose = td.path().join(".minecraft/saves/Loose");
        std::fs::create_dir_all(loose.join("datapacks")).unwrap();
        std::fs::write(loose.join("datapacks/vm.zip"), datapack_zip()).unwrap();

        let out = remove_from_library_at(td.path(), "vm.zip", true)
            .await
            .unwrap();

        assert_eq!(
            out.worlds,
            vec![crate::datapacks::WorldRemoval::Removed {
                world: "Loose".into()
            }]
        );
        assert!(out.removed_from_library);
        assert!(!loose.join("datapacks/vm.zip").exists());
        assert!(!loose.join("level.dat").exists());
    }

    #[tokio::test]
    async fn a_reinstall_still_refreshes_an_only_old_world() {
        // §0.2 I7 / §3 L.13 Q1: D2 deliberately does NOT gate the same-name
        // refresh. It never touches level.dat, and skipping it would leave the
        // world on the old bytes while the registry records the new version,
        // which no update check could ever repair.
        let _lock = crate::test_env_lock();
        let td = tempfile::tempdir().unwrap();
        install_named_at(td.path(), "vm.zip", &datapack_zip(), None)
            .await
            .unwrap();
        let wd = td.path().join(".minecraft/saves/Restoring");
        crate::datapacks::level_dat::test_support::seed_old(&wd, &["file/vm.zip"], &[]);
        std::fs::create_dir_all(wd.join("datapacks")).unwrap();
        std::fs::write(wd.join("datapacks/vm.zip"), datapack_zip()).unwrap();

        let out = install_named_at(td.path(), "vm.zip", &datapack_zip_v2(), None)
            .await
            .unwrap();

        assert_eq!(
            out.refreshed,
            vec![crate::datapacks::WorldMigration::Refreshed {
                world: "Restoring".into()
            }]
        );
        assert_eq!(
            std::fs::read(wd.join("datapacks/vm.zip")).unwrap(),
            datapack_zip_v2()
        );
        assert!(!wd.join("level.dat").exists());
    }

    fn library_names(root: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(library_dir_at(root))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    #[tokio::test]
    async fn install_normalises_an_upper_case_zip_extension() {
        let td = tempfile::tempdir().unwrap();
        let out = install_named_at(td.path(), "Pack.ZIP", &datapack_zip(), None)
            .await
            .unwrap();
        assert_eq!(out.pack.filename, "Pack.zip");
        assert_eq!(library_names(td.path()), vec!["Pack.zip"]);
    }

    /// §0.5 A21. Asserts on `read_dir` names, so it holds on every file system.
    #[tokio::test]
    async fn installing_over_a_legacy_upper_case_zip_is_refused_and_keeps_the_row() {
        let td = tempfile::tempdir().unwrap();
        let lib = library_dir_at(td.path());
        std::fs::create_dir_all(&lib).unwrap();
        let legacy = datapack_zip();
        std::fs::write(lib.join("Pack.ZIP"), &legacy).unwrap(); // adopted by registry::reconcile
        let err = install_named_at(td.path(), "Pack.zip", &datapack_zip_v2(), None)
            .await
            .unwrap_err();
        assert!(
            matches!(&err, Error::DatapackLegacyCaseName { legacy, .. } if legacy == "Pack.ZIP"),
            "got {err:?}"
        );
        assert_eq!(library_names(td.path()), vec!["Pack.ZIP"]);
        assert_eq!(std::fs::read(lib.join("Pack.ZIP")).unwrap(), legacy);
        assert_eq!(list_at(td.path()).await.unwrap()[0].filename, "Pack.ZIP");
    }

    /// §0.5 A24: one root rule; `./pack.mcmeta` is not at the root.
    #[tokio::test]
    async fn an_install_refuses_a_dot_slash_pack_mcmeta_zip() {
        let td = tempfile::tempdir().unwrap();
        let bytes = crate::datapacks::detect::test_support::zip_of(&[
            ("./pack.mcmeta", MCMETA),
            ("data/vm/function/t.mcfunction", b"x"),
        ]);
        let err = install_named_at(td.path(), "dot.zip", &bytes, None)
            .await
            .unwrap_err();
        assert!(
            matches!(
                err,
                Error::DatapackInvalid {
                    reason: DatapackRejection::NotAPack,
                    ..
                }
            ),
            "got {err:?}"
        );
    }

    /// Fallback Q1: a world whose `datapacks/` could not be listed may hold
    /// the pack. The cascade reports it Failed and keeps the library copy, and
    /// touches nothing in that world.
    #[tokio::test]
    async fn a_cascade_keeps_the_library_copy_when_a_world_cannot_be_checked() {
        let _lock = crate::test_env_lock();
        let td = tempfile::tempdir().unwrap();
        install_named_at(td.path(), "vm.zip", &datapack_zip(), None)
            .await
            .unwrap();
        let wd = crate::datapacks::world_link::test_util::game_world(td.path(), "Locked");
        std::fs::write(wd.join("datapacks"), b"a file, not a folder").unwrap();
        let level_before = std::fs::read(wd.join("level.dat")).unwrap();

        let out = remove_from_library_at(td.path(), "vm.zip", true)
            .await
            .unwrap();

        assert!(!out.removed_from_library);
        assert!(
            matches!(
                out.worlds.as_slice(),
                [crate::datapacks::WorldRemoval::Failed { world, .. }] if world == "Locked"
            ),
            "{:?}",
            out.worlds
        );
        assert!(library_dir_at(td.path()).join("vm.zip").exists());
        assert_eq!(
            std::fs::read(wd.join("datapacks")).unwrap(),
            b"a file, not a folder"
        );
        assert_eq!(std::fs::read(wd.join("level.dat")).unwrap(), level_before);
    }

    /// Fallback Q1: a same-name reinstall cannot tell whether a world it
    /// could not list holds the stale pack, so that world is reported Failed
    /// rather than silently left on the old bytes.
    #[tokio::test]
    async fn a_refresh_reports_a_world_it_cannot_check() {
        let _lock = crate::test_env_lock();
        let td = tempfile::tempdir().unwrap();
        install_named_at(td.path(), "vm.zip", &datapack_zip(), None)
            .await
            .unwrap();
        let wd = crate::datapacks::world_link::test_util::game_world(td.path(), "Locked");
        std::fs::write(wd.join("datapacks"), b"a file, not a folder").unwrap();

        let out = install_named_at(td.path(), "vm.zip", &datapack_zip_v2(), None)
            .await
            .unwrap();

        assert!(
            matches!(
                out.refreshed.as_slice(),
                [crate::datapacks::WorldMigration::Failed { world, .. }] if world == "Locked"
            ),
            "{:?}",
            out.refreshed
        );
    }

    /// §0.5 A21 refuses only a LEGACY `X.ZIP` row. A name that differs in case
    /// from a modern `.zip` row is an ordinary install (on NTFS, a reinstall of
    /// the same file), never the legacy refusal.
    #[tokio::test]
    async fn installing_a_case_variant_of_a_modern_zip_row_is_not_refused_as_legacy() {
        let _lock = crate::test_env_lock();
        let td = tempfile::tempdir().unwrap();
        install_named_at(td.path(), "VM.zip", &datapack_zip(), None)
            .await
            .unwrap();
        let got = install_named_at(td.path(), "vm.zip", &datapack_zip_v2(), None).await;
        assert!(
            !matches!(got, Err(Error::DatapackLegacyCaseName { .. })),
            "got {got:?}"
        );
    }

    /// A FRESH install has no bytes of ours in any world, so a world that
    /// cannot be checked cannot hold a stale copy: nothing to report.
    #[tokio::test]
    async fn a_fresh_install_reports_nothing_for_a_world_it_cannot_check() {
        let _lock = crate::test_env_lock();
        let td = tempfile::tempdir().unwrap();
        let wd = crate::datapacks::world_link::test_util::game_world(td.path(), "Locked");
        std::fs::write(wd.join("datapacks"), b"a file, not a folder").unwrap();

        let out = install_named_at(td.path(), "vm.zip", &datapack_zip(), None)
            .await
            .unwrap();

        assert!(
            !out.refreshed
                .iter()
                .any(|m| matches!(m, crate::datapacks::WorldMigration::Failed { .. })),
            "{:?}",
            out.refreshed
        );
    }

    /// A library removal without cascade leaves a world holding the file, which
    /// is that world's own copy from then on. A fresh install under the same
    /// name has no old library copy to compare it with, so it refreshes
    /// nothing: the world's file is reported as not ours and left as it is.
    #[tokio::test]
    async fn a_fresh_install_leaves_a_same_named_world_file_alone() {
        let _lock = crate::test_env_lock();
        let td = tempfile::tempdir().unwrap();
        install_named_at(td.path(), "vm.zip", &datapack_zip(), None)
            .await
            .unwrap();
        let wd = crate::datapacks::world_link::test_util::game_world(td.path(), "Alpha");
        crate::datapacks::world_link::add_to_world_at(td.path(), "Alpha", "vm.zip")
            .await
            .unwrap();
        remove_from_library_at(td.path(), "vm.zip", false)
            .await
            .unwrap();

        let out = install_named_at(td.path(), "vm.zip", &datapack_zip_v2(), None)
            .await
            .unwrap();

        assert_eq!(
            out.refreshed,
            vec![crate::datapacks::WorldMigration::SkippedNotOurs {
                world: "Alpha".into()
            }]
        );
        assert_eq!(
            std::fs::read(wd.join("datapacks").join("vm.zip")).unwrap(),
            datapack_zip()
        );
    }

    /// A reinstall whose `saves/` cannot be listed still installs: the library
    /// file and its row are written. That no world could be refreshed comes
    /// back as a failed entry in `refreshed`, never as "not installed".
    #[tokio::test]
    async fn a_reinstall_that_cannot_list_saves_installs_and_reports_the_refresh() {
        let _lock = crate::test_env_lock();
        let td = tempfile::tempdir().unwrap();
        install_named_at(td.path(), "vm.zip", &datapack_zip(), None)
            .await
            .unwrap();
        std::fs::create_dir_all(td.path().join(".minecraft")).unwrap();
        std::fs::write(td.path().join(".minecraft").join("saves"), b"a file").unwrap();

        let out = install_named_at(td.path(), "vm.zip", &datapack_zip_v2(), None)
            .await
            .unwrap();

        assert!(
            matches!(
                out.refreshed.as_slice(),
                [crate::datapacks::WorldMigration::Failed { .. }]
            ),
            "{:?}",
            out.refreshed
        );
        assert_eq!(
            std::fs::read(library_dir_at(td.path()).join("vm.zip")).unwrap(),
            datapack_zip_v2()
        );
        assert_eq!(out.pack.sha1, sha1_hex(&datapack_zip_v2()));
    }

    /// A local install carries no provenance, so it is never conflict-checked:
    /// installing a file under a name the library already holds replaces it,
    /// and the same-name fan-out refreshes the worlds. Its per-world report
    /// reaches the caller, as a catalog install's does.
    #[tokio::test]
    async fn a_local_reinstall_reports_its_world_refresh() {
        let _lock = crate::test_env_lock();
        let td = tempfile::tempdir().unwrap();
        install_named_at(td.path(), "vm.zip", &datapack_zip(), None)
            .await
            .unwrap();
        std::fs::create_dir_all(td.path().join(".minecraft")).unwrap();
        std::fs::write(td.path().join(".minecraft").join("saves"), b"a file").unwrap();
        let picked = tempfile::tempdir().unwrap();
        let src = picked.path().join("vm.zip");
        std::fs::write(&src, datapack_zip_v2()).unwrap();

        let out = install_local_at(td.path(), &src).await.unwrap();

        assert!(
            matches!(
                out.refreshed.as_slice(),
                [crate::datapacks::WorldMigration::Failed { .. }]
            ),
            "{:?}",
            out.refreshed
        );
        assert_eq!(out.pack.sha1, sha1_hex(&datapack_zip_v2()));
    }
}
