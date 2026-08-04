//! Tauri command wrappers for OS clipboard access.

use tauri_plugin_clipboard_manager::ClipboardExt;

/// Read the OS clipboard as text, for the right-click Paste item.
///
/// Goes through the Rust plugin rather than `navigator.clipboard.readText()`
/// because the three engines Lucerna ships on — WebView2, WebKitGTK and
/// WKWebView — do not agree on whether a custom-scheme origin may read the
/// clipboard without a permission prompt. A refusal would be silent and
/// platform-specific, i.e. found by a user rather than by CI. The plugin is
/// already a dependency (`write_image` backs the screenshot copy), so this
/// adds no crate and no JS package.
///
/// Reading happens only in direct response to the user picking Paste — never
/// on a timer, on focus, or at startup.
#[tauri::command]
#[specta::specta]
pub fn clipboard_read_text(app: tauri::AppHandle) -> Result<String, crate::error::Error> {
    app.clipboard()
        .read_text()
        .map_err(|e| crate::error::Error::io("<clipboard>", e.to_string()))
}
