//! `window-state.json` in the data root: the saved main-window geometry.
//! Read with "absent" told apart from "could not read"; replaced in one step;
//! never creates its folder, so a late write cannot bring back a data root a
//! move has just deleted. Callers serialize saves (`persist`), which is why one
//! fixed temp name is enough — and a temp a crash left is overwritten next time.

use super::geometry::{self, SavedWindow};
use std::io::{self, Write};
use std::path::Path;

/// Why a saved state cannot be used. The two are told apart because only a
/// damaged file may be replaced: an unreadable one (locked by an antivirus, a
/// sharing violation) may be perfectly good, so `persist` does not overwrite it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// It could not be read.
    Unreadable(String),
    /// It was read, and it is not a usable window state.
    Damaged(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreadable(reason) | Self::Damaged(reason) => f.write_str(reason),
        }
    }
}

/// The saved state; `Ok(None)` only when the file does not exist. Anything
/// else that stops it being used is an `Err` with the reason, for the log.
pub fn load(path: &Path) -> Result<Option<SavedWindow>, LoadError> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(LoadError::Unreadable(format!(
                "cannot read {} ({e})",
                path.display()
            )))
        }
    };
    let raw: SavedWindow = serde_json::from_slice(&bytes).map_err(|e| {
        LoadError::Damaged(format!("{} is not a window state ({e})", path.display()))
    })?;
    geometry::validated(raw).map(Some).ok_or_else(|| {
        LoadError::Damaged(format!(
            "{} holds an impossible size or position",
            path.display()
        ))
    })
}

/// Replace the file with `saved`: a flushed temp beside it, then a rename —
/// a power cut leaves the old file or the new one, never a torn one. A temp a
/// failed write or rename left is removed; a failed removal is logged.
pub fn save(path: &Path, saved: &SavedWindow) -> Result<(), String> {
    let json = serde_json::to_vec_pretty(saved).map_err(|e| format!("serialize: {e}"))?;
    let tmp = path.with_extension("tmp");
    if let Err(e) = write_flushed(&tmp, &json) {
        discard(&tmp);
        return Err(format!("cannot write {} ({e})", tmp.display()));
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        discard(&tmp);
        return Err(format!("cannot replace {} ({e})", path.display()));
    }
    Ok(())
}

/// `File::create` fails when the folder is gone: the folder is never created.
fn write_flushed(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = std::fs::File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn discard(tmp: &Path) {
    match std::fs::remove_file(tmp) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => crate::diag!(
            "[window] could not remove the temp file {} ({e})",
            tmp.display()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::geometry::{Point, Size};

    fn sample() -> SavedWindow {
        SavedWindow {
            position: Some(Point {
                x: -1500.0,
                y: 20.0,
            }),
            size: Size {
                width: 1084.0,
                height: 861.0,
            },
            maximized: true,
        }
    }

    #[test]
    fn an_absent_file_is_nothing_saved() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(&dir.path().join("window-state.json")), Ok(None));
    }

    #[test]
    fn a_saved_state_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("window-state.json");
        save(&path, &sample()).unwrap();
        assert_eq!(load(&path), Ok(Some(sample())));
    }

    #[test]
    fn a_save_leaves_no_temp_behind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("window-state.json");
        save(&path, &sample()).unwrap();
        save(&path, &sample()).unwrap();
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["window-state.json".to_string()]);
    }

    #[test]
    fn a_damaged_file_is_an_error_not_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("window-state.json");
        std::fs::write(&path, b"{ not json").unwrap();
        assert!(matches!(load(&path), Err(LoadError::Damaged(_))));
    }

    #[test]
    fn impossible_values_are_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("window-state.json");
        std::fs::write(&path, br#"{"size":{"width":0,"height":700}}"#).unwrap();
        assert!(matches!(load(&path), Err(LoadError::Damaged(_))));
    }

    #[test]
    fn an_unreadable_path_is_an_error_not_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("window-state.json");
        std::fs::create_dir(&path).unwrap();
        assert!(matches!(load(&path), Err(LoadError::Unreadable(_))));
    }

    #[test]
    fn a_file_without_optional_fields_reads_with_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("window-state.json");
        std::fs::write(&path, br#"{"size":{"width":1000,"height":700}}"#).unwrap();
        let loaded = load(&path).unwrap().unwrap();
        assert_eq!(loaded.position, None);
        assert!(!loaded.maximized);
    }

    #[test]
    fn a_save_never_creates_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("moved-away");
        assert!(save(&gone.join("window-state.json"), &sample()).is_err());
        assert!(!gone.exists());
    }
}
