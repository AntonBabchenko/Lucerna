//! Phase 3 of a world migration: reconnect the staged world's `datapacks/`
//! to the TARGET instance's library, one filename at a time (spec §5, A1).
//!
//! # Invariant (test-pinned below)
//!
//! A migration never replaces a file in the target library, and never
//! triggers a refresh onto any world other than the one it is creating.
//! Identity is decided per FILENAME, never per hash across the library:
//!
//! * name absent in the target library → adopt under that name via
//!   `install_named_at` (a fresh name has `old_sha = None`, so its same-name
//!   fan-out skips every candidate — `placements.rs`'s documented safe
//!   direction), then link;
//! * name present, identical on-disk bytes → hardlink the world's file to
//!   the library file with `materialize`;
//! * name present, different (or unreadable) bytes → leave the plain copy
//!   and say so; the library is not touched.
//!
//! Never `install_local_at`: its `provenance: None` path skips the same-name
//! conflict gate and would `place_bytes` over the library file, fanning a
//! downgrade into every other world linked to it (spec A1).
//!
//! # Writer discipline
//!
//! This file names no write primitive. Bytes reach the target library only
//! through `install_named_at` (→ `store::place_bytes`, temp + rename); a
//! world-side file is only ever REPLACED through `store::materialize` (temp
//! sibling + rename), never written through — on the rename path a world
//! file is still a hardlink into the SOURCE library, and writing through it
//! would corrupt the source (`mods::store`'s module doc).
//!
//! No per-entry failure is fatal: a library that cannot accept one pack must
//! not cost the user a complete, playable world. The only `Err` is a
//! `read_dir` on `<stage>/datapacks/` failing for a reason other than
//! NotFound — "could not tell" what the world holds, answered restrictively
//! by failing the step (the orchestrator is still before the point of no
//! return, so nothing is lost).

use std::path::Path;

use crate::datapacks::library::{self, sha1_hex};
use crate::datapacks::{
    library_dir_at, registry, DatapackProvenance, InstalledDatapack, WorldMigration,
};
use crate::error::{Error, Result};
use crate::mods::store::{self, LinkPolicy, Placement};

use super::{DatapackMigration, DatapackResult, LeftReason, MigrationPath, Relinked};

/// Reconnect every `.zip` directly under `<stage>/datapacks/` to the target
/// library (module doc). `src_root` / `dst_root` are the instance roots
/// `datapacks::library` / `registry` take. `path` is which §4 path staged the
/// world: on `Renamed` every entry left unlinked is de-shared from the SOURCE
/// library ([`deshare`]).
///
/// Visits regular files whose name ends in `.zip` (case-insensitive) — the
/// rule `world_link::list_on_disk_entries` uses, minus directories. A
/// directory is a folder pack, counted and left as it is; a symlink or any
/// other file is ignored. A missing `datapacks/` folder is zero packs.
pub(crate) async fn relink_datapacks_at(
    stage: &Path,
    src_root: &Path,
    dst_root: &Path,
    path: MigrationPath,
) -> Result<Relinked> {
    let dp_dir = stage.join("datapacks");
    let mut rd = match tokio::fs::read_dir(&dp_dir).await {
        Ok(rd) => rd,
        // Absent is a fact: a world without a datapacks/ folder has no packs.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Relinked {
                datapacks: Vec::new(),
                folders_copied: 0,
            })
        }
        // Anything else is "could not tell" — the folder may hold packs this
        // step cannot see. Restrictive answer: fail the step rather than
        // report a world with zero packs (module doc).
        Err(e) => return Err(Error::io(dp_dir.display().to_string(), e)),
    };

    let mut folders_copied = 0u32;
    let mut zips: Vec<String> = Vec::new();
    loop {
        let entry = match rd.next_entry().await {
            Ok(Some(entry)) => entry,
            Ok(None) => break,
            Err(e) => return Err(Error::io(dp_dir.display().to_string(), e)),
        };
        let ft = match entry.file_type().await {
            Ok(ft) => ft,
            Err(e) => {
                // Neither a folder pack nor a zip can be claimed for an entry
                // whose type is unknown; it is left exactly as it is.
                crate::diag!(
                    "worlds::migrate: relink skipped {} — could not read its type: {e}",
                    entry.path().display()
                );
                continue;
            }
        };
        // `file_type` never follows symlinks. A symlinked entry is not
        // visited (spec §5): the copy path never produced one, and on the
        // rename path it points wherever the user pointed it.
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            // A folder datapack: copied as it is, never zipped, never adopted
            // (the library is zip-only and level.dat names `file/<folder>`).
            folders_copied += 1;
            continue;
        }
        if !ft.is_file() {
            continue;
        }
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            // Not representable as a library filename; left in place.
            crate::diag!(
                "worlds::migrate: relink skipped a non-UTF-8 name under {}",
                dp_dir.display()
            );
            continue;
        };
        if name.to_ascii_lowercase().ends_with(".zip") {
            zips.push(name.to_string());
        }
    }
    // `read_dir` order is unspecified; the outcome the UI renders is not.
    zips.sort_unstable();

    if zips.is_empty() {
        return Ok(Relinked {
            datapacks: Vec::new(),
            folders_copied,
        });
    }

    let source_rows = source_registry_rows(src_root).await;
    let lib = library_dir_at(dst_root);
    let mut datapacks = Vec::with_capacity(zips.len());
    for name in zips {
        let world_file = dp_dir.join(&name);
        let lib_file = lib.join(&name);
        let result = relink_one(&world_file, &lib_file, &name, dst_root, &source_rows).await;
        if matches!(path, MigrationPath::Renamed)
            && matches!(result, DatapackResult::LeftAsCopy { .. })
        {
            deshare(&world_file).await;
        }
        datapacks.push(DatapackMigration {
            filename: name,
            result,
        });
    }
    Ok(Relinked {
        datapacks,
        folders_copied,
    })
}

/// One `.zip` entry. Infallible by design: every failure becomes a typed
/// `LeftAsCopy` reason plus a `diag!` line naming the cause.
async fn relink_one(
    world_file: &Path,
    lib_file: &Path,
    name: &str,
    dst_root: &Path,
    source_rows: &[InstalledDatapack],
) -> DatapackResult {
    match tokio::fs::try_exists(lib_file).await {
        Ok(false) => adopt_then_link(world_file, lib_file, name, dst_root, source_rows).await,
        Ok(true) => link_if_identical(world_file, lib_file, name).await,
        Err(e) => {
            // Could not stat the library name. Fallback direction: the world
            // keeps its plain copy (spec §5 step 1) — the restrictive answer,
            // since both adopting and linking would act on a library entry
            // this step could not see. The direction is the same as for a
            // different pack — no adopt, no link — but the reason must say
            // what happened: a stat failure is `Io`, not a different pack
            // holding the name. The real cause goes to Logs.
            crate::diag!(
                "worlds::migrate: could not stat library entry {} for {name}: {e}; left as a copy",
                lib_file.display()
            );
            DatapackResult::LeftAsCopy {
                reason: LeftReason::Io,
            }
        }
    }
}

/// Name absent in the target library: adopt the world's bytes under that
/// name (with the SOURCE registry's provenance when it has one), then link
/// the world's file to the new library file.
async fn adopt_then_link(
    world_file: &Path,
    lib_file: &Path,
    name: &str,
    dst_root: &Path,
    source_rows: &[InstalledDatapack],
) -> DatapackResult {
    let bytes = match tokio::fs::read(world_file).await {
        Ok(b) => b,
        Err(e) => {
            crate::diag!(
                "worlds::migrate: could not read {} to adopt it: {e}; left as a copy",
                world_file.display()
            );
            return DatapackResult::LeftAsCopy {
                reason: LeftReason::Unreadable,
            };
        }
    };
    let provenance = provenance_for(name, source_rows);
    match library::install_named_at(dst_root, name, &bytes, provenance.as_ref()).await {
        Ok(report) => {
            // The name was absent, so `old_sha` was `None` and the fan-out
            // skips every candidate. Anything but `SkippedNotOurs` here would
            // mean the invariant in the module doc broke — logged loudly.
            if report
                .refreshed
                .iter()
                .any(|m| !matches!(m, WorldMigration::SkippedNotOurs { .. }))
            {
                crate::diag!(
                    "worlds::migrate: INVARIANT: adopting {name} touched other worlds: {:?}",
                    report.refreshed
                );
            }
        }
        Err(Error::DatapackInvalid { reason, .. }) => {
            crate::diag!("worlds::migrate: {name} is not a datapack ({reason:?}); left as a copy");
            return DatapackResult::LeftAsCopy {
                reason: LeftReason::NotADatapack { reason },
            };
        }
        Err(Error::DatapackTooLarge { .. }) => {
            crate::diag!("worlds::migrate: {name} is over the library's size cap; left as a copy");
            return DatapackResult::LeftAsCopy {
                reason: LeftReason::TooLarge,
            };
        }
        // `ModsFilenameConflict` cannot fire for an absent name (its gate needs
        // an existing library file); `ModsUnsafeFilename`, `ModsInstancePath`
        // and a failed registry write land here.
        Err(e) => {
            crate::diag!(
                "worlds::migrate: could not adopt {name} into the target library: {e}; left as a copy"
            );
            return DatapackResult::LeftAsCopy {
                reason: LeftReason::Io,
            };
        }
    }
    match store::materialize(lib_file, world_file, LinkPolicy::LinkIfPossible).await {
        // Adopted means adopted AND linked.
        Ok(Placement::Linked) => DatapackResult::Adopted,
        // The library has the pack, but the filesystem could not link it: the
        // world holds an independent copy — never call that "linked".
        Ok(Placement::Copied) => DatapackResult::CopiedNotLinked,
        Err(e) => {
            crate::diag!(
                "worlds::migrate: adopted {name} but could not link {}: {}; left as a copy",
                world_file.display(),
                e.details()
            );
            DatapackResult::LeftAsCopy {
                reason: LeftReason::LinkFailed,
            }
        }
    }
}

/// Name present in the target library: link only when the two files hold
/// identical bytes, hashed ON DISK (never from the registry, which
/// `reconcile` retains by name without re-hashing). Anything else leaves the
/// plain copy and the library untouched.
async fn link_if_identical(world_file: &Path, lib_file: &Path, name: &str) -> DatapackResult {
    let lib_sha = match sha1_of(lib_file).await {
        Ok(s) => s,
        Err(e) => {
            crate::diag!(
                "worlds::migrate: could not hash library entry {}: {e}; {name} left as a copy",
                lib_file.display()
            );
            return DatapackResult::LeftAsCopy {
                reason: LeftReason::Unreadable,
            };
        }
    };
    let world_sha = match sha1_of(world_file).await {
        Ok(s) => s,
        Err(e) => {
            crate::diag!(
                "worlds::migrate: could not hash {}: {e}; left as a copy",
                world_file.display()
            );
            return DatapackResult::LeftAsCopy {
                reason: LeftReason::Unreadable,
            };
        }
    };
    if lib_sha != world_sha {
        // A different pack owns this name in the target. Do not adopt, do not
        // touch the library: the world keeps its plain copy.
        return DatapackResult::LeftAsCopy {
            reason: LeftReason::NameHeldByDifferentPack,
        };
    }
    match store::materialize(lib_file, world_file, LinkPolicy::LinkIfPossible).await {
        Ok(Placement::Linked) => DatapackResult::Linked,
        Ok(Placement::Copied) => DatapackResult::CopiedNotLinked,
        Err(e) => {
            crate::diag!(
                "worlds::migrate: could not link {} to the library: {}; left as a copy",
                world_file.display(),
                e.details()
            );
            DatapackResult::LeftAsCopy {
                reason: LeftReason::LinkFailed,
            }
        }
    }
}

/// SHA-1 of a file's bytes, read with tokio and hashed off the executor —
/// the same offload `install_named_at` uses, for the same reason.
async fn sha1_of(path: &Path) -> std::io::Result<String> {
    let bytes = tokio::fs::read(path).await?;
    tokio::task::spawn_blocking(move || sha1_hex(&bytes))
        .await
        .map_err(|e| std::io::Error::other(format!("join: {e}")))
}

/// The source registry row for `name` (case-folded, as `install_named_at`'s
/// own lookup — NTFS folds non-ASCII too), when it carries a full catalog
/// identity. A row without `source`, `project_id` or `version_id` is a local
/// install and yields `None`, exactly what a local install records.
fn provenance_for(name: &str, rows: &[InstalledDatapack]) -> Option<DatapackProvenance> {
    let want = name.to_lowercase();
    let row = rows.iter().find(|r| r.filename.to_lowercase() == want)?;
    Some(DatapackProvenance {
        source: row.source?,
        project_id: row.project_id.clone()?,
        version_id: row.version_id.clone()?,
        version_number: row.version_number.clone(),
    })
}

/// The source instance's registry rows — the only record of a pack's catalog
/// provenance. Read once per migration and never written by this step
/// (`registry::list` may persist its own reconciliation, exactly as opening
/// the source's Datapacks tab does).
///
/// Fallback: an unreadable source registry yields no provenance at all.
/// Direction — every adopted pack is then recorded as a local install, which
/// leaves the target's update checker silent for it rather than claiming a
/// catalog identity nobody verified; the adoption itself is unaffected. The
/// cause goes to Logs.
async fn source_registry_rows(src_root: &Path) -> Vec<InstalledDatapack> {
    match registry::list(src_root).await {
        Ok(rows) => rows,
        Err(e) => {
            crate::diag!(
                "worlds::migrate: could not read the source datapack registry at {}: {e}; adopting without provenance",
                src_root.display()
            );
            Vec::new()
        }
    }
}

/// Rename path only. A world file that ended up NOT linked to the target
/// library is still a hardlink into the SOURCE library — playable, but a
/// cross-instance share the move must not leave behind, and "left as a plain
/// copy" would be false. `materialize(world_file, world_file, ForceCopy)`
/// copies the bytes through the name into a fresh temp sibling and renames
/// that over the name (`mods::store`: `copy_into_temp`, then `commit`): the
/// only write is to the temp file, and the rename replaces one directory
/// entry, so the source library's own name keeps the old inode untouched.
///
/// Recovery-path result checked: on failure the file still holds the right
/// bytes (a valid link), only the sharing survives — the entry keeps its own
/// `LeftAsCopy` reason and the cause goes to Logs.
async fn deshare(world_file: &Path) {
    match store::materialize(world_file, world_file, LinkPolicy::ForceCopy).await {
        Ok(_) => {}
        Err(e) => crate::diag!(
            "worlds::migrate: could not de-share {} from the source library: {}",
            world_file.display(),
            e.details()
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::path::PathBuf;

    use zip::write::SimpleFileOptions;

    use super::*;
    use crate::datapacks::world_link::test_util::{datapack_zip, hardlink_lock, seed_library};
    use crate::error::DatapackRejection;
    use crate::mods::platform::ModSource;

    /// A zip with no `pack.mcmeta`: the library classifies it `NotAPack`.
    fn not_a_pack_zip() -> Vec<u8> {
        let mut zw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zw.start_file("readme.txt", opts).unwrap();
        zw.write_all(b"not a datapack").unwrap();
        zw.finish().unwrap().into_inner()
    }

    struct Fixture {
        _td: tempfile::TempDir,
        src_root: PathBuf,
        dst_root: PathBuf,
        stage: PathBuf,
    }

    /// Two instance roots, and a stage that sits where `stage_world_at` puts
    /// it — a dot-directory under the TARGET's `saves/` — with an empty
    /// `datapacks/` folder.
    fn fixture() -> Fixture {
        let td = tempfile::tempdir().unwrap();
        let src_root = td.path().join("src");
        let dst_root = td.path().join("dst");
        let stage = dst_root
            .join(".minecraft")
            .join("saves")
            .join(".tmp-migrate-copy-World-1");
        std::fs::create_dir_all(stage.join("datapacks")).unwrap();
        std::fs::create_dir_all(&src_root).unwrap();
        Fixture {
            _td: td,
            src_root,
            dst_root,
            stage,
        }
    }

    fn stage_file(f: &Fixture, name: &str, bytes: &[u8]) -> PathBuf {
        let p = f.stage.join("datapacks").join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    fn lib_file(root: &Path, name: &str) -> PathBuf {
        library_dir_at(root).join(name)
    }

    async fn relink(f: &Fixture, path: MigrationPath) -> Relinked {
        relink_datapacks_at(&f.stage, &f.src_root, &f.dst_root, path)
            .await
            .unwrap()
    }

    /// The store's write-through idiom (`mods::store` tests): mutate the
    /// bytes through `a` IN PLACE and see whether `b` observes it. One
    /// physical file ⇒ observed; independent copies ⇒ not. Test-only — the
    /// one place allowed to write through a link, because it asserts the
    /// very property production code must never rely on.
    fn same_physical_file(a: &Path, b: &Path) -> bool {
        std::fs::write(a, b"MUTATED-THROUGH-A").expect("mutate through a");
        std::fs::read(b).expect("read b") == b"MUTATED-THROUGH-A"
    }

    /// Spec §9.5 — the A1 invariant. The target library holds `foo.zip` v2,
    /// hardlinked into target world B; the migrating world carries `foo.zip`
    /// v1. Neither the library file nor B may change.
    #[tokio::test]
    async fn a_name_held_by_different_bytes_is_left_as_a_copy_and_the_library_is_untouched() {
        let _lock = hardlink_lock();
        let f = fixture();
        let v1 = datapack_zip(48);
        let v2 = datapack_zip(57);
        assert_ne!(v1, v2);
        seed_library(&f.dst_root, "foo.zip", 57).await;
        let b_dp = f.dst_root.join(".minecraft/saves/B/datapacks");
        std::fs::create_dir_all(&b_dp).unwrap();
        std::fs::hard_link(lib_file(&f.dst_root, "foo.zip"), b_dp.join("foo.zip")).unwrap();
        let staged = stage_file(&f, "foo.zip", &v1);

        let out = relink(&f, MigrationPath::Copied).await;

        assert_eq!(
            out.datapacks,
            vec![DatapackMigration {
                filename: "foo.zip".into(),
                result: DatapackResult::LeftAsCopy {
                    reason: LeftReason::NameHeldByDifferentPack,
                },
            }]
        );
        assert_eq!(out.folders_copied, 0);
        assert_eq!(
            std::fs::read(lib_file(&f.dst_root, "foo.zip")).unwrap(),
            v2,
            "a migration never replaces a target library file"
        );
        assert_eq!(
            std::fs::read(b_dp.join("foo.zip")).unwrap(),
            v2,
            "no refresh onto another world"
        );
        assert_eq!(
            std::fs::read(&staged).unwrap(),
            v1,
            "the migrating world keeps its plain copy"
        );
        assert_eq!(registry::list(&f.dst_root).await.unwrap().len(), 1);
    }

    /// Spec §9.6. Absent name ⇒ adopted under that name, linked, and the
    /// SOURCE registry's provenance (looked up case-folded) lands in the
    /// target registry so update checking is not inert there.
    #[tokio::test]
    async fn an_absent_name_is_adopted_with_the_source_rows_provenance() {
        let _lock = hardlink_lock();
        let f = fixture();
        let bytes = datapack_zip(48);
        let prov = DatapackProvenance {
            source: ModSource::Modrinth,
            project_id: "terra".into(),
            version_id: "v7".into(),
            version_number: Some("2.1.0".into()),
        };
        library::install_named_at(&f.src_root, "Terra.zip", &bytes, Some(&prov))
            .await
            .unwrap();
        let staged = stage_file(&f, "terra.zip", &bytes);

        let out = relink(&f, MigrationPath::Copied).await;

        assert_eq!(out.datapacks.len(), 1);
        assert_eq!(out.datapacks[0].filename, "terra.zip");
        let rows = registry::list(&f.dst_root).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].filename, "terra.zip");
        assert_eq!(rows[0].source, Some(ModSource::Modrinth));
        assert_eq!(rows[0].project_id.as_deref(), Some("terra"));
        assert_eq!(rows[0].version_id.as_deref(), Some("v7"));
        assert_eq!(rows[0].version_number.as_deref(), Some("2.1.0"));
        let target_lib = lib_file(&f.dst_root, "terra.zip");
        assert_eq!(std::fs::read(&target_lib).unwrap(), bytes);
        let src_rows = registry::list(&f.src_root).await.unwrap();
        assert_eq!(
            src_rows[0].version_id.as_deref(),
            Some("v7"),
            "the source registry is read, never rewritten"
        );
        // Write-through LAST: it mutates the library file.
        match out.datapacks[0].result {
            DatapackResult::Adopted => assert!(
                same_physical_file(&target_lib, &staged),
                "Adopted means adopted AND linked"
            ),
            DatapackResult::CopiedNotLinked => assert!(
                !same_physical_file(&target_lib, &staged),
                "a copy must stay independent"
            ),
            ref other => panic!("expected Adopted or CopiedNotLinked, got {other:?}"),
        }
    }

    /// The adopt path's half of the invariant: a target world holding a
    /// hand-dropped file under the SAME name is not touched by the adoption's
    /// same-name fan-out (`old_sha` is `None` for a fresh name).
    #[tokio::test]
    async fn adopting_a_new_name_never_refreshes_another_worlds_foreign_file() {
        let _lock = hardlink_lock();
        let f = fixture();
        let theirs = f.dst_root.join(".minecraft/saves/B/datapacks");
        std::fs::create_dir_all(&theirs).unwrap();
        std::fs::write(theirs.join("new.zip"), b"the user's own bytes").unwrap();
        let staged = stage_file(&f, "new.zip", &datapack_zip(48));

        let out = relink(&f, MigrationPath::Copied).await;

        assert!(
            matches!(
                out.datapacks[0].result,
                DatapackResult::Adopted | DatapackResult::CopiedNotLinked
            ),
            "got {:?}",
            out.datapacks[0].result
        );
        assert_eq!(
            std::fs::read(theirs.join("new.zip")).unwrap(),
            b"the user's own bytes",
            "a foreign same-named world file must never be overwritten"
        );
        assert_eq!(
            std::fs::read(lib_file(&f.dst_root, "new.zip")).unwrap(),
            datapack_zip(48)
        );
        assert_eq!(std::fs::read(&staged).unwrap(), datapack_zip(48));
    }

    /// Spec §9.7. Same name, same bytes ⇒ a hardlink to the library file,
    /// proven with the write-through idiom and branched on the placement.
    #[tokio::test]
    async fn a_same_name_same_bytes_pack_becomes_a_link_to_the_library() {
        let _lock = hardlink_lock();
        let f = fixture();
        seed_library(&f.dst_root, "vm.zip", 48).await;
        let staged = stage_file(&f, "vm.zip", &datapack_zip(48));
        let lib = lib_file(&f.dst_root, "vm.zip");

        let out = relink(&f, MigrationPath::Copied).await;

        assert_eq!(out.datapacks.len(), 1);
        assert_eq!(
            registry::list(&f.dst_root).await.unwrap().len(),
            1,
            "linking adds no registry row"
        );
        match out.datapacks[0].result {
            DatapackResult::Linked => assert!(
                same_physical_file(&lib, &staged),
                "Linked must mean one physical file"
            ),
            DatapackResult::CopiedNotLinked => assert!(
                !same_physical_file(&lib, &staged),
                "a copy must stay independent"
            ),
            ref other => panic!("expected Linked or CopiedNotLinked, got {other:?}"),
        }
    }

    /// `materialize`'s documented fallback: when the filesystem cannot link,
    /// the outcome says so — a copy is never reported as `Linked`/`Adopted`.
    #[tokio::test]
    async fn when_linking_is_impossible_the_result_never_claims_a_link() {
        let _seam = crate::test_seam::scope(&[("LUCERNA_TEST_FORCE_LINK_FAILURE", "1")]);
        let f = fixture();
        seed_library(&f.dst_root, "vm.zip", 48).await;
        let staged_same = stage_file(&f, "vm.zip", &datapack_zip(48));
        stage_file(&f, "new.zip", &datapack_zip(57));

        let out = relink(&f, MigrationPath::Copied).await;

        let names: Vec<&str> = out.datapacks.iter().map(|d| d.filename.as_str()).collect();
        assert_eq!(
            names,
            vec!["new.zip", "vm.zip"],
            "outcome order is by filename"
        );
        assert_eq!(out.datapacks[0].result, DatapackResult::CopiedNotLinked);
        assert_eq!(out.datapacks[1].result, DatapackResult::CopiedNotLinked);
        assert_eq!(
            std::fs::read(lib_file(&f.dst_root, "new.zip")).unwrap(),
            datapack_zip(57),
            "the pack is still adopted into the library"
        );
        assert!(!same_physical_file(
            &lib_file(&f.dst_root, "vm.zip"),
            &staged_same
        ));
    }

    /// Spec §9.8. A zip the library refuses stays a plain copy with a typed
    /// reason; the step still succeeds and nothing lands in the library.
    #[tokio::test]
    async fn a_zip_the_library_refuses_stays_a_plain_copy_and_the_step_still_succeeds() {
        let f = fixture();
        let staged = stage_file(&f, "notes.zip", &not_a_pack_zip());

        let out = relink(&f, MigrationPath::Copied).await;

        assert_eq!(
            out.datapacks,
            vec![DatapackMigration {
                filename: "notes.zip".into(),
                result: DatapackResult::LeftAsCopy {
                    reason: LeftReason::NotADatapack {
                        reason: DatapackRejection::NotAPack,
                    },
                },
            }]
        );
        assert!(
            !lib_file(&f.dst_root, "notes.zip").try_exists().unwrap(),
            "a refused pack must not land in the library"
        );
        assert_eq!(std::fs::read(&staged).unwrap(), not_a_pack_zip());
        assert!(registry::list(&f.dst_root).await.unwrap().is_empty());
    }

    /// Spec §9.9. A folder pack is counted and left exactly as it is — never
    /// zipped into the library; a non-zip file is ignored.
    #[tokio::test]
    async fn a_folder_pack_is_counted_and_never_zipped_into_the_library() {
        let _lock = hardlink_lock();
        let f = fixture();
        let folder = f.stage.join("datapacks").join("MyFolderPack");
        std::fs::create_dir_all(folder.join("data")).unwrap();
        std::fs::write(
            folder.join("pack.mcmeta"),
            br#"{"pack":{"pack_format":48}}"#,
        )
        .unwrap();
        stage_file(&f, "vm.zip", &datapack_zip(48));
        stage_file(&f, "README.txt", b"ignored");

        let out = relink(&f, MigrationPath::Copied).await;

        assert_eq!(out.folders_copied, 1);
        assert_eq!(out.datapacks.len(), 1);
        assert_eq!(out.datapacks[0].filename, "vm.zip");
        assert!(!lib_file(&f.dst_root, "MyFolderPack.zip")
            .try_exists()
            .unwrap());
        assert!(
            folder.join("pack.mcmeta").is_file(),
            "the folder pack is left exactly as it was"
        );
        let rows = registry::list(&f.dst_root).await.unwrap();
        let names: Vec<&str> = rows.iter().map(|r| r.filename.as_str()).collect();
        assert_eq!(names, vec!["vm.zip"]);
    }

    /// Spec §9.10. Rename path: the world's file is a hardlink into the SOURCE
    /// library (what a Move brings along). Left unlinked in the target, it
    /// must be de-shared — a write through the source library name must no
    /// longer reach it — while keeping its own bytes.
    #[tokio::test]
    async fn on_the_rename_path_a_pack_left_as_a_copy_no_longer_shares_the_source_library_file() {
        let _lock = hardlink_lock();
        let f = fixture();
        seed_library(&f.src_root, "foo.zip", 48).await;
        let src_lib = lib_file(&f.src_root, "foo.zip");
        let staged = f.stage.join("datapacks").join("foo.zip");
        std::fs::hard_link(&src_lib, &staged).unwrap();
        seed_library(&f.dst_root, "foo.zip", 57).await;

        let out = relink(&f, MigrationPath::Renamed).await;

        assert_eq!(
            out.datapacks[0].result,
            DatapackResult::LeftAsCopy {
                reason: LeftReason::NameHeldByDifferentPack,
            }
        );
        assert_eq!(
            std::fs::read(&staged).unwrap(),
            datapack_zip(48),
            "the world keeps its own bytes"
        );
        assert_eq!(
            std::fs::read(lib_file(&f.dst_root, "foo.zip")).unwrap(),
            datapack_zip(57),
            "the target library is untouched"
        );
        assert!(
            !same_physical_file(&src_lib, &staged),
            "a write through the source library name must no longer reach the moved world"
        );
    }

    /// A stat failure on the library name — here ENOTDIR, a FILE where the
    /// target library directory should be — is `Io`: the same "left as a
    /// copy" direction, never dressed up as a different pack holding the
    /// name. Unix only: Windows reports a path through a file as not found.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_library_name_that_cannot_be_stat_ed_is_left_as_io_not_a_different_pack() {
        let f = fixture();
        let staged = stage_file(&f, "vm.zip", &datapack_zip(48));
        std::fs::write(
            library_dir_at(&f.dst_root),
            b"a file where the library directory should be",
        )
        .unwrap();

        let out = relink(&f, MigrationPath::Copied).await;

        assert_eq!(
            out.datapacks,
            vec![DatapackMigration {
                filename: "vm.zip".into(),
                result: DatapackResult::LeftAsCopy {
                    reason: LeftReason::Io,
                },
            }]
        );
        assert_eq!(
            std::fs::read(&staged).unwrap(),
            datapack_zip(48),
            "the world keeps its plain copy"
        );
    }

    #[tokio::test]
    async fn a_world_without_a_datapacks_folder_has_zero_packs() {
        let td = tempfile::tempdir().unwrap();
        let stage = td.path().join("stage");
        std::fs::create_dir_all(&stage).unwrap();

        let out = relink_datapacks_at(
            &stage,
            &td.path().join("src"),
            &td.path().join("dst"),
            MigrationPath::Copied,
        )
        .await
        .unwrap();

        assert!(out.datapacks.is_empty());
        assert_eq!(out.folders_copied, 0);
        assert!(
            !td.path().join("dst").try_exists().unwrap(),
            "nothing is created for nothing"
        );
    }
}
