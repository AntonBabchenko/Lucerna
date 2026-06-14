use super::*;

/// List all stored accounts.
#[tauri::command]
#[specta::specta]
pub fn list_accounts(
    app: tauri::AppHandle,
) -> Result<Vec<crate::accounts::store::Account>, crate::error::Error> {
    crate::accounts::list_accounts(&app)
}

/// Currently active account, or None if no account is set.
#[tauri::command]
#[specta::specta]
pub fn get_active_account(
    app: tauri::AppHandle,
) -> Result<Option<crate::accounts::store::Account>, crate::error::Error> {
    crate::accounts::get_active_account(&app)
}

/// Set the active account by id. Errors AccountNotSet if id is unknown.
#[tauri::command]
#[specta::specta]
pub fn set_active_account(app: tauri::AppHandle, id: String) -> Result<(), crate::error::Error> {
    crate::accounts::set_active_account(&app, &id)
}

/// Remove an account. If it was active, the next account becomes active;
/// if none remain, active_id becomes None.
#[tauri::command]
#[specta::specta]
pub fn remove_account(app: tauri::AppHandle, id: String) -> Result<(), crate::error::Error> {
    crate::accounts::remove_account(&app, &id)
}

/// Add an offline account. Idempotent — same name produces same UUID.
#[tauri::command]
#[specta::specta]
pub fn add_offline_account(
    app: tauri::AppHandle,
    name: String,
) -> Result<crate::accounts::store::Account, crate::error::Error> {
    crate::accounts::add_offline_account(&app, &name)
}

/// Begin a Microsoft sign-in flow. Opens the OAuth browser, exchanges
/// the auth code for tokens, queries the Mojang API for a profile,
/// and persists the account. Returns the new Account.
#[tauri::command]
#[specta::specta]
pub async fn begin_microsoft_signin(
    app: tauri::AppHandle,
) -> crate::error::Result<crate::accounts::store::Account> {
    crate::accounts::microsoft::sign_in(&app).await
}

/// Refresh an existing Microsoft account by id. Queries the Mojang API
/// to validate/update the profile, and re-persists. Returns the updated Account.
#[tauri::command]
#[specta::specta]
pub async fn refresh_microsoft_account(
    app: tauri::AppHandle,
    id: String,
) -> crate::error::Result<crate::accounts::store::Account> {
    crate::accounts::microsoft::refresh(&app, &id).await
}

/// Resolve an account's skin (cache-first) by its Minecraft UUID. Returns
/// `None` when the account has no skin or it cannot be fetched — the UI
/// falls back to a letter avatar. Never errors on a cosmetic miss.
#[tauri::command]
#[specta::specta]
pub async fn account_skin(
    app: tauri::AppHandle,
    uuid: String,
) -> Result<Option<crate::accounts::skins::AccountSkin>, crate::error::Error> {
    Ok(crate::accounts::skins::get_account_skin(&app, &uuid, false).await)
}
