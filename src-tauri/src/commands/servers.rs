/// List the instance's saved multiplayer servers (from `servers.dat`).
/// Empty Vec when the file does not exist yet. Async + `spawn_blocking`: a
/// file read, off the main thread.
#[tauri::command]
#[specta::specta]
pub async fn list_saved_servers(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<Vec<crate::servers::SavedServer>, crate::error::Error> {
    tokio::task::spawn_blocking(move || crate::servers::list_saved_servers(&app, &instance_id))
        .await
        .map_err(|e| crate::error::Error::io("<list_saved_servers>", format!("join: {e}")))?
}

/// Append a server to `servers.dat`. Rejected while the game is running.
/// On an instance that shares its server list, the shared list is updated in
/// the same call (the settings-sync point runs after the write).
#[tauri::command]
#[specta::specta]
pub async fn add_saved_server(
    app: tauri::AppHandle,
    instance_id: String,
    name: String,
    address: String,
) -> Result<(), crate::error::Error> {
    tokio::task::spawn_blocking(move || {
        crate::servers::add_saved_server(&app, &instance_id, &name, &address)
    })
    .await
    .map_err(|e| crate::error::Error::io("<add_saved_server>", format!("join: {e}")))?
}

/// Remove the server at `index`, guarded by `expected_address`. Rejected
/// while the game is running; a changed list yields `SavedServerListChanged`.
/// Updates the shared list like [`add_saved_server`].
#[tauri::command]
#[specta::specta]
pub async fn remove_saved_server(
    app: tauri::AppHandle,
    instance_id: String,
    index: u32,
    expected_address: String,
) -> Result<(), crate::error::Error> {
    tokio::task::spawn_blocking(move || {
        crate::servers::remove_saved_server(&app, &instance_id, index as usize, &expected_address)
    })
    .await
    .map_err(|e| crate::error::Error::io("<remove_saved_server>", format!("join: {e}")))?
}

/// Ping one saved server for its status (players / version / response time).
///
/// Requires the `allow_server_ping` permission (Settings → Game, off by
/// default): without it this returns `ConsentedChannelDisabled` and no packet
/// is sent. A server that does not answer yields `NoAnswer`, never an error.
#[tauri::command]
#[specta::specta]
pub async fn ping_server(
    app: tauri::AppHandle,
    address: String,
) -> Result<crate::servers::ping::ServerPingOutcome, crate::error::Error> {
    crate::servers::ping::ping_address(&app, &address).await
}
