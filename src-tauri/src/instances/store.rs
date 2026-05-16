//! Atomic read/write for `instance.json` and `app.json`.
//!
//! Writes go to `<target>.tmp` first, then `rename()` atomically replaces
//! the original. Reads return `Err(Error::Io)` with the path on failure,
//! same shape as the rest of the codebase.

use crate::error::{Error, Result};
use crate::instances::schema::{AppFile, InstanceFile};
use std::path::Path;

/// Read `instance.json` from disk. Errors with `Io` on missing or
/// malformed file (the caller — `scan::list_all` — converts this to
/// "skip with warning"; direct callers like `get_instance` propagate).
pub fn read_instance_json(path: &Path) -> Result<InstanceFile> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| Error::io(path.display().to_string(), e))?;
    serde_json::from_str(&raw)
        .map_err(|e| Error::io(path.display().to_string(), format!("parse: {e}")))
}

pub fn write_instance_json(path: &Path, value: &InstanceFile) -> Result<()> {
    write_atomic(path, value)
}

/// Read `app.json`. Missing file → return `AppFile::default()` (callers
/// treat it as "no migration yet"). Malformed → `Err`.
pub fn read_app_json(path: &Path) -> Result<AppFile> {
    match std::fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str(&raw)
            .map_err(|e| Error::io(path.display().to_string(), format!("parse: {e}"))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(AppFile::default()),
        Err(e) => Err(Error::io(path.display().to_string(), e)),
    }
}

pub fn write_app_json(path: &Path, value: &AppFile) -> Result<()> {
    write_atomic(path, value)
}

fn write_atomic<T: serde::Serialize>(target: &Path, value: &T) -> Result<()> {
    let parent = target
        .parent()
        .ok_or_else(|| Error::io(target.display().to_string(), "no parent dir"))?;
    std::fs::create_dir_all(parent)
        .map_err(|e| Error::io(parent.display().to_string(), e))?;
    let tmp = target.with_extension("tmp");
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| Error::io(target.display().to_string(), format!("serialize: {e}")))?;
    std::fs::write(&tmp, json).map_err(|e| Error::io(tmp.display().to_string(), e))?;
    std::fs::rename(&tmp, target)
        .map_err(|e| Error::io(target.display().to_string(), e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::schema::LoaderKind;
    use tempfile::tempdir;

    fn sample_instance() -> InstanceFile {
        InstanceFile {
            version: 1,
            id: "3f4a-bbbb".into(),
            name: "Default".into(),
            mc_version: "1.20.4".into(),
            loader: LoaderKind::Vanilla,
            loader_version: None,
            max_heap_mb: 2048,
            extra_jvm_args: String::new(),
            created_unix_ms: 1_700_000_000_000.0,
        }
    }

    #[test]
    fn write_then_read_instance_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("3f4a-bbbb/instance.json");
        let value = sample_instance();
        write_instance_json(&path, &value).unwrap();
        let back = read_instance_json(&path).unwrap();
        assert_eq!(value, back);
    }

    #[test]
    fn write_creates_parent_dirs() {
        let dir = tempdir().unwrap();
        // Two levels of missing parent.
        let path = dir.path().join("nested/x/y/instance.json");
        write_instance_json(&path, &sample_instance()).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn write_is_atomic_no_tmp_leak_on_success() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("inst/instance.json");
        write_instance_json(&path, &sample_instance()).unwrap();
        let tmp = path.with_extension("tmp");
        assert!(!tmp.exists(), "temp file should be renamed away");
    }

    #[test]
    fn read_missing_instance_json_errors() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nope/instance.json");
        let err = read_instance_json(&path).unwrap_err();
        match err {
            Error::Io { .. } => {}
            other => panic!("expected Io, got {other:?}"),
        }
    }

    #[test]
    fn read_malformed_instance_json_errors() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("inst/instance.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "not json").unwrap();
        let err = read_instance_json(&path).unwrap_err();
        match err {
            Error::Io { details, .. } => assert!(details.contains("parse")),
            other => panic!("expected Io, got {other:?}"),
        }
    }

    #[test]
    fn read_missing_app_json_returns_default() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("app.json");
        let value = read_app_json(&path).unwrap();
        assert_eq!(value, AppFile::default());
    }

    #[test]
    fn write_then_read_app_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("app.json");
        let value = AppFile {
            version: 1,
            active_instance: Some("3f4a-bbbb".into()),
        };
        write_app_json(&path, &value).unwrap();
        let back = read_app_json(&path).unwrap();
        assert_eq!(value, back);
    }
}
