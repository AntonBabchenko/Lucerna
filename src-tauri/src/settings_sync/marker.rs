//! `<instance>/lucerna/settings-sync/running.json` (spec §B3, R4): written
//! after the game spawns, removed by the exit step. A marker left by an
//! earlier launcher session means that launcher stopped while the game ran —
//! the game may still be writing the files a sync point would touch.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

use super::failure::At;
use super::state_dir;
use crate::error::{Error, Result};
use crate::platform::ImageMatch;

/// This launcher process, once: a marker carrying it was written by us, and
/// the process registry — not the marker — says whether that game runs.
static SESSION: LazyLock<String> = LazyLock::new(crate::instances::ids::new_id);

#[derive(Debug, Serialize, Deserialize)]
struct Marker {
    pid: u32,
    started_unix_ms: u64,
    session: String,
}

/// What a marker says before a sync point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerState {
    /// No marker, or one that was stale and is now removed.
    Clear,
    /// The PID runs Java: a game from before Lucerna restarted may still run.
    Alive,
    /// Could not tell (no image source, access denied, an unreadable marker).
    Unknown,
}

pub(crate) fn path(instance_root: &Path) -> PathBuf {
    state_dir(instance_root).join("running.json")
}

pub fn write(instance_root: &Path, pid: u32, started_unix_ms: u64) -> Result<()> {
    let p = path(instance_root);
    let marker = Marker {
        pid,
        started_unix_ms,
        session: SESSION.clone(),
    };
    let bytes = serde_json::to_vec(&marker)
        .map_err(|e| Error::io(p.display().to_string(), format!("serialize: {e}")))?;
    super::fsio::write_atomic(At::records(None), &p, &bytes)
}

pub fn remove(instance_root: &Path) -> Result<()> {
    super::fsio::remove_if_present(At::records(None), &path(instance_root))
}

/// Classify the marker, removing it when it is stale (checked). One this
/// process wrote is cleared at once: the process registry, which the caller
/// consulted, is the truth about this session's games.
pub fn check(instance_root: &Path, probe: &dyn Fn(u32) -> ImageMatch) -> Result<MarkerState> {
    let Some(bytes) = super::fsio::read_if_present(At::records(None), &path(instance_root))? else {
        return Ok(MarkerState::Clear);
    };
    let Ok(marker) = serde_json::from_slice::<Marker>(&bytes) else {
        // A marker we cannot read names no PID: ask, do not guess.
        return Ok(MarkerState::Unknown);
    };
    if marker.session == *SESSION {
        remove(instance_root)?;
        return Ok(MarkerState::Clear);
    }
    match probe(marker.pid) {
        ImageMatch::Yes => Ok(MarkerState::Alive),
        ImageMatch::No => {
            remove(instance_root)?;
            Ok(MarkerState::Clear)
        }
        ImageMatch::Unknown => Ok(MarkerState::Unknown),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn earlier_session(root: &Path, pid: u32) {
        let m = Marker {
            pid,
            started_unix_ms: 1,
            session: "an-earlier-launcher".into(),
        };
        let p = path(root);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, serde_json::to_vec(&m).unwrap()).unwrap();
    }

    #[test]
    fn no_marker_is_clear() {
        let td = tempfile::tempdir().unwrap();
        assert_eq!(
            check(td.path(), &|_| ImageMatch::Yes).unwrap(),
            MarkerState::Clear
        );
    }

    #[test]
    fn our_own_marker_is_cleared_without_probing() {
        let td = tempfile::tempdir().unwrap();
        write(td.path(), 42, 1).unwrap();
        assert!(path(td.path()).exists(), "written");
        assert_eq!(
            check(td.path(), &|_| panic!("our own game: the registry knows")).unwrap(),
            MarkerState::Clear
        );
        assert!(!path(td.path()).exists(), "removed");
    }

    #[test]
    fn an_earlier_sessions_game_still_running_blocks() {
        let td = tempfile::tempdir().unwrap();
        earlier_session(td.path(), 7);
        assert_eq!(
            check(td.path(), &|_| ImageMatch::Yes).unwrap(),
            MarkerState::Alive
        );
        assert!(path(td.path()).exists(), "kept");
    }

    #[test]
    fn a_pid_now_owned_by_another_program_is_stale() {
        let td = tempfile::tempdir().unwrap();
        earlier_session(td.path(), 7);
        assert_eq!(
            check(td.path(), &|_| ImageMatch::No).unwrap(),
            MarkerState::Clear
        );
        assert!(!path(td.path()).exists(), "removed");
    }

    #[test]
    fn could_not_tell_asks_and_keeps_the_marker() {
        let td = tempfile::tempdir().unwrap();
        earlier_session(td.path(), 7);
        assert_eq!(
            check(td.path(), &|_| ImageMatch::Unknown).unwrap(),
            MarkerState::Unknown
        );
        assert!(
            path(td.path()).exists(),
            "kept until the user says the game is closed"
        );
    }

    #[test]
    fn a_garbled_marker_is_could_not_tell() {
        let td = tempfile::tempdir().unwrap();
        let p = path(td.path());
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"{").unwrap();
        assert_eq!(
            check(td.path(), &|_| ImageMatch::No).unwrap(),
            MarkerState::Unknown
        );
    }
}
