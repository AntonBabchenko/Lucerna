//! Filesystem helpers for the worlds module.
//!
//! `validate_segment` is the path-safety gate: every `world_folder_name`
//! and `backup_filename` accepted by a Tauri command flows through it
//! BEFORE any FS operation. Mirrors the project review's HIGH-3 stance
//! (mods::install::install_one validation).

use crate::error::Error;
use std::path::Path;

const RESERVED_WIN: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Reject any input that isn't a safe single path segment.
///
/// Rejections:
/// - empty
/// - contains `/`, `\`, or `:`
/// - exactly `..` or contains `..`
/// - starts with `.` (hidden / current-dir)
/// - length > 255 chars (filesystem cap)
/// - case-insensitive match against Windows reserved names
pub fn validate_segment(name: &str) -> Result<(), Error> {
    if name.is_empty() {
        return Err(Error::WorldPathInvalid {
            name: name.into(),
            reason: "empty name".into(),
        });
    }
    if name.contains('/') || name.contains('\\') || name.contains(':') {
        return Err(Error::WorldPathInvalid {
            name: name.into(),
            reason: "contains path separator or colon".into(),
        });
    }
    if name.contains("..") {
        return Err(Error::WorldPathInvalid {
            name: name.into(),
            reason: "contains '..'".into(),
        });
    }
    if name.starts_with('.') {
        return Err(Error::WorldPathInvalid {
            name: name.into(),
            reason: "starts with '.'".into(),
        });
    }
    if name.len() > 255 {
        return Err(Error::WorldPathInvalid {
            name: name.into(),
            reason: "longer than 255 bytes".into(),
        });
    }
    let upper = name.to_ascii_uppercase();
    if RESERVED_WIN.contains(&upper.as_str()) {
        return Err(Error::WorldPathInvalid {
            name: name.into(),
            reason: "Windows reserved name".into(),
        });
    }
    Ok(())
}

/// Recursively sum file sizes under `path`. Missing path is treated
/// as size 0 (caller policy — used by `list_worlds` for the optional
/// `<instance>/backups/<world>/` dir on never-backed-up worlds).
pub fn dir_size(path: &Path) -> Result<u64, Error> {
    if !path.exists() {
        return Ok(0);
    }
    let mut total: u64 = 0;
    for entry in std::fs::read_dir(path).map_err(|e| Error::io(path.display().to_string(), e))? {
        let entry = entry.map_err(|e| Error::io(path.display().to_string(), e))?;
        let meta = entry
            .metadata()
            .map_err(|e| Error::io(entry.path().display().to_string(), e))?;
        if meta.is_file() {
            total = total.saturating_add(meta.len());
        } else if meta.is_dir() {
            total = total.saturating_add(dir_size(&entry.path())?);
        }
        // symlinks: ignored (saves don't have them; defensive)
    }
    Ok(total)
}

/// Latest mtime among all files under `path`. Returns 0 for missing
/// or empty dirs (sentinel — caller renders as "never" or omits).
/// Milliseconds since UNIX epoch.
pub fn dir_mtime_recursive(path: &Path) -> Result<u64, Error> {
    if !path.exists() {
        return Ok(0);
    }
    let mut latest_ms: u64 = 0;
    for entry in std::fs::read_dir(path).map_err(|e| Error::io(path.display().to_string(), e))? {
        let entry = entry.map_err(|e| Error::io(path.display().to_string(), e))?;
        let meta = entry
            .metadata()
            .map_err(|e| Error::io(entry.path().display().to_string(), e))?;
        if meta.is_file() {
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
            let sub = dir_mtime_recursive(&entry.path())?;
            latest_ms = latest_ms.max(sub);
        }
    }
    Ok(latest_ms)
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
    use tempfile::tempdir;

    #[test]
    fn dir_size_empty_dir_is_zero() {
        let td = tempdir().unwrap();
        assert_eq!(dir_size(td.path()).unwrap(), 0);
    }

    #[test]
    fn dir_size_sums_files_and_recurses() {
        let td = tempdir().unwrap();
        fs::write(td.path().join("a.txt"), b"hello").unwrap(); // 5
        let sub = td.path().join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("b.bin"), b"\x00\x01\x02\x03").unwrap(); // 4
        fs::write(sub.join("c.bin"), vec![0u8; 100]).unwrap(); // 100
        assert_eq!(dir_size(td.path()).unwrap(), 5 + 4 + 100);
    }

    #[test]
    fn dir_size_handles_zero_byte_files() {
        let td = tempdir().unwrap();
        fs::write(td.path().join("empty.dat"), b"").unwrap();
        assert_eq!(dir_size(td.path()).unwrap(), 0);
    }

    #[test]
    fn dir_size_returns_zero_for_nonexistent() {
        let td = tempdir().unwrap();
        let missing = td.path().join("does-not-exist");
        // Caller policy: missing = 0, not an error. The list_worlds
        // path uses this so a freshly created instance with no
        // backups/<world>/ dir reports backup_count=0 cleanly.
        assert_eq!(dir_size(&missing).unwrap(), 0);
    }

    use std::time::Duration;

    #[test]
    fn dir_mtime_recursive_returns_max_among_files() {
        let td = tempdir().unwrap();
        fs::write(td.path().join("a.txt"), b"a").unwrap();
        // Sleep so the second file gets a strictly-later mtime.
        // 50ms is enough on every filesystem we care about.
        std::thread::sleep(Duration::from_millis(50));
        let later = td.path().join("b.txt");
        fs::write(&later, b"b").unwrap();
        let mt = dir_mtime_recursive(td.path()).unwrap();
        let later_mt = std::fs::metadata(&later)
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        assert_eq!(mt, later_mt);
    }

    #[test]
    fn dir_mtime_recursive_returns_zero_for_empty_dir() {
        let td = tempdir().unwrap();
        // Empty directory: no files to consider, so 0 (sentinel).
        assert_eq!(dir_mtime_recursive(td.path()).unwrap(), 0);
    }

    #[test]
    fn dir_mtime_recursive_returns_zero_for_missing_dir() {
        let td = tempdir().unwrap();
        assert_eq!(dir_mtime_recursive(&td.path().join("missing")).unwrap(), 0);
    }
}
