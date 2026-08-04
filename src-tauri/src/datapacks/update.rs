//! The datapack update: one catalog version replacing another in the library
//! AND in every world holding the pack, preserving each world's own
//! enabled/disabled choice.
//!
//! The slice-2 design (§8.5) rewrote this algorithm after the audit found six
//! independent defects in the naive version, three of which lose or duplicate
//! user content. The two that shape THIS file:
//!
//! * **Unchanged filename means steps 3-5 do not run.** `install_named_at`'s
//!   own fan-out already refreshes every world holding the name; running the
//!   removal step afterwards would delete the file the install just wrote.
//! * **No rollback on partial failure.** Rolling back already-migrated worlds
//!   means rewriting each level.dat a second time purely to undo. Instead the
//!   old library file and registry row stay, the per-world report says exactly
//!   which worlds moved, and a re-run converges because a migrated world no
//!   longer holds the old filename.
//!
//! The per-world half (delete-old-entry, three-case enabled reading,
//! `forget_ci`, identity verification) lives in
//! [`world_link::migrate_placements`], which takes `level_dat_lock` itself —
//! see its doc for why it cannot be composed from the public entry points.

use std::path::Path;

use crate::datapacks::{
    library, world_link, DatapackProvenance, DatapackUpdateOutcome, WorldMigration,
};
use crate::error::{Error, Result};

/// Apply one resolved update: `bytes` is the target version's verified
/// content, `new_filename` its platform filename, `old_filename` the library
/// entry being replaced. The caller owns download and verification.
pub async fn update_at(
    instance_root: &Path,
    old_filename: &str,
    new_filename: &str,
    bytes: &[u8],
    provenance: &DatapackProvenance,
) -> Result<DatapackUpdateOutcome> {
    // `old_filename` feeds `placements_of` and `remove_at`, both of which join
    // it into world paths; validate at the boundary like every other entry
    // point. `new_filename` is validated inside `install_named_at`.
    if !crate::pathsafe::is_safe_filename(old_filename) {
        return Err(Error::ModsUnsafeFilename {
            filename: old_filename.to_string(),
        });
    }

    // Case-insensitive: NTFS is, so names differing only in case address the
    // SAME file, and treating that as a rename would run the migrate-and-
    // remove path against the file just installed. The old (registered)
    // spelling wins — installing under it keeps the registry and the
    // directory entry consistent on every platform.
    if new_filename.eq_ignore_ascii_case(old_filename) {
        let install =
            library::install_named_at(instance_root, old_filename, bytes, Some(provenance))
                .await?;
        return Ok(DatapackUpdateOutcome {
            pack: install.pack,
            migrations: install.refreshed,
            completed: true,
        });
    }

    let install =
        library::install_named_at(instance_root, new_filename, bytes, Some(provenance)).await?;
    // `install.refreshed` covers worlds that already held the NEW name (rare,
    // but a re-run after a partial failure lands here); the migration below
    // covers worlds still on the OLD name. Together they are the full
    // per-world report.
    let mut migrations = install.refreshed;
    let moved = world_link::migrate_placements(instance_root, old_filename, new_filename).await;
    let failed = moved
        .iter()
        .any(|m| matches!(m, WorldMigration::Failed { .. }));
    migrations.extend(moved);

    if failed {
        // No rollback (§8.5). The old library file also cannot be cleaned up
        // yet: the re-run's identity verification compares world copies
        // against ITS bytes, so deleting it would make the unmigrated worlds
        // look foreign and the retry could never finish.
        return Ok(DatapackUpdateOutcome {
            pack: install.pack,
            migrations,
            completed: false,
        });
    }

    library::remove_at(instance_root, old_filename).await?;
    Ok(DatapackUpdateOutcome {
        pack: install.pack,
        migrations,
        completed: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datapacks::level_dat;
    use std::io::Write;
    use std::path::PathBuf;
    use zip::write::SimpleFileOptions;

    fn zip_with(tick_body: &[u8]) -> Vec<u8> {
        let mut zw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zw.start_file("pack.mcmeta", opts).unwrap();
        zw.write_all(br#"{"pack":{"pack_format":48,"description":"Vein Miner"}}"#)
            .unwrap();
        zw.start_file("data/vm/function/tick.mcfunction", opts)
            .unwrap();
        zw.write_all(tick_body).unwrap();
        zw.finish().unwrap().into_inner()
    }

    fn v1_zip() -> Vec<u8> {
        zip_with(b"say v1")
    }

    fn v2_zip() -> Vec<u8> {
        zip_with(b"say v2")
    }

    fn prov(version_id: &str) -> DatapackProvenance {
        DatapackProvenance {
            source: crate::mods::platform::ModSource::Modrinth,
            project_id: "veinminer".into(),
            version_id: version_id.into(),
            version_number: Some(format!("{version_id}.0")),
        }
    }

    fn world_dir(root: &Path, world: &str) -> PathBuf {
        root.join(".minecraft").join("saves").join(world)
    }

    async fn seed_world(root: &Path, world: &str, filename: &str) {
        std::fs::create_dir_all(world_dir(root, world)).unwrap();
        world_link::add_to_world_at(root, world, filename)
            .await
            .unwrap();
    }

    fn level_dat_bytes(root: &Path, world: &str) -> Vec<u8> {
        std::fs::read(world_dir(root, world).join("level.dat")).unwrap()
    }

    #[tokio::test]
    async fn an_unchanged_filename_is_one_refresh_not_a_delete() {
        // §8.5 defect 2: the naive algorithm ran its removal step
        // unconditionally, so when the filename did not change it DELETED the
        // file step 1 had just written. The unchanged case must be exactly
        // `install_named_at` — fan-out refresh, level.dat untouched.
        let _lock = crate::test_env_lock();
        let td = tempfile::tempdir().unwrap();
        library::install_named_at(td.path(), "vm.zip", &v1_zip(), Some(&prov("v1")))
            .await
            .unwrap();
        seed_world(td.path(), "Alpha", "vm.zip").await;
        // The user turned it OFF — the state the update must preserve.
        world_link::set_enabled_in_world_at(td.path(), "Alpha", "vm.zip", false)
            .await
            .unwrap();
        let level_dat_before = level_dat_bytes(td.path(), "Alpha");

        let out = update_at(td.path(), "vm.zip", "vm.zip", &v2_zip(), &prov("v2"))
            .await
            .unwrap();

        assert!(out.completed);
        assert!(
            matches!(
                out.migrations.as_slice(),
                [WorldMigration::Migrated { world, .. }] if world == "Alpha"
            ),
            "got {:?}",
            out.migrations
        );
        assert_eq!(
            std::fs::read(td.path().join("datapacks/vm.zip")).unwrap(),
            v2_zip(),
            "the library file must hold the NEW bytes, not be deleted"
        );
        assert_eq!(
            std::fs::read(world_dir(td.path(), "Alpha").join("datapacks/vm.zip")).unwrap(),
            v2_zip()
        );
        let listed = library::list_at(td.path()).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].version_id.as_deref(), Some("v2"));
        assert_eq!(
            level_dat_bytes(td.path(), "Alpha"),
            level_dat_before,
            "a same-name refresh must not touch level.dat at all"
        );
    }

    #[tokio::test]
    async fn a_case_only_rename_is_treated_as_unchanged() {
        // NTFS is case-insensitive: `VM.zip` and `vm.zip` address the SAME
        // file, so treating a case-only difference as a rename would run the
        // migrate-and-remove path against the file just installed. The old
        // (registered) spelling wins; only the provenance moves forward.
        let _lock = crate::test_env_lock();
        let td = tempfile::tempdir().unwrap();
        library::install_named_at(td.path(), "VM.zip", &v1_zip(), Some(&prov("v1")))
            .await
            .unwrap();

        let out = update_at(td.path(), "VM.zip", "vm.zip", &v2_zip(), &prov("v2"))
            .await
            .unwrap();

        assert!(out.completed);
        assert_eq!(out.pack.filename, "VM.zip", "the registered spelling wins");
        let listed = library::list_at(td.path()).await.unwrap();
        assert_eq!(listed.len(), 1, "exactly one row, not old+new: {listed:?}");
        assert_eq!(listed[0].filename, "VM.zip");
        assert_eq!(listed[0].version_id.as_deref(), Some("v2"));
        assert_eq!(
            std::fs::read(td.path().join("datapacks/VM.zip")).unwrap(),
            v2_zip()
        );
    }

    #[tokio::test]
    async fn a_renamed_update_migrates_worlds_and_removes_the_old_pack() {
        let _lock = crate::test_env_lock();
        let td = tempfile::tempdir().unwrap();
        library::install_named_at(td.path(), "vm-1.zip", &v1_zip(), Some(&prov("v1")))
            .await
            .unwrap();
        seed_world(td.path(), "Alpha", "vm-1.zip").await;
        seed_world(td.path(), "Beta", "vm-1.zip").await;
        world_link::set_enabled_in_world_at(td.path(), "Beta", "vm-1.zip", false)
            .await
            .unwrap();

        let out = update_at(td.path(), "vm-1.zip", "vm-2.zip", &v2_zip(), &prov("v2"))
            .await
            .unwrap();

        assert!(out.completed);
        assert!(
            !td.path().join("datapacks/vm-1.zip").exists(),
            "the old library file must be gone after a completed update"
        );
        let listed = library::list_at(td.path()).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].filename, "vm-2.zip");
        assert_eq!(listed[0].version_id.as_deref(), Some("v2"));

        for (world, want_enabled) in [("Alpha", true), ("Beta", false)] {
            let dp = world_dir(td.path(), world).join("datapacks");
            assert!(
                !dp.join("vm-1.zip").exists(),
                "{world}: the OLD world file must be gone — present-and-unlisted \
                 is auto-enabled and BOTH versions would load"
            );
            assert_eq!(std::fs::read(dp.join("vm-2.zip")).unwrap(), v2_zip());
            let (root, _) = level_dat::read_at(&world_dir(td.path(), world)).unwrap();
            let (enabled, disabled) = level_dat::lists(&root);
            assert!(
                !enabled.iter().chain(disabled.iter()).any(|s| s.contains("vm-1")),
                "{world}: level.dat must not name the old file: {enabled:?} {disabled:?}"
            );
            let entry_list = if want_enabled { &enabled } else { &disabled };
            assert!(
                entry_list.iter().any(|s| s == "file/vm-2.zip"),
                "{world}: expected vm-2 enabled={want_enabled}: {enabled:?} {disabled:?}"
            );
        }
    }

    /// Windows-only: holding a handle without `FILE_SHARE_DELETE` is what a
    /// running game does, and makes the old-file removal fail
    /// deterministically. The no-rollback policy itself is
    /// platform-independent and CI runs this on Windows.
    #[cfg(windows)]
    #[tokio::test]
    async fn a_failed_world_keeps_both_library_files_and_a_rerun_converges() {
        use std::os::windows::fs::OpenOptionsExt;

        let _lock = crate::test_env_lock();
        let td = tempfile::tempdir().unwrap();
        library::install_named_at(td.path(), "vm-1.zip", &v1_zip(), Some(&prov("v1")))
            .await
            .unwrap();
        seed_world(td.path(), "Alpha", "vm-1.zip").await;
        seed_world(td.path(), "Beta", "vm-1.zip").await;
        world_link::set_enabled_in_world_at(td.path(), "Beta", "vm-1.zip", false)
            .await
            .unwrap();
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0x1 /* FILE_SHARE_READ: no delete sharing */)
            .open(world_dir(td.path(), "Beta").join("datapacks/vm-1.zip"))
            .unwrap();

        let out = update_at(td.path(), "vm-1.zip", "vm-2.zip", &v2_zip(), &prov("v2"))
            .await
            .unwrap();

        assert!(!out.completed);
        assert!(
            out.migrations
                .iter()
                .any(|m| matches!(m, WorldMigration::Failed { world, .. } if world == "Beta")),
            "got {:?}",
            out.migrations
        );
        assert!(
            td.path().join("datapacks/vm-1.zip").exists(),
            "no rollback, no premature cleanup: the old library file must stay \
             so the re-run can identity-verify Beta's copy"
        );
        assert!(td.path().join("datapacks/vm-2.zip").exists());

        // Release the handle; the re-run must converge.
        drop(held);
        let out = update_at(td.path(), "vm-1.zip", "vm-2.zip", &v2_zip(), &prov("v2"))
            .await
            .unwrap();
        assert!(out.completed, "got {:?}", out.migrations);
        assert!(!td.path().join("datapacks/vm-1.zip").exists());
        let dp = world_dir(td.path(), "Beta").join("datapacks");
        assert!(!dp.join("vm-1.zip").exists());
        assert!(dp.join("vm-2.zip").exists());
        let (root, _) = level_dat::read_at(&world_dir(td.path(), "Beta")).unwrap();
        let (_enabled, disabled) = level_dat::lists(&root);
        assert!(
            disabled.iter().any(|s| s == "file/vm-2.zip"),
            "Beta's OFF choice must survive the retried update: {disabled:?}"
        );
    }

    #[tokio::test]
    async fn an_unsafe_old_filename_is_rejected_at_the_boundary() {
        let td = tempfile::tempdir().unwrap();
        let err = update_at(td.path(), "../escape.zip", "vm.zip", &v2_zip(), &prov("v2"))
            .await
            .unwrap_err();
        assert!(matches!(err, Error::ModsUnsafeFilename { .. }), "got {err:?}");
    }
}
