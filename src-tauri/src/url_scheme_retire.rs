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
//! - the registry unreadable (`SchemeState::Unknown`) → neither the key nor the
//!   flag is touched. "Could not tell" is not "no key": clearing the flag here
//!   would orphan a key that is really there, because the next start would see
//!   the flag off and never look again.
//! - `unregister` fails → the flag is left as it is, so the next start retries.
//! - the flag write fails → reported; `app.json` is unchanged on disk, so the
//!   next start sees the same state and retries by itself.

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
    let (settings, settings_read_error) = match read_app_json(app_json) {
        Ok(file) => (Some(file), None),
        Err(e) => (None, Some(e)),
    };
    // `Some` only when the user is on record as having opted in. A missing file
    // reads as defaults (flag off), an unreadable one as "no record".
    let opted_in_file = settings.filter(|f| f.general.register_url_scheme);

    let outcome = match (retire_action(opted_in_file.is_some(), state), opted_in_file) {
        (RetireAction::Nothing, _) => RetireOutcome::Nothing,
        (RetireAction::ClearFlagOnly, Some(file)) => clear_flag(app_json, file, false),
        // `retire_action` answers ClearFlagOnly only for opted_in == true, which
        // is exactly `Some`. Kept total instead of unwrapping.
        (RetireAction::ClearFlagOnly, None) => RetireOutcome::Nothing,
        (RetireAction::RemoveKey, file) => match unregister() {
            // Leave the flag alone: the next start sees the same state and retries.
            Err(e) => RetireOutcome::RemoveFailed(e),
            Ok(()) => match file {
                Some(file) => clear_flag(app_json, file, true),
                // Never opted in (or no readable record): nothing to write, and
                // app.json is neither rewritten nor created.
                None => RetireOutcome::Removed,
            },
        },
    };

    RetireReport {
        outcome,
        settings_read_error,
    }
}

fn clear_flag(app_json: &Path, file: AppFile, key_removed: bool) -> RetireOutcome {
    let cleared = AppFile {
        general: GeneralSettings {
            register_url_scheme: false,
            ..file.general
        },
        ..file
    };
    match write_app_json(app_json, &cleared) {
        Ok(()) if key_removed => RetireOutcome::Removed,
        Ok(()) => RetireOutcome::FlagCleared,
        Err(error) => RetireOutcome::FlagWriteFailed { key_removed, error },
    }
}

/// One `diag!` line per thing that actually happened. A run that did nothing —
/// every start after the first, and every start for users who never opted in —
/// logs nothing.
pub fn log_report(report: &RetireReport) {
    if let Some(e) = &report.settings_read_error {
        crate::diag!("[url-scheme] app.json unreadable, treated as not opted in: {e}");
    }
    match &report.outcome {
        RetireOutcome::Nothing => {}
        RetireOutcome::Removed => {
            crate::diag!("[url-scheme] removed the retired lucerna:// registration")
        }
        RetireOutcome::FlagCleared => {
            crate::diag!("[url-scheme] cleared the retired lucerna:// setting (no key present)")
        }
        RetireOutcome::RemoveFailed(e) => crate::diag!(
            "[url-scheme] could not remove the lucerna:// registration, will retry next start: {e}"
        ),
        RetireOutcome::FlagWriteFailed { key_removed, error } => crate::diag!(
            "[url-scheme] could not clear the lucerna:// setting (key removed: {key_removed}), will retry next start: {error}"
        ),
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
    fn an_unreadable_registry_keeps_the_consent_flag_for_the_next_start() {
        let dir = tempdir().unwrap();
        let app_json = dir.path().join("app.json");
        write_settings(&app_json, true);
        let before = std::fs::read(&app_json).unwrap();
        let called = Cell::new(false);

        let report = retire_url_scheme(&app_json, SchemeState::Unknown, || {
            called.set(true);
            Ok(())
        });

        assert!(!called.get(), "nothing is removed on a failed read");
        assert!(matches!(report.outcome, RetireOutcome::Nothing));
        // The flag is the only record that lets a later start finish the job.
        assert_eq!(std::fs::read(&app_json).unwrap(), before);
    }

    /// `write_app_json` is atomic via `<stem>.tmp` + rename, so a directory
    /// squatting on that tmp path makes the write fail on every OS while the
    /// read still succeeds. If the tmp naming ever changes these tests go red
    /// (outcome `Removed` / `FlagCleared`) rather than passing vacuously.
    fn block_settings_writes(app_json: &Path) {
        std::fs::create_dir(app_json.with_extension("tmp")).unwrap();
    }

    #[test]
    fn a_failed_flag_write_after_a_removal_is_reported_and_keeps_the_flag() {
        let dir = tempdir().unwrap();
        let app_json = dir.path().join("app.json");
        write_settings(&app_json, true);
        block_settings_writes(&app_json);

        let report = retire_url_scheme(&app_json, SchemeState::Registered, || Ok(()));

        assert!(matches!(
            report.outcome,
            RetireOutcome::FlagWriteFailed {
                key_removed: true,
                ..
            }
        ));
        assert!(flag(&app_json), "a failed write must not half-apply");
    }

    #[test]
    fn a_failed_flag_write_with_no_key_is_reported_and_keeps_the_flag() {
        let dir = tempdir().unwrap();
        let app_json = dir.path().join("app.json");
        write_settings(&app_json, true);
        block_settings_writes(&app_json);

        let report = retire_url_scheme(&app_json, SchemeState::NotRegistered, || Ok(()));

        assert!(matches!(
            report.outcome,
            RetireOutcome::FlagWriteFailed {
                key_removed: false,
                ..
            }
        ));
        assert!(flag(&app_json));
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
