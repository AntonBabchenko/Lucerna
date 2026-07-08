//! Per-instance custom picture ("icon.png"): decode an uploaded PNG, normalize
//! it to a canonical 256x256 square, and persist it next to instance.json.
//! The presence of the file is the "has custom icon" state (no schema field).
//! Delivery to the UI mirrors `accounts::skins` (base64 PNG over IPC).

use crate::error::{Error, Result};
use base64::Engine;
use serde::Serialize;
use specta::Type;
use std::path::Path;

/// Canonical stored edge (px). The UI receives this back as a data URL.
const ICON_EDGE: u32 = 256;
/// Reject absurd payloads before allocating an image buffer (~16 MB).
const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;

/// Base64 PNG returned to the UI (mirrors `accounts::skins::AccountSkin`).
#[derive(Debug, Clone, Serialize, Type, PartialEq)]
pub struct InstanceIcon {
    pub png_base64: String,
}

/// Decode `png_base64`, normalize to a 256x256 PNG, and atomically write it to
/// `icon_path`. Errors if the payload is oversized or not a decodable image.
pub fn write_icon(icon_path: &Path, png_base64: &str) -> Result<()> {
    let raw = base64::engine::general_purpose::STANDARD
        .decode(png_base64.as_bytes())
        .map_err(|e| Error::io("<instance icon>", format!("decode base64: {e}")))?;
    if raw.len() > MAX_INPUT_BYTES {
        return Err(Error::io("<instance icon>", "image too large".to_string()));
    }
    let img = image::load_from_memory(&raw)
        .map_err(|e| Error::io("<instance icon>", format!("decode image: {e}")))?;
    let square = img.resize_to_fill(ICON_EDGE, ICON_EDGE, image::imageops::FilterType::Lanczos3);
    let mut png = Vec::new();
    square
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| Error::io("<instance icon>", format!("encode png: {e}")))?;
    write_atomic(icon_path, &png)
}

/// Read `icon_path` and return it as base64, or `None` if absent.
pub fn read_icon(icon_path: &Path) -> Result<Option<InstanceIcon>> {
    if !icon_path.is_file() {
        return Ok(None);
    }
    let bytes =
        std::fs::read(icon_path).map_err(|e| Error::io(icon_path.display().to_string(), e))?;
    let png_base64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(Some(InstanceIcon { png_base64 }))
}

/// Remove `icon_path` if present. Idempotent: a missing file is success.
pub fn clear_icon(icon_path: &Path) -> Result<()> {
    match std::fs::remove_file(icon_path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::io(icon_path.display().to_string(), e)),
    }
}

/// True iff a custom icon exists on disk.
pub fn has_icon(icon_path: &Path) -> bool {
    icon_path.is_file()
}

/// Write bytes atomically: temp file + rename (mirrors `store::write_atomic`).
fn write_atomic(target: &Path, bytes: &[u8]) -> Result<()> {
    let parent = target
        .parent()
        .ok_or_else(|| Error::io(target.display().to_string(), "no parent dir"))?;
    std::fs::create_dir_all(parent).map_err(|e| Error::io(parent.display().to_string(), e))?;
    let tmp = target.with_extension("png.tmp");
    std::fs::write(&tmp, bytes).map_err(|e| Error::io(tmp.display().to_string(), e))?;
    std::fs::rename(&tmp, target).map_err(|e| Error::io(target.display().to_string(), e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// Encode a solid-color PNG of `w`x`h` as base64 (a valid decodable input).
    fn png_base64(w: u32, h: u32) -> String {
        let buf = image::RgbImage::from_pixel(w, h, image::Rgb([10, 20, 30]));
        let dynimg = image::DynamicImage::ImageRgb8(buf);
        let mut bytes = Vec::new();
        dynimg
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    }

    #[test]
    fn write_then_read_roundtrips_a_256_square_png() {
        let d = tempdir().unwrap();
        let path = d.path().join("instances/i1/icon.png");
        write_icon(&path, &png_base64(800, 400)).unwrap();

        assert!(has_icon(&path));
        let icon = read_icon(&path).unwrap().expect("icon present");
        let raw = base64::engine::general_purpose::STANDARD
            .decode(icon.png_base64.as_bytes())
            .unwrap();
        let decoded = image::load_from_memory(&raw).unwrap();
        assert_eq!(decoded.width(), 256);
        assert_eq!(decoded.height(), 256);
    }

    #[test]
    fn read_icon_returns_none_when_absent() {
        let d = tempdir().unwrap();
        let path = d.path().join("instances/none/icon.png");
        assert!(!has_icon(&path));
        assert_eq!(read_icon(&path).unwrap(), None);
    }

    #[test]
    fn write_icon_rejects_a_non_image_payload() {
        let d = tempdir().unwrap();
        let path = d.path().join("instances/bad/icon.png");
        let junk = base64::engine::general_purpose::STANDARD.encode(b"not a png at all");
        assert!(write_icon(&path, &junk).is_err());
        assert!(!path.exists(), "a rejected write must not leave a file");
    }

    #[test]
    fn clear_icon_is_idempotent() {
        let d = tempdir().unwrap();
        let path = d.path().join("instances/i2/icon.png");
        // Missing file: ok.
        clear_icon(&path).unwrap();
        // Present file: removed.
        write_icon(&path, &png_base64(64, 64)).unwrap();
        assert!(has_icon(&path));
        clear_icon(&path).unwrap();
        assert!(!has_icon(&path));
    }
}
