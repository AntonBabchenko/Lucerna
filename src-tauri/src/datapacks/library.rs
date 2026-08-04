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
pub async fn install_local_at(instance_root: &Path, src: &Path) -> Result<InstalledDatapack> {
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
        // A local install has no world fan-out to report — the filename is
        // freshly derived from the folder name, so any pre-existing same-named
        // pack is caught by the conflict check, not silently refreshed.
        install_named_at(instance_root, &filename, &bytes, None)
            .await
            .map(|r| r.pack)
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
        install_named_at(instance_root, &name, &bytes, None)
            .await
            .map(|r| r.pack)
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
    if !filename.to_ascii_lowercase().ends_with(".zip") {
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
    let old_bytes = tokio::fs::read(&dest).await.ok();
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
            // Full Unicode folding, not `eq_ignore_ascii_case`: NTFS folds
            // Cyrillic and friends too, so an ASCII-only match would miss the
            // row for a non-ASCII name the filesystem just resolved, and a
            // same-project update would fail as a spurious conflict.
            let want = filename.to_lowercase();
            let same_project = registry::list(instance_root)
                .await
                .ok()
                .and_then(|rows| rows.into_iter().find(|r| r.filename.to_lowercase() == want))
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
        pack_format: meta.pack_format,
        name: meta
            .description
            .unwrap_or_else(|| filename.trim_end_matches(".zip").to_string()),
        source: provenance.map(|p| p.source),
        project_id: provenance.map(|p| p.project_id.clone()),
        version_id: provenance.map(|p| p.version_id.clone()),
        version_number: provenance.and_then(|p| p.version_number.clone()),
        installed_at: Utc::now().to_rfc3339(),
    };
    registry::add(instance_root, entry.clone()).await?;
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
/// `world_link::remove_from_world_at` — file unlinked, level.dat entries
/// dropped. That entry point takes `level_dat_lock` itself; the calls here are
/// sequential, never nested under it, so this cannot deadlock. A same-named
/// file that is not ours is left alone either way. A per-world failure does
/// not abort the others, but it does keep the library copy and registry row —
/// see [`crate::datapacks::LibraryRemoval::removed_from_library`].
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

    let placements = crate::datapacks::world_link::placements_of(instance_root, filename).await;
    let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut worlds = Vec::with_capacity(placements.len());
    let mut any_failed = false;
    for p in placements {
        visited.insert(p.world.to_lowercase());
        if !p.is_ours {
            worlds.push(WorldRemoval::KeptNotOurs { world: p.world });
            continue;
        }
        if !cascade {
            worlds.push(WorldRemoval::KeptNoCascade { world: p.world });
            continue;
        }
        match crate::datapacks::world_link::remove_from_world_at(instance_root, &p.world, filename)
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
    // the user deleted the file by hand — was invisible above. A cascade must
    // clear those names too: leaving one puts the world on Minecraft's "data
    // packs are no longer present" screen after a removal whose purpose was
    // preventing exactly that prompt. `remove_from_world_at` is already the
    // documented orphan-repair path (a missing file is Ok; the name is still
    // cleared).
    if cascade {
        for world in worlds_naming(instance_root, filename).await {
            if visited.contains(&world.to_lowercase()) {
                continue;
            }
            match crate::datapacks::world_link::remove_from_world_at(
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

/// Worlds whose `level.dat` names `filename` in either list — regardless of
/// whether the file is on disk, which is exactly what `placements_of` cannot
/// answer. A world whose level.dat is unreadable is skipped, mirroring the
/// listing's degradation policy: one locked world must not block the sweep.
async fn worlds_naming(instance_root: &Path, filename: &str) -> Vec<String> {
    let entry = crate::datapacks::level_dat_entry(filename);
    let saves_dir = instance_root.join(".minecraft").join("saves");
    let Ok(rd) = std::fs::read_dir(&saves_dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in rd.flatten() {
        let Ok(meta) = e.metadata() else { continue };
        if !meta.is_dir() {
            continue;
        }
        let Some(world) = e.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if crate::worlds::fs::validate_segment(&world).is_err() {
            continue;
        }
        let Ok((root, _framing)) = crate::datapacks::world_link::read_level_dat_or_empty(&e.path())
        else {
            continue;
        };
        let (enabled, disabled) = crate::datapacks::level_dat::lists(&root);
        if crate::datapacks::world_link::contains_ci(&enabled, &entry)
            || crate::datapacks::world_link::contains_ci(&disabled, &entry)
        {
            out.push(world);
        }
    }
    out
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
    async fn installs_a_zip_and_records_pack_format_and_name() {
        let td = tempfile::tempdir().unwrap();
        let entry = install_named_at(td.path(), "VeinMiner.zip", &datapack_zip(), None)
            .await
            .unwrap();

        assert_eq!(entry.pack.pack_format, Some(48));
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

        let entry = install_local_at(td.path(), &src).await.unwrap();

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
        std::fs::create_dir_all(saves.join("Alpha")).unwrap();
        std::fs::create_dir_all(saves.join("Beta")).unwrap();
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
        std::fs::create_dir_all(saves.join("Ours")).unwrap();
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
        // (the user deleted it by hand) has no placement, but a cascade must
        // clear the name anyway — leaving it puts the world on Minecraft's
        // "data packs are no longer present" screen after a removal whose
        // purpose was preventing exactly that prompt.
        let _lock = crate::test_env_lock();
        let td = tempfile::tempdir().unwrap();
        install_named_at(td.path(), "vm.zip", &datapack_zip(), None)
            .await
            .unwrap();
        let saves = td.path().join(".minecraft").join("saves");
        std::fs::create_dir_all(saves.join("Alpha")).unwrap();
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
            enabled.is_empty() && disabled.is_empty(),
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
        std::fs::create_dir_all(saves.join("Alpha")).unwrap();
        std::fs::create_dir_all(saves.join("Beta")).unwrap();
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
                enabled.is_empty() && disabled.is_empty(),
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
        std::fs::create_dir_all(saves.join("Alpha")).unwrap();
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
        std::fs::create_dir_all(saves.join("Alpha")).unwrap();
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
}
