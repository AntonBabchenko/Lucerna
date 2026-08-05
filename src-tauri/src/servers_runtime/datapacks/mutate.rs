//! Install, toggle and removal for a server world's datapacks.

use std::path::Path;

use crate::datapacks::{level_dat, level_dat_entry};
use crate::error::{Error, Result};

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
}
