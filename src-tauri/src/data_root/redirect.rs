use crate::error::{Error, Result};
use std::path::{Path, PathBuf};

/// The bootstrap file that names a custom data root. Lives at the OS-default
/// app-data dir, never under the resolved root.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct Redirect {
    pub path: PathBuf,
}

/// Where an unusable pointer is set aside before anything replaces or removes
/// it: it may be the only record of where the user's data lives.
pub const SET_ASIDE_FILE: &str = "data-location.corrupt.json";

/// What reading the pointer file told us. Four outcomes on purpose: "absent"
/// is the ONLY one that may mean "the user configured nothing". An unreadable
/// or corrupt pointer means a custom root may exist that we cannot name, and
/// every consumer has to decide what that means for it — there is no
/// `Option`-returning reader any more, so the decision cannot be skipped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PointerRead {
    /// `NotFound` — and only `NotFound`.
    Absent,
    Present(Redirect),
    /// Any other I/O error, rendered.
    Unreadable(String),
    /// The bytes were read, but they are not a usable pointer: not JSON, not
    /// this shape, or a `path` that is empty or not absolute.
    Corrupt,
}

/// Read the pointer file.
pub fn read_state(file: &Path) -> PointerRead {
    // Bytes, not a string: `read_to_string` reports non-UTF-8 garbage as an
    // I/O error, which would make a corrupt file look unreadable.
    let bytes = match std::fs::read(file) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return PointerRead::Absent,
        Err(e) => return PointerRead::Unreadable(e.to_string()),
    };
    match serde_json::from_slice::<Redirect>(&bytes) {
        // An empty or relative path parses, but is no pointer: it would be
        // probed relative to the working directory.
        Ok(redirect) if redirect.path.is_absolute() => PointerRead::Present(redirect),
        Ok(_) | Err(_) => PointerRead::Corrupt,
    }
}

/// Move an unusable pointer out of the way, keeping its content. Returns where
/// it went, or `None` when there was nothing to set aside (the pointer is
/// absent or perfectly readable). Every failure is an error: the caller is
/// about to destroy the original.
pub fn set_aside_unusable(file: &Path) -> Result<Option<PathBuf>> {
    match read_state(file) {
        PointerRead::Absent | PointerRead::Present(_) => return Ok(None),
        PointerRead::Unreadable(_) | PointerRead::Corrupt => {}
    }
    let aside = file.with_file_name(SET_ASIDE_FILE);
    // `rename` replaces an existing FILE on every platform, but not a
    // directory, and an older set-aside entry may be one (an unreadable
    // pointer can be a directory). Clear the slot first; a failure here stops
    // the caller before it destroys anything.
    match std::fs::symlink_metadata(&aside) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(Error::io(aside.display().to_string(), e)),
        Ok(_) => crate::data_root::recovery::remove_no_follow(&aside)
            .map_err(|e| Error::io(aside.display().to_string(), e))?,
    }
    std::fs::rename(file, &aside).map_err(|e| Error::io(file.display().to_string(), e))?;
    Ok(Some(aside))
}

/// Atomically write the redirect (tmp + rename), creating the parent if needed.
pub fn write(file: &Path, redirect: &Redirect) -> Result<()> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent.display().to_string(), e))?;
    }
    let tmp = file.with_extension("tmp");
    let json = serde_json::to_string_pretty(redirect)
        .map_err(|e| Error::io(file.display().to_string(), format!("serialize: {e}")))?;
    std::fs::write(&tmp, json).map_err(|e| Error::io(tmp.display().to_string(), e))?;
    std::fs::rename(&tmp, file).map_err(|e| Error::io(file.display().to_string(), e))
}

/// Remove the redirect (back to default). Missing file is not an error.
pub fn remove(file: &Path) -> Result<()> {
    match std::fs::remove_file(file) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::io(file.display().to_string(), e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// Absolute on BOTH platforms: `/x` is not absolute on Windows.
    fn abs(rel: &str) -> PathBuf {
        #[cfg(windows)]
        {
            PathBuf::from(format!("C:\\{rel}"))
        }
        #[cfg(not(windows))]
        {
            PathBuf::from(format!("/{rel}"))
        }
    }

    #[test]
    fn missing_reads_absent() {
        let d = tempdir().unwrap();
        assert_eq!(
            read_state(&d.path().join("data-location.json")),
            PointerRead::Absent
        );
    }

    #[test]
    fn write_then_read_roundtrips() {
        let d = tempdir().unwrap();
        let f = d.path().join("data-location.json");
        let r = Redirect {
            path: abs("LucernaData"),
        };
        write(&f, &r).unwrap();
        assert_eq!(read_state(&f), PointerRead::Present(r));
    }

    // Replaces `corrupt_reads_none_not_error`, which pinned the bug: a corrupt
    // pointer read as "no pointer", and startup then ran from the wrong root
    // with nothing gated.
    #[test]
    fn corrupt_is_corrupt_not_absent() {
        let d = tempdir().unwrap();
        let f = d.path().join("data-location.json");
        let cases: [&[u8]; 5] = [
            b"{ not json",
            b"\xff\xfe\x00garbage that is not even UTF-8",
            b"{\"path\":\"\"}",
            b"{\"path\":\"relative/LucernaData\"}",
            b"[]",
        ];
        for bytes in cases {
            std::fs::write(&f, bytes).unwrap();
            assert_eq!(
                read_state(&f),
                PointerRead::Corrupt,
                "{:?}",
                String::from_utf8_lossy(bytes)
            );
        }
    }

    #[test]
    fn a_directory_in_place_of_the_pointer_is_unreadable() {
        // Portable stand-in for "the read failed with something other than
        // NotFound" (permissions, a dying disk): reading a directory fails on
        // all three OSes, and never with NotFound.
        let d = tempdir().unwrap();
        let f = d.path().join("data-location.json");
        std::fs::create_dir(&f).unwrap();
        assert!(
            matches!(read_state(&f), PointerRead::Unreadable(_)),
            "{:?}",
            read_state(&f)
        );
    }

    #[test]
    fn set_aside_renames_an_unusable_pointer_and_reports_where() {
        let d = tempdir().unwrap();
        let f = d.path().join("data-location.json");
        std::fs::write(&f, b"{ not json").unwrap();
        let aside = set_aside_unusable(&f).unwrap();
        assert_eq!(aside, Some(d.path().join(SET_ASIDE_FILE)));
        assert!(!f.exists(), "the unusable pointer is out of the way");
        assert_eq!(
            std::fs::read(d.path().join(SET_ASIDE_FILE)).unwrap(),
            b"{ not json",
            "its content is kept"
        );
    }

    #[test]
    fn set_aside_replaces_an_older_set_aside_file() {
        let d = tempdir().unwrap();
        let f = d.path().join("data-location.json");
        std::fs::write(d.path().join(SET_ASIDE_FILE), b"older").unwrap();
        std::fs::write(&f, b"newer garbage").unwrap();
        set_aside_unusable(&f).unwrap();
        assert_eq!(
            std::fs::read(d.path().join(SET_ASIDE_FILE)).unwrap(),
            b"newer garbage"
        );
    }

    #[test]
    fn set_aside_leaves_a_missing_or_readable_pointer_alone() {
        let d = tempdir().unwrap();
        let f = d.path().join("data-location.json");
        assert_eq!(set_aside_unusable(&f).unwrap(), None);
        let r = Redirect {
            path: abs("LucernaData"),
        };
        write(&f, &r).unwrap();
        assert_eq!(set_aside_unusable(&f).unwrap(), None);
        assert_eq!(read_state(&f), PointerRead::Present(r));
    }

    #[test]
    fn remove_is_idempotent() {
        let d = tempdir().unwrap();
        let f = d.path().join("data-location.json");
        remove(&f).unwrap(); // missing → ok
        write(
            &f,
            &Redirect {
                path: PathBuf::from("X:/x"),
            },
        )
        .unwrap();
        remove(&f).unwrap();
        assert!(!f.exists());
    }
}
