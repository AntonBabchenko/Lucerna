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

#[tauri::command]
#[specta::specta]
pub fn screenshot_thumbnail(
    app: tauri::AppHandle,
    instance_id: String,
    file_name: String,
) -> Result<String, crate::error::Error> {
    let path = crate::screenshots::screenshot_path(&app, &instance_id, &file_name)?;
    crate::screenshots::thumb::render_data_url(&app, &path, crate::screenshots::thumb::Tier::Thumb)
}

#[tauri::command]
#[specta::specta]
pub fn screenshot_preview(
    app: tauri::AppHandle,
    instance_id: String,
    file_name: String,
) -> Result<String, crate::error::Error> {
    let path = crate::screenshots::screenshot_path(&app, &instance_id, &file_name)?;
    crate::screenshots::thumb::render_data_url(
        &app,
        &path,
        crate::screenshots::thumb::Tier::Preview,
    )
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

#[tauri::command]
#[specta::specta]
pub fn save_screenshot_copy(
    app: tauri::AppHandle,
    instance_id: String,
    file_name: String,
    dest: String,
) -> Result<(), crate::error::Error> {
    crate::screenshots::save_screenshot_copy(&app, &instance_id, &file_name, &dest)
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
pub fn copy_screenshot_to_clipboard(
    app: tauri::AppHandle,
    instance_id: String,
    file_name: String,
) -> Result<(), crate::error::Error> {
    let path = crate::screenshots::screenshot_path(&app, &instance_id, &file_name)?;
    let image = crate::screenshots::thumb::clipboard_image(&path)?;
    app.clipboard()
        .write_image(&image)
        .map_err(|e| crate::error::Error::io("<clipboard>", e.to_string()))
}

#[tauri::command]
#[specta::specta]
pub fn save_annotated_screenshot(
    app: tauri::AppHandle,
    instance_id: String,
    file_name: String,
    overlay_png_base64: String,
    crop: Option<crate::screenshots::CropFrac>,
    dest: String,
) -> Result<(), crate::error::Error> {
    crate::screenshots::save_annotated(
        &app,
        &instance_id,
        &file_name,
        &overlay_png_base64,
        crop,
        &dest,
    )
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
