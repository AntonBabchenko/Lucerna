//! Cosmetics orchestration: guarantee a fresh MC access token before any
//! cape/skin operation, cache cape textures on disk (content-addressed, no
//! TTL), and validate uploaded skins before they leave the machine.

use crate::error::{Error, Result};
use base64::Engine;

/// Refresh margin. A cape/skin call takes well under a second, so a short
/// margin avoids needless refreshes while never using an about-to-expire token.
const FRESH_MARGIN_SECS: f64 = 120.0;

/// True when the token is missing an expiry or expires within the margin.
fn is_token_stale(expires_at: Option<f64>, now: f64) -> bool {
    expires_at
        .map(|e| e <= now + FRESH_MARGIN_SECS)
        .unwrap_or(true)
}

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
    let stale = is_token_stale(acc.expires_at, crate::accounts::now_secs());
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
    let dir = crate::paths::capes_dir(app).ok()?;
    fetch_or_cache_cape(&dir, url).await
}

/// Disk-cache core: hit reads base64 from `<dir>/<hash>.png`; miss fetches via
/// the network chokepoint, rejects non-2xx/empty (writing nothing), else caches
/// and returns. No TTL — cape URLs are content-addressed.
async fn fetch_or_cache_cape(dir: &std::path::Path, url: &str) -> Option<String> {
    let hash = cape_hash(url)?;
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
    let _ = std::fs::create_dir_all(dir);
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
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

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

    #[test]
    fn token_stale_within_margin() {
        // now=1000, margin=120 -> threshold 1120; expiry 1050 <= 1120 => stale
        assert!(is_token_stale(Some(1050.0), 1000.0));
    }
    #[test]
    fn token_fresh_beyond_margin() {
        assert!(!is_token_stale(Some(2000.0), 1000.0));
    }
    #[test]
    fn token_stale_exactly_at_boundary() {
        assert!(is_token_stale(Some(1120.0), 1000.0));
    }
    #[test]
    fn token_stale_when_no_expiry() {
        assert!(is_token_stale(None, 1000.0));
    }

    #[tokio::test]
    async fn cape_cache_miss_then_hit_fetches_once() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/texture/abc"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![1u8, 2, 3, 4]))
            .expect(1)
            .mount(&server)
            .await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let dir = tempfile::tempdir().unwrap();
        let url = format!("{}/texture/abc", server.uri());
        let first = fetch_or_cache_cape(dir.path(), &url).await.unwrap();
        let second = fetch_or_cache_cape(dir.path(), &url).await.unwrap();
        assert_eq!(first, second);
        assert!(dir.path().join("abc.png").exists());
    }

    #[tokio::test]
    async fn cape_cache_404_returns_none_and_writes_nothing() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/texture/missing"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let dir = tempfile::tempdir().unwrap();
        let url = format!("{}/texture/missing", server.uri());
        assert!(fetch_or_cache_cape(dir.path(), &url).await.is_none());
        assert!(!dir.path().join("missing.png").exists());
    }
}
