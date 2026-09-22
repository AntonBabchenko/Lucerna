//! OS keyring storage for the CurseForge API key.
//!
//! One slot in the system credential store — Windows Credential Manager,
//! macOS Keychain, or the Linux Secret Service — reached through
//! `accounts::keychain`, which owns the per-platform backend, the one
//! process-wide gate, and the mapping of the keyring's own errors to
//! `Error::Keyring`. Under `cargo test` that module redirects every call to an
//! in-memory store, so the suite never reaches the OS keyring at all
//! (essential for headless Linux CI runners, which have no keyring daemon),
//! and the "no test ever touches the real prod key" guarantee from
//! [[project_keyring_test_clobber_bug]] holds structurally rather than by a
//! separate slot alone.
//!
//! The resolved key is cached for the session: outbound CurseForge requests
//! never pay a keyring round trip, and a locked keyring prompts at most once.

use crate::accounts::keychain;
use crate::error::Error;
use crate::mods::platform::KeyStatus;

const SERVICE: &str = "lucerna";
#[cfg(not(test))]
const USERNAME: &str = "curseforge-api-key";
/// Sentinel kept for the `unit_tests_use_a_separate_keyring_slot` test
/// below — pinned in case the `#[cfg(test)]` redirection in
/// `accounts::keychain` is ever lifted without also restoring the per-slot
/// USERNAME scoping.
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

fn key() -> keychain::Key {
    keychain::Key::new(SERVICE, USERNAME)
}

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

/// The key status for Settings: a fresh keyring read (Retry is a re-read),
/// which also refreshes the session cache — a keyring that was locked at
/// startup and is open now serves the personal key again from here on.
pub fn status() -> KeyStatus {
    read_status(EMBEDDED_KEY)
}

pub fn read_status(embedded: Option<&str>) -> KeyStatus {
    let mut cache = cache();
    let read = get();
    match &read {
        Ok(stored) => *cache = Some(stored.clone()),
        Err(e) => crate::diag!("curseforge key: status read failed: {e}"),
    }
    drop(cache);
    key_status_from(read, embedded)
}

/// The personal key as last read or written this session. `None` = not
/// consulted yet; `Some(None)` = nothing stored, or the read failed (logged).
/// Every reader or writer of the slot holds this lock ACROSS its keyring call,
/// so the cache can never hold an answer older than the last completed write
/// (lock order: this, then the keychain gate — nothing takes them the other
/// way round).
static CACHE: std::sync::Mutex<Option<Option<String>>> = std::sync::Mutex::new(None);

fn cache() -> std::sync::MutexGuard<'static, Option<Option<String>>> {
    // A panic while holding the lock poisons it; the value inside is a whole
    // `Option` either way, so it stays usable.
    CACHE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Fill the cache at startup, off the main thread, so the session's first
/// CurseForge request finds the answer ready instead of paying the keyring
/// round trip (or an unlock prompt) on a runtime worker. A request that beats
/// the warm-up does that one read itself, behind the same lock.
pub fn warm() {
    resolve_with_cache(EMBEDDED_KEY);
}

/// The effective key from the session cache; the keyring is read at most once
/// per session (set / clear / a status read refresh the cache), so no
/// CurseForge request pays a D-Bus round trip and a locked keyring prompts at
/// most once.
pub fn resolve_with_cache(embedded: Option<&str>) -> Option<String> {
    let mut cache = cache();
    let stored = match &*cache {
        Some(stored) => stored.clone(),
        None => {
            let stored = match get() {
                Ok(stored) => stored,
                Err(e) => {
                    // A keyring that cannot be read is "no personal key" for the
                    // rest of the session: requests fall back to the build's key
                    // (or fail as keyless), and Settings shows the read failure
                    // with a Check again that refreshes this cache. Logged once,
                    // not per request.
                    crate::diag!(
                        "curseforge key: keyring read failed, no personal key this session: {e}"
                    );
                    None
                }
            };
            *cache = Some(stored.clone());
            stored
        }
    };
    drop(cache);
    resolve_with(stored, embedded)
}

/// Absent from the keyring vs. unreadable — the two are never folded. The
/// error itself is logged by the caller (`read_status`); the status crosses
/// IPC as a bare token and the Settings form offers a Retry.
pub fn key_status_from(read: Result<Option<String>, Error>, embedded: Option<&str>) -> KeyStatus {
    let embedded = embedded.filter(|k| !k.is_empty());
    match read {
        Ok(Some(_)) => KeyStatus::Set,
        Ok(None) if embedded.is_some() => KeyStatus::SetBuiltin,
        Ok(None) => KeyStatus::Missing,
        Err(_) if embedded.is_some() => KeyStatus::UnknownEmbedded,
        Err(_) => KeyStatus::Unknown,
    }
}

/// A direct keyring read — absent is `Ok(None)`, unreadable is
/// `Error::Keyring`. Request paths use `resolve()` (cached) instead.
pub fn get() -> Result<Option<String>, Error> {
    keychain::retrieve(&key())
}

pub fn set(value: &str) -> Result<(), Error> {
    let mut cache = cache();
    keychain::store(&key(), value)?;
    *cache = Some(Some(value.to_string()));
    Ok(())
}

/// Remove the stored key. Deleting what is not there is a success.
pub fn clear() -> Result<(), Error> {
    let mut cache = cache();
    keychain::delete(&key())?;
    *cache = Some(None);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_status_tells_absent_from_unreadable() {
        let err = || {
            Err(Error::Keyring {
                op: crate::error::KeyringOp::Read,
                details: "x".into(),
            })
        };
        assert_eq!(key_status_from(Ok(Some("k".into())), None), KeyStatus::Set);
        assert_eq!(
            key_status_from(Ok(Some("k".into())), Some("e")),
            KeyStatus::Set
        );
        // No personal key, the build's own serves — its own state (INT-01).
        assert_eq!(key_status_from(Ok(None), Some("e")), KeyStatus::SetBuiltin);
        assert_eq!(key_status_from(Ok(None), None), KeyStatus::Missing);
        assert_eq!(
            key_status_from(err(), Some("e")),
            KeyStatus::UnknownEmbedded
        );
        assert_eq!(key_status_from(err(), None), KeyStatus::Unknown);
        // An unset CI secret expands to "": not a built-in key.
        assert_eq!(key_status_from(err(), Some("")), KeyStatus::Unknown);
    }

    #[test]
    fn unit_tests_use_a_separate_keyring_slot() {
        // Sentinel — if the #[cfg(test)] in-memory redirection in
        // `accounts::keychain` is ever lifted, the test backend would fall
        // back to the OS keyring; this assertion catches the lift by checking
        // the USERNAME scoping survived.
        assert_eq!(USERNAME, "curseforge-api-key-test");
    }

    #[test]
    fn in_memory_backend_round_trips() {
        // Smoke-test that the test backend honors set/get/clear. Serialized
        // via the shared lock; safe to interleave with other tests (each sets
        // then clears).
        let _g = crate::test_env_lock();
        clear().unwrap();
        assert_eq!(get().unwrap(), None);
        set("smoke").unwrap();
        assert_eq!(get().unwrap().as_deref(), Some("smoke"));
        clear().unwrap();
        assert_eq!(get().unwrap(), None);
    }

    #[test]
    fn set_and_clear_write_through_the_session_cache() {
        let _g = crate::test_env_lock();
        clear().unwrap();
        assert_eq!(resolve_with_cache(None), None);
        set("k1").unwrap();
        assert_eq!(resolve_with_cache(None).as_deref(), Some("k1"));
        set("k2").unwrap();
        assert_eq!(resolve_with_cache(None).as_deref(), Some("k2"));
        clear().unwrap();
        assert_eq!(resolve_with_cache(None), None);
    }

    #[test]
    fn a_resolved_key_is_served_from_the_cache_without_a_second_read() {
        let _g = crate::test_env_lock();
        clear().unwrap();
        set("cached").unwrap();
        assert_eq!(resolve_with_cache(None).as_deref(), Some("cached"));
        // The keyring is now unreadable; the session answer stands.
        keychain::test_backend::fail_next(&key(), crate::error::KeyringOp::Read, "locked");
        assert_eq!(resolve_with_cache(None).as_deref(), Some("cached"));
        // The injection was never consumed: nothing read the keyring.
        assert!(matches!(get(), Err(Error::Keyring { .. })));
        clear().unwrap();
    }

    #[test]
    fn a_status_read_refreshes_the_cache() {
        let _g = crate::test_env_lock();
        clear().unwrap(); // cache: nothing stored
                          // Written behind the cache's back (another process, or a keyring that
                          // was locked when the cache was filled and is open now).
        keychain::store(&key(), "later").unwrap();
        assert_eq!(resolve_with_cache(None), None);
        assert_eq!(read_status(None), KeyStatus::Set);
        assert_eq!(resolve_with_cache(None).as_deref(), Some("later"));
        clear().unwrap();
    }

    #[test]
    fn an_unreadable_keyring_is_no_personal_key_until_something_refreshes() {
        let _g = crate::test_env_lock();
        clear().unwrap();
        keychain::store(&key(), "hidden").unwrap();
        // Drop the cache entry so the next resolve has to read.
        *CACHE.lock().unwrap() = None;
        keychain::test_backend::fail_next(&key(), crate::error::KeyringOp::Read, "locked");
        assert_eq!(resolve_with_cache(None), None);
        // That read consumed the failure and cached "nothing": the (now
        // readable) key is still not seen until a status read refreshes.
        assert_eq!(resolve_with_cache(None), None);
        assert_eq!(read_status(None), KeyStatus::Set);
        assert_eq!(resolve_with_cache(None).as_deref(), Some("hidden"));
        clear().unwrap();
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
