/// List the instance's saved multiplayer servers (from `servers.dat`).
/// Empty Vec when the file does not exist yet.
#[tauri::command]
#[specta::specta]
pub fn list_saved_servers(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<Vec<crate::servers::SavedServer>, crate::error::Error> {
    crate::servers::list_saved_servers(&app, &instance_id)
}

/// Append a server to `servers.dat`. Rejected while the game is running.
#[tauri::command]
#[specta::specta]
pub fn add_saved_server(
    app: tauri::AppHandle,
    instance_id: String,
    name: String,
    address: String,
) -> Result<(), crate::error::Error> {
    crate::servers::add_saved_server(&app, &instance_id, &name, &address)
}

/// Remove the server at `index`, guarded by `expected_address`. Rejected
/// while the game is running; a changed list yields `SavedServerListChanged`.
#[tauri::command]
#[specta::specta]
pub fn remove_saved_server(
    app: tauri::AppHandle,
    instance_id: String,
    index: u32,
    expected_address: String,
) -> Result<(), crate::error::Error> {
    crate::servers::remove_saved_server(&app, &instance_id, index as usize, &expected_address)
}
