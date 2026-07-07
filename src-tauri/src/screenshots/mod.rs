//! Per-instance Minecraft screenshot viewer backend.
//!
//! Screenshots live at `<instance>/.minecraft/screenshots/*.png`. All FS
//! access stays here (mirrors `worlds`); the frontend only ever sees typed
//! `Screenshot` rows and `data:` URLs produced by `thumb`.

pub mod fs;
pub mod thumb;

use crate::error::{Error, Result};
use serde::Serialize;
use specta::Type;
use std::path::{Path, PathBuf};

/// One screenshot file, surfaced to the UI. `instance_name` is filled for both
/// the per-instance tab and the global gallery so the gallery can group/filter.
#[derive(Debug, Clone, Serialize, Type)]
pub struct Screenshot {
    pub instance_id: String,
    pub instance_name: String,
    pub file_name: String,
    pub size_bytes: f64,
    pub modified_unix_ms: f64,
}

/// Pure core: enumerate image files directly under a concrete `screenshots/`
/// dir, newest-first. Testable without a Tauri `AppHandle`. Missing dir →
/// empty Vec (not an error). Non-image / non-UTF-8 / unsafe entries skipped.
pub fn list_in_dir(dir: &Path, instance_id: &str, instance_name: &str) -> Result<Vec<Screenshot>> {
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|e| Error::io(dir.display().to_string(), e))? {
        let entry = entry.map_err(|e| Error::io(dir.display().to_string(), e))?;
        // `file_type()` does NOT follow symlinks (unlike `metadata()`): skip
        // symlinks / reparse points so a crafted link inside the screenshots
        // dir can't redirect listing or decoding at an arbitrary file. Mirrors
        // the worlds module (worlds/import.rs).
        let ft = entry
            .file_type()
            .map_err(|e| Error::io(entry.path().display().to_string(), e))?;
        if ft.is_symlink() || !ft.is_file() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(String::from) else {
            continue;
        };
        if !fs::is_image_file(&name) || fs::validate_segment(&name).is_err() {
            continue;
        }
        let size_bytes = entry
            .metadata()
            .map(|m| m.len())
            .map_err(|e| Error::io(entry.path().display().to_string(), e))? as f64;
        let modified_unix_ms = fs::file_mtime_ms(&entry.path()) as f64;
        out.push(Screenshot {
            instance_id: instance_id.to_string(),
            instance_name: instance_name.to_string(),
            file_name: name,
            size_bytes,
            modified_unix_ms,
        });
    }
    // Newest first.
    out.sort_by(|a, b| {
        b.modified_unix_ms
            .partial_cmp(&a.modified_unix_ms)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(out)
}

/// `<instance>/.minecraft/screenshots/`. Created by Minecraft on first F2.
pub fn screenshots_dir(app: &tauri::AppHandle, instance_id: &str) -> Result<PathBuf> {
    crate::paths::minecraft_dir(app, instance_id)
        .map(|p| p.join("screenshots"))
        .map_err(|e| Error::io("<screenshots_dir>", e))
}

/// Look up an instance's display name by id. Empty string if not found.
///
/// Accepted trade-off: this rescans all instances on each per-instance tab
/// reload. `list_all_screenshots` avoids the rescan by reusing names from its
/// single enumeration, but `list_screenshots` only receives an id. At launcher
/// scale (tens of local instances) the extra scan is negligible.
fn instance_name(app: &tauri::AppHandle, instance_id: &str) -> String {
    crate::instances::list_instances_with_status(app)
        .ok()
        .and_then(|list| list.into_iter().find(|i| i.id == instance_id))
        .map(|i| i.name)
        .unwrap_or_default()
}

/// Screenshots of a single instance, newest-first.
pub fn list_screenshots(app: &tauri::AppHandle, instance_id: &str) -> Result<Vec<Screenshot>> {
    let dir = screenshots_dir(app, instance_id)?;
    let name = instance_name(app, instance_id);
    list_in_dir(&dir, instance_id, &name)
}

/// Screenshots across every instance, flattened, newest-first.
pub fn list_all_screenshots(app: &tauri::AppHandle) -> Result<Vec<Screenshot>> {
    let instances = crate::instances::list_instances_with_status(app)?;
    let mut out = Vec::new();
    for inst in instances {
        let dir = screenshots_dir(app, &inst.id)?;
        out.extend(list_in_dir(&dir, &inst.id, &inst.name)?);
    }
    out.sort_by(|a, b| {
        b.modified_unix_ms
            .partial_cmp(&a.modified_unix_ms)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(out)
}

/// Absolute path to a single screenshot, with path-safety enforced. Errors
/// `ScreenshotNotFound` if the file is absent (e.g. deleted concurrently).
pub fn screenshot_path(
    app: &tauri::AppHandle,
    instance_id: &str,
    file_name: &str,
) -> Result<PathBuf> {
    fs::validate_segment(file_name)?;
    let path = screenshots_dir(app, instance_id)?.join(file_name);
    // `symlink_metadata` (lstat) does NOT follow links: reject a symlink /
    // reparse point masquerading as a screenshot so thumbnail / preview / copy /
    // delete / reveal can't be redirected at an arbitrary file. A genuinely
    // missing file (concurrent delete) falls through to the same not-found.
    let is_regular_file = std::fs::symlink_metadata(&path)
        .map(|m| m.is_file())
        .unwrap_or(false);
    if !is_regular_file {
        return Err(Error::ScreenshotNotFound {
            instance_id: instance_id.into(),
            filename: file_name.into(),
        });
    }
    Ok(path)
}

/// Move a screenshot to the OS recycle bin (reversible).
pub fn delete_screenshot(app: &tauri::AppHandle, instance_id: &str, file_name: &str) -> Result<()> {
    let path = screenshot_path(app, instance_id, file_name)?;
    // Capture mtime before deletion so the matching cache entries can be evicted.
    let mtime = fs::file_mtime_ms(&path);
    trash::delete(&path).map_err(|e| Error::io(path.display().to_string(), e))?;
    // Best-effort: drop this file's cached thumbnail/preview so the disposable
    // cache doesn't retain entries for files the user has removed.
    thumb::evict(app, &path, mtime);
    Ok(())
}

/// Copy the ORIGINAL screenshot to a user-chosen destination path.
pub fn save_screenshot_copy(
    app: &tauri::AppHandle,
    instance_id: &str,
    file_name: &str,
    dest: &str,
) -> Result<()> {
    let src = screenshot_path(app, instance_id, file_name)?;
    std::fs::copy(&src, dest).map_err(|e| Error::io(dest.to_string(), e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs as stdfs;
    use std::time::Duration;
    use tempfile::tempdir;

    fn write_png(dir: &Path, name: &str) {
        stdfs::write(dir.join(name), b"\x89PNG\r\n\x1a\n").unwrap();
    }

    #[test]
    fn missing_dir_is_empty() {
        let td = tempdir().unwrap();
        let dir = td.path().join("screenshots");
        assert!(list_in_dir(&dir, "id", "Name").unwrap().is_empty());
    }

    #[test]
    fn lists_images_newest_first_and_skips_non_images() {
        let td = tempdir().unwrap();
        let dir = td.path();
        write_png(dir, "older.png");
        std::thread::sleep(Duration::from_millis(50));
        write_png(dir, "newer.png");
        stdfs::write(dir.join("notes.txt"), b"x").unwrap();
        stdfs::create_dir_all(dir.join("subdir")).unwrap();
        let got = list_in_dir(dir, "id", "Name").unwrap();
        let names: Vec<&str> = got.iter().map(|s| s.file_name.as_str()).collect();
        assert_eq!(names, vec!["newer.png", "older.png"]);
        assert_eq!(got[0].instance_id, "id");
        assert_eq!(got[0].instance_name, "Name");
    }

    // A symlink/reparse point inside the screenshots dir must never be surfaced
    // (it could point at an arbitrary file). Unix-only: creating a symlink on
    // Windows needs a privilege CI runners lack; the guard itself is portable.
    #[cfg(unix)]
    #[test]
    fn skips_symlinks() {
        use std::os::unix::fs::symlink;
        let td = tempdir().unwrap();
        let dir = td.path();
        write_png(dir, "real.png");
        // A .png-named symlink pointing at a file OUTSIDE the screenshots dir.
        let outside = tempdir().unwrap();
        let target = outside.path().join("secret.png");
        stdfs::write(&target, b"\x89PNG\r\n\x1a\n").unwrap();
        symlink(&target, dir.join("link.png")).unwrap();
        let got = list_in_dir(dir, "id", "Name").unwrap();
        let names: Vec<&str> = got.iter().map(|s| s.file_name.as_str()).collect();
        assert_eq!(names, vec!["real.png"]);
    }
}
