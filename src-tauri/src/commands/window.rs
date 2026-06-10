use super::*;

/// Resize the main window between the full launcher and the compact
/// launch-pad strip. Pure window-layer op; persisting the choice is the
/// caller's job (the frontend writes GeneralSettings.compact_mode).
#[tauri::command]
#[specta::specta]
pub async fn window_set_compact(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::window::WindowSizeState>,
    compact: bool,
    content_height: Option<f64>,
) -> Result<(), crate::error::Error> {
    crate::window::set_compact(&app, compact, content_height, &state)
}

#[tauri::command]
#[specta::specta]
pub fn greet(name: String) -> Greeting {
    Greeting {
        message: format!("Hello, {name}! — Lucerna is alive."),
    }
}
