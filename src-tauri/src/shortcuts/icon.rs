//! Shortcut icons.
//!
//! A Windows `.lnk` can only point at an `.ico` (or an exe/dll resource), while
//! an instance's picture is a PNG. This module bridges the two: it renders
//! `<instance>/icon.png` into `<instance>/icon.ico` and keeps that file honest
//! for as long as the instance exists.
//!
//! The invariant every function here maintains: **if `icon.ico` exists, it is
//! valid and current.** We do not track which `.lnk` files a user has created,
//! so we can never rewrite them — the file they point at must therefore never
//! become missing or stale. Linux needs none of this: a `.desktop` entry's
//! `Icon=` takes an absolute path to the PNG directly.

use crate::error::{Error, Result};
use image::codecs::ico::{IcoEncoder, IcoFrame};
use image::ExtendedColorType;
use std::path::{Path, PathBuf};

/// Frame sizes packed into every generated ICO — see the test for why all four.
const ICO_SIZES: [u32; 4] = [16, 32, 48, 256];

/// The launcher's own icon, embedded at compile time. Used when an instance's
/// picture is cleared: see [`refresh_if_present`].
const APP_ICO: &[u8] = include_bytes!("../../icons/icon.ico");

/// Refuse an absurd `icon.png` before allocating an image buffer. Ours is always
/// a 256x256 PNG, but the file sits in a directory the user can browse and edit.
const MAX_PNG_BYTES: u64 = 16 * 1024 * 1024;

/// Encode `png` as a multi-size ICO with PNG-compressed frames. Windows has
/// accepted PNG frames since Vista and Tauri v2 already requires Windows 10, so
/// one encoder path covers every size.
pub fn to_ico(png: &[u8]) -> Result<Vec<u8>> {
    let img = image::load_from_memory(png)
        .map_err(|e| Error::io("<shortcut icon>", format!("decode png: {e}")))?;
    let mut frames = Vec::with_capacity(ICO_SIZES.len());
    for size in ICO_SIZES {
        let scaled = img
            .resize_to_fill(size, size, image::imageops::FilterType::Lanczos3)
            .to_rgba8();
        frames.push(
            IcoFrame::as_png(scaled.as_raw(), size, size, ExtendedColorType::Rgba8)
                .map_err(|e| Error::io("<shortcut icon>", format!("ico frame {size}: {e}")))?,
        );
    }
    let mut out = Vec::new();
    IcoEncoder::new(&mut out)
        .encode_images(&frames)
        .map_err(|e| Error::io("<shortcut icon>", format!("encode ico: {e}")))?;
    Ok(out)
}

/// Make sure the instance's shortcut icon exists and matches its picture, and
/// return the path a shortcut should point at.
///
/// `None` means the instance has no picture. The caller must then leave the
/// shortcut's icon field **unset** rather than pointing it anywhere: an unset
/// field makes the shell fall back to the target exe's own icon, whereas a path
/// that does not resolve produces no icon at all.
pub fn ensure_for_shortcut(png: &Path, ico: &Path) -> Result<Option<PathBuf>> {
    if !png.is_file() {
        return Ok(None);
    }
    write_from_png(png, ico)?;
    Ok(Some(ico.to_path_buf()))
}

/// Keep an already-generated `icon.ico` in step with the instance picture.
///
/// Called after the picture is set or cleared. Two deliberate asymmetries:
///
/// - **No `.ico` yet ⇒ do nothing.** Instances nobody made a shortcut for stay
///   free of the extra file.
/// - **Picture cleared ⇒ overwrite with the launcher's icon, never delete.**
///   Existing `.lnk` files point at this path and cannot be rewritten, so the
///   file has to keep existing. Writing the app icon reaches the same result as
///   "there never was a picture" by a defined route.
pub fn refresh_if_present(png: &Path, ico: &Path) -> Result<()> {
    if !ico.is_file() {
        return Ok(());
    }
    if png.is_file() {
        write_from_png(png, ico)
    } else {
        write_atomic(ico, APP_ICO)
    }
}

fn write_from_png(png: &Path, ico: &Path) -> Result<()> {
    let len = std::fs::metadata(png)
        .map_err(|e| Error::io(png.display().to_string(), e))?
        .len();
    if len > MAX_PNG_BYTES {
        return Err(Error::io(
            png.display().to_string(),
            "instance picture too large".to_string(),
        ));
    }
    let bytes = std::fs::read(png).map_err(|e| Error::io(png.display().to_string(), e))?;
    write_atomic(ico, &to_ico(&bytes)?)
}

/// Temp file + rename, so a shortcut can never point at a half-written icon.
/// Mirrors the private `write_atomic` in `instances::icon` and `servers` — this
/// codebase keeps a small local copy per module rather than a shared helper.
fn write_atomic(target: &Path, bytes: &[u8]) -> Result<()> {
    let parent = target
        .parent()
        .ok_or_else(|| Error::io(target.display().to_string(), "no parent dir"))?;
    std::fs::create_dir_all(parent).map_err(|e| Error::io(parent.display().to_string(), e))?;
    let tmp = target.with_extension("ico.tmp");
    std::fs::write(&tmp, bytes).map_err(|e| Error::io(tmp.display().to_string(), e))?;
    std::fs::rename(&tmp, target).map_err(|e| Error::io(target.display().to_string(), e))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A solid-colour PNG, the shape `instances::icon::write_icon` stores.
    fn png_bytes(edge: u32) -> Vec<u8> {
        solid_png(edge, [10, 20, 30])
    }

    fn solid_png(edge: u32, rgb: [u8; 3]) -> Vec<u8> {
        let img = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            edge,
            edge,
            image::Rgb(rgb),
        ));
        let mut out = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .expect("encode png");
        out
    }

    #[test]
    fn ico_carries_every_shell_size() {
        // Explorer picks a frame per view: 16 in the details list, 32/48 in
        // medium tiles, 256 in extra-large. A single 256 frame is downscaled by
        // the shell and looks mushy in list views, so all four must be present.
        let ico = to_ico(&png_bytes(256)).expect("encode ico");
        // The decoder exposes the largest frame; assert that first, then the
        // directory entries for the rest.
        let decoded =
            image::load_from_memory_with_format(&ico, image::ImageFormat::Ico).expect("decode ico");
        assert_eq!((decoded.width(), decoded.height()), (256, 256));

        // ICONDIR: reserved(2) + type(2) + count(2), then one 16-byte entry per
        // frame whose first two bytes are width and height (0 means 256).
        assert_eq!(u16::from_le_bytes([ico[4], ico[5]]), 4, "frame count");
        let sizes: Vec<u32> = (0..4)
            .map(|i| {
                let w = ico[6 + i * 16];
                if w == 0 {
                    256
                } else {
                    u32::from(w)
                }
            })
            .collect();
        assert_eq!(sizes, vec![16, 32, 48, 256]);
    }

    #[test]
    fn to_ico_rejects_a_non_image_payload() {
        assert!(to_ico(b"not an image at all").is_err());
    }

    #[test]
    fn ensure_generates_the_ico_and_returns_its_path() {
        let d = tempfile::tempdir().expect("tempdir");
        let png = d.path().join("icon.png");
        let ico = d.path().join("icon.ico");
        std::fs::write(&png, png_bytes(256)).expect("write png");

        let resolved = ensure_for_shortcut(&png, &ico).expect("ensure");

        assert_eq!(resolved.as_deref(), Some(ico.as_path()));
        assert!(ico.is_file());
    }

    #[test]
    fn ensure_returns_none_when_the_instance_has_no_picture() {
        // The caller must then leave ICON_LOCATION unset entirely: a dead icon
        // path shows NO icon, while an absent one falls back to the exe's.
        let d = tempfile::tempdir().expect("tempdir");
        let png = d.path().join("icon.png");
        let ico = d.path().join("icon.ico");

        assert_eq!(ensure_for_shortcut(&png, &ico).expect("ensure"), None);
        assert!(!ico.exists(), "no shortcut icon, no file");
    }

    #[test]
    fn refresh_does_nothing_for_an_instance_that_never_had_a_shortcut() {
        let d = tempfile::tempdir().expect("tempdir");
        let png = d.path().join("icon.png");
        let ico = d.path().join("icon.ico");
        std::fs::write(&png, png_bytes(256)).expect("write png");

        refresh_if_present(&png, &ico).expect("refresh");

        assert!(
            !ico.exists(),
            "only instances with an .ico get one refreshed"
        );
    }

    #[test]
    fn refresh_follows_a_changed_picture() {
        let d = tempfile::tempdir().expect("tempdir");
        let png = d.path().join("icon.png");
        let ico = d.path().join("icon.ico");
        std::fs::write(&png, png_bytes(256)).expect("write png");
        ensure_for_shortcut(&png, &ico).expect("ensure");
        let before = std::fs::read(&ico).expect("read ico");

        // A different picture: same size, different colour.
        std::fs::write(&png, solid_png(256, [200, 30, 40])).expect("replace png");
        refresh_if_present(&png, &ico).expect("refresh");

        assert_ne!(std::fs::read(&ico).expect("read ico"), before);
    }

    #[test]
    fn clearing_the_picture_leaves_the_launcher_icon_behind() {
        // The load-bearing case. Shortcuts already on the user's desktop point
        // at this exact path and we cannot rewrite them, so deleting the file
        // would leave every one of them pointing at nothing.
        let d = tempfile::tempdir().expect("tempdir");
        let png = d.path().join("icon.png");
        let ico = d.path().join("icon.ico");
        std::fs::write(&png, png_bytes(256)).expect("write png");
        ensure_for_shortcut(&png, &ico).expect("ensure");

        std::fs::remove_file(&png).expect("clear picture");
        refresh_if_present(&png, &ico).expect("refresh");

        assert!(ico.is_file(), "the .ico must survive a cleared picture");
        assert_eq!(std::fs::read(&ico).expect("read ico"), APP_ICO);
    }
}
