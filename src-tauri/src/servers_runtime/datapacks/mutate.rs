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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datapacks::{level_dat, level_dat_entry, WorldPackState};
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
}
