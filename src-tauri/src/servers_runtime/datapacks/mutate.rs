//! Install, toggle and removal for a server world's datapacks.

use std::path::Path;

use crate::datapacks::{level_dat, level_dat_entry, pack_meta, DatapackProvenance};
use crate::error::{DatapackRejection, Error, Result};
use crate::servers_runtime::installed::ServerInstalledRecord;

use super::level_dat_lock;

/// Enable or disable one pack in the world's `level.dat`. The file itself is
/// never touched — this is the game's own mechanism, so what the launcher
/// shows is exactly what `/datapack list` shows.
///
/// Works for folder packs too: `level.dat` does not distinguish them.
///
/// Refuses when `level.dat` is absent — see [`Error::ServerWorldNotCreated`].
pub async fn set_enabled(world_dir: &Path, filename: &str, enabled: bool) -> Result<()> {
    if !crate::pathsafe::is_safe_filename(filename) {
        return Err(Error::ModsUnsafeFilename {
            filename: filename.to_string(),
        });
    }
    let _guard = level_dat_lock().lock().await;
    if !world_dir.join("level.dat").exists() {
        return Err(Error::ServerWorldNotCreated);
    }
    let (mut root, framing) = level_dat::read_at(world_dir)?;
    if level_dat::set_enabled(&mut root, &level_dat_entry(filename), enabled)? {
        level_dat::write_at(world_dir, &root, framing).await?;
    }
    Ok(())
}

/// Remove a pack from the world: the on-disk entry, its `level.dat` name and
/// its sidecar row.
///
/// Type-directed, like the client's own removal: `remove_dir_all` for a
/// directory, `remove_file` for a file, and a missing entry is success —
/// which is what makes this the repair for a ghost row (a `level.dat` name
/// whose file is already gone, in either list).
///
/// Tolerates an absent `level.dat`: a pack installed before the server's
/// first boot has no level.dat half to clear, and failing here would make it
/// unremovable. (The toggle takes the opposite call — see [`set_enabled`].)
pub async fn remove(world_dir: &Path, filename: &str) -> Result<()> {
    if !crate::pathsafe::is_safe_filename(filename) {
        return Err(Error::ModsUnsafeFilename {
            filename: filename.to_string(),
        });
    }
    let dp_dir = world_dir.join("datapacks");
    let path = dp_dir.join(filename);
    // Deliberate defence-in-depth, not an oversight: `is_safe_filename` above
    // already rejects separators, so `path` cannot actually escape `dp_dir`.
    // This guards the unconditional `remove_dir_all` below against that
    // invariant ever being wrong.
    if !path.starts_with(&dp_dir) {
        return Err(Error::ServerFileInvalid {
            filename: filename.to_string(),
            reason: "path escapes the datapacks dir".into(),
        });
    }

    match std::fs::metadata(&path) {
        Ok(m) if m.is_dir() => {
            std::fs::remove_dir_all(&path).map_err(|e| Error::io(path.display().to_string(), e))?
        }
        Ok(_) => {
            std::fs::remove_file(&path).map_err(|e| Error::io(path.display().to_string(), e))?
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(Error::io(path.display().to_string(), e)),
    }

    {
        let _guard = level_dat_lock().lock().await;
        if world_dir.join("level.dat").exists() {
            let (mut root, framing) = level_dat::read_at(world_dir)?;
            if level_dat::forget_ci(&mut root, &level_dat_entry(filename))? {
                level_dat::write_at(world_dir, &root, framing).await?;
            }
        }
    }

    super::sidecar::forget(world_dir, filename)
}

/// Place verified bytes into the world's `datapacks/` and record a sidecar
/// row. `provenance: None` is a local install (file picker / drag-drop);
/// `Some` is a catalog install.
///
/// The same-name conflict rule is slice 2's provenance table, with the server
/// world dir as the analogue of the client library dir:
///
/// * local install — the admin picked this exact file under this exact name;
///   never blocked (slice 1 pinned "reinstalling a newer zip refreshes", and a
///   differing-bytes rule would break it);
/// * catalog install onto a row from the SAME project — the update path; allow;
/// * catalog install onto a local pack, a different project's pack, or a
///   DIRECTORY (never provably ours) — two packs competing for one name;
///   `ModsFilenameConflict`.
///
/// No `level.dat` write: a fresh pack lands present-and-unlisted, which the
/// game auto-enables on boot — the honest server-side default, and exactly
/// why `state::derive`'s `(true, _, false)` arm exists.
pub async fn install_bytes(
    world_dir: &Path,
    filename: &str,
    bytes: &[u8],
    provenance: Option<&DatapackProvenance>,
) -> Result<ServerInstalledRecord> {
    if !crate::pathsafe::is_safe_filename(filename) {
        return Err(Error::ModsUnsafeFilename {
            filename: filename.to_string(),
        });
    }
    // Minecraft's pack scanner loads directories and `*.zip` only, and a
    // non-zip name written here is worse than one that never loads: the
    // sidecar reconcile adopts only `.zip`, so the row is dropped on the next
    // listing while the file stays on disk — invisible and unremovable.
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

    // Classification and hashing each walk the whole archive; a catalog
    // install is an automated path where a large pack would stall the async
    // executor.
    let owned = bytes.to_vec();
    let name_for_join = filename.to_string();
    let (kind, meta, sha1) = tokio::task::spawn_blocking(move || {
        (
            pack_meta::classify(&owned),
            pack_meta::read_meta(&owned),
            crate::datapacks::library::sha1_hex(&owned),
        )
    })
    .await
    .map_err(|e| Error::io(name_for_join, format!("join: {e}")))?;

    if kind != pack_meta::PackKind::Datapack {
        let reason = match kind {
            pack_meta::PackKind::ResourcePack => DatapackRejection::IsAResourcePack,
            pack_meta::PackKind::Neither => DatapackRejection::NotAPack,
            // Unreachable: `kind` was computed once above and is not Datapack.
            pack_meta::PackKind::Datapack => unreachable!("kind != Datapack was just checked"),
        };
        return Err(Error::DatapackInvalid {
            filename: filename.to_string(),
            reason,
        });
    }

    let dp_dir = world_dir.join("datapacks");
    let dest = dp_dir.join(filename);
    if !dest.starts_with(&dp_dir) {
        return Err(Error::ServerFileInvalid {
            filename: filename.to_string(),
            reason: "path escapes the datapacks dir".into(),
        });
    }

    if let Some(prov) = provenance {
        if let Ok(meta_dest) = std::fs::metadata(&dest) {
            let want = filename.to_lowercase();
            let existing_row = super::sidecar::reconcile(world_dir)
                .into_iter()
                .find(|r| r.filename.to_lowercase() == want);
            // A directory can never be proven ours — it has no file sha1, and
            // placing bytes here would rename a zip over a whole pack folder.
            let same_project = !meta_dest.is_dir()
                && existing_row.as_ref().is_some_and(|row| {
                    row.source == Some(prov.source)
                        && row.project_id.as_deref() == Some(prov.project_id.as_str())
                });
            if !same_project {
                return Err(Error::ModsFilenameConflict {
                    filename: filename.to_string(),
                    existing_sha: existing_row.map(|r| r.sha1).unwrap_or_default(),
                    incoming_sha: sha1,
                });
            }
        }
    }

    std::fs::create_dir_all(&dp_dir).map_err(|e| Error::io(dp_dir.display().to_string(), e))?;
    // Temp-then-rename: a bare `std::fs::write` here would leave a truncated
    // pack on disk if the install were interrupted mid-write, and the server
    // would refuse to load it.
    crate::mods::store::place_bytes(&dest, bytes)
        .await
        .map_err(|e| Error::io(e.path.display().to_string(), e.details()))?;

    let record = ServerInstalledRecord {
        filename: filename.to_string(),
        sha1,
        source: provenance.map(|p| p.source),
        project_id: provenance.map(|p| p.project_id.clone()),
        version_id: provenance.map(|p| p.version_id.clone()),
        name: meta
            .description
            .or_else(|| Some(filename.trim_end_matches(".zip").to_string())),
        version_number: provenance.and_then(|p| p.version_number.clone()),
        enrich_attempted: false,
    };
    super::sidecar::upsert_by_filename(world_dir, record.clone())?;
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datapacks::{level_dat, WorldPackState};
    use std::io::Write;

    fn datapack_zip(body: &[u8]) -> Vec<u8> {
        let mut zw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zw.start_file("pack.mcmeta", opts).unwrap();
        zw.write_all(br#"{"pack":{"pack_format":48,"description":"Pack"}}"#)
            .unwrap();
        zw.start_file("data/ns/function/tick.mcfunction", opts)
            .unwrap();
        zw.write_all(body).unwrap();
        zw.finish().unwrap().into_inner()
    }

    fn world(packs: &[&str]) -> tempfile::TempDir {
        let td = tempfile::tempdir().unwrap();
        let dp = td.path().join("datapacks");
        std::fs::create_dir_all(&dp).unwrap();
        for n in packs {
            std::fs::write(dp.join(n), datapack_zip(b"x")).unwrap();
        }
        td
    }

    /// What a first boot leaves behind: a level.dat carrying unmodelled tags
    /// this module must preserve byte-for-byte across its edits.
    async fn boot_world(world_dir: &std::path::Path) {
        let mut data = std::collections::HashMap::new();
        data.insert(
            "LevelName".to_string(),
            fastnbt::Value::String("srv".into()),
        );
        data.insert("RandomSeed".to_string(), fastnbt::Value::Long(4242));
        let mut root = std::collections::HashMap::new();
        root.insert("Data".to_string(), fastnbt::Value::Compound(data));
        level_dat::write_at(
            world_dir,
            &fastnbt::Value::Compound(root),
            level_dat::Framing::Gzip,
        )
        .await
        .unwrap();
    }

    fn state_of(world_dir: &std::path::Path, name: &str) -> Option<WorldPackState> {
        super::super::listing::entries(world_dir)
            .into_iter()
            .find(|e| e.record.filename == name)
            .and_then(|e| e.state)
    }

    #[tokio::test]
    async fn the_toggle_round_trips_and_preserves_unmodelled_tags() {
        let td = world(&["p.zip"]);
        boot_world(td.path()).await;

        set_enabled(td.path(), "p.zip", false).await.unwrap();
        assert_eq!(state_of(td.path(), "p.zip"), Some(WorldPackState::Disabled));
        set_enabled(td.path(), "p.zip", true).await.unwrap();
        assert_eq!(state_of(td.path(), "p.zip"), Some(WorldPackState::Enabled));

        let (root, _) = level_dat::read_at(td.path()).unwrap();
        let fastnbt::Value::Compound(top) = &root else {
            panic!("level.dat root is not a compound")
        };
        let fastnbt::Value::Compound(data) = &top["Data"] else {
            panic!("Data is not a compound")
        };
        assert_eq!(data["RandomSeed"], fastnbt::Value::Long(4242));
        assert_eq!(data["LevelName"], fastnbt::Value::String("srv".into()));
    }

    #[tokio::test]
    async fn the_toggle_refuses_before_the_world_exists_and_writes_nothing() {
        // Audit #7: create → add packs → first boot is the normal flow. Writing
        // a stub level.dat into a dir Minecraft is about to generate into would
        // be ineffective at best (generation writes its own) and corrupting at
        // worst.
        let td = world(&["p.zip"]);
        let err = set_enabled(td.path(), "p.zip", false).await.unwrap_err();
        assert!(
            matches!(err, crate::error::Error::ServerWorldNotCreated),
            "got {err:?}"
        );
        assert!(
            !td.path().join("level.dat").exists(),
            "the refusal must not have created a level.dat"
        );
    }

    #[tokio::test]
    async fn the_toggle_works_on_a_folder_pack() {
        let td = world(&[]);
        std::fs::create_dir_all(td.path().join("datapacks").join("Folder")).unwrap();
        boot_world(td.path()).await;
        set_enabled(td.path(), "Folder", false).await.unwrap();
        assert_eq!(
            state_of(td.path(), "Folder"),
            Some(WorldPackState::Disabled)
        );
    }

    #[tokio::test]
    async fn the_toggle_rejects_an_unsafe_filename() {
        let td = world(&[]);
        boot_world(td.path()).await;
        assert!(set_enabled(td.path(), "../escape.zip", true).await.is_err());
    }

    #[tokio::test]
    async fn a_redundant_toggle_writes_nothing() {
        // `set_enabled` writes only when the lists actually changed. Without
        // that guard every no-op toggle would rewrite level.dat — and roll the
        // backup, discarding the last good copy for no reason.
        let td = world(&["p.zip"]);
        boot_world(td.path()).await;
        set_enabled(td.path(), "p.zip", false).await.unwrap();
        let after_first = std::fs::read(td.path().join("level.dat")).unwrap();
        let backup = td.path().join("level.dat_lucerna.bak");
        let backup_before = std::fs::read(&backup).ok();

        set_enabled(td.path(), "p.zip", false).await.unwrap();

        assert_eq!(
            std::fs::read(td.path().join("level.dat")).unwrap(),
            after_first,
            "a no-op toggle must not rewrite level.dat"
        );
        assert_eq!(
            std::fs::read(&backup).ok(),
            backup_before,
            "and must not roll the backup"
        );
    }

    #[tokio::test]
    async fn removal_clears_the_file_the_level_dat_name_and_the_sidecar_row() {
        let td = world(&["p.zip"]);
        boot_world(td.path()).await;
        set_enabled(td.path(), "p.zip", false).await.unwrap();
        super::super::sidecar::reconcile(td.path()); // adopt the row

        remove(td.path(), "p.zip").await.unwrap();

        assert!(!td.path().join("datapacks").join("p.zip").exists());
        let (root, _) = level_dat::read_at(td.path()).unwrap();
        let (en, dis) = level_dat::lists(&root);
        assert!(
            en.is_empty() && dis.is_empty(),
            "the name must be gone from BOTH lists"
        );
        assert!(crate::servers_runtime::installed::load(td.path()).is_empty());
    }

    #[tokio::test]
    async fn removal_clears_a_case_drifted_level_dat_name() {
        let td = world(&["veinminer.zip"]);
        boot_world(td.path()).await;
        set_enabled(td.path(), "VeinMiner.zip", false)
            .await
            .unwrap();
        remove(td.path(), "veinminer.zip").await.unwrap();
        let (root, _) = level_dat::read_at(td.path()).unwrap();
        let (en, dis) = level_dat::lists(&root);
        assert!(
            en.is_empty() && dis.is_empty(),
            "forget_ci must fold the case"
        );
    }

    #[tokio::test]
    async fn removal_repairs_a_ghost_row_of_either_kind() {
        // Audit #8/#10: the union surfaces a level.dat name whose file is gone
        // as a row, so a flow has to be able to clear it — for the Orphaned
        // (Enabled-list) kind AND the NotAdded (Disabled-only) kind.
        let td = world(&[]);
        boot_world(td.path()).await;
        set_enabled(td.path(), "ghost-on.zip", true).await.unwrap();
        set_enabled(td.path(), "ghost-off.zip", false)
            .await
            .unwrap();

        remove(td.path(), "ghost-on.zip").await.unwrap();
        // Selectivity: clearing one ghost must not take the other with it.
        assert_eq!(
            state_of(td.path(), "ghost-off.zip"),
            Some(WorldPackState::NotAdded),
            "removing one ghost name must not clear the other"
        );
        remove(td.path(), "ghost-off.zip").await.unwrap();

        assert!(super::super::listing::entries(td.path()).is_empty());
    }

    #[tokio::test]
    async fn removal_deletes_a_folder_pack_whole() {
        let td = world(&[]);
        let folder = td.path().join("datapacks").join("Folder");
        std::fs::create_dir_all(folder.join("data")).unwrap();
        std::fs::write(folder.join("pack.mcmeta"), b"{}").unwrap();
        boot_world(td.path()).await;
        remove(td.path(), "Folder").await.unwrap();
        assert!(
            !folder.exists(),
            "a directory needs remove_dir_all, not remove_file"
        );
    }

    #[tokio::test]
    async fn removal_succeeds_before_the_world_exists() {
        // The mirror of the toggle's refusal: a pack installed before first
        // boot must still be removable, so removal SKIPS the level.dat half
        // rather than failing on it.
        let td = world(&["p.zip"]);
        assert!(!td.path().join("level.dat").exists());
        remove(td.path(), "p.zip").await.unwrap();
        assert!(!td.path().join("datapacks").join("p.zip").exists());
        assert!(!td.path().join("level.dat").exists(), "still no level.dat");
    }

    #[tokio::test]
    async fn removal_is_idempotent() {
        let td = world(&["p.zip"]);
        boot_world(td.path()).await;
        remove(td.path(), "p.zip").await.unwrap();
        remove(td.path(), "p.zip").await.unwrap();
    }

    fn prov(project: &str, version: &str) -> crate::datapacks::DatapackProvenance {
        crate::datapacks::DatapackProvenance {
            source: crate::mods::platform::ModSource::Modrinth,
            project_id: project.into(),
            version_id: version.into(),
            version_number: Some(format!("{version}.0")),
        }
    }

    #[tokio::test]
    async fn a_catalog_install_records_full_provenance() {
        // The slice-2 lesson: `classify_asset_update` answers UpToDate forever
        // when version_id is None, so an install that drops provenance makes
        // update checking permanently inert.
        let td = world(&[]);
        install_bytes(
            td.path(),
            "t.zip",
            &datapack_zip(b"v1"),
            Some(&prov("terralith", "v1")),
        )
        .await
        .unwrap();
        let rows = crate::servers_runtime::installed::load(td.path());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].version_id.as_deref(), Some("v1"));
        assert_eq!(rows[0].project_id.as_deref(), Some("terralith"));
    }

    #[tokio::test]
    async fn a_local_install_records_a_provenance_less_row() {
        let td = world(&[]);
        install_bytes(td.path(), "hand.zip", &datapack_zip(b"x"), None)
            .await
            .unwrap();
        let rows = crate::servers_runtime::installed::load(td.path());
        assert_eq!(rows.len(), 1);
        assert!(rows[0].source.is_none() && rows[0].sha1.len() == 40);
    }

    #[tokio::test]
    async fn a_catalog_install_will_not_clobber_a_hand_installed_pack_of_the_same_name() {
        let td = world(&[]);
        install_bytes(td.path(), "t.zip", &datapack_zip(b"mine"), None)
            .await
            .unwrap();
        let err = install_bytes(
            td.path(),
            "t.zip",
            &datapack_zip(b"theirs"),
            Some(&prov("terralith", "v1")),
        )
        .await
        .unwrap_err();
        assert!(
            matches!(err, Error::ModsFilenameConflict { .. }),
            "got {err:?}"
        );
        assert_eq!(
            std::fs::read(td.path().join("datapacks").join("t.zip")).unwrap(),
            datapack_zip(b"mine"),
            "the user's pack must be untouched"
        );
    }

    #[tokio::test]
    async fn a_catalog_install_onto_the_same_project_is_the_update_path_and_is_allowed() {
        let td = world(&[]);
        install_bytes(
            td.path(),
            "t.zip",
            &datapack_zip(b"v1"),
            Some(&prov("terralith", "v1")),
        )
        .await
        .unwrap();
        install_bytes(
            td.path(),
            "t.zip",
            &datapack_zip(b"v2"),
            Some(&prov("terralith", "v2")),
        )
        .await
        .unwrap();
        assert_eq!(
            std::fs::read(td.path().join("datapacks").join("t.zip")).unwrap(),
            datapack_zip(b"v2")
        );
    }

    #[tokio::test]
    async fn a_local_install_over_an_existing_name_is_never_blocked() {
        // Slice 1 pinned this deliberately: reinstalling a newer zip by hand is
        // the ordinary workflow, and the conflict rule keys on PROVENANCE, not
        // on differing bytes.
        let td = world(&[]);
        install_bytes(td.path(), "t.zip", &datapack_zip(b"v1"), None)
            .await
            .unwrap();
        install_bytes(td.path(), "t.zip", &datapack_zip(b"v2"), None)
            .await
            .unwrap();
        assert_eq!(
            std::fs::read(td.path().join("datapacks").join("t.zip")).unwrap(),
            datapack_zip(b"v2")
        );
    }

    #[tokio::test]
    async fn installing_over_a_folder_of_the_same_name_is_always_a_conflict() {
        let td = world(&[]);
        std::fs::create_dir_all(td.path().join("datapacks").join("t.zip")).unwrap();
        let err = install_bytes(
            td.path(),
            "t.zip",
            &datapack_zip(b"x"),
            Some(&prov("p", "v1")),
        )
        .await
        .unwrap_err();
        assert!(
            matches!(err, Error::ModsFilenameConflict { .. }),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn install_rejects_a_resource_pack_and_a_non_zip_name() {
        let td = world(&[]);
        let mut zw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zw.start_file("pack.mcmeta", opts).unwrap();
        zw.write_all(br#"{"pack":{"pack_format":34}}"#).unwrap();
        zw.start_file("assets/minecraft/textures/x.png", opts)
            .unwrap();
        zw.write_all(b"\x89PNG").unwrap();
        let rp = zw.finish().unwrap().into_inner();

        assert!(install_bytes(td.path(), "rp.zip", &rp, None).await.is_err());
        assert!(install_bytes(td.path(), "p.jar", &datapack_zip(b"x"), None)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn install_writes_no_level_dat_so_a_fresh_pack_reads_as_enabled() {
        let td = world(&[]);
        install_bytes(td.path(), "p.zip", &datapack_zip(b"x"), None)
            .await
            .unwrap();
        assert!(!td.path().join("level.dat").exists());
        assert_eq!(state_of(td.path(), "p.zip"), Some(WorldPackState::Enabled));
    }
}
