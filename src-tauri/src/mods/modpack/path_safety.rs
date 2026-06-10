//! Validation for relative paths declared by a modpack (a `.mrpack`
//! `files[]` `path`, or an `overrides/` entry). A path is "safe" only if
//! it stays strictly inside the target directory: no `..`, no absolute
//! root, no Windows drive letter, no backslash separator, not empty.
//!
//! This is the string-level guard. Callers that touch the filesystem
//! (`install_asset`, `overrides::extract`) additionally canonicalize and
//! assert containment — defense in depth.

use std::path::{Component, Path};

/// `true` iff `path` is a safe relative path to join under a base dir.
pub fn is_safe_relative_path(path: &str) -> bool {
    if path.is_empty() {
        return false;
    }
    // Backslash is a separator on Windows — `Path::components` would not
    // flag `mods\..\x`, so reject backslashes outright.
    if path.contains('\\') {
        return false;
    }
    // Absolute (POSIX root).
    if path.starts_with('/') {
        return false;
    }
    // Windows drive letter, e.g. `C:/...`.
    if path.len() > 1 && path.as_bytes()[1] == b':' {
        return false;
    }
    // Every component must be a plain name — rejects `..`, `.`, root,
    // and any prefix component.
    for c in Path::new(path).components() {
        if !matches!(c, Component::Normal(_)) {
            return false;
        }
    }
    true
}

/// `true` iff `name` is a safe **single-segment** filename to join under a
/// base dir — stricter than [`is_safe_relative_path`], which also accepts
/// nested relative paths like `config/foo.json`. A mod jar destination is one
/// filename, never a nested path, so a separator or traversal component is
/// rejected outright. Used to validate the platform-supplied
/// `primary_file.filename` before joining it under an instance's `mods/`.
pub fn is_safe_filename(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    // No directory separators at all — a filename is a single segment.
    if name.contains('/') || name.contains('\\') {
        return false;
    }
    // Exactly one component, and it must be a plain name — rejects `.`, `..`,
    // root, drive-letter prefixes, and anything else `Component::Normal`
    // would not cover.
    let mut comps = Path::new(name).components();
    match (comps.next(), comps.next()) {
        (Some(Component::Normal(_)), None) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_safe_paths() {
        for p in [
            "mods/sodium.jar",
            "resourcepacks/RP.zip",
            "shaderpacks/Complementary.zip",
            "config/sodium/options.json",
            "options.txt",
        ] {
            assert!(is_safe_relative_path(p), "{p} should be safe");
        }
    }

    #[test]
    fn rejects_unsafe_paths() {
        for p in [
            "",
            "../escape.jar",
            "mods/../../etc/passwd",
            "/abs/path",
            "C:/windows/x",
            "mods\\back.jar",
            "./leading-dot",
        ] {
            assert!(!is_safe_relative_path(p), "{p} should be unsafe");
        }
    }

    #[test]
    fn safe_filename_accepts_plain_names() {
        for n in [
            "sodium-fabric-0.5.3.jar",
            "REI_1.20.jar",
            "options.txt",
            "a.jar",
        ] {
            assert!(is_safe_filename(n), "{n} should be a safe filename");
        }
    }

    #[test]
    fn safe_filename_rejects_separators_and_traversal() {
        for n in [
            "",
            ".",
            "..",
            "../escape.jar",
            "../../etc/passwd",
            "sub/dir.jar",  // nested path is not a single filename
            "sub\\dir.jar", // Windows separator
            "/abs.jar",
            "C:/x.jar",
            "./leading.jar",
        ] {
            assert!(!is_safe_filename(n), "{n} should be rejected");
        }
    }
}
