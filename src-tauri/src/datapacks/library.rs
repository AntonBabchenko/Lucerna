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
        install_named_at(instance_root, &filename, &bytes).await
    } else {
        // Minecraft's pack-folder scanner only loads directory entries and
        // `*.zip` — a file whose CONTENT is a valid datapack but whose NAME
        // doesn't end `.zip` would install cleanly, list in the registry and
        // the UI, and link into a world, then simply never load. Reject the
        // name before spending a read on the bytes.
        if !name.to_ascii_lowercase().ends_with(".zip") {
            return Err(Error::DatapackInvalid {
                filename: name,
                reason: DatapackRejection::NotAZip,
            });
        }
        let bytes = tokio::fs::read(src)
            .await
            .map_err(|e| Error::io(src.display().to_string(), e))?;
        install_named_at(instance_root, &name, &bytes).await
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
) -> Result<InstalledDatapack> {
    if !crate::pathsafe::is_safe_filename(filename) {
        return Err(Error::ModsUnsafeFilename {
            filename: filename.to_string(),
        });
    }

    let kind = pack_meta::classify(bytes);
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
    tokio::fs::create_dir_all(&lib)
        .await
        .map_err(|e| Error::ModsInstancePath {
            path: lib.display().to_string(),
            details: e.to_string(),
        })?;
    let dest = lib.join(filename);
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
    // world's own `datapacks/`). Re-materialize onto every world that
    // already has a same-named file so they all pick up the new bytes. A
    // failure on one world must not abort the install — the library file is
    // already in place — so this only logs and continues.
    for world_file in crate::datapacks::world_link::worlds_linking(instance_root, filename) {
        if let Err(e) = crate::mods::store::materialize(
            &dest,
            &world_file,
            crate::mods::store::LinkPolicy::LinkIfPossible,
        )
        .await
        {
            crate::diag!(
                "datapacks: failed to refresh {} from the reinstalled library file {}: {}",
                world_file.display(),
                dest.display(),
                e.details()
            );
        }
    }

    let meta = pack_meta::read_meta(bytes);
    let entry = InstalledDatapack {
        filename: filename.to_string(),
        sha1: sha1_hex(bytes),
        size_bytes: bytes.len() as f64,
        pack_format: meta.pack_format,
        name: meta
            .description
            .unwrap_or_else(|| filename.trim_end_matches(".zip").to_string()),
        source: None,
        project_id: None,
        version_id: None,
        installed_at: Utc::now().to_rfc3339(),
    };
    registry::add(instance_root, entry.clone()).await?;
    Ok(entry)
}

/// Delegates to [`registry::list`] — reconciled against the library dir on
/// every call.
pub async fn list_at(instance_root: &Path) -> Result<Vec<InstalledDatapack>> {
    registry::list(instance_root).await
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
        let entry = install_named_at(td.path(), "VeinMiner.zip", &datapack_zip())
            .await
            .unwrap();

        assert_eq!(entry.pack_format, Some(48));
        assert_eq!(entry.name, "Vein Miner");
        assert!(td.path().join("datapacks/VeinMiner.zip").exists());
        let listed = list_at(td.path()).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].filename, "VeinMiner.zip");
    }

    #[tokio::test]
    async fn rejects_a_resource_pack_with_the_right_error() {
        let td = tempfile::tempdir().unwrap();
        let err = install_named_at(td.path(), "Faithful.zip", &resource_pack_zip())
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
        let err = install_named_at(td.path(), "../escape.zip", &datapack_zip())
            .await
            .unwrap_err();

        assert!(matches!(err, Error::ModsUnsafeFilename { .. }));
        assert!(!td.path().join("datapacks").exists());
    }

    #[tokio::test]
    async fn remove_at_deletes_the_file_and_empties_the_listing() {
        let td = tempfile::tempdir().unwrap();
        install_named_at(td.path(), "VeinMiner.zip", &datapack_zip())
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
        install_named_at(td.path(), "vm.zip", &datapack_zip())
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
        install_named_at(td.path(), "vm.zip", &new_bytes)
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
    async fn reinstalling_with_no_worlds_linking_it_yet_is_a_plain_reinstall() {
        let td = tempfile::tempdir().unwrap();
        install_named_at(td.path(), "vm.zip", &datapack_zip())
            .await
            .unwrap();

        // No `.minecraft/saves/` at all — `worlds_linking`'s missing-saves
        // case must yield an empty fan-out, not an error.
        let entry = install_named_at(td.path(), "vm.zip", &datapack_zip_v2())
            .await
            .unwrap();

        assert_eq!(entry.filename, "vm.zip");
        assert_eq!(
            std::fs::read(td.path().join("datapacks/vm.zip")).unwrap(),
            datapack_zip_v2()
        );
    }
}
