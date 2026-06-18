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

#[cfg(not(test))]
use crate::error::Error;
use crate::error::Result;

const SERVICE_REFRESH: &str = "lucerna-microsoft-refresh";
const SERVICE_MC_ACCESS: &str = "lucerna-mc-access";
const SERVICE_SFTP_PASSWORD: &str = "lucerna-sftp-password";

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

/// A namespaced key in the OS keyring. Construct via the helpers above.
pub struct Key {
    service: &'static str,
    account: String,
}

#[cfg(not(test))]
pub fn store(key: &Key, value: &str) -> Result<()> {
    let entry = keyring::Entry::new(key.service, &key.account)
        .map_err(|e| Error::io("OS keyring", format!("entry: {e}")))?;
    entry
        .set_password(value)
        .map_err(|e| Error::io("OS keyring", format!("set: {e}")))?;
    Ok(())
}

#[cfg(not(test))]
pub fn retrieve(key: &Key) -> Result<Option<String>> {
    let entry = keyring::Entry::new(key.service, &key.account)
        .map_err(|e| Error::io("OS keyring", format!("entry: {e}")))?;
    match entry.get_password() {
        Ok(s) => Ok(Some(s)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(Error::io("OS keyring", format!("get: {e}"))),
    }
}

#[cfg(not(test))]
pub fn delete(key: &Key) -> Result<()> {
    let entry = keyring::Entry::new(key.service, &key.account)
        .map_err(|e| Error::io("OS keyring", format!("entry: {e}")))?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(Error::io("OS keyring", format!("delete: {e}"))),
    }
}

// ----- test-only in-memory backend -----

#[cfg(test)]
mod test_backend {
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
