//! Tauri command wrappers for the screenshots module.

use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_opener::OpenerExt;

#[tauri::command]
#[specta::specta]
pub fn list_screenshots(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<Vec<crate::screenshots::Screenshot>, crate::error::Error> {
    crate::screenshots::list_screenshots(&app, &instance_id)
}

#[tauri::command]
#[specta::specta]
pub fn list_all_screenshots(
    app: tauri::AppHandle,
) -> Result<Vec<crate::screenshots::Screenshot>, crate::error::Error> {
    crate::screenshots::list_all_screenshots(&app)
}

// Thumbnail/preview/clipboard commands are `async` + `spawn_blocking`: on a
// cache miss they PNG-decode a full-resolution (up to 4K) screenshot, resize,
// and re-encode — 100-300 ms of pure CPU that a sync command would spend on
// the main thread. A gallery's first open fires dozens of these at once.

#[tauri::command]
#[specta::specta]
pub async fn screenshot_thumbnail(
    app: tauri::AppHandle,
    instance_id: String,
    file_name: String,
) -> Result<String, crate::error::Error> {
    let path = crate::screenshots::screenshot_path(&app, &instance_id, &file_name)?;
    tokio::task::spawn_blocking(move || {
        crate::screenshots::thumb::render_data_url(
            &app,
            &path,
            crate::screenshots::thumb::Tier::Thumb,
        )
    })
    .await
    .map_err(|e| crate::error::Error::io("<screenshot_thumbnail>", format!("join: {e}")))?
}

#[tauri::command]
#[specta::specta]
pub async fn screenshot_preview(
    app: tauri::AppHandle,
    instance_id: String,
    file_name: String,
) -> Result<String, crate::error::Error> {
    let path = crate::screenshots::screenshot_path(&app, &instance_id, &file_name)?;
    tokio::task::spawn_blocking(move || {
        crate::screenshots::thumb::render_data_url(
            &app,
            &path,
            crate::screenshots::thumb::Tier::Preview,
        )
    })
    .await
    .map_err(|e| crate::error::Error::io("<screenshot_preview>", format!("join: {e}")))?
}

#[tauri::command]
#[specta::specta]
pub fn delete_screenshot(
    app: tauri::AppHandle,
    instance_id: String,
    file_name: String,
) -> Result<(), crate::error::Error> {
    crate::screenshots::delete_screenshot(&app, &instance_id, &file_name)
}

/// Copy a screenshot to a destination the user picked in the save dialog.
///
/// Refuses a destination inside the Lucerna program directory, data root, or
/// OS-default app-data dir, except the instance's own `screenshots/` folder —
/// that folder is under the data root on every install (and under the program
/// directory too on a portable one, where the root lives beside the
/// executable), and it is where the launcher's own default save path points.
// Sync command = main thread: `fs::copy` of a full-size PNG (several MB,
// possibly onto a slow or removable destination the user picked in the save
// dialog) blocks the window for the whole write. Same shape as
// `screenshot_thumbnail`: `async` with the body on `spawn_blocking`.
#[tauri::command]
#[specta::specta]
pub async fn save_screenshot_copy(
    app: tauri::AppHandle,
    instance_id: String,
    file_name: String,
    dest: String,
) -> Result<(), crate::error::Error> {
    let shots = crate::screenshots::screenshots_dir(&app, &instance_id)?;
    crate::pathsafe::validate_export_dest(&app, std::path::Path::new(&dest), Some(&shots))?;
    tokio::task::spawn_blocking(move || {
        crate::screenshots::save_screenshot_copy(&app, &instance_id, &file_name, &dest)
    })
    .await
    .map_err(|e| crate::error::Error::io("<save_screenshot_copy>", format!("join: {e}")))?
}

#[tauri::command]
#[specta::specta]
pub async fn reveal_screenshot(
    app: tauri::AppHandle,
    instance_id: String,
    file_name: String,
) -> Result<(), crate::error::Error> {
    let path = crate::screenshots::screenshot_path(&app, &instance_id, &file_name)?;
    app.opener()
        .reveal_item_in_dir(&path)
        .map_err(|e| crate::error::Error::io(path.display().to_string(), format!("opener: {e}")))
}

#[tauri::command]
#[specta::specta]
pub async fn open_screenshots_folder(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<(), crate::error::Error> {
    let dir = crate::screenshots::screenshots_dir(&app, &instance_id)?;
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| crate::error::Error::io(dir.display().to_string(), e))?;
    app.opener()
        .open_path(dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| crate::error::Error::io(dir.display().to_string(), format!("opener: {e}")))?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn copy_screenshot_to_clipboard(
    app: tauri::AppHandle,
    instance_id: String,
    file_name: String,
) -> Result<(), crate::error::Error> {
    let path = crate::screenshots::screenshot_path(&app, &instance_id, &file_name)?;
    // Full-image decode off the main thread; the clipboard write itself is cheap.
    let image =
        tokio::task::spawn_blocking(move || crate::screenshots::thumb::clipboard_image(&path))
            .await
            .map_err(|e| crate::error::Error::io("<clipboard>", format!("join: {e}")))??;
    app.clipboard()
        .write_image(&image)
        .map_err(|e| crate::error::Error::io("<clipboard>", e.to_string()))
}

/// Composite the annotation overlay onto the screenshot and write the result
/// to a destination the user picked in the save dialog.
///
/// Refuses a destination inside the Lucerna program directory, data root, or
/// OS-default app-data dir, except the instance's own `screenshots/` folder —
/// `annotated_default_path` proposes a path in exactly that folder, which is
/// under the data root on every install and inside the program directory too
/// on a portable one.
// Sync command = main thread (see the thumbnail comment above): this one
// PNG-decodes the full-resolution original, composites the overlay, crops,
// and re-encodes — seconds of pure CPU on a 4K screenshot. Same shape as
// `screenshot_thumbnail`: `async` with the whole body on `spawn_blocking`.
#[tauri::command]
#[specta::specta]
pub async fn save_annotated_screenshot(
    app: tauri::AppHandle,
    instance_id: String,
    file_name: String,
    overlay_png_base64: String,
    crop: Option<crate::screenshots::CropFrac>,
    dest: String,
) -> Result<(), crate::error::Error> {
    let shots = crate::screenshots::screenshots_dir(&app, &instance_id)?;
    crate::pathsafe::validate_export_dest(&app, std::path::Path::new(&dest), Some(&shots))?;
    tokio::task::spawn_blocking(move || {
        crate::screenshots::save_annotated(
            &app,
            &instance_id,
            &file_name,
            &overlay_png_base64,
            crop,
            &dest,
        )
    })
    .await
    .map_err(|e| crate::error::Error::io("<save_annotated_screenshot>", format!("join: {e}")))?
}

#[tauri::command]
#[specta::specta]
pub fn annotated_default_path(
    app: tauri::AppHandle,
    instance_id: String,
    file_name: String,
) -> Result<String, crate::error::Error> {
    crate::screenshots::annotated_default_path(&app, &instance_id, &file_name)
}
