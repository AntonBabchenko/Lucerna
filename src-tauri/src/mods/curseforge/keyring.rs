//! OS keyring storage for the CurseForge API key.
//!
//! Production code uses the system credential store via the `keyring`
//! crate (Windows Credential Manager / macOS Keychain / Linux libsecret).
//! Under `cargo test`, all three functions are redirected to an
//! in-memory store — see the `#[cfg(test)]` block below. That removes
//! the OS-keyring dependency from the test suite (essential for headless
//! Linux CI runners, which have no keyring daemon) and strengthens the
//! "no test ever touches the real prod key" guarantee from
//! [[project_keyring_test_clobber_bug]]: tests now cannot reach the OS
//! keyring at all, not just a separate slot of it.

use crate::error::Error;

const SERVICE: &str = "lucerna";
#[cfg(not(test))]
const USERNAME: &str = "curseforge-api-key";
/// Sentinel kept for the `unit_tests_use_a_separate_keyring_slot` test
/// below — pinned in case the `#[cfg(test)]` redirection is ever lifted
/// without also restoring the per-slot USERNAME scoping.
#[cfg(test)]
const USERNAME: &str = "curseforge-api-key-test";

// --- production backend -------------------------------------------------

#[cfg(not(test))]
pub fn get() -> Result<Option<String>, Error> {
    let entry = ::keyring::Entry::new(SERVICE, USERNAME).map_err(map_keyring_err)?;
    match entry.get_password() {
        Ok(s) => Ok(Some(s)),
        Err(::keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(map_keyring_err(e)),
    }
}

#[cfg(not(test))]
pub fn set(value: &str) -> Result<(), Error> {
    let entry = ::keyring::Entry::new(SERVICE, USERNAME).map_err(map_keyring_err)?;
    entry.set_password(value).map_err(map_keyring_err)
}

#[cfg(not(test))]
pub fn clear() -> Result<(), Error> {
    let entry = ::keyring::Entry::new(SERVICE, USERNAME).map_err(map_keyring_err)?;
    match entry.delete_credential() {
        Ok(_) => Ok(()),
        Err(::keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(map_keyring_err(e)),
    }
}

#[cfg(not(test))]
fn map_keyring_err(_e: ::keyring::Error) -> Error {
    Error::ModsPlatformAuth {
        kind: crate::error::ModsAuthKind::Invalid,
    }
    // Note: keyring-level errors (lock, permission) are surfaced as
    // "auth invalid" so the UI can prompt to re-enter. The verbose
    // details `{e}` would expose OS-internal Credential Manager errors
    // that are not user-actionable; we swallow them deliberately.
}

// --- test backend (in-memory) ------------------------------------------

#[cfg(test)]
static TEST_KEY: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

#[cfg(test)]
pub fn get() -> Result<Option<String>, Error> {
    Ok(TEST_KEY.lock().unwrap().clone())
}

#[cfg(test)]
pub fn set(value: &str) -> Result<(), Error> {
    *TEST_KEY.lock().unwrap() = Some(value.to_string());
    Ok(())
}

#[cfg(test)]
pub fn clear() -> Result<(), Error> {
    *TEST_KEY.lock().unwrap() = None;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_tests_use_a_separate_keyring_slot() {
        // Sentinel — if the #[cfg(test)] in-memory redirection above
        // is ever lifted, the test backend would fall back to the OS
        // keyring; this assertion catches the lift by checking the
        // USERNAME scoping survived.
        assert_eq!(USERNAME, "curseforge-api-key-test");
    }

    #[test]
    fn in_memory_backend_round_trips() {
        // Smoke-test that the test backend honors set/get/clear.
        // Serialized via the global TEST_KEY mutex; safe to interleave
        // with other tests (each sets then clears).
        clear().unwrap();
        assert_eq!(get().unwrap(), None);
        set("smoke").unwrap();
        assert_eq!(get().unwrap().as_deref(), Some("smoke"));
        clear().unwrap();
        assert_eq!(get().unwrap(), None);
    }
}
