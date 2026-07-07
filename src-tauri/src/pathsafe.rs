//! Shared path-safety gate: validate that a user-supplied string is a safe
//! single path segment before it is joined onto any directory. Returns a
//! reason string on rejection; callers map it into their own typed error
//! (`worlds` → WorldPathInvalid, `screenshots` → ScreenshotPathInvalid).

const RESERVED_WIN: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// `Ok(())` if `name` is a safe single path segment, otherwise `Err(reason)`.
///
/// Rejections: empty; contains `/`, `\`, or `:`; contains `..`; starts with
/// `.`; longer than 255 bytes; case-insensitive Windows reserved name.
pub fn validate_segment(name: &str) -> Result<(), &'static str> {
    if name.is_empty() {
        return Err("empty name");
    }
    if name.contains('/') || name.contains('\\') || name.contains(':') {
        return Err("contains path separator or colon");
    }
    if name.contains("..") {
        return Err("contains '..'");
    }
    if name.starts_with('.') {
        return Err("starts with '.'");
    }
    if name.len() > 255 {
        return Err("longer than 255 bytes");
    }
    let upper = name.to_ascii_uppercase();
    if RESERVED_WIN.contains(&upper.as_str()) {
        return Err("Windows reserved name");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_normal_names() {
        assert!(validate_segment("2026-07-07_21.14.30.png").is_ok());
        assert!(validate_segment("My Survival World").is_ok());
        assert!(validate_segment("мир42").is_ok());
    }

    #[test]
    fn rejects_traversal_and_separators() {
        assert!(validate_segment("").is_err());
        assert!(validate_segment("foo/bar").is_err());
        assert!(validate_segment("foo\\bar").is_err());
        assert!(validate_segment("C:foo").is_err());
        assert!(validate_segment("..").is_err());
        assert!(validate_segment("../escape").is_err());
        assert!(validate_segment(".hidden").is_err());
        assert!(validate_segment(&"x".repeat(256)).is_err());
    }

    #[test]
    fn rejects_reserved_windows_names_case_insensitive() {
        for name in &["CON", "con", "Aux", "nul", "COM1", "lpt9"] {
            assert!(
                validate_segment(name).is_err(),
                "expected reject for {name}"
            );
        }
    }
}
