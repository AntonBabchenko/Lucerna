//! What went wrong with a settings-sync file, typed. The user is told WHICH
//! file (this instance's own, the shared copy, its sync settings, Lucerna's
//! records) and a plain reason class; the raw OS or parser text goes to
//! `lucerna.log`, never to the screen. The same shape as network errors
//! (`network::failure`): a reason the UI can say, not a string.

use std::fmt::Display;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::Kind;
use crate::error::Error;

/// Which file a sync step failed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SyncFile {
    /// This instance's own file in `.minecraft/` (`servers.dat`, …).
    Game,
    /// The shared copy every member instance syncs with.
    Shared,
    /// This instance's sync settings (`lucerna/settings-sync.json`).
    Settings,
    /// Lucerna's own records of the last sync (baselines, pending files,
    /// the running-game marker).
    Records,
}

/// Why, in the words the UI uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum FileReason {
    /// The file does not parse, or ends early.
    Damaged,
    /// Denied, or held by another program.
    NoAccess,
    NotFound,
    /// Anything else: the log has the details.
    Other,
}

/// What a path is to settings sync, given at the call site that knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct At {
    pub file: SyncFile,
    pub kind: Option<Kind>,
}

impl At {
    pub fn game(kind: Kind) -> Self {
        Self {
            file: SyncFile::Game,
            kind: Some(kind),
        }
    }

    pub fn shared(kind: Kind) -> Self {
        Self {
            file: SyncFile::Shared,
            kind: Some(kind),
        }
    }

    pub fn settings() -> Self {
        Self {
            file: SyncFile::Settings,
            kind: None,
        }
    }

    pub fn records(kind: Option<Kind>) -> Self {
        Self {
            file: SyncFile::Records,
            kind,
        }
    }
}

/// The reason class of an OS error.
pub fn reason_of(e: &io::Error) -> FileReason {
    match e.kind() {
        io::ErrorKind::NotFound => FileReason::NotFound,
        io::ErrorKind::PermissionDenied => FileReason::NoAccess,
        io::ErrorKind::InvalidData | io::ErrorKind::UnexpectedEof => FileReason::Damaged,
        _ if held_by_another_program(e) => FileReason::NoAccess,
        _ => FileReason::Other,
    }
}

/// Windows reports a file another program holds open (the game, an
/// antivirus) as a sharing or lock violation, not as "denied".
#[cfg(windows)]
fn held_by_another_program(e: &io::Error) -> bool {
    // ERROR_SHARING_VIOLATION (32), ERROR_LOCK_VIOLATION (33).
    matches!(e.raw_os_error(), Some(32 | 33))
}

/// Elsewhere these codes mean other things (EPIPE, EDOM): never a lock.
#[cfg(not(windows))]
fn held_by_another_program(_: &io::Error) -> bool {
    false
}

fn error(at: At, path: &Path, reason: FileReason, details: String) -> Error {
    Error::SettingsSyncFile {
        file: at.file,
        file_kind: at.kind,
        reason,
        path: path.display().to_string(),
        details,
    }
}

/// An OS error on `path`.
pub fn io(at: At, path: &Path, e: io::Error) -> Error {
    error(at, path, reason_of(&e), e.to_string())
}

/// `path` was read but does not parse.
pub fn damaged(at: At, path: &Path, why: impl Display) -> Error {
    error(at, path, FileReason::Damaged, why.to_string())
}

/// The file and reason of an error, for a failure the UI shows; anything
/// that is not a settings-sync file error is "other", with no file named.
pub fn classify(e: &Error) -> (Option<SyncFile>, FileReason) {
    match e {
        Error::SettingsSyncFile { file, reason, .. } => (Some(*file), *reason),
        _ => (None, FileReason::Other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_errors_get_the_class_the_ui_says() {
        let of = |k| reason_of(&io::Error::from(k));
        assert_eq!(of(io::ErrorKind::NotFound), FileReason::NotFound);
        assert_eq!(of(io::ErrorKind::PermissionDenied), FileReason::NoAccess);
        assert_eq!(of(io::ErrorKind::InvalidData), FileReason::Damaged);
        assert_eq!(of(io::ErrorKind::UnexpectedEof), FileReason::Damaged);
        assert_eq!(of(io::ErrorKind::OutOfMemory), FileReason::Other);
    }

    #[cfg(windows)]
    #[test]
    fn a_file_another_program_holds_is_no_access_on_windows() {
        // ERROR_SHARING_VIOLATION and ERROR_LOCK_VIOLATION.
        for code in [32, 33] {
            assert_eq!(
                reason_of(&io::Error::from_raw_os_error(code)),
                FileReason::NoAccess
            );
        }
    }

    #[test]
    fn a_parse_failure_is_damaged_and_keeps_its_text_for_the_log() {
        let e = damaged(At::shared(Kind::Servers), Path::new("s.dat"), "not NBT");
        assert_eq!(classify(&e), (Some(SyncFile::Shared), FileReason::Damaged));
        assert!(e.to_string().contains("not NBT"), "{e}");
    }
}
