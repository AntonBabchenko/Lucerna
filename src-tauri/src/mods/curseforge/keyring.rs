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

/// Only the production backend below references SERVICE; under `cargo test`
/// that backend is compiled out (in-memory redirection), so the constant is
/// cfg-scoped to match — same split as USERNAME.
#[cfg(not(test))]
const SERVICE: &str = "lucerna";
#[cfg(not(test))]
const USERNAME: &str = "curseforge-api-key";
/// Sentinel kept for the `unit_tests_use_a_separate_keyring_slot` test
/// below — pinned in case the `#[cfg(test)]` redirection is ever lifted
/// without also restoring the per-slot USERNAME scoping.
#[cfg(test)]
const USERNAME: &str = "curseforge-api-key-test";

/// The CurseForge API key baked into the binary at compile time, when the
/// `LUCERNA_CURSEFORGE_API_KEY` env var is set during the build. Resolved by
/// `option_env!`, so the value is embedded in the shipped binary: release
/// builds carry the maintainer's key (injected from a CI secret in
/// `release.yml`), while self-built/dev binaries without the env var have
/// `None` here and fall back to a user-entered key. Mirrors `CLIENT_ID` in
/// `accounts::microsoft::oauth`. NOT a true secret — it is extractable from
/// the binary; see docs/SECURITY.md.
const EMBEDDED_KEY: Option<&str> = option_env!("LUCERNA_CURSEFORGE_API_KEY");

/// Precedence logic for the effective CurseForge key: a personal key the user
/// stored in the OS keyring wins; the build's embedded key is the fallback; an
/// empty embedded value (an unset CI secret expanding to "") counts as absent.
/// Pure — split from `resolve()` so the precedence is unit-tested without a
/// compile-time env var.
pub fn resolve_with(stored: Option<String>, embedded: Option<&str>) -> Option<String> {
    stored.or_else(|| embedded.filter(|k| !k.is_empty()).map(str::to_string))
}

/// The effective CurseForge API key for outbound requests: the user's stored
/// key if present, else the build's embedded key. `None` only when neither
/// exists (a keyless self-build) — callers then surface the existing
/// "key missing" path.
pub fn resolve() -> Option<String> {
    resolve_with_cache(EMBEDDED_KEY)
}

/// The effective key from the session cache; the keyring is read at most once
/// per session (set / clear refresh the cache), so no CurseForge request pays
/// a D-Bus round trip and a locked keyring prompts at most once.
pub fn resolve_with_cache(embedded: Option<&str>) -> Option<String> {
    // RED STUB (push 1): reads every time, swallows the error as before.
    resolve_with(get().ok().flatten(), embedded)
}

/// Absent from the keyring vs. unreadable — the two are never folded.
pub fn key_status_from(
    read: Result<Option<String>, Error>,
    embedded: Option<&str>,
) -> crate::mods::platform::KeyStatus {
    use crate::mods::platform::KeyStatus;
    // RED STUB (push 1): a read failure still reads as "missing".
    let _ = &read;
    if resolve_with(read.ok().flatten(), embedded).is_some() {
        KeyStatus::Set
    } else {
        KeyStatus::Missing
    }
}

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
    fn key_status_tells_absent_from_unreadable() {
        use crate::mods::platform::KeyStatus;
        let err = || {
            Err(Error::Keyring {
                op: crate::error::KeyringOp::Read,
                details: "x".into(),
            })
        };
        assert_eq!(key_status_from(Ok(Some("k".into())), None), KeyStatus::Set);
        // The built-in key reads as "set" — INT-01, reversed by batch 7.
        assert_eq!(key_status_from(Ok(None), Some("e")), KeyStatus::Set);
        assert_eq!(key_status_from(Ok(None), None), KeyStatus::Missing);
        assert_eq!(
            key_status_from(err(), Some("e")),
            KeyStatus::UnknownEmbedded
        );
        assert_eq!(key_status_from(err(), None), KeyStatus::Unknown);
    }

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
        let _g = crate::test_env_lock();
        clear().unwrap();
        assert_eq!(get().unwrap(), None);
        set("smoke").unwrap();
        assert_eq!(get().unwrap().as_deref(), Some("smoke"));
        clear().unwrap();
        assert_eq!(get().unwrap(), None);
    }

    #[test]
    fn resolve_with_prefers_personal_over_embedded() {
        assert_eq!(
            resolve_with(Some("personal".into()), Some("embedded")),
            Some("personal".to_string())
        );
    }

    #[test]
    fn resolve_with_uses_personal_when_no_embedded() {
        assert_eq!(
            resolve_with(Some("personal".into()), None),
            Some("personal".to_string())
        );
    }

    #[test]
    fn resolve_with_falls_back_to_embedded() {
        assert_eq!(
            resolve_with(None, Some("embedded")),
            Some("embedded".to_string())
        );
    }

    #[test]
    fn resolve_with_none_when_neither() {
        assert_eq!(resolve_with(None, None), None);
    }

    #[test]
    fn resolve_with_treats_empty_embedded_as_absent() {
        assert_eq!(resolve_with(None, Some("")), None);
    }

    #[test]
    fn resolve_returns_stored_key_from_keyring() {
        // EMBEDDED_KEY is None in a normal test build (env unset at compile),
        // so resolve() reflects the in-memory keyring. Serialize against other
        // keyring-touching tests via the shared lock.
        let _g = crate::test_env_lock();
        clear().unwrap();
        set("stored-key").unwrap();
        assert_eq!(resolve(), Some("stored-key".to_string()));
        clear().unwrap();
    }
}
