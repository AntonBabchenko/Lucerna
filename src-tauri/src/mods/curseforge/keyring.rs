//! OS keyring storage for the CurseForge API key.

use crate::error::Error;

const SERVICE: &str = "ftlauncher";
#[cfg(not(test))]
const USERNAME: &str = "curseforge-api-key";
/// Unit tests run against a separate keyring slot so `cargo test` can
/// never read, overwrite, or delete the user's real CurseForge key.
/// `#[cfg(test)]` is set when this crate is compiled for its own unit
/// tests — exactly when `mods/modpack/curseforge.rs`'s `keyring::set`/
/// `clear` test helpers run.
#[cfg(test)]
const USERNAME: &str = "curseforge-api-key-test";

pub fn get() -> Result<Option<String>, Error> {
    let entry = ::keyring::Entry::new(SERVICE, USERNAME).map_err(map_keyring_err)?;
    match entry.get_password() {
        Ok(s) => Ok(Some(s)),
        Err(::keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(map_keyring_err(e)),
    }
}

pub fn set(value: &str) -> Result<(), Error> {
    let entry = ::keyring::Entry::new(SERVICE, USERNAME).map_err(map_keyring_err)?;
    entry.set_password(value).map_err(map_keyring_err)
}

pub fn clear() -> Result<(), Error> {
    let entry = ::keyring::Entry::new(SERVICE, USERNAME).map_err(map_keyring_err)?;
    match entry.delete_credential() {
        Ok(_) => Ok(()),
        Err(::keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(map_keyring_err(e)),
    }
}

fn map_keyring_err(_e: ::keyring::Error) -> Error {
    Error::ModsPlatformAuth { kind: crate::error::ModsAuthKind::Invalid }
    // Note: keyring-level errors (lock, permission) are surfaced as
    // "auth invalid" so the UI can prompt to re-enter. The verbose
    // details `{e}` would expose OS-internal Credential Manager errors
    // that are not user-actionable; we swallow them deliberately.
}

#[cfg(test)]
mod tests {
    use super::*;

    // Keyring tests touch the live OS keychain. They run serially and
    // clean up after themselves. CI runners typically have a usable
    // session keyring (Windows runners always; Linux runners require
    // dbus + secret-service which may not be available in headless mode).
    // The test is `#[ignore]` by default so the unit-test suite stays
    // hermetic; the integration smoke is run manually before the squash.

    #[test]
    #[ignore]
    fn round_trip() {
        clear().ok();
        assert_eq!(get().unwrap(), None);
        set("test-key").unwrap();
        assert_eq!(get().unwrap().as_deref(), Some("test-key"));
        clear().unwrap();
        assert_eq!(get().unwrap(), None);
    }

    #[test]
    fn unit_tests_use_a_separate_keyring_slot() {
        // Guards the #[cfg(test)] split: if the conditional is ever
        // removed, `cargo test` would again read/overwrite/delete the
        // user's real CurseForge key. `mods/modpack/curseforge.rs`'s
        // tests call keyring::set/clear, which use this USERNAME.
        assert_eq!(USERNAME, "curseforge-api-key-test");
    }
}
