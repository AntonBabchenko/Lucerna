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

/// The close dialog's words for the native fallback, in the interface
/// language. Sent with the tray labels; English until they arrive.
#[tauri::command]
#[specta::specta]
pub async fn close_set_labels(labels: crate::close::CloseLabels) {
    crate::close::set_labels(labels);
}

/// "My dialog for this ask is on screen." False = superseded, or the native
/// dialog already took over: the frontend closes its modal.
#[tauri::command]
#[specta::specta]
pub async fn app_close_ask_shown(generation: u32) -> bool {
    crate::close::ask_shown(generation)
}

/// The user cancelled the close. The question is over, so the scheduled
/// hide-to-tray may hide the window again.
#[tauri::command]
#[specta::specta]
pub async fn app_cancel_close(generation: u32) {
    crate::close::cancel(generation);
}

/// The user chose to close. Re-checks before exiting: anything that appeared
/// while the dialog was open is named in a new ask rather than killed unseen.
#[tauri::command]
#[specta::specta]
pub async fn app_confirm_close(
    app: tauri::AppHandle,
    generation: u32,
    shown: crate::data_root::blockers::CloseLosses,
) -> Result<(), crate::error::Error> {
    crate::close::confirm(&app, generation, shown).await
}

#[tauri::command]
#[specta::specta]
pub fn greet(name: String) -> Greeting {
    Greeting {
        message: format!("Hello, {name}! — Lucerna is alive."),
    }
}
