//! One-time retirement of the `lucerna://` scheme registration.
//!
//! Versions 0.21.0–0.24.x had an opt-in toggle that registered the scheme under
//! `HKCU\Software\Classes\lucerna`. The toggle is gone, which would leave anyone
//! who had turned it on with a key and no way to turn it off — so every start
//! runs this once: decide (`platform::protocol::retire_action`), remove the key
//! if it is provably ours, and settle the consent flag in `app.json`.
//!
//! Path-based and with an injected `unregister` so it is testable without an
//! `AppHandle` or a registry. Design spec:
//! `2026-09-21-retire-url-scheme-design.md` §5.
//!
//! Failure handling (Fallback discipline, all four questions):
//! - `app.json` unreadable → treated as "not opted in" (we never delete a key on
//!   the strength of a record we could not read) and reported, not folded into
//!   `false` silently. A key pointing at this exe is still removed — that row of
//!   the table does not depend on the flag.
//! - `unregister` fails → the flag is left as it is, so the next start retries.
//! - the flag write fails → reported; the next start sees "no key + flag set"
//!   and retries by itself.

use crate::error::Error;
use crate::instances::schema::{AppFile, GeneralSettings};
use crate::instances::store::{read_app_json, write_app_json};
use crate::platform::protocol::{retire_action, RetireAction, SchemeState};
use std::path::Path;

/// What one retirement run actually did.
#[derive(Debug)]
pub enum RetireOutcome {
    /// Nothing to do, and nothing was touched.
    Nothing,
    /// The key was removed (and the consent flag cleared, if it was set).
    Removed,
    /// There was no key; the stale consent flag was cleared.
    FlagCleared,
    /// Removing the key failed. The consent flag was deliberately not touched.
    RemoveFailed(std::io::Error),
    /// The consent flag could not be written back. `key_removed` says whether
    /// the key itself had already been removed in this run.
    FlagWriteFailed { key_removed: bool, error: Error },
}

#[derive(Debug)]
pub struct RetireReport {
    pub outcome: RetireOutcome,
    /// `Some` when `app.json` could not be read; the run then assumed the user
    /// had not opted in.
    pub settings_read_error: Option<Error>,
}

pub fn retire_url_scheme(
    app_json: &Path,
    state: SchemeState,
    unregister: impl FnOnce() -> std::io::Result<()>,
) -> RetireReport {
    // RED stub (Task 1): does nothing. Replaced in Task 2.
    let _ = (app_json, state);
    drop(unregister);
    RetireReport {
        outcome: RetireOutcome::Nothing,
        settings_read_error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use tempfile::tempdir;

    fn write_settings(path: &Path, opted_in: bool) {
        write_app_json(
            path,
            &AppFile {
                general: GeneralSettings {
                    register_url_scheme: opted_in,
                    ..GeneralSettings::default()
                },
                ..AppFile::default()
            },
        )
        .unwrap();
    }

    fn flag(path: &Path) -> bool {
        read_app_json(path).unwrap().general.register_url_scheme
    }

    #[test]
    fn opted_in_and_registered_removes_the_key_then_clears_the_flag() {
        let dir = tempdir().unwrap();
        let app_json = dir.path().join("app.json");
        write_settings(&app_json, true);
        let called = Cell::new(false);

        let report = retire_url_scheme(&app_json, SchemeState::Registered, || {
            called.set(true);
            Ok(())
        });

        assert!(called.get(), "unregister must be called");
        assert!(matches!(report.outcome, RetireOutcome::Removed));
        assert!(report.settings_read_error.is_none());
        assert!(!flag(&app_json), "consent flag must be cleared");
    }

    #[test]
    fn a_failed_removal_leaves_the_flag_set_so_the_next_start_retries() {
        let dir = tempdir().unwrap();
        let app_json = dir.path().join("app.json");
        write_settings(&app_json, true);

        let report = retire_url_scheme(&app_json, SchemeState::Registered, || {
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
        });

        assert!(matches!(report.outcome, RetireOutcome::RemoveFailed(_)));
        assert!(flag(&app_json), "flag must survive a failed removal");
    }

    #[test]
    fn never_opted_in_but_registered_removes_the_key_without_rewriting_settings() {
        let dir = tempdir().unwrap();
        let app_json = dir.path().join("app.json");
        write_settings(&app_json, false);
        let before = std::fs::read(&app_json).unwrap();
        let called = Cell::new(false);

        let report = retire_url_scheme(&app_json, SchemeState::Registered, || {
            called.set(true);
            Ok(())
        });

        assert!(called.get());
        assert!(matches!(report.outcome, RetireOutcome::Removed));
        assert_eq!(std::fs::read(&app_json).unwrap(), before);
    }

    #[test]
    fn a_missing_settings_file_is_never_created() {
        let dir = tempdir().unwrap();
        let app_json = dir.path().join("app.json");

        let report = retire_url_scheme(&app_json, SchemeState::Registered, || Ok(()));

        assert!(matches!(report.outcome, RetireOutcome::Removed));
        // A missing app.json is a valid "defaults" state, not a read failure.
        assert!(report.settings_read_error.is_none());
        assert!(!app_json.exists());
    }

    #[test]
    fn opted_in_with_no_key_only_clears_the_flag() {
        let dir = tempdir().unwrap();
        let app_json = dir.path().join("app.json");
        write_settings(&app_json, true);
        let called = Cell::new(false);

        let report = retire_url_scheme(&app_json, SchemeState::NotRegistered, || {
            called.set(true);
            Ok(())
        });

        assert!(!called.get(), "there is no key to remove");
        assert!(matches!(report.outcome, RetireOutcome::FlagCleared));
        assert!(!flag(&app_json));
    }

    #[test]
    fn a_foreign_key_without_recorded_consent_is_left_alone() {
        let dir = tempdir().unwrap();
        let app_json = dir.path().join("app.json");
        write_settings(&app_json, false);
        let before = std::fs::read(&app_json).unwrap();
        let called = Cell::new(false);

        let report = retire_url_scheme(&app_json, SchemeState::RegisteredToOtherPath, || {
            called.set(true);
            Ok(())
        });

        assert!(!called.get());
        assert!(matches!(report.outcome, RetireOutcome::Nothing));
        assert_eq!(std::fs::read(&app_json).unwrap(), before);
    }

    #[test]
    fn unreadable_settings_count_as_not_opted_in_and_are_reported() {
        let dir = tempdir().unwrap();
        let app_json = dir.path().join("app.json");
        std::fs::write(&app_json, b"{ not json").unwrap();
        let called = Cell::new(false);

        let report = retire_url_scheme(&app_json, SchemeState::RegisteredToOtherPath, || {
            called.set(true);
            Ok(())
        });

        assert!(!called.get(), "no consent on record ⇒ a foreign key stays");
        assert!(matches!(report.outcome, RetireOutcome::Nothing));
        assert!(report.settings_read_error.is_some());
        assert_eq!(std::fs::read(&app_json).unwrap(), b"{ not json");
    }

    #[test]
    fn unreadable_settings_do_not_protect_a_key_that_points_at_this_exe() {
        let dir = tempdir().unwrap();
        let app_json = dir.path().join("app.json");
        std::fs::write(&app_json, b"{ not json").unwrap();
        let called = Cell::new(false);

        let report = retire_url_scheme(&app_json, SchemeState::Registered, || {
            called.set(true);
            Ok(())
        });

        assert!(called.get());
        assert!(matches!(report.outcome, RetireOutcome::Removed));
        assert!(report.settings_read_error.is_some());
        // The malformed file is not "repaired" by overwriting it.
        assert_eq!(std::fs::read(&app_json).unwrap(), b"{ not json");
    }
}
