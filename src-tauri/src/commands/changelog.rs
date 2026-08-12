// Post-update "What's new" support commands. The changelog itself is rendered
// entirely on the frontend from the embedded CHANGELOG.md; the backend only
// (a) reports the running version so the UI can tell an update happened, and
// (b) persists which version's changelog the user has already been shown so the
// prompt fires once per version. `changelog_mark_seen` mirrors `update_dismiss`
// — a read-modify-write of app.json that leaves everything else untouched.

/// The running launcher version (compile-time `CARGO_PKG_VERSION`), the same
/// source the updater and the CHANGELOG headings use. Infallible.
#[tauri::command]
#[specta::specta]
pub async fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Persist that the user has been shown the post-update changelog for
/// `version`, so the "What's new" prompt is not shown again for it.
#[tauri::command]
#[specta::specta]
pub async fn changelog_mark_seen(
    app: tauri::AppHandle,
    version: String,
) -> crate::error::Result<()> {
    let path =
        crate::paths::app_file(&app).map_err(|e| crate::error::Error::io("<app_file>", e))?;
    let mut current = crate::instances::store::read_app_json(&path)?;
    current.changelog_seen_version = Some(version);
    crate::instances::store::write_app_json(&path, &current)
}
