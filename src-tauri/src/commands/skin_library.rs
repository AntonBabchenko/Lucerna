use super::*;
use crate::accounts::microsoft::mc_services::SkinVariant;
use crate::accounts::skin_library as skin_lib;
use crate::error::Error;

/// One saved skin, PNG payload included so the grid renders in one round-trip.
#[derive(Debug, Serialize, Type)]
pub struct SkinLibraryItem {
    pub id: String,
    pub name: String,
    pub variant: SkinVariant,
    pub cape_id: Option<String>,
    pub created_at: f64,
    pub skin_png_base64: String,
}

fn lib_dir(app: &tauri::AppHandle) -> Result<std::path::PathBuf, Error> {
    crate::paths::skin_library_dir(app).map_err(|e| Error::io("<skin_library dir>", e))
}

fn to_item(entry: skin_lib::SkinLibraryEntry, skin_png_base64: String) -> SkinLibraryItem {
    SkinLibraryItem {
        id: entry.id,
        name: entry.name,
        variant: entry.variant,
        cape_id: entry.cape_id,
        created_at: entry.created_at,
        skin_png_base64,
    }
}

/// All saved skins, stored order.
#[tauri::command]
#[specta::specta]
pub async fn skin_library_list(app: tauri::AppHandle) -> Result<Vec<SkinLibraryItem>, Error> {
    Ok(skin_lib::list(&lib_dir(&app)?)
        .into_iter()
        .map(|(e, b64)| to_item(e, b64))
        .collect())
}

/// Save a new entry. The PNG is validated (64×64/64×32) before it hits disk.
#[tauri::command]
#[specta::specta]
pub async fn skin_library_save(
    app: tauri::AppHandle,
    name: String,
    variant: SkinVariant,
    cape_id: Option<String>,
    png_base64: String,
) -> Result<SkinLibraryItem, Error> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(png_base64.as_bytes())
        .map_err(|e| Error::CosmeticImageInvalid {
            details: format!("bad base64: {e}"),
        })?;
    let entry = skin_lib::save(
        &lib_dir(&app)?,
        &name,
        variant,
        cape_id,
        &bytes,
        crate::accounts::now_secs(),
    )?;
    Ok(to_item(entry, png_base64))
}

/// Edit an entry's metadata (name / variant / cape). The PNG never changes.
#[tauri::command]
#[specta::specta]
pub async fn skin_library_update(
    app: tauri::AppHandle,
    id: String,
    name: String,
    variant: SkinVariant,
    cape_id: Option<String>,
) -> Result<(), Error> {
    skin_lib::update(&lib_dir(&app)?, &id, &name, variant, cape_id)
}

/// Delete an entry and its PNG file.
#[tauri::command]
#[specta::specta]
pub async fn skin_library_delete(app: tauri::AppHandle, id: String) -> Result<(), Error> {
    skin_lib::delete(&lib_dir(&app)?, &id)
}
