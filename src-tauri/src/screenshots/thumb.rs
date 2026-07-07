//! Downscale screenshots to cached JPEG thumbnails/previews and return them as
//! `data:` URLs. Two tiers: THUMB for the grid, PREVIEW for the lightbox. The
//! cache lives in the app cache dir (disposable), keyed by source path + mtime,
//! so a re-saved screenshot regenerates automatically.

use crate::error::{Error, Result};
use base64::Engine;
use sha1::{Digest, Sha1};
use std::path::{Path, PathBuf};
use tauri::Manager;

/// Longest-edge caps and JPEG quality per tier.
const THUMB_EDGE: u32 = 320;
const THUMB_QUALITY: u8 = 80;
const PREVIEW_EDGE: u32 = 2560;
const PREVIEW_QUALITY: u8 = 88;
/// Clipboard copies are downscaled past this longest edge to bound the raw
/// RGBA buffer handed to the OS clipboard (an 8K screenshot is ~130 MB raw).
/// 4096 lets any 4K-native screenshot through untouched.
const CLIPBOARD_EDGE: u32 = 4096;

#[derive(Clone, Copy)]
pub enum Tier {
    Thumb,
    Preview,
}

impl Tier {
    fn params(self) -> (u32, u8, &'static str) {
        match self {
            Tier::Thumb => (THUMB_EDGE, THUMB_QUALITY, "t"),
            Tier::Preview => (PREVIEW_EDGE, PREVIEW_QUALITY, "p"),
        }
    }
}

/// `<app cache dir>/screenshots/`. Created lazily.
fn cache_dir(app: &tauri::AppHandle) -> Result<PathBuf> {
    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| Error::io("<cache_dir>", e))?
        .join("screenshots");
    std::fs::create_dir_all(&dir).map_err(|e| Error::io(dir.display().to_string(), e))?;
    Ok(dir)
}

/// Cache filename = sha1(source path | mtime | tier | edge).jpg.
fn cache_key(source: &Path, mtime_ms: u64, tier: Tier) -> String {
    let (edge, _q, tag) = tier.params();
    let mut h = Sha1::new();
    h.update(source.to_string_lossy().as_bytes());
    h.update(mtime_ms.to_le_bytes());
    h.update([tag.as_bytes()[0], (edge & 0xff) as u8, (edge >> 8) as u8]);
    format!("{}.jpg", hex::encode(h.finalize()))
}

/// Encode `img` as JPEG bytes at `quality`, RGB (screenshots are opaque).
fn encode_jpeg(img: &image::DynamicImage, quality: u8) -> Result<Vec<u8>> {
    let rgb = img.to_rgb8();
    let mut buf: Vec<u8> = Vec::new();
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality);
    enc.encode_image(&rgb)
        .map_err(|e| Error::io("<jpeg encode>", e))?;
    Ok(buf)
}

/// Downscale `path` to `tier`, using/refreshing the on-disk cache, and return a
/// `data:image/jpeg;base64,...` URL. Never upscales.
pub fn render_data_url(app: &tauri::AppHandle, path: &Path, tier: Tier) -> Result<String> {
    let mtime = super::fs::file_mtime_ms(path);
    let cache_path = cache_dir(app)?.join(cache_key(path, mtime, tier));

    let bytes = if let Ok(cached) = std::fs::read(&cache_path) {
        cached
    } else {
        let (edge, quality, _tag) = tier.params();
        let img = image::ImageReader::open(path)
            .map_err(|e| Error::io(path.display().to_string(), e))?
            .decode()
            .map_err(|e| Error::io(path.display().to_string(), e))?;
        let (w, h) = (img.width(), img.height());
        let resized = if w.max(h) > edge {
            img.thumbnail(edge, edge)
        } else {
            img
        };
        let jpeg = encode_jpeg(&resized, quality)?;
        let _ = std::fs::write(&cache_path, &jpeg);
        jpeg
    };

    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(format!("data:image/jpeg;base64,{b64}"))
}

/// Load the original image as a clipboard-ready `tauri::image::Image` (RGBA),
/// downscaled past `CLIPBOARD_EDGE` so very large screenshots don't spike memory.
pub fn clipboard_image(path: &Path) -> Result<tauri::image::Image<'static>> {
    let img = image::ImageReader::open(path)
        .map_err(|e| Error::io(path.display().to_string(), e))?
        .decode()
        .map_err(|e| Error::io(path.display().to_string(), e))?;
    let img = if img.width().max(img.height()) > CLIPBOARD_EDGE {
        img.thumbnail(CLIPBOARD_EDGE, CLIPBOARD_EDGE)
    } else {
        img
    };
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    Ok(tauri::image::Image::new_owned(rgba.into_raw(), w, h))
}

/// Best-effort removal of both cache tiers for a source file (e.g. after the
/// user deletes the screenshot). The cache is disposable — it lives in the OS
/// app-cache dir and is safe to lose — so failures here are ignored.
pub fn evict(app: &tauri::AppHandle, source: &Path, mtime_ms: u64) {
    let Ok(dir) = cache_dir(app) else {
        return;
    };
    for tier in [Tier::Thumb, Tier::Preview] {
        let _ = std::fs::remove_file(dir.join(cache_key(source, mtime_ms, tier)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn tiny_png() -> Vec<u8> {
        let mut img = image::RgbaImage::new(4, 2);
        for p in img.pixels_mut() {
            *p = image::Rgba([200, 30, 30, 255]);
        }
        let dynimg = image::DynamicImage::ImageRgba8(img);
        let mut buf = Vec::new();
        dynimg
            .write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();
        buf
    }

    #[test]
    fn encode_jpeg_produces_nonempty_jpeg_magic() {
        let img = image::load_from_memory(&tiny_png()).unwrap();
        let jpeg = encode_jpeg(&img, 80).unwrap();
        assert_eq!(&jpeg[0..2], &[0xff, 0xd8]);
    }

    #[test]
    fn cache_key_changes_with_mtime_and_tier() {
        let p = Path::new("/tmp/a.png");
        let k1 = cache_key(p, 100, Tier::Thumb);
        let k2 = cache_key(p, 200, Tier::Thumb);
        let k3 = cache_key(p, 100, Tier::Preview);
        assert_ne!(k1, k2);
        assert_ne!(k1, k3);
    }
}
