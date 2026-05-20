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
}
