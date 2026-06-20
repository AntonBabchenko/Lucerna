//! Account management. v0.2.0 added the multi-account schema (N offline
//! accounts with `active_id` selection) replacing the v0.1.0 single-account
//! flat file. Microsoft auth was implemented and then deferred — the work
//! is preserved under git tag `v0.2.0-msauth-attempt` for future revival.

pub mod keychain;
pub mod microsoft;
pub mod offline;
pub mod offline_name;
pub mod ops;
pub mod skins;
pub mod store;

use crate::error::{Error, Result};
use crate::paths::account_file;
use store::{read_account_file, write_account_file};

pub use store::{upsert_microsoft_account, Account, AccountKind};

/// Current Unix time in fractional seconds; `0.0` if the system clock is
/// before the epoch (unreachable in practice). Shared by the MS-auth
/// token-expiry math and the skin-cache freshness check.
pub(crate) fn now_secs() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// List every stored account. Returns an empty vec for a fresh install.
pub fn list_accounts(app: &tauri::AppHandle) -> Result<Vec<Account>> {
    let path = account_file(app).map_err(|e| Error::io("<app data dir>/account.json", e))?;
    Ok(read_account_file(&path)?.accounts)
}

/// The currently active account, or `None` if no account is set as active.
/// `remove_account` auto-picks the next as active, so this `None` arm is
/// mostly the empty-install case.
pub fn get_active_account(app: &tauri::AppHandle) -> Result<Option<Account>> {
    let path = account_file(app).map_err(|e| Error::io("<app data dir>/account.json", e))?;
    let file = read_account_file(&path)?;
    let Some(active_id) = file.active_id else {
        return Ok(None);
    };
    Ok(file.accounts.into_iter().find(|a| a.id == active_id))
}

/// Set the active account by id. `Err(Error::AccountNotSet)` if no such id exists.
pub fn set_active_account(app: &tauri::AppHandle, id: &str) -> Result<()> {
    let path = account_file(app).map_err(|e| Error::io("<app data dir>/account.json", e))?;
    let mut file = read_account_file(&path)?;
    ops::set_active(&mut file, id)?;
    write_account_file(&path, &file)
}

/// Remove the account with the given id. If the removed account was active,
/// the next account in the list becomes active; if no accounts remain,
/// `active_id` becomes `None`.
pub fn remove_account(app: &tauri::AppHandle, id: &str) -> Result<()> {
    let path = account_file(app).map_err(|e| Error::io("<app data dir>/account.json", e))?;
    let mut file = read_account_file(&path)?;
    ops::remove(&mut file, id);
    write_account_file(&path, &file)?;

    // Best-effort keychain cleanup for Microsoft accounts. Don't surface
    // errors — the account is already gone from disk; orphan keychain
    // entries are harmless (overwritten by the next sign-in with the
    // same uuid → same id derivation).
    let _ = keychain::delete(&keychain::refresh_token_key(id));
    let _ = keychain::delete(&keychain::mc_access_key(id));
    Ok(())
}

/// Add an offline account. UUID is deterministically derived from `name`.
/// If an account with the same UUID already exists, returns the existing
/// entry (idempotent — same name → same uuid → same entry).
pub fn add_offline_account(app: &tauri::AppHandle, name: &str) -> Result<Account> {
    let path = account_file(app).map_err(|e| Error::io("<app data dir>/account.json", e))?;
    let mut file = read_account_file(&path)?;
    let account = ops::add_offline(&mut file, name)?;
    write_account_file(&path, &file)?;
    Ok(account)
}
