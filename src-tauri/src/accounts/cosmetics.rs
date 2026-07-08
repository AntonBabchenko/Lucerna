//! Cosmetics orchestration: guarantee a fresh MC access token before any
//! cape/skin operation, cache cape textures on disk (content-addressed, no
//! TTL), and validate uploaded skins before they leave the machine.

use crate::error::{Error, Result};
use base64::Engine;

/// Refresh margin. A cape/skin call takes well under a second, so a short
/// margin avoids needless refreshes while never using an about-to-expire token.
const FRESH_MARGIN_SECS: f64 = 120.0;

/// Return a valid MC access token for `account_id`, silently refreshing via the
/// stored refresh token when `expires_at` is within the margin (or unknown).
pub async fn ensure_fresh_token(app: &tauri::AppHandle, account_id: &str) -> Result<String> {
    let path = crate::paths::account_file(app).map_err(|e| Error::io("<account.json>", e))?;
    let file = crate::accounts::store::read_account_file(&path)?;
    let acc = file
        .accounts
        .iter()
        .find(|a| a.id == account_id)
        .ok_or_else(|| Error::AuthFailed {
            stage: "cosmetics".into(),
            details: format!("account {account_id} not found"),
        })?;
    let stale = acc
        .expires_at
        .map(|e| e <= crate::accounts::now_secs() + FRESH_MARGIN_SECS)
        .unwrap_or(true);
    if stale {
        crate::accounts::microsoft::refresh(app, account_id).await?;
    }
    crate::accounts::keychain::retrieve(&crate::accounts::keychain::mc_access_key(account_id))?
        .ok_or_else(|| Error::AuthFailed {
            stage: "cosmetics".into(),
            details: "no MC access token in keyring".into(),
        })
}

/// Texture hash = last path segment of a `textures.minecraft.net/texture/<hash>`
/// URL. `None` for an empty/degenerate URL.
fn cape_hash(url: &str) -> Option<String> {
    let seg = url.rsplit('/').next().unwrap_or("");
    if seg.is_empty() {
        None
    } else {
        Some(seg.to_string())
    }
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Cache-first cape texture. Returns base64 PNG, or `None` on any miss/error
/// (cape previews are cosmetic; the UI falls back to a placeholder tile).
pub async fn get_cape_texture(app: &tauri::AppHandle, url: &str) -> Option<String> {
    let hash = cape_hash(url)?;
    let dir = crate::paths::capes_dir(app).ok()?;
    let file = dir.join(format!("{hash}.png"));
    if let Ok(bytes) = std::fs::read(&file) {
        return Some(b64(&bytes));
    }
    let resp = crate::network::request::get(url, &[], "account_cape")
        .await
        .ok()?;
    if !(200..300).contains(&resp.status) || resp.body.is_empty() {
        return None;
    }
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(&file, &resp.body);
    Some(b64(&resp.body))
}

/// Validate an uploaded skin: must decode as PNG and be 64×64 or 64×32.
pub fn validate_skin_png(bytes: &[u8]) -> Result<()> {
    use image::GenericImageView;
    let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Png).map_err(|e| {
        Error::CosmeticImageInvalid {
            details: format!("not a valid PNG: {e}"),
        }
    })?;
    let (w, h) = img.dimensions();
    if (w, h) == (64, 64) || (w, h) == (64, 32) {
        Ok(())
    } else {
        Err(Error::CosmeticImageInvalid {
            details: format!("skin must be 64x64 or 64x32, got {w}x{h}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_of(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::new(w, h);
        let mut buf = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut buf, image::ImageFormat::Png)
            .unwrap();
        buf.into_inner()
    }

    #[test]
    fn validate_accepts_64x64_and_64x32() {
        assert!(validate_skin_png(&png_of(64, 64)).is_ok());
        assert!(validate_skin_png(&png_of(64, 32)).is_ok());
    }

    #[test]
    fn validate_rejects_wrong_size() {
        let err = validate_skin_png(&png_of(32, 32)).unwrap_err();
        assert!(matches!(
            err,
            crate::error::Error::CosmeticImageInvalid { .. }
        ));
    }

    #[test]
    fn validate_rejects_non_png() {
        let err = validate_skin_png(b"not a png").unwrap_err();
        assert!(matches!(
            err,
            crate::error::Error::CosmeticImageInvalid { .. }
        ));
    }

    #[test]
    fn cape_cache_hash_is_last_url_segment() {
        assert_eq!(
            cape_hash("https://textures.minecraft.net/texture/abc123def"),
            Some("abc123def".to_string())
        );
        assert_eq!(cape_hash(""), None);
    }
}
