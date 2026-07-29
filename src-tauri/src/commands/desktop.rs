// =========================================================================
// Desktop integration (Session 4): inbound launch intents, `lucerna://`
// scheme registration, and desktop launch shortcuts.
// =========================================================================

/// Drain the pending launch intent, if any.
///
/// Called by the frontend once on mount (covers a cold start — the OS spawned
/// us with a `lucerna://` URL or a shortcut's `--launch` in argv, whatever the
/// webview load order) and again on every `intent-pending` event (covers a
/// second launch whose argv the single-instance guard forwarded into the
/// running process). Take-once, so a mount drain and an event drain that race
/// cannot both act on the same intent.
#[tauri::command]
#[specta::specta]
pub fn take_pending_intent(app: tauri::AppHandle) -> Option<crate::cli::LaunchIntent> {
    use tauri::Manager;
    app.state::<crate::cli::PendingIntent>().take()
}
