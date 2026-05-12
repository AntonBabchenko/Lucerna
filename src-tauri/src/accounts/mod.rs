//! Account management: offline (slice 2) and Microsoft (v0.2.0).

pub mod offline;
pub mod store;

use crate::error::Result;
use crate::paths::account_file;

pub use offline::derive_offline_uuid;
pub use store::{read_account, write_account, Account};

/// Read the current account from disk, if one is set.
pub fn get_current(app: &tauri::AppHandle) -> Result<Option<Account>> {
    let path = account_file(app)
        .map_err(|e| crate::error::Error::io("<app data dir>/account.json", e))?;
    read_account(&path)
}

/// Persist an offline account with the given display name.
/// The UUID is derived deterministically — re-setting the same name
/// re-derives the same UUID.
pub fn set_offline(app: &tauri::AppHandle, name: &str) -> Result<Account> {
    let path = account_file(app)
        .map_err(|e| crate::error::Error::io("<app data dir>/account.json", e))?;
    let account = Account {
        name: name.to_string(),
        uuid: derive_offline_uuid(name).to_string(),
    };
    write_account(&path, &account)?;
    Ok(account)
}
