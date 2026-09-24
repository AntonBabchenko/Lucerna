//! Whether a world's `level.dat` is there. The answer has three states and a
//! fourth, "could not tell"; never two.
//!
//! Minecraft treats a folder as a world when `level.dat` OR `level.dat_old`
//! is a regular file (`hasWorldData`, the singleplayer list filter). When
//! `level.dat` is missing or unreadable, it reads `level.dat_old` and restores
//! `level.dat` from it. So "no level.dat" covers two different facts:
//!   * `OnlyOld`: a world that lost its `level.dat`. A new one written here
//!     would parse cleanly and switch the game's recovery off.
//!   * `Absent`: on the client, a folder the game does not list as a world; on
//!     the server, a world that has not been generated yet.
//!
//! The two-state checks this replaces (`Path::exists`, `symlink_metadata`)
//! folded the first into the second, and `exists` also folded "could not
//! stat" into "absent" (Fallback discipline Q2).

use std::path::Path;

use serde::Serialize;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum LevelDatPresence {
    /// `level.dat` is a regular file. This is the only state Lucerna edits.
    Present,
    /// No `level.dat`, and `level.dat_old` is a regular file. The game reads the
    /// copy and restores `level.dat` from it; Lucerna must not pre-empt that.
    OnlyOld,
    /// Neither file. Client: the game does not list this folder as a world.
    /// Server: a world the server has not generated yet.
    Absent,
}

/// `Err` means Lucerna could not tell. Every writer refuses on it. The
/// client world listing fails with it, as it does on an unreadable
/// `level.dat`; the library overview and the server listing show the state
/// as unknown.
///
/// `std::fs::metadata` follows symlinks, as the game's `Files.exists` /
/// `isRegularFile` do, so a dangling `level.dat` link reads as missing here,
/// exactly as in the game. A missing world directory yields `NotFound` for
/// both files, which is `Absent`: correct for a server that has never booted.
pub fn of(world_dir: &Path) -> Result<LevelDatPresence> {
    if is_regular_file(world_dir, "level.dat")? {
        return Ok(LevelDatPresence::Present);
    }
    if is_regular_file(world_dir, "level.dat_old")? {
        return Ok(LevelDatPresence::OnlyOld);
    }
    Ok(LevelDatPresence::Absent)
}

/// `Ok(true)` for a regular file and `Ok(false)` only for `NotFound`. A
/// directory or anything else at that path is something the game cannot read
/// either, so it is "could not tell", never "absent".
fn is_regular_file(world_dir: &Path, name: &str) -> Result<bool> {
    let path = world_dir.join(name);
    match std::fs::metadata(&path) {
        Ok(meta) if meta.is_file() => Ok(true),
        Ok(_) => Err(Error::io(
            path.display().to_string(),
            format!("{name} is not a regular file"),
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(Error::io(path.display().to_string(), e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Presence reads metadata only, so any bytes can stand in for a level file.
    fn file(path: &Path) {
        std::fs::write(path, b"x").unwrap();
    }

    #[test]
    fn present_absent_and_only_old_are_told_apart() {
        let td = tempfile::tempdir().unwrap();
        let w = td.path();
        assert_eq!(of(w).unwrap(), LevelDatPresence::Absent);
        file(&w.join("level.dat_old"));
        assert_eq!(of(w).unwrap(), LevelDatPresence::OnlyOld);
        file(&w.join("level.dat"));
        assert_eq!(
            of(w).unwrap(),
            LevelDatPresence::Present,
            "level.dat wins over its backup"
        );
    }

    #[test]
    fn a_world_directory_that_does_not_exist_is_absent() {
        // A server that has never booted has no runtime/<level>/ yet.
        let td = tempfile::tempdir().unwrap();
        assert_eq!(
            of(&td.path().join("world")).unwrap(),
            LevelDatPresence::Absent
        );
    }

    #[test]
    fn a_level_dat_that_is_a_directory_is_could_not_tell() {
        // `Path::exists` called this "present", and the read then failed with
        // Windows error 5, which `map_read_err` turns into a false "quit
        // Minecraft". The game cannot read a directory either, and a surviving
        // backup does not make the answer OnlyOld.
        let td = tempfile::tempdir().unwrap();
        std::fs::create_dir(td.path().join("level.dat")).unwrap();
        file(&td.path().join("level.dat_old"));
        let err = of(td.path()).unwrap_err();
        assert!(matches!(err, Error::Io { .. }), "got {err:?}");
    }

    #[test]
    fn a_level_dat_old_that_is_a_directory_is_could_not_tell() {
        let td = tempfile::tempdir().unwrap();
        std::fs::create_dir(td.path().join("level.dat_old")).unwrap();
        assert!(matches!(of(td.path()), Err(Error::Io { .. })));
    }

    /// Rule 1: `metadata` follows links, as the game's `Files.isRegularFile`
    /// does, so a dangling `level.dat` link is missing here, and the game falls
    /// back to `level.dat_old` for it too.
    #[cfg(unix)]
    #[test]
    fn a_dangling_level_dat_link_reads_as_missing() {
        let td = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(td.path().join("gone"), td.path().join("level.dat")).unwrap();
        file(&td.path().join("level.dat_old"));
        assert_eq!(of(td.path()).unwrap(), LevelDatPresence::OnlyOld);
    }
}
