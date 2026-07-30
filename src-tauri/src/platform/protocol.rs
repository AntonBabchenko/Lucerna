//! `lucerna://` URL-scheme registration with the OS.
//!
//! Windows only in v1 (see the design spec §4.5): per-user
//! `HKCU\Software\Classes\lucerna`, matching the installer's current-user
//! install mode — no elevation, no machine-wide change. Linux needs a
//! `.desktop` handler plus a MIME-database refresh and varies by desktop
//! environment; macOS needs `CFBundleURLTypes` in `Info.plist`. Neither can be
//! verified on the maintainer's machine, so both report `Unsupported` rather
//! than shipping untested OS integration.
//!
//! Registration is **opt-in** (Settings → General, default off). Writing to a
//! user's registry unasked is the kind of surprise this project refuses, and the
//! paste-a-URL half of import-by-link works with no registration at all.

use serde::Serialize;
use specta::Type;
use std::path::Path;

/// Registry key (under `HKCU`) that owns the scheme. Named here so the Settings
/// UI can show the user exactly what gets written.
pub const SCHEME_KEY: &str = "Software\\Classes\\lucerna";

/// Human-readable key description Explorer shows for the scheme.
const SCHEME_DESCRIPTION: &str = "URL:Lucerna modpack link";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SchemeState {
    /// Registered, pointing at this exe.
    Registered,
    /// Registered by a Lucerna at a different path — moved, reinstalled, or a
    /// portable copy. Distinguished from `Registered` so the app can re-assert
    /// the key instead of leaving links pointing at a stale binary.
    RegisteredToOtherPath,
    NotRegistered,
    /// This OS has no per-user scheme registration we support.
    Unsupported,
}

/// The exact `shell\open\command` value. `%1` is the URL the shell substitutes;
/// both it and the exe path are quoted so a URL (or an install path) containing
/// a space cannot split into extra argv entries. Pure, so it is asserted on
/// every platform.
pub fn command_value(exe: &Path) -> String {
    format!("\"{}\" \"%1\"", exe.display())
}

/// The exe path recorded inside a `shell\open\command` value, if the value has
/// the shape [`command_value`] writes. Lets `state` tell `Registered` from
/// `RegisteredToOtherPath` without a second parser.
pub fn exe_from_command_value(value: &str) -> Option<&str> {
    let rest = value.strip_prefix('"')?;
    let end = rest.find('"')?;
    let exe = &rest[..end];
    if exe.is_empty() {
        return None;
    }
    Some(exe)
}

/// Register the scheme for the current user, pointing at `exe`. Idempotent.
pub fn register(exe: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        win::register(exe)
    }
    #[cfg(not(windows))]
    {
        let _ = exe;
        Err(unsupported_io())
    }
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
        Err(unsupported_io())
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

#[cfg(not(windows))]
fn unsupported_io() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "URL-scheme registration is Windows-only in this version",
    )
}

#[cfg(windows)]
mod win {
    use super::{command_value, exe_from_command_value, SchemeState, SCHEME_DESCRIPTION};
    use std::io;
    use std::path::Path;
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyW, RegDeleteTreeW, RegOpenKeyExW, RegQueryValueExW,
        RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, REG_SZ,
    };

    const SCHEME_KEY: &str = super::SCHEME_KEY;
    const ICON_KEY: &str = "Software\\Classes\\lucerna\\DefaultIcon";
    const COMMAND_KEY: &str = "Software\\Classes\\lucerna\\shell\\open\\command";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Write one REG_SZ value (`name = None` → the key's default value) into
    /// `HKCU\<subkey>`, creating the key and any missing parents.
    ///
    /// SAFETY (applies to the whole body): standard Win32 registry FFI. Every
    /// pointer is to a local that outlives its call, sizes are passed in bytes
    /// as the API expects, and the key opened by `RegCreateKeyW` is closed on
    /// both the success and error paths.
    fn write_sz(subkey: &str, name: Option<&str>, value: &str) -> io::Result<()> {
        let subkey_w = wide(subkey);
        let name_w = name.map(wide);
        let data = wide(value);
        unsafe {
            let mut hkey: HKEY = std::ptr::null_mut();
            // RegCreateKeyW creates all missing intermediate keys and needs no
            // Win32_Security feature (same reason gpu.rs uses it).
            let rc = RegCreateKeyW(HKEY_CURRENT_USER, subkey_w.as_ptr(), &mut hkey);
            if rc != ERROR_SUCCESS {
                return Err(io::Error::from_raw_os_error(rc as i32));
            }
            // REG_SZ byte count includes the NUL terminator `wide` appended.
            let bytes = (data.len() * 2) as u32;
            let rc = RegSetValueExW(
                hkey,
                name_w
                    .as_ref()
                    .map(|n| n.as_ptr())
                    .unwrap_or(std::ptr::null()),
                0,
                REG_SZ,
                data.as_ptr() as *const u8,
                bytes,
            );
            RegCloseKey(hkey);
            if rc == ERROR_SUCCESS {
                Ok(())
            } else {
                Err(io::Error::from_raw_os_error(rc as i32))
            }
        }
    }

    /// Read a key's default REG_SZ value. `None` on any error (absent key,
    /// absent value, wrong type).
    fn read_default_sz(subkey: &str) -> Option<String> {
        let subkey_w = wide(subkey);
        // SAFETY: as `write_sz` — locals outlive the calls, `len` is a byte
        // count, and the opened key is closed before returning.
        unsafe {
            let mut hkey: HKEY = std::ptr::null_mut();
            if RegOpenKeyExW(HKEY_CURRENT_USER, subkey_w.as_ptr(), 0, KEY_READ, &mut hkey)
                != ERROR_SUCCESS
            {
                return None;
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
            if rc != ERROR_SUCCESS {
                return None;
            }
            // `len` counts bytes including the NUL terminator; drop it.
            let chars = (len as usize / 2).saturating_sub(1);
            Some(String::from_utf16_lossy(&buf[..chars]))
        }
    }

    pub fn register(exe: &Path) -> io::Result<()> {
        // Order matters only for tidiness: the scheme key itself first, so a
        // half-written registration is still recognisable as ours.
        write_sz(SCHEME_KEY, None, SCHEME_DESCRIPTION)?;
        // `URL Protocol` is the empty-string marker that makes the shell treat
        // this key as a URL scheme handler at all.
        write_sz(SCHEME_KEY, Some("URL Protocol"), "")?;
        write_sz(ICON_KEY, None, &format!("{},0", exe.display()))?;
        write_sz(COMMAND_KEY, None, &command_value(exe))?;
        Ok(())
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
        let Some(command) = read_default_sz(COMMAND_KEY) else {
            return SchemeState::NotRegistered;
        };
        match exe_from_command_value(&command) {
            // Windows paths are case-insensitive, so a case difference is the
            // same binary, not a stale registration.
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
    fn command_value_quotes_the_exe_and_the_url_placeholder() {
        let v = command_value(Path::new(r"C:\Program Files\Lucerna\lucerna.exe"));
        assert_eq!(v, r#""C:\Program Files\Lucerna\lucerna.exe" "%1""#);
    }

    #[test]
    fn exe_round_trips_out_of_a_command_value() {
        // A path with a space is the case the quoting exists for.
        let exe = r"C:\Users\A B\lucerna.exe";
        let v = command_value(Path::new(exe));
        assert_eq!(exe_from_command_value(&v), Some(exe));
    }

    #[test]
    fn a_command_value_we_did_not_write_yields_no_exe() {
        // Unquoted, empty, and non-command values must not be mistaken for ours
        // — `state` treats "no exe" as a stale registration to re-assert.
        assert_eq!(exe_from_command_value("lucerna.exe %1"), None);
        assert_eq!(exe_from_command_value(""), None);
        assert_eq!(exe_from_command_value("\"\" \"%1\""), None);
        assert_eq!(exe_from_command_value("\"unterminated"), None);
    }

    #[test]
    fn the_documented_key_matches_what_register_writes() {
        // The Settings UI shows SCHEME_KEY to the user; it must be the real key.
        assert_eq!(SCHEME_KEY, "Software\\Classes\\lucerna");
    }

    #[cfg(not(windows))]
    #[test]
    fn unsupported_platforms_report_unsupported_rather_than_lying() {
        assert_eq!(
            state(Path::new("/usr/bin/lucerna")),
            SchemeState::Unsupported
        );
        assert!(register(Path::new("/usr/bin/lucerna")).is_err());
        assert!(unregister().is_err());
    }
}
