//! `lucerna://` URL-scheme key: detection and removal only.
//!
//! Versions 0.21.0–0.24.x offered an opt-in toggle (Settings → Integrations)
//! that registered the scheme per-user under `HKCU\Software\Classes\lucerna`.
//! Nothing ever produced such links, so the toggle was retired. This module
//! contains no code that creates or updates the key — it can only tell whether a
//! key exists and whom it points at (`state`), remove it (`unregister`), and
//! decide whether removal is ours to do (`retire_action`). The orchestration
//! lives in `crate::url_scheme_retire`.
//!
//! The receiving side is unaffected: `cli::parse` still demotes any command
//! line carrying a `lucerna:` token to an untrusted `OpenUrl`, so a key that is
//! still present on some machine keeps opening the import dialog and nothing
//! more.
//!
//! Windows only; every other OS reports `Unsupported`.

use std::path::Path;

/// Registry key (under `HKCU`) that owned the scheme.
pub const SCHEME_KEY: &str = "Software\\Classes\\lucerna";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemeState {
    /// Registered, pointing at this exe.
    Registered,
    /// Registered with a command that names a different path — a moved or
    /// reinstalled Lucerna, a portable copy, or a command we did not write.
    RegisteredToOtherPath,
    /// The registry answered, and there is no key.
    NotRegistered,
    /// The registry could not be read (access denied, policy, a transient
    /// failure). Deliberately NOT folded into `NotRegistered`: "could not tell"
    /// must not be acted on as "absent" — see `retire_action`.
    Unknown,
    /// This OS has no per-user scheme registration we ever supported.
    Unsupported,
}

/// The exe path recorded inside a `shell\open\command` value, if the value has
/// the shape the old registration wrote: `"<exe>" "%1"`.
pub fn exe_from_command_value(value: &str) -> Option<&str> {
    let rest = value.strip_prefix('"')?;
    let end = rest.find('"')?;
    let exe = &rest[..end];
    if exe.is_empty() {
        return None;
    }
    Some(exe)
}

/// Remove the scheme registration for the current user. Succeeds when the key
/// is already absent (the desired end state is the same).
pub fn unregister() -> std::io::Result<()> {
    #[cfg(windows)]
    {
        win::unregister()
    }
    #[cfg(not(windows))]
    {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "URL-scheme registration was Windows-only",
        ))
    }
}

/// Current registration state relative to `exe`.
pub fn state(exe: &Path) -> SchemeState {
    #[cfg(windows)]
    {
        win::state(exe)
    }
    #[cfg(not(windows))]
    {
        let _ = exe;
        SchemeState::Unsupported
    }
}

/// What the one-time retirement of the scheme registration should do for one
/// (consent flag, OS state) pair. The table and its reasoning live in the design
/// spec `2026-09-21-retire-url-scheme-design.md` §5.1; the orchestration is
/// `crate::url_scheme_retire`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetireAction {
    /// Touch nothing: there is no key, or the key is not provably ours.
    Nothing,
    /// No key to remove, but the consent flag is still set — settle the record.
    ClearFlagOnly,
    /// Remove the key, then clear the consent flag if it was set.
    RemoveKey,
}

pub fn retire_action(opted_in: bool, state: SchemeState) -> RetireAction {
    // Exhaustive over `SchemeState` on purpose (no `_` arm on the state): a new
    // variant must be given an answer here rather than inheriting one.
    match (state, opted_in) {
        // The command has the exact shape we wrote and names this binary.
        (SchemeState::Registered, _) => RetireAction::RemoveKey,
        // The user consented through this data root, and the old startup
        // self-heal would have overwritten this key anyway.
        (SchemeState::RegisteredToOtherPath, true) => RetireAction::RemoveKey,
        // Not provably ours: another product named Lucerna, or another copy
        // with its own settings. When we cannot tell, we do not delete.
        (SchemeState::RegisteredToOtherPath, false) => RetireAction::Nothing,
        (SchemeState::NotRegistered, true) => RetireAction::ClearFlagOnly,
        (SchemeState::NotRegistered, false) => RetireAction::Nothing,
        // RED step of the review fix: `Unknown` deliberately inherits what the
        // old code did when it folded a failed read into `NotRegistered`, so the
        // new tests fail first. Corrected in the next commit.
        (SchemeState::Unknown, true) => RetireAction::ClearFlagOnly,
        (SchemeState::Unknown, false) => RetireAction::Nothing,
        (SchemeState::Unsupported, _) => RetireAction::Nothing,
    }
}

#[cfg(windows)]
mod win {
    use super::{exe_from_command_value, SchemeState};
    use std::io;
    use std::path::Path;
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegDeleteTreeW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER,
        KEY_READ,
    };

    const SCHEME_KEY: &str = super::SCHEME_KEY;
    const COMMAND_KEY: &str = "Software\\Classes\\lucerna\\shell\\open\\command";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// What reading a key's default string value found. Three answers, because
    /// "the key is absent" and "the registry would not tell us" lead to
    /// different actions (Fallback discipline: discrimination).
    enum DefaultSz {
        /// The key does not exist.
        Absent,
        /// The key exists. The string is empty when it has no default value or
        /// one longer than any command we ever wrote — i.e. present, not ours.
        Present(String),
        /// The registry could not be read.
        Failed,
    }

    fn read_default_sz(subkey: &str) -> DefaultSz {
        let subkey_w = wide(subkey);
        // SAFETY: standard Win32 registry FFI. Every pointer is to a local that
        // outlives its call, `len` is a byte count as the API expects, and the
        // opened key is closed before returning.
        unsafe {
            let mut hkey: HKEY = std::ptr::null_mut();
            let rc = RegOpenKeyExW(HKEY_CURRENT_USER, subkey_w.as_ptr(), 0, KEY_READ, &mut hkey);
            if rc == ERROR_FILE_NOT_FOUND {
                return DefaultSz::Absent;
            }
            if rc != ERROR_SUCCESS {
                return DefaultSz::Failed;
            }
            let mut buf = [0u16; 1024];
            let mut len = (buf.len() * 2) as u32;
            let rc = RegQueryValueExW(
                hkey,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                buf.as_mut_ptr() as *mut u8,
                &mut len,
            );
            RegCloseKey(hkey);
            if rc == ERROR_FILE_NOT_FOUND || rc == ERROR_MORE_DATA {
                // The key is there, but it has no default value, or one that
                // does not fit a buffer far larger than any path we wrote.
                // Either way: a key exists and its command is not ours.
                return DefaultSz::Present(String::new());
            }
            if rc != ERROR_SUCCESS {
                return DefaultSz::Failed;
            }
            // `len` counts bytes including the NUL terminator; drop it.
            let chars = (len as usize / 2).saturating_sub(1);
            DefaultSz::Present(String::from_utf16_lossy(&buf[..chars]))
        }
    }

    pub fn unregister() -> io::Result<()> {
        let subkey = wide(SCHEME_KEY);
        // SAFETY: single FFI call with a pointer to a local that outlives it;
        // RegDeleteTreeW with a null second arg deletes the subkey's whole tree.
        let rc = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, subkey.as_ptr()) };
        if rc == ERROR_SUCCESS || rc == ERROR_FILE_NOT_FOUND {
            // Already absent is the desired end state, not an error.
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(rc as i32))
        }
    }

    pub fn state(exe: &Path) -> SchemeState {
        let command = match read_default_sz(COMMAND_KEY) {
            DefaultSz::Absent => return SchemeState::NotRegistered,
            DefaultSz::Failed => return SchemeState::Unknown,
            DefaultSz::Present(command) => command,
        };
        match exe_from_command_value(&command) {
            // Windows paths are case-insensitive, so a case difference is the
            // same binary, not a different registration.
            Some(registered) if registered.eq_ignore_ascii_case(&exe.display().to_string()) => {
                SchemeState::Registered
            }
            _ => SchemeState::RegisteredToOtherPath,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exe_is_read_out_of_a_command_value_with_a_space_in_the_path() {
        // The shape the retired registration wrote: both halves quoted so a
        // path with a space stays one token.
        assert_eq!(
            exe_from_command_value(r#""C:\Users\A B\lucerna.exe" "%1""#),
            Some(r"C:\Users\A B\lucerna.exe")
        );
    }

    #[test]
    fn a_command_value_we_did_not_write_yields_no_exe() {
        // Unquoted, empty, and non-command values must not be mistaken for ours
        // — `state` reports "no exe" as a key that points somewhere else.
        assert_eq!(exe_from_command_value("lucerna.exe %1"), None);
        assert_eq!(exe_from_command_value(""), None);
        assert_eq!(exe_from_command_value("\"\" \"%1\""), None);
        assert_eq!(exe_from_command_value("\"unterminated"), None);
    }

    #[cfg(not(windows))]
    #[test]
    fn unsupported_platforms_report_unsupported_rather_than_lying() {
        assert_eq!(
            state(Path::new("/usr/bin/lucerna")),
            SchemeState::Unsupported
        );
        assert!(unregister().is_err());
    }

    #[test]
    fn a_key_pointing_at_this_exe_is_removed_whatever_the_flag_says() {
        // The command has the exact shape we wrote and names this binary, so it
        // is provably ours even when the consent record was lost or reset.
        assert_eq!(
            retire_action(true, SchemeState::Registered),
            RetireAction::RemoveKey
        );
        assert_eq!(
            retire_action(false, SchemeState::Registered),
            RetireAction::RemoveKey
        );
    }

    #[test]
    fn a_key_pointing_elsewhere_is_removed_only_with_recorded_consent() {
        assert_eq!(
            retire_action(true, SchemeState::RegisteredToOtherPath),
            RetireAction::RemoveKey
        );
        // No consent on record and the key names another binary: it may belong
        // to another product or another copy with its own settings. Leave it.
        assert_eq!(
            retire_action(false, SchemeState::RegisteredToOtherPath),
            RetireAction::Nothing
        );
    }

    #[test]
    fn with_no_key_only_a_stale_consent_flag_is_settled() {
        assert_eq!(
            retire_action(true, SchemeState::NotRegistered),
            RetireAction::ClearFlagOnly
        );
        assert_eq!(
            retire_action(false, SchemeState::NotRegistered),
            RetireAction::Nothing
        );
    }

    #[test]
    fn an_unreadable_registry_touches_neither_the_key_nor_the_flag() {
        // "Could not tell" is not "absent". Clearing the consent flag on a failed
        // read would orphan a key that is really there: the next start would see
        // flag=false and never look at it again.
        assert_eq!(
            retire_action(true, SchemeState::Unknown),
            RetireAction::Nothing
        );
        assert_eq!(
            retire_action(false, SchemeState::Unknown),
            RetireAction::Nothing
        );
    }

    #[test]
    fn unsupported_platforms_are_never_touched() {
        assert_eq!(
            retire_action(true, SchemeState::Unsupported),
            RetireAction::Nothing
        );
        assert_eq!(
            retire_action(false, SchemeState::Unsupported),
            RetireAction::Nothing
        );
    }
}
