use super::*;
use crate::accounts::cosmetics as cos;
use crate::accounts::microsoft::cosmetics as mc_cos;
use crate::accounts::microsoft::mc_services::{self, SkinVariant};
use crate::error::Error;

/// One cape a user owns, prepared for the picker grid.
#[derive(Debug, Serialize, Type)]
pub struct CapeInfo {
    pub id: String,
    pub alias: Option<String>,
    pub texture_url: String,
    pub is_active: bool,
}

/// Cosmetics snapshot the modal loads on open.
#[derive(Debug, Serialize, Type)]
pub struct Cosmetics {
    pub capes: Vec<CapeInfo>,
    pub active_variant: SkinVariant,
}

fn account_uuid(app: &tauri::AppHandle, account_id: &str) -> Option<String> {
    let path = crate::paths::account_file(app).ok()?;
    let file = crate::accounts::store::read_account_file(&path).ok()?;
    file.accounts
        .iter()
        .find(|a| a.id == account_id)
        .map(|a| a.uuid.clone())
}

/// Load owned capes + the active skin variant for an account.
#[tauri::command]
#[specta::specta]
pub async fn get_cosmetics(app: tauri::AppHandle, account_id: String) -> Result<Cosmetics, Error> {
    let token = cos::ensure_fresh_token(&app, &account_id).await?;
    let profile = mc_services::fetch_profile(&token).await?;
    let capes = profile
        .capes
        .iter()
        .map(|c| CapeInfo {
            id: c.id.clone(),
            alias: c.alias.clone(),
            texture_url: c.url.clone(),
            is_active: c.state.eq_ignore_ascii_case("active"),
        })
        .collect();
    let active_variant = profile
        .skins
        .iter()
        .find(|s| s.state.eq_ignore_ascii_case("active"))
        .and_then(|s| s.variant.as_deref())
        .map(|v| {
            if v.eq_ignore_ascii_case("slim") {
                SkinVariant::Slim
            } else {
                SkinVariant::Classic
            }
        })
        .unwrap_or(SkinVariant::Classic);
    Ok(Cosmetics {
        capes,
        active_variant,
    })
}

/// Cache-first cape texture as base64 PNG (None on cosmetic miss).
#[tauri::command]
#[specta::specta]
pub async fn cape_texture(app: tauri::AppHandle, url: String) -> Result<Option<String>, Error> {
    Ok(cos::get_cape_texture(&app, &url).await)
}

#[tauri::command]
#[specta::specta]
pub async fn set_active_cape(
    app: tauri::AppHandle,
    account_id: String,
    cape_id: String,
) -> Result<(), Error> {
    let token = cos::ensure_fresh_token(&app, &account_id).await?;
    mc_cos::set_active_cape(&token, &cape_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn clear_active_cape(app: tauri::AppHandle, account_id: String) -> Result<(), Error> {
    let token = cos::ensure_fresh_token(&app, &account_id).await?;
    mc_cos::clear_active_cape(&token).await
}

/// Upload a skin. `png_base64` is validated (PNG, 64×64/64×32) before it leaves
/// the machine. On success the avatar skin cache is force-refreshed so the
/// sidebar head updates.
#[tauri::command]
#[specta::specta]
pub async fn upload_skin(
    app: tauri::AppHandle,
    account_id: String,
    png_base64: String,
    variant: SkinVariant,
) -> Result<(), Error> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(png_base64.as_bytes())
        .map_err(|e| Error::CosmeticImageInvalid {
            details: format!("bad base64: {e}"),
        })?;
    cos::validate_skin_png(&bytes)?;
    let token = cos::ensure_fresh_token(&app, &account_id).await?;
    mc_cos::upload_skin(&token, &bytes, variant).await?;
    if let Some(uuid) = account_uuid(&app, &account_id) {
        let _ = crate::accounts::skins::get_account_skin(&app, &uuid, true).await;
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn reset_skin(app: tauri::AppHandle, account_id: String) -> Result<(), Error> {
    let token = cos::ensure_fresh_token(&app, &account_id).await?;
    mc_cos::reset_skin(&token).await?;
    if let Some(uuid) = account_uuid(&app, &account_id) {
        let _ = crate::accounts::skins::get_account_skin(&app, &uuid, true).await;
    }
    Ok(())
}
