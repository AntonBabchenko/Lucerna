use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::instances::import::model::{scan_content, ForeignInstance};
use crate::instances::import::readers::LauncherReader;
use crate::instances::schema::{ForeignLauncher, LoaderKind};

/// The Modrinth App ("theseus"). Game content lives in
/// `<ModrinthApp>/profiles/<path>/`; all metadata (version, loader) lives in
/// the sibling SQLite `app.db` (`profiles` table). There is no per-profile
/// JSON on disk, so the DB is the only metadata source.
pub struct ModrinthAppReader;

/// One row of the `profiles` table we care about.
struct ProfileRow {
    path: String,
    name: String,
    game_version: String,
    mod_loader: String,
    mod_loader_version: Option<String>,
    override_mc_memory_max: Option<u32>,
}

fn loader_from_str(s: &str) -> LoaderKind {
    match s.to_ascii_lowercase().as_str() {
        "fabric" => LoaderKind::Fabric,
        "quilt" => LoaderKind::Quilt,
        "forge" => LoaderKind::Forge,
        "neoforge" => LoaderKind::NeoForge,
        _ => LoaderKind::Vanilla,
    }
}

/// `<profiles_root>/../app.db`.
fn db_path_for_profiles_root(profiles_root: &Path) -> Option<PathBuf> {
    let db = profiles_root.parent()?.join("app.db");
    db.is_file().then_some(db)
}

/// Read every installed profile row from `app.db` (read-only). Errors are
/// swallowed into an empty vec — a locked/garbled DB never fails discovery.
fn read_rows(db: &Path) -> Vec<ProfileRow> {
    let Ok(conn) =
        rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
    else {
        return vec![];
    };
    let Ok(mut stmt) = conn.prepare(
        "SELECT path, name, game_version, mod_loader, mod_loader_version, \
         override_mc_memory_max FROM profiles WHERE install_stage = 'installed'",
    ) else {
        return vec![];
    };
    let rows = stmt.query_map([], |r| {
        Ok(ProfileRow {
            path: r.get(0)?,
            name: r.get(1)?,
            game_version: r.get(2)?,
            mod_loader: r.get(3)?,
            mod_loader_version: r.get(4)?,
            // Stored as a signed integer (megabytes); clamp negatives and
            // narrow to the u32 the rest of the import pipeline expects.
            override_mc_memory_max: r.get::<_, Option<i64>>(5)?.map(|v| v.max(0) as u32),
        })
    });
    match rows {
        Ok(iter) => iter.filter_map(|r| r.ok()).collect(),
        Err(_) => vec![],
    }
}

fn build(profiles_root: &Path, row: &ProfileRow) -> ForeignInstance {
    let game_dir = profiles_root.join(&row.path);
    let loader = loader_from_str(&row.mod_loader);
    // A Vanilla profile carries no meaningful loader version even if the DB
    // left a residual string (mirrors the CurseForge/ATLauncher readers).
    let loader_version = if loader == LoaderKind::Vanilla {
        None
    } else {
        row.mod_loader_version.clone().filter(|s| !s.is_empty())
    };
    ForeignInstance {
        source: ForeignLauncher::ModrinthApp,
        name: row.name.clone(),
        root: game_dir.clone(),
        minecraft_dir: game_dir.clone(),
        mc_version: row.game_version.clone(),
        loader,
        loader_version,
        max_heap_mb: row.override_mc_memory_max,
        extra_jvm_args: None,
        content: scan_content(&game_dir),
        known_mods: vec![],
    }
}

impl LauncherReader for ModrinthAppReader {
    fn launcher(&self) -> ForeignLauncher {
        ForeignLauncher::ModrinthApp
    }

    fn default_roots(&self) -> Vec<PathBuf> {
        crate::platform::default_launcher_roots()
            .into_iter()
            .filter(|p| {
                let s = p.to_string_lossy().to_lowercase();
                p.ends_with("profiles") && (s.contains("modrinthapp") || s.contains("theseus"))
            })
            .collect()
    }

    fn detect(&self, dir: &Path) -> bool {
        // A manually-picked Modrinth profile dir: its parent is `profiles`
        // with a sibling `app.db` holding a row whose `path` is this folder.
        // Re-opens the DB (read-only, cheap) per call — only the manual
        // folder-pick path uses `detect`; auto-scan goes via `expand_root`,
        // which reads the DB once. Do not call this per-entry in a loop.
        let Some(profiles_root) = dir.parent() else {
            return false;
        };
        let Some(db) = db_path_for_profiles_root(profiles_root) else {
            return false;
        };
        let name = dir.file_name().and_then(|s| s.to_str()).unwrap_or("");
        read_rows(&db).iter().any(|r| r.path == name)
    }

    fn read(&self, dir: &Path) -> Result<ForeignInstance> {
        let profiles_root = dir
            .parent()
            .ok_or_else(|| Error::ImportInstanceUnreadable {
                launcher: "modrinth_app".into(),
                details: "no parent profiles dir".into(),
            })?;
        let db = db_path_for_profiles_root(profiles_root).ok_or_else(|| {
            Error::ImportInstanceUnreadable {
                launcher: "modrinth_app".into(),
                details: "app.db not found".into(),
            }
        })?;
        let name = dir.file_name().and_then(|s| s.to_str()).unwrap_or("");
        read_rows(&db)
            .iter()
            .find(|r| r.path == name)
            .map(|r| build(profiles_root, r))
            .ok_or_else(|| Error::ImportInstanceUnreadable {
                launcher: "modrinth_app".into(),
                details: format!("no profile row for {name}"),
            })
    }

    fn expand_root(&self, root: &Path) -> Vec<ForeignInstance> {
        let Some(db) = db_path_for_profiles_root(root) else {
            return vec![];
        };
        read_rows(&db).iter().map(|r| build(root, r)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal Modrinth `app.db` with a single installed profile, plus
    /// the profile's content dir. Returns the temp dir (kept alive by caller)
    /// and the profiles root path.
    fn fake_modrinth() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let app = tmp.path().join("ModrinthApp");
        let profiles = app.join("profiles");
        let game = profiles.join("Fabric 1.21.1");
        std::fs::create_dir_all(game.join("mods")).unwrap();
        std::fs::write(game.join("mods/a.jar"), b"x").unwrap();

        let conn = rusqlite::Connection::open(app.join("app.db")).unwrap();
        conn.execute_batch(
            "CREATE TABLE profiles (
                path TEXT, install_stage TEXT, name TEXT, game_version TEXT,
                mod_loader TEXT, mod_loader_version TEXT, override_mc_memory_max INTEGER
             );
             INSERT INTO profiles VALUES
               ('Fabric 1.21.1','installed','Fabric 1.21.1','1.21.1','fabric','0.16.5',NULL),
               ('half','not_installed','half','1.20.1','forge',NULL,NULL);",
        )
        .unwrap();
        (tmp, profiles)
    }

    #[test]
    fn expand_root_reads_installed_profiles_only() {
        let (_tmp, profiles) = fake_modrinth();
        let found = ModrinthAppReader.expand_root(&profiles);
        assert_eq!(found.len(), 1, "only the installed profile");
        let fi = &found[0];
        assert_eq!(fi.source, ForeignLauncher::ModrinthApp);
        assert_eq!(fi.name, "Fabric 1.21.1");
        assert_eq!(fi.mc_version, "1.21.1");
        assert_eq!(fi.loader, LoaderKind::Fabric);
        assert_eq!(fi.loader_version.as_deref(), Some("0.16.5"));
        assert!(fi
            .content
            .iter()
            .any(|c| c.category == crate::instances::import::model::ContentCategory::Mods));
    }

    #[test]
    fn detect_and_read_a_single_profile_dir() {
        let (_tmp, profiles) = fake_modrinth();
        let dir = profiles.join("Fabric 1.21.1");
        assert!(ModrinthAppReader.detect(&dir));
        assert_eq!(ModrinthAppReader.read(&dir).unwrap().mc_version, "1.21.1");
    }

    #[test]
    fn vanilla_profile_has_no_loader_version() {
        // A vanilla profile with a residual mod_loader_version must report None.
        let tmp = tempfile::tempdir().unwrap();
        let app = tmp.path().join("ModrinthApp");
        let profiles = app.join("profiles");
        std::fs::create_dir_all(profiles.join("Vanilla 1.21/saves")).unwrap();
        std::fs::write(profiles.join("Vanilla 1.21/saves/w.dat"), b"x").unwrap();
        let conn = rusqlite::Connection::open(app.join("app.db")).unwrap();
        conn.execute_batch(
            "CREATE TABLE profiles (
                path TEXT, install_stage TEXT, name TEXT, game_version TEXT,
                mod_loader TEXT, mod_loader_version TEXT, override_mc_memory_max INTEGER
             );
             INSERT INTO profiles VALUES
               ('Vanilla 1.21','installed','Vanilla 1.21','1.21','vanilla','0.16.5',NULL);",
        )
        .unwrap();

        let found = ModrinthAppReader.expand_root(&profiles);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].loader, LoaderKind::Vanilla);
        assert_eq!(
            found[0].loader_version, None,
            "vanilla clears residual version"
        );
    }

    #[test]
    fn detect_false_without_db() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("profiles/Some");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!ModrinthAppReader.detect(&dir));
    }
}
