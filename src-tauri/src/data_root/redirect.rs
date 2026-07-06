use crate::error::{Error, Result};
use std::path::{Path, PathBuf};

/// The bootstrap file that names a custom data root. `None` value means "use
/// the default location". Lives at the OS-default app-data dir, never under the
/// resolved root.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct Redirect {
    pub path: PathBuf,
}

/// Read the redirect file. Missing file → Ok(None). Unparseable → Ok(None)
/// (treat a corrupt redirect as "use default" rather than bricking startup).
pub fn read(file: &Path) -> Result<Option<Redirect>> {
    match std::fs::read_to_string(file) {
        Ok(raw) => Ok(serde_json::from_str(&raw).ok()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::io(file.display().to_string(), e)),
    }
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

    #[test]
    fn missing_reads_none() {
        let d = tempdir().unwrap();
        assert_eq!(read(&d.path().join("data-location.json")).unwrap(), None);
    }

    #[test]
    fn write_then_read_roundtrips() {
        let d = tempdir().unwrap();
        let f = d.path().join("data-location.json");
        let r = Redirect {
            path: PathBuf::from("D:/LucernaData"),
        };
        write(&f, &r).unwrap();
        assert_eq!(read(&f).unwrap(), Some(r));
    }

    #[test]
    fn corrupt_reads_none_not_error() {
        let d = tempdir().unwrap();
        let f = d.path().join("data-location.json");
        std::fs::write(&f, "{ not json").unwrap();
        assert_eq!(read(&f).unwrap(), None);
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
