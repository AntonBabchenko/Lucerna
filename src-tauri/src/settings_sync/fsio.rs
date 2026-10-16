//! Small file primitives every part of settings sync uses: read with
//! "absent" told apart from "could not read", and an atomic replace.

use std::io;
use std::path::Path;

use super::failure::{self, At};
use crate::error::Result;

/// The file's bytes; `Ok(None)` only when it does not exist. Any other
/// failure (denied, a directory in its place, a broken disk) is an error:
/// "could not tell" is never "nothing here".
pub(crate) fn read_if_present(at: At, path: &Path) -> Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(failure::io(at, path, e)),
    }
}

/// Replace `target` with `bytes` in one step: a temp file of its own beside
/// it, flushed to disk, then a rename — a power cut leaves the old file or
/// the new one, never an empty one. The temp name is unique, so two writers
/// never share one. A temp left by a failed write or rename is removed; if
/// that removal fails too it is logged (the next write uses a new name).
pub(crate) fn write_atomic(at: At, target: &Path, bytes: &[u8]) -> Result<()> {
    let parent = target
        .parent()
        .ok_or_else(|| failure::io(at, target, io::Error::other("no parent folder")))?;
    std::fs::create_dir_all(parent).map_err(|e| failure::io(at, parent, e))?;
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = parent.join(format!(".{name}.{}.tmp", crate::instances::ids::new_id()));
    if let Err(e) = write_flushed(&tmp, bytes) {
        discard(&tmp);
        return Err(failure::io(at, &tmp, e));
    }
    if let Err(e) = std::fs::rename(&tmp, target) {
        discard(&tmp);
        return Err(failure::io(at, target, e));
    }
    Ok(())
}

fn write_flushed(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::File::create(path)?;
    f.write_all(bytes)?;
    f.sync_all()
}

/// Remove a temp file a failed write left; a failure is logged.
fn discard(tmp: &Path) {
    match std::fs::remove_file(tmp) {
        Ok(()) => {}
        Err(r) if r.kind() == io::ErrorKind::NotFound => {}
        Err(r) => crate::diag!(
            "settings sync: could not remove the temp file {} ({r})",
            tmp.display()
        ),
    }
}

/// Remove `path`; a file that is already gone is fine, any other failure is
/// an error.
pub(crate) fn remove_if_present(at: At, path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(failure::io(at, path, e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at() -> At {
        At::game(super::super::Kind::Servers)
    }

    #[test]
    fn absent_is_none_and_a_directory_is_an_error() {
        let td = tempfile::tempdir().unwrap();
        assert!(read_if_present(at(), &td.path().join("nope"))
            .unwrap()
            .is_none());
        std::fs::create_dir(td.path().join("dir")).unwrap();
        assert!(read_if_present(at(), &td.path().join("dir")).is_err());
    }

    #[test]
    fn write_replaces_and_leaves_no_temp() {
        let td = tempfile::tempdir().unwrap();
        let f = td.path().join("a").join("x.dat");
        write_atomic(at(), &f, b"one").unwrap();
        write_atomic(at(), &f, b"two").unwrap();
        assert_eq!(std::fs::read(&f).unwrap(), b"two");
        let left: Vec<_> = std::fs::read_dir(td.path().join("a")).unwrap().collect();
        assert_eq!(left.len(), 1, "no temp file left");
    }

    #[test]
    fn removing_an_absent_file_is_fine() {
        let td = tempfile::tempdir().unwrap();
        remove_if_present(at(), &td.path().join("nope")).unwrap();
    }
}
