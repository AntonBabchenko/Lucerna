//! Filesystem helpers for the worlds module.
//!
//! `validate_segment` is the path-safety gate: every `world_folder_name`
//! and `backup_filename` accepted by a Tauri command flows through it
//! BEFORE any FS operation. Mirrors the project review's HIGH-3 stance
//! (mods::install::install_one validation).

use crate::error::Error;
use std::path::Path;

/// Reject any input that isn't a safe single path segment. Delegates the
/// checks to `crate::pathsafe`; maps the reason into `WorldPathInvalid`.
pub fn validate_segment(name: &str) -> Result<(), Error> {
    crate::pathsafe::validate_segment(name).map_err(|reason| Error::WorldPathInvalid {
        name: name.into(),
        reason: reason.into(),
    })
}

/// One recursive walk computing BOTH per-world totals `list_worlds` needs:
/// `(total file bytes, latest file mtime in ms since the UNIX epoch)`.
/// Replaces the `dir_size` + `dir_mtime_recursive` pair, which each walked
/// the same tree — two `read_dir` + `metadata` passes per world where one
/// suffices. Missing path → `(0, 0)` and symlinks are ignored — byte-for-byte
/// the callers' policy under the pair this replaces (the `exists()` probe is
/// kept deliberately: this is a merge, not a semantics change).
pub fn dir_size_and_mtime(path: &Path) -> Result<(u64, u64), Error> {
    if !path.exists() {
        return Ok((0, 0));
    }
    let mut total: u64 = 0;
    let mut latest_ms: u64 = 0;
    for entry in std::fs::read_dir(path).map_err(|e| Error::io(path.display().to_string(), e))? {
        let entry = entry.map_err(|e| Error::io(path.display().to_string(), e))?;
        let meta = entry
            .metadata()
            .map_err(|e| Error::io(entry.path().display().to_string(), e))?;
        if meta.is_file() {
            total = total.saturating_add(meta.len());
            if let Ok(ms) = meta
                .modified()
                .and_then(|t| {
                    t.duration_since(std::time::UNIX_EPOCH)
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
                })
                .map(|d| d.as_millis() as u64)
            {
                latest_ms = latest_ms.max(ms);
            }
        } else if meta.is_dir() {
            let (sub_size, sub_ms) = dir_size_and_mtime(&entry.path())?;
            total = total.saturating_add(sub_size);
            latest_ms = latest_ms.max(sub_ms);
        }
        // symlinks: ignored (saves don't have them; defensive)
    }
    Ok((total, latest_ms))
}

/// Cheap "last played" proxy for a world directory: the mtime of its
/// `level.dat` (Minecraft rewrites it on every save/exit), falling back to
/// the world directory's own mtime. At most two `stat`s — no recursive walk
/// (that's `dir_size_and_mtime`, reserved for the Worlds tab). Returns 0
/// when neither path can be stat'd. Milliseconds since the UNIX epoch.
pub fn world_recency_ms(world_dir: &Path) -> u64 {
    let level_dat = world_dir.join("level.dat");
    // Try level.dat first; on any failure (absent, or removed by a concurrent
    // save between two stats) fall back to the directory's own mtime. Avoids a
    // separate exists() probe and the to_path_buf allocation.
    std::fs::metadata(&level_dat)
        .or_else(|_| std::fs::metadata(world_dir))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_segment_accepts_normal_name() {
        assert!(validate_segment("My Survival World").is_ok());
        assert!(validate_segment("test-1.20.4").is_ok());
        assert!(validate_segment("мир42").is_ok());
    }

    #[test]
    fn validate_segment_rejects_empty() {
        assert!(matches!(
            validate_segment(""),
            Err(Error::WorldPathInvalid { .. })
        ));
    }

    #[test]
    fn validate_segment_rejects_slash() {
        assert!(matches!(
            validate_segment("foo/bar"),
            Err(Error::WorldPathInvalid { .. })
        ));
    }

    #[test]
    fn validate_segment_rejects_backslash() {
        assert!(matches!(
            validate_segment("foo\\bar"),
            Err(Error::WorldPathInvalid { .. })
        ));
    }

    #[test]
    fn validate_segment_rejects_dot_dot() {
        assert!(matches!(
            validate_segment(".."),
            Err(Error::WorldPathInvalid { .. })
        ));
        assert!(matches!(
            validate_segment("foo..bar"),
            Err(Error::WorldPathInvalid { .. })
        ));
        assert!(matches!(
            validate_segment("../escape"),
            Err(Error::WorldPathInvalid { .. })
        ));
    }

    #[test]
    fn validate_segment_rejects_drive_letter() {
        assert!(matches!(
            validate_segment("C:foo"),
            Err(Error::WorldPathInvalid { .. })
        ));
    }

    #[test]
    fn validate_segment_rejects_leading_dot() {
        assert!(matches!(
            validate_segment(".hidden"),
            Err(Error::WorldPathInvalid { .. })
        ));
    }

    #[test]
    fn validate_segment_rejects_overlong() {
        let too_long = "x".repeat(256);
        assert!(matches!(
            validate_segment(&too_long),
            Err(Error::WorldPathInvalid { .. })
        ));
    }

    #[test]
    fn validate_segment_rejects_reserved_windows_names_case_insensitive() {
        for name in &["CON", "con", "Aux", "nul", "COM1", "lpt9"] {
            assert!(
                matches!(validate_segment(name), Err(Error::WorldPathInvalid { .. })),
                "expected reject for {name}"
            );
        }
    }

    use std::fs;
    use std::time::Duration;
    use tempfile::tempdir;

    #[test]
    fn dir_size_and_mtime_matches_fixture_totals() {
        let td = tempdir().unwrap();
        fs::write(td.path().join("a.txt"), b"hello").unwrap(); // 5 bytes
        let sub = td.path().join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("b.bin"), b"\x00\x01\x02\x03").unwrap(); // 4 bytes

        // Sleep so the last file gets a strictly-later mtime (same 50ms the
        // neighbouring mtime tests rely on).
        std::thread::sleep(Duration::from_millis(50));
        let latest = sub.join("c.bin");
        fs::write(&latest, vec![0u8; 100]).unwrap(); // 100 bytes, newest
        let (size, mtime) = dir_size_and_mtime(td.path()).unwrap();
        assert_eq!(size, 5 + 4 + 100);
        let latest_mt = std::fs::metadata(&latest)
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        assert_eq!(mtime, latest_mt);
    }

    #[test]
    fn dir_size_and_mtime_empty_dir_is_zero_zero() {
        let td = tempdir().unwrap();
        assert_eq!(dir_size_and_mtime(td.path()).unwrap(), (0, 0));
    }

    #[test]
    fn dir_size_and_mtime_missing_path_is_zero_zero() {
        let td = tempdir().unwrap();
        // Caller policy carried over from the old pair: missing = (0, 0),
        // not an error (never-backed-up world / "never" mtime sentinel).
        assert_eq!(
            dir_size_and_mtime(&td.path().join("missing")).unwrap(),
            (0, 0)
        );
    }

    #[test]
    fn world_recency_prefers_level_dat_mtime() {
        let td = tempdir().unwrap();
        // An older file, then level.dat written later — recency must track level.dat.
        fs::write(td.path().join("region.bin"), b"x").unwrap();
        std::thread::sleep(Duration::from_millis(50));
        let level = td.path().join("level.dat");
        fs::write(&level, b"y").unwrap();
        let got = world_recency_ms(td.path());
        let level_mt = std::fs::metadata(&level)
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        assert_eq!(got, level_mt);
    }

    #[test]
    fn world_recency_falls_back_to_dir_mtime_without_level_dat() {
        let td = tempdir().unwrap();
        // No level.dat — must return the directory's own mtime, not just any
        // non-zero value.
        let dir_mt = std::fs::metadata(td.path())
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        assert_eq!(world_recency_ms(td.path()), dir_mt);
    }

    #[test]
    fn world_recency_zero_for_missing_path() {
        let td = tempdir().unwrap();
        assert_eq!(world_recency_ms(&td.path().join("nope")), 0);
    }
}
