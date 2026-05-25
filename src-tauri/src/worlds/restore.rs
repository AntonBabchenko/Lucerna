//! Restore-side operations.

use crate::error::{Error, Result};
use crate::worlds::{backups_root, fs as wfs, saves_dir, zip as wzip, RestoreMode, RestoredWorld};
use chrono::Utc;
use std::path::PathBuf;

/// Public entrypoint. Resolves paths from the AppHandle then
/// delegates to `restore_backup_at_saves`.
pub async fn restore_backup(
    app: &tauri::AppHandle,
    instance_id: &str,
    world_folder_name: &str,
    backup_filename: &str,
    mode: RestoreMode,
) -> Result<RestoredWorld> {
    wfs::validate_segment(world_folder_name)?;
    wfs::validate_segment(backup_filename)?;
    let saves = saves_dir(app, instance_id)?;
    let backups_dir = backups_root(app, instance_id)?.join(world_folder_name);
    let backup_path = backups_dir.join(backup_filename);
    restore_backup_at_saves(&saves, &backups_dir, &backup_path, world_folder_name, mode).await
}

/// Test-friendly variant that takes the saves dir + backups dir
/// directly (skips the AppHandle path-resolution). Public API uses
/// this internally; integration tests use it too.
pub async fn restore_backup_at_saves(
    saves: &std::path::Path,
    backups_dir: &std::path::Path,
    backup_path: &std::path::Path,
    world_folder_name: &str,
    mode: RestoreMode,
) -> Result<RestoredWorld> {
    wfs::validate_segment(world_folder_name)?;
    if !backup_path.is_file() {
        return Err(Error::BackupNotFound {
            instance_id: "<test>".into(),
            world_folder: world_folder_name.into(),
            filename: backup_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("?")
                .into(),
        });
    }
    match mode {
        RestoreMode::Replace => {
            restore_replace(saves, backups_dir, backup_path, world_folder_name).await
        }
        RestoreMode::AsCopy => restore_as_copy(saves, backup_path, world_folder_name).await,
    }
}

async fn restore_replace(
    saves: &std::path::Path,
    backups_dir: &std::path::Path,
    backup_path: &std::path::Path,
    world_folder: &str,
) -> Result<RestoredWorld> {
    let world_path = saves.join(world_folder);
    if !world_path.is_dir() {
        return Err(Error::WorldNotFound {
            instance_id: "<unknown>".into(),
            folder_name: world_folder.into(),
        });
    }

    // 1. Auto-pre-restore zip BEFORE any destructive step. Failure
    //    here aborts cleanly — original world untouched.
    let pre_restore_name = format!("pre-restore-{}.zip", Utc::now().format("%Y-%m-%dT%H-%M-%S"));
    let pre_restore_path = backups_dir.join(&pre_restore_name);
    let world_clone = world_path.clone();
    let pre_clone = pre_restore_path.clone();
    let world_folder_owned = world_folder.to_string();
    tokio::task::spawn_blocking(move || {
        wzip::zip_dir(&world_clone, &pre_clone, &world_folder_owned)
    })
    .await
    .map_err(|e| Error::io(pre_restore_path.display().to_string(), format!("join: {e}")))??;

    // 2. Rename world to .tmp-restoring-<random>. Atomic on same vol.
    let tmp_suffix: String = (0..8)
        .map(|_| {
            let n = (Utc::now().timestamp_nanos_opt().unwrap_or(0) as u32) % 16;
            std::char::from_digit(n, 16).unwrap()
        })
        .collect();
    let tmp_path = saves.join(format!(".tmp-restoring-{tmp_suffix}"));
    std::fs::rename(&world_path, &tmp_path)
        .map_err(|e| Error::io(world_path.display().to_string(), e))?;

    // 3. Extract the chosen backup into the (now-empty) world path.
    //    If extract fails, rename tmp back to undo step 2 and bubble.
    let result = (|| -> Result<()> {
        std::fs::create_dir_all(&world_path)
            .map_err(|e| Error::io(world_path.display().to_string(), e))?;
        // The zip's root is named after the world (we put it there in
        // backup_world / zip_dir). So the zip content goes to a temp
        // staging dir, then we move the inner root into world_path to
        // unwrap one level. Simpler alternative: just extract to
        // saves/, since the zip's root == world_folder name, the
        // extract recreates saves/<world_folder>/. Do that.
        wzip::extract_zip(backup_path, saves)?;
        if !world_path.is_dir() {
            return Err(Error::BackupCorrupt {
                filename: backup_path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("?")
                    .into(),
                details: format!("extract did not produce expected folder '{world_folder}/'"),
            });
        }
        Ok(())
    })();

    match result {
        Ok(()) => {
            // 4. Drop the tmp. Best-effort: if remove_dir_all errors,
            //    log but don't fail the restore — the live world is
            //    healthy; the tmp dir is cosmetic clutter.
            let _ = std::fs::remove_dir_all(&tmp_path);
            Ok(RestoredWorld {
                final_folder_name: world_folder.into(),
            })
        }
        Err(e) => {
            // Roll back: nuke whatever extract left half-written, put
            // the original back, bubble the original error.
            let _ = std::fs::remove_dir_all(&world_path);
            let _ = std::fs::rename(&tmp_path, &world_path);
            Err(e)
        }
    }
}

async fn restore_as_copy(
    saves: &std::path::Path,
    backup_path: &std::path::Path,
    world_folder: &str,
) -> Result<RestoredWorld> {
    // Pick a free `<world> (restored)` name (suffix on collision).
    let mut chosen = format!("{world_folder} (restored)");
    if saves.join(&chosen).exists() {
        let mut ok = false;
        for i in 2..=999 {
            chosen = format!("{world_folder} (restored {i})");
            if !saves.join(&chosen).exists() {
                ok = true;
                break;
            }
        }
        if !ok {
            return Err(Error::WorldNameUnresolvable {
                folder_name: world_folder.into(),
            });
        }
    }

    // Extract zip into a temp staging dir, then rename the inner
    // folder to the chosen name. The zip's root is `<world>/` so the
    // staging dir contains `staging/<world>/...` — we rename that to
    // `saves/<chosen>/`.
    let tmp_extract = saves.join(format!(".tmp-as-copy-{}", &chosen.replace(' ', "_")));
    let _ = std::fs::remove_dir_all(&tmp_extract); // stale from earlier failure
    std::fs::create_dir_all(&tmp_extract)
        .map_err(|e| Error::io(tmp_extract.display().to_string(), e))?;
    let backup_clone = backup_path.to_path_buf();
    let tmp_clone = tmp_extract.clone();
    let result = tokio::task::spawn_blocking(move || wzip::extract_zip(&backup_clone, &tmp_clone))
        .await
        .map_err(|e| Error::io(tmp_extract.display().to_string(), format!("join: {e}")))?;
    if let Err(e) = result {
        let _ = std::fs::remove_dir_all(&tmp_extract);
        return Err(e);
    }
    let inner = tmp_extract.join(world_folder);
    if !inner.is_dir() {
        let _ = std::fs::remove_dir_all(&tmp_extract);
        return Err(Error::BackupCorrupt {
            filename: backup_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("?")
                .into(),
            details: format!("zip root was not '{world_folder}/'"),
        });
    }
    let final_path = saves.join(&chosen);
    std::fs::rename(&inner, &final_path)
        .map_err(|e| Error::io(final_path.display().to_string(), e))?;
    let _ = std::fs::remove_dir_all(&tmp_extract);
    Ok(RestoredWorld {
        final_folder_name: chosen,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worlds::zip as wzip;
    use std::fs;
    use tempfile::tempdir;

    fn make_world_with_files(
        world_name: &str,
        files: &[(&str, &[u8])],
    ) -> (tempfile::TempDir, PathBuf, PathBuf) {
        let td = tempdir().unwrap();
        let saves = td.path().join("saves");
        let backups = td.path().join("backups").join(world_name);
        fs::create_dir_all(saves.join(world_name)).unwrap();
        fs::create_dir_all(&backups).unwrap();
        for (rel, bytes) in files {
            let p = saves.join(world_name).join(rel);
            if let Some(parent) = p.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&p, *bytes).unwrap();
        }
        (td, saves, backups)
    }

    #[tokio::test]
    async fn restore_replace_rolls_back_on_extract_failure() {
        // World "W" with marker file inside.
        let (_td, saves, backups_dir) = make_world_with_files("W", &[("marker.txt", b"original")]);

        // Place a CORRUPT backup zip in backups_dir — extract will fail.
        let bad_backup = backups_dir.join("2026-05-24T10-00-00.zip");
        fs::write(&bad_backup, b"NOT A ZIP").unwrap();

        let r = restore_replace(&saves, &backups_dir, &bad_backup, "W").await;
        assert!(
            matches!(r, Err(Error::BackupCorrupt { .. })),
            "expected BackupCorrupt, got: {r:?}"
        );

        // Critical assertion: the ORIGINAL world is still intact.
        let marker = saves.join("W").join("marker.txt");
        assert!(marker.is_file(), "world rollback failed");
        assert_eq!(fs::read(&marker).unwrap(), b"original");

        // And the auto-pre-restore zip was created (safety net visible).
        let pre_restore: Vec<_> = fs::read_dir(&backups_dir)
            .unwrap()
            .filter_map(|e| {
                let n = e.ok()?.file_name().into_string().ok()?;
                if n.starts_with("pre-restore-") {
                    Some(n)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(
            pre_restore.len(),
            1,
            "expected exactly one pre-restore zip, found {pre_restore:?}"
        );
    }

    #[tokio::test]
    async fn restore_replace_happy_path_swaps_contents() {
        // World "W" with v1 content.
        let (_td, saves, backups_dir) = make_world_with_files("W", &[("file.txt", b"v1")]);
        // Make a real backup of v1.
        let backup_path = backups_dir.join("2026-05-24T10-00-00.zip");
        wzip::zip_dir(&saves.join("W"), &backup_path, "W").unwrap();
        // Now mutate the world to v2.
        fs::write(saves.join("W").join("file.txt"), b"v2").unwrap();

        let r = restore_replace(&saves, &backups_dir, &backup_path, "W")
            .await
            .unwrap();
        assert_eq!(r.final_folder_name, "W");
        // World now contains v1 again.
        assert_eq!(fs::read(saves.join("W").join("file.txt")).unwrap(), b"v1");
        // Pre-restore captured v2 (so user can roll back the rollback).
        let pre_restore: Vec<_> = fs::read_dir(&backups_dir)
            .unwrap()
            .filter_map(|e| {
                let n = e.ok()?.file_name().into_string().ok()?;
                if n.starts_with("pre-restore-") {
                    Some(n)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(pre_restore.len(), 1);
    }

    #[tokio::test]
    async fn restore_as_copy_suffixes_on_conflict() {
        let (_td, saves, backups_dir) = make_world_with_files("W", &[("file.txt", b"v1")]);
        let backup_path = backups_dir.join("2026-05-24T10-00-00.zip");
        wzip::zip_dir(&saves.join("W"), &backup_path, "W").unwrap();

        // Pre-create "W (restored)" so the first attempt collides.
        fs::create_dir_all(saves.join("W (restored)")).unwrap();
        fs::write(saves.join("W (restored)").join("placeholder"), b"x").unwrap();

        let r = restore_as_copy(&saves, &backup_path, "W").await.unwrap();
        assert_eq!(r.final_folder_name, "W (restored 2)");
        // Original and the pre-existing collider untouched.
        assert!(saves.join("W").is_dir());
        assert_eq!(
            fs::read(saves.join("W (restored)").join("placeholder")).unwrap(),
            b"x"
        );
        // New copy contains v1.
        assert_eq!(
            fs::read(saves.join("W (restored 2)").join("file.txt")).unwrap(),
            b"v1"
        );
    }

    #[tokio::test]
    async fn restore_as_copy_basic_no_conflict() {
        let (_td, saves, backups_dir) = make_world_with_files("W", &[("file.txt", b"v1")]);
        let backup_path = backups_dir.join("2026-05-24T10-00-00.zip");
        wzip::zip_dir(&saves.join("W"), &backup_path, "W").unwrap();

        let r = restore_as_copy(&saves, &backup_path, "W").await.unwrap();
        assert_eq!(r.final_folder_name, "W (restored)");
        assert_eq!(
            fs::read(saves.join("W (restored)").join("file.txt")).unwrap(),
            b"v1"
        );
    }
}
