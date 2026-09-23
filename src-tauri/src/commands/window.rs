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

/// Apply (and arm) the expanded window's min-height floor — the measured
/// sidebar content height. `hug` is true only for the one-shot startup
/// application (resize to the content height); later content-change
/// applications pass false (grow only when buttons would clip).
#[tauri::command]
#[specta::specta]
pub async fn window_set_expanded_floor(
    app: tauri::AppHandle,
    height: f64,
    hug: bool,
) -> Result<(), crate::error::Error> {
    crate::window::set_expanded_floor(&app, height, hug)
}

/// Store the tray menu's strings in the interface language. The tray is only
/// built while the window is hidden, and the language can only change while it
/// is shown, so the next build is always the one that needs them.
#[tauri::command]
#[specta::specta]
pub async fn tray_set_labels(labels: crate::tray::TrayLabels) {
    crate::tray::set_labels(labels);
}

#[tauri::command]
#[specta::specta]
pub fn greet(name: String) -> Greeting {
    Greeting {
        message: format!("Hello, {name}! — Lucerna is alive."),
    }
}
