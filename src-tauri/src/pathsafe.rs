//! Shared path-safety gate: validate that a user-supplied string is a safe
//! single path segment before it is joined onto any directory. Returns a
//! reason string on rejection; callers map it into their own typed error
//! (`worlds` → WorldPathInvalid, `screenshots` → ScreenshotPathInvalid).

/// True iff `name` is a Windows reserved device name (case-insensitive):
/// `CON`/`PRN`/`AUX`/`NUL` and `COM1`..`COM9` / `LPT1`..`LPT9` (`COM0`/`LPT0`
/// are NOT reserved). Shared by [`validate_segment`] and `naming::is_reserved`
/// so the list can't drift between the two gates.
pub fn is_reserved_windows_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "con" | "prn" | "aux" | "nul" => true,
        _ => {
            (lower.starts_with("com") || lower.starts_with("lpt"))
                && lower.len() == 4
                && matches!(lower.as_bytes()[3], b'1'..=b'9')
        }
    }
}

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
    if is_reserved_windows_name(name) {
        return Err("Windows reserved name");
    }
    Ok(())
}

/// `true` iff `name` is a safe **single-segment** filename to join under a
/// base directory (a mod/plugin jar name, a platform-supplied asset name).
///
/// This guard screens a name (from a directory listing, a platform API, or
/// user input) before it is joined onto a directory, so it must reject every
/// escape vector on *every* host OS — not just the one we happen to run on.
/// `\` is a path separator and `C:` a drive prefix on Windows, but both are
/// legal filename characters on Unix, so `std::path::Path` parsing alone
/// would let `a\b.jar` / `C:evil.jar` slip through on a Unix build. Screen
/// those explicitly, then require exactly one `Component::Normal` (which
/// catches `/`, `..`, `.`, absolute paths, and empty).
///
/// Unlike [`validate_segment`] this allows leading dots, long names, and
/// Windows reserved stems — jar filenames come from external ecosystems we
/// don't control; this gate only guarantees the join can't escape the
/// directory.
pub fn is_safe_filename(name: &str) -> bool {
    if name.contains('\\') || name.contains(':') {
        return false;
    }
    let mut comps = std::path::Path::new(name).components();
    matches!(comps.next(), Some(std::path::Component::Normal(_))) && comps.next().is_none()
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
        // com0/lpt0 are NOT reserved.
        assert!(validate_segment("com0").is_ok());
        assert!(validate_segment("lpt0").is_ok());
    }

    #[test]
    fn safe_filename_accepts_plain_names_and_dotfiles() {
        for n in ["sodium-fabric-0.5.3.jar", "a.jar", ".hidden.jar", "CON.jar"] {
            assert!(is_safe_filename(n), "{n} should be a safe filename");
        }
    }

    #[test]
    fn safe_filename_rejects_escape_vectors_on_every_os() {
        for n in [
            "",
            ".",
            "..",
            "../escape.jar",
            "sub/dir.jar",
            "sub\\dir.jar", // Windows separator — must fail on Unix builds too
            "C:evil.jar",   // drive-relative — must fail on Unix builds too
            "C:/x.jar",
            "/abs.jar",
        ] {
            assert!(!is_safe_filename(n), "{n} should be rejected");
        }
    }
}
