//! Renaming an instance's directory.
//!
//! Because the directory name IS the id (see
//! [`crate::instances::store::read_instance_json`]), this changes the instance's
//! identity. `instance.json` is deliberately NOT rewritten: nothing inside it
//! needs updating, and a write that could fail *after* a successful rename would
//! only widen the half-state window.
//!
//! One code path serves two user-facing actions — "rename this folder" and
//! "repair this folder's unlaunchable name". They differ only in what the dialog
//! pre-fills.

use crate::error::{Error, Result};
use crate::naming::{derive_base, is_reserved};
use std::path::Path;

/// Rename `<parent>/<current_id>` to a directory derived from `new_name`, and
/// return the new id.
///
/// Pure filesystem work: the caller owns the busy guards and the `app.json`
/// update. `derive_base` is called with an EMPTY fallback so that "nothing
/// usable survives" is reportable as an error instead of silently becoming
/// `instance`; the ladder still transliterates before giving up.
pub fn rename_dir(parent: &Path, current_id: &str, new_name: &str) -> Result<String> {
    let target = derive_base(new_name, None, "");
    if target.is_empty() {
        return Err(Error::InstanceDirNameEmpty);
    }
    if target == current_id {
        // The common case: the user opened the dialog and confirmed without
        // editing. Touching the filesystem here would be pure risk.
        return Ok(target);
    }
    if is_reserved(&target) {
        return Err(Error::InstanceDirNameReserved { name: target });
    }
    let from = parent.join(current_id);
    let to = parent.join(&target);
    // Check-then-act, deliberately. `fs::rename` refuses an existing destination
    // on Windows, but POSIX permits replacing an EMPTY directory — which would
    // silently destroy another instance on Linux and macOS. The single-instance
    // guard means one launcher process and the user is the only actor here, so
    // the race this leaves open is not reachable in practice.
    if to.exists() {
        return Err(Error::InstanceDirNameTaken { name: target });
    }
    std::fs::rename(&from, &to).map_err(|e| Error::io(to.display().to_string(), e))?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn renames_the_directory_and_returns_the_new_id() {
        let dir = tempdir().unwrap();
        let parent = dir.path().join("instances");
        std::fs::create_dir_all(parent.join("Old-Name/.minecraft")).unwrap();
        let new_id = rename_dir(&parent, "Old-Name", "Новое имя").unwrap();
        assert_eq!(new_id, "Novoe-imia");
        assert!(parent.join("Novoe-imia/.minecraft").is_dir());
        assert!(!parent.join("Old-Name").exists());
    }

    #[test]
    fn no_op_when_the_slug_is_unchanged() {
        let dir = tempdir().unwrap();
        let parent = dir.path().join("instances");
        std::fs::create_dir_all(parent.join("My-Pack")).unwrap();
        let new_id = rename_dir(&parent, "My-Pack", "My Pack").unwrap();
        assert_eq!(new_id, "My-Pack");
        assert!(parent.join("My-Pack").is_dir());
    }

    #[test]
    fn refuses_a_taken_name() {
        let dir = tempdir().unwrap();
        let parent = dir.path().join("instances");
        std::fs::create_dir_all(parent.join("A-Pack")).unwrap();
        std::fs::create_dir_all(parent.join("B-Pack")).unwrap();
        let err = rename_dir(&parent, "A-Pack", "B-Pack").unwrap_err();
        assert!(matches!(err, Error::InstanceDirNameTaken { .. }));
    }

    #[test]
    fn refuses_to_replace_an_empty_directory() {
        // POSIX lets `rename` replace an EMPTY directory; Windows does not.
        // Without the explicit existence check this silently destroyed another
        // instance on Linux and macOS.
        let dir = tempdir().unwrap();
        let parent = dir.path().join("instances");
        std::fs::create_dir_all(parent.join("Source-Pack/.minecraft")).unwrap();
        std::fs::create_dir_all(parent.join("Empty-Pack")).unwrap();
        let err = rename_dir(&parent, "Source-Pack", "Empty-Pack").unwrap_err();
        assert!(matches!(err, Error::InstanceDirNameTaken { .. }));
        assert!(
            parent.join("Source-Pack/.minecraft").is_dir(),
            "source must be untouched"
        );
    }

    #[test]
    fn refuses_a_reserved_name() {
        let dir = tempdir().unwrap();
        let parent = dir.path().join("instances");
        std::fs::create_dir_all(parent.join("Pack")).unwrap();
        let err = rename_dir(&parent, "Pack", "CON").unwrap_err();
        assert!(matches!(err, Error::InstanceDirNameReserved { .. }));
    }

    #[test]
    fn refuses_a_name_that_slugs_to_nothing() {
        let dir = tempdir().unwrap();
        let parent = dir.path().join("instances");
        std::fs::create_dir_all(parent.join("Pack")).unwrap();
        let err = rename_dir(&parent, "Pack", "🎮🎮").unwrap_err();
        assert!(matches!(err, Error::InstanceDirNameEmpty));
    }
}
