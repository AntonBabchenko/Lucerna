//! Filesystem helpers for the screenshots module. `validate_segment` maps the
//! shared path-safety check into the screenshot-specific error variant.

use crate::error::Error;
use std::path::Path;

/// Reject any input that isn't a safe single path segment (screenshot filename).
pub fn validate_segment(name: &str) -> Result<(), Error> {
    crate::pathsafe::validate_segment(name).map_err(|reason| Error::ScreenshotPathInvalid {
        name: name.into(),
        reason: reason.into(),
    })
}

/// Case-insensitive check that a filename is an image we display.
pub fn is_image_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".png") || lower.ends_with(".jpg") || lower.ends_with(".jpeg")
}

/// mtime of a file in ms since the UNIX epoch; 0 if unavailable.
pub fn file_mtime_ms(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_image_file_matches_png_and_jpeg_case_insensitive() {
        assert!(is_image_file("shot.png"));
        assert!(is_image_file("SHOT.PNG"));
        assert!(is_image_file("a.jpg"));
        assert!(is_image_file("a.jpeg"));
        assert!(!is_image_file("level.dat"));
        assert!(!is_image_file("notes.txt"));
    }

    #[test]
    fn validate_segment_rejects_traversal() {
        assert!(matches!(
            validate_segment("../escape.png"),
            Err(Error::ScreenshotPathInvalid { .. })
        ));
    }
}
