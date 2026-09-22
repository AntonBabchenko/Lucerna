//! OS keyring wrapper for Microsoft account secrets.
//!
//! Stores two per-account secrets:
//! - `refresh_token` — long-lived MS refresh token, used by
//!   `microsoft::refresh` to renew the MC access token without user
//!   interaction. Service slot: `"lucerna-microsoft-refresh"`.
//! - `mc_access_token` — short-lived MC services access token, used by
//!   `launch::args::substitution_map` to fill `auth_access_token` in the
//!   JVM argv. Service slot: `"lucerna-mc-access"`.
//!
//! Both keyed by the account's local id (`Account.id`). On Sign out,
//! `remove_account` clears both. On Microsoft account refresh, both are
//! overwritten.
//!
//! Tests redirect through an in-memory `HashMap` (`#[cfg(test)]`) so
//! `cargo test` never touches the real Windows Credential Manager. See
//! `mods/curseforge/keyring.rs` for the precedent and the bug that
//! caused this pattern to be invented (memory:
//! project_keyring_test_clobber_bug).

use crate::error::{Error, KeyringOp, Result};

const SERVICE_REFRESH: &str = "lucerna-microsoft-refresh";
const SERVICE_MC_ACCESS: &str = "lucerna-mc-access";
const SERVICE_SFTP_PASSWORD: &str = "lucerna-sftp-password";
const SERVICE_AI_KEY: &str = "lucerna-ai-api-key";

pub fn refresh_token_key(account_id: &str) -> Key {
    Key {
        service: SERVICE_REFRESH,
        account: account_id.to_string(),
    }
}

pub fn mc_access_key(account_id: &str) -> Key {
    Key {
        service: SERVICE_MC_ACCESS,
        account: account_id.to_string(),
    }
}

/// Keyring key for a server's SFTP upload password, namespaced by server id.
pub fn sftp_password_key(server_id: &str) -> Key {
    Key {
        service: SERVICE_SFTP_PASSWORD,
        account: server_id.to_string(),
    }
}

/// One API key per AI translation provider, keyed by the provider's stable
/// id (`"anthropic"`, `"gemini"`, `"groq"`). `Local` needs no key.
pub fn ai_provider_key(provider_id: &str) -> Key {
    Key {
        service: SERVICE_AI_KEY,
        account: provider_id.to_string(),
    }
}

/// A namespaced key in the OS keyring. Construct via the helpers above, or
/// `Key::new` for a slot another module owns (the CurseForge key).
pub struct Key {
    service: &'static str,
    account: String,
}

impl Key {
    pub fn new(service: &'static str, account: impl Into<String>) -> Self {
        Key {
            service,
            account: account.into(),
        }
    }
}

/// Absence is `Ok(None)`; everything else is the keyring's own words under the op.
pub fn map_read(res: std::result::Result<String, keyring::Error>) -> Result<Option<String>> {
    // RED STUB (push 1): today's shape — an IO error with raw text.
    match res {
        Ok(s) => Ok(Some(s)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(Error::io("OS keyring", format!("get: {e}"))),
    }
}

pub fn map_write(res: std::result::Result<(), keyring::Error>) -> Result<()> {
    // RED STUB (push 1).
    res.map_err(|e| Error::io("OS keyring", format!("set: {e}")))
}

pub fn map_delete(res: std::result::Result<(), keyring::Error>) -> Result<()> {
    // RED STUB (push 1).
    match res {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(Error::io("OS keyring", format!("delete: {e}"))),
    }
}

#[cfg(not(test))]
fn entry(key: &Key) -> Result<keyring::Entry> {
    keyring::Entry::new(key.service, &key.account).map_err(|e| Error::Keyring {
        op: KeyringOp::Read,
        details: format!("entry: {e}"),
    })
}
#[cfg(not(test))]
pub fn store(key: &Key, value: &str) -> Result<()> {
    map_write(entry(key)?.set_password(value))
}
#[cfg(not(test))]
pub fn retrieve(key: &Key) -> Result<Option<String>> {
    map_read(entry(key)?.get_password())
}
#[cfg(not(test))]
pub fn delete(key: &Key) -> Result<()> {
    map_delete(entry(key)?.delete_credential())
}

// ----- test-only in-memory backend -----

#[cfg(test)]
pub(crate) mod test_backend {
    use super::Key;
    use std::collections::HashMap;
    use std::sync::Mutex;

    static STORE: once_cell::sync::Lazy<Mutex<HashMap<(&'static str, String), String>>> =
        once_cell::sync::Lazy::new(|| Mutex::new(HashMap::new()));

    pub fn store(key: &Key, value: &str) {
        STORE
            .lock()
            .unwrap()
            .insert((key.service, key.account.clone()), value.to_string());
    }

    pub fn retrieve(key: &Key) -> Option<String> {
        STORE
            .lock()
            .unwrap()
            .get(&(key.service, key.account.clone()))
            .cloned()
    }

    pub fn delete(key: &Key) {
        STORE
            .lock()
            .unwrap()
            .remove(&(key.service, key.account.clone()));
    }

    /// Make the NEXT call of `op` fail with `details` — one shot. This is how
    /// the honest paths (a token that could not be deleted, a status that
    /// could not be read) are pinned against an in-memory store that never
    /// fails on its own.
    pub fn fail_next(op: crate::error::KeyringOp, details: &str) {
        // RED STUB (push 1): ignored.
        let _ = (op, details);
    }
}

#[cfg(test)]
pub fn store(key: &Key, value: &str) -> Result<()> {
    test_backend::store(key, value);
    Ok(())
}

#[cfg(test)]
pub fn retrieve(key: &Key) -> Result<Option<String>> {
    Ok(test_backend::retrieve(key))
}

#[cfg(test)]
pub fn delete(key: &Key) -> Result<()> {
    test_backend::delete(key);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boxed(msg: &str) -> Box<dyn std::error::Error + Send + Sync> {
        Box::new(std::io::Error::other(msg.to_owned()))
    }

    #[test]
    fn no_entry_is_absence_not_an_error() {
        assert!(matches!(map_read(Err(keyring::Error::NoEntry)), Ok(None)));
        assert!(map_delete(Err(keyring::Error::NoEntry)).is_ok());
    }

    #[test]
    fn a_platform_failure_is_a_keyring_error_with_its_op() {
        match map_read(Err(keyring::Error::PlatformFailure(boxed("dbus down")))) {
            Err(Error::Keyring {
                op: KeyringOp::Read,
                details,
            }) => assert!(details.contains("dbus down"), "{details}"),
            other => panic!("{other:?}"),
        }
        match map_write(Err(keyring::Error::NoStorageAccess(boxed("locked")))) {
            Err(Error::Keyring {
                op: KeyringOp::Write,
                ..
            }) => {}
            other => panic!("{other:?}"),
        }
        match map_delete(Err(keyring::Error::PlatformFailure(boxed("x")))) {
            Err(Error::Keyring {
                op: KeyringOp::Delete,
                ..
            }) => {}
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_test_backend_can_be_told_to_fail_once() {
        let k = sftp_password_key("inject");
        store(&k, "p").unwrap();
        test_backend::fail_next(KeyringOp::Delete, "injected");
        match delete(&k) {
            Err(Error::Keyring {
                op: KeyringOp::Delete,
                details,
            }) => assert_eq!(details, "injected"),
            other => panic!("{other:?}"),
        }
        delete(&k).unwrap(); // the injection is one-shot
    }

    #[test]
    fn store_retrieve_round_trip_refresh_token() {
        let k = refresh_token_key("of-test-1");
        store(&k, "refresh_xyz").unwrap();
        assert_eq!(retrieve(&k).unwrap().as_deref(), Some("refresh_xyz"));
        delete(&k).unwrap();
        assert_eq!(retrieve(&k).unwrap(), None);
    }

    #[test]
    fn store_retrieve_round_trip_mc_access_token() {
        let k = mc_access_key("of-test-2");
        store(&k, "mc_abc123").unwrap();
        assert_eq!(retrieve(&k).unwrap().as_deref(), Some("mc_abc123"));
        delete(&k).unwrap();
        assert_eq!(retrieve(&k).unwrap(), None);
    }

    #[test]
    fn sftp_password_key_namespaced_per_server() {
        let k = sftp_password_key("srv-1");
        assert_eq!(k.service, "lucerna-sftp-password");
        assert_eq!(k.account, "srv-1");
    }

    #[test]
    fn ai_provider_keys_are_per_provider() {
        let anthropic = ai_provider_key("anthropic");
        let groq = ai_provider_key("groq");
        store(&anthropic, "key-a").expect("store anthropic");
        store(&groq, "key-g").expect("store groq");
        assert_eq!(
            retrieve(&anthropic).expect("get"),
            Some("key-a".to_string())
        );
        assert_eq!(retrieve(&groq).expect("get"), Some("key-g".to_string()));
        delete(&anthropic).expect("delete");
        assert_eq!(retrieve(&anthropic).expect("get"), None);
        assert_eq!(retrieve(&groq).expect("get"), Some("key-g".to_string()));
    }

    #[test]
    fn refresh_and_mc_access_use_separate_service_slots() {
        let r = refresh_token_key("of-test-3");
        let m = mc_access_key("of-test-3");
        store(&r, "refresh").unwrap();
        store(&m, "access").unwrap();
        // Same account id, different service → independent values.
        assert_eq!(retrieve(&r).unwrap().as_deref(), Some("refresh"));
        assert_eq!(retrieve(&m).unwrap().as_deref(), Some("access"));
    }
}
