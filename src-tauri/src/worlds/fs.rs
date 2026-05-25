//! Filesystem helpers for the worlds module.
//!
//! `validate_segment` is the path-safety gate: every `world_folder_name`
//! and `backup_filename` accepted by a Tauri command flows through it
//! BEFORE any FS operation. Mirrors the project review's HIGH-3 stance
//! (mods::install::install_one validation).

use crate::error::Error;

const RESERVED_WIN: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Reject any input that isn't a safe single path segment.
///
/// Rejections:
/// - empty
/// - contains `/`, `\`, or `:`
/// - exactly `..` or contains `..`
/// - starts with `.` (hidden / current-dir)
/// - length > 255 chars (filesystem cap)
/// - case-insensitive match against Windows reserved names
pub fn validate_segment(name: &str) -> Result<(), Error> {
    if name.is_empty() {
        return Err(Error::WorldPathInvalid {
            name: name.into(),
            reason: "empty name".into(),
        });
    }
    if name.contains('/') || name.contains('\\') || name.contains(':') {
        return Err(Error::WorldPathInvalid {
            name: name.into(),
            reason: "contains path separator or colon".into(),
        });
    }
    if name.contains("..") {
        return Err(Error::WorldPathInvalid {
            name: name.into(),
            reason: "contains '..'".into(),
        });
    }
    if name.starts_with('.') {
        return Err(Error::WorldPathInvalid {
            name: name.into(),
            reason: "starts with '.'".into(),
        });
    }
    if name.len() > 255 {
        return Err(Error::WorldPathInvalid {
            name: name.into(),
            reason: "longer than 255 bytes".into(),
        });
    }
    let upper = name.to_ascii_uppercase();
    if RESERVED_WIN.contains(&upper.as_str()) {
        return Err(Error::WorldPathInvalid {
            name: name.into(),
            reason: "Windows reserved name".into(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_segment_accepts_normal_name() {
        assert!(validate_segment("My Survival World").is_ok());
        assert!(validate_segment("test-1.20.4").is_ok());
        assert!(validate_segment("мир42").is_ok());
    }

    #[test]
    fn validate_segment_rejects_empty() {
        assert!(matches!(
            validate_segment(""),
            Err(Error::WorldPathInvalid { .. })
        ));
    }

    #[test]
    fn validate_segment_rejects_slash() {
        assert!(matches!(
            validate_segment("foo/bar"),
            Err(Error::WorldPathInvalid { .. })
        ));
    }

    #[test]
    fn validate_segment_rejects_backslash() {
        assert!(matches!(
            validate_segment("foo\\bar"),
            Err(Error::WorldPathInvalid { .. })
        ));
    }

    #[test]
    fn validate_segment_rejects_dot_dot() {
        assert!(matches!(
            validate_segment(".."),
            Err(Error::WorldPathInvalid { .. })
        ));
        assert!(matches!(
            validate_segment("foo..bar"),
            Err(Error::WorldPathInvalid { .. })
        ));
        assert!(matches!(
            validate_segment("../escape"),
            Err(Error::WorldPathInvalid { .. })
        ));
    }

    #[test]
    fn validate_segment_rejects_drive_letter() {
        assert!(matches!(
            validate_segment("C:foo"),
            Err(Error::WorldPathInvalid { .. })
        ));
    }

    #[test]
    fn validate_segment_rejects_leading_dot() {
        assert!(matches!(
            validate_segment(".hidden"),
            Err(Error::WorldPathInvalid { .. })
        ));
    }

    #[test]
    fn validate_segment_rejects_overlong() {
        let too_long = "x".repeat(256);
        assert!(matches!(
            validate_segment(&too_long),
            Err(Error::WorldPathInvalid { .. })
        ));
    }

    #[test]
    fn validate_segment_rejects_reserved_windows_names_case_insensitive() {
        for name in &["CON", "con", "Aux", "nul", "COM1", "lpt9"] {
            assert!(
                matches!(validate_segment(name), Err(Error::WorldPathInvalid { .. })),
                "expected reject for {name}"
            );
        }
    }
}
