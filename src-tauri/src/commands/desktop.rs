// =========================================================================
// Desktop integration (Session 4): inbound launch intents and desktop
// launch shortcuts. (`lucerna://` scheme registration was retired — see
// `crate::url_scheme_retire`.)
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
    let intent = app.state::<crate::cli::PendingIntent>().take()?;
    // A shortcut's `--launch` token is a `uid` (rename-proof); older shortcuts
    // and the frontend's own calls use the directory name. Resolve either to the
    // CURRENT directory name here, so everything downstream keeps speaking one
    // language. An unresolvable token is passed through untouched: the launch
    // command then reports `InstanceNotFound` for it, which is the honest
    // outcome — far better than the old behaviour of silently creating and
    // launching an empty instance directory.
    Some(match intent {
        crate::cli::LaunchIntent::Launch {
            instance,
            quick_play,
        } => crate::cli::LaunchIntent::Launch {
            instance: crate::instances::resolve_launch_target(&app, &instance).unwrap_or(instance),
            quick_play,
        },
        other => other,
    })
}

/// Whether this OS supports desktop shortcuts. The UI hides the entry point when
/// it does not, rather than offering a button that can only ever fail.
#[tauri::command]
#[specta::specta]
pub fn shortcut_supported() -> bool {
    crate::shortcuts::supported()
}

/// Create a desktop shortcut that launches `target` in one click. Returns the
/// created file's path so the UI can show it and offer "open folder".
#[tauri::command]
#[specta::specta]
pub fn shortcut_create(
    app: tauri::AppHandle,
    target: crate::shortcuts::ShortcutTarget,
    label: String,
) -> crate::error::Result<String> {
    crate::data_root::reject_if_fallen_back(&app)?;
    crate::shortcuts::create(&app, &target, &label)
}

/// Default shortcut file name for a target, so the dialog's name field starts
/// pre-filled with the same value the backend would pick.
#[tauri::command]
#[specta::specta]
pub fn shortcut_default_name(
    instance_name: String,
    target: crate::shortcuts::ShortcutTarget,
) -> String {
    crate::shortcuts::default_file_stem(&instance_name, &target)
}
