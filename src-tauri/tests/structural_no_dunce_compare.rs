//! Structural guard: `dunce` is for paths SHOWN to the user, never for paths
//! COMPARED.
//!
//! `std::fs::canonicalize` answers on Windows in one verbatim form (`\\?\C:\…`).
//! `dunce` strips that prefix only when it can — a path under MAX_PATH (260) on
//! a local drive, with no reserved name or trailing dot — so a short root and a
//! long child resolved through dunce never share a prefix: `starts_with` said
//! "outside", and a pack import on a long data root was refused as a path
//! escape (the v0.29 regression pass, F1). Containment, overlap and equality
//! checks go through `pathsafe::ResolvedPath`, which resolves with std and can
//! only be compared with another `ResolvedPath`.
//!
//! The type keeps a comparison honest once both sides are `ResolvedPath`; this
//! guard keeps dunce from creeping back in beside it. Any mention of the crate
//! in `src/` outside the allowlist fails — `dunce::canonicalize`,
//! `dunce::realpath`, `dunce::simplified`, `use dunce::{…}`, `use dunce as d`
//! alike — test modules included, since a line scan cannot see `#[cfg(test)]`.
//! A file joins the allowlist only with the reason its dunce path is displayed,
//! not compared.

use std::fs;
use std::path::{Path, PathBuf};

/// Files that may use `dunce`, relative to `src/`, each with why.
const ALLOWED: &[(&str, &str)] = &[(
    "java/detect.rs",
    "the Java home the UI lists; its exclusion check compares ResolvedPaths",
)];

/// A code line (not a `//` comment) that names the crate.
fn mentions_dunce(line: &str) -> bool {
    !line.trim_start().starts_with("//") && line.contains("dunce")
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read_dir src") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn dunce_resolves_only_paths_shown_to_the_user() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);

    let mut violations = Vec::new();
    for file in files {
        let rel = file
            .strip_prefix(&src)
            .expect("under src")
            .to_string_lossy()
            .replace('\\', "/");
        if ALLOWED.iter().any(|(allowed, _why)| *allowed == rel) {
            continue;
        }
        let content = fs::read_to_string(&file).expect("read rust file");
        for (i, line) in content.lines().enumerate() {
            if mentions_dunce(line) {
                violations.push(format!("src/{rel}:{}: {}", i + 1, line.trim()));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "`dunce` outside the display allowlist — compare paths as \
         `pathsafe::ResolvedPath`s:\n{}",
        violations.join("\n"),
    );
}

#[test]
fn every_allowlisted_file_still_uses_dunce() {
    // A stale entry would silently re-admit a file that no longer needs it.
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for (rel, why) in ALLOWED {
        let content = fs::read_to_string(src.join(rel)).expect("allowlisted file exists");
        assert!(
            content.lines().any(mentions_dunce),
            "src/{rel} is allowlisted ({why}) but no longer uses dunce — drop the entry"
        );
    }
}

#[test]
fn the_matcher_sees_every_way_to_reach_the_crate() {
    for line in [
        "    let p = dunce::canonicalize(&x)?;",
        "    let p = dunce::realpath(&x)?;",
        "    let p = dunce::simplified(&std::fs::canonicalize(&x)?).to_path_buf();",
        "use dunce::canonicalize;",
        "use dunce::{canonicalize as canon};",
        "use dunce as d;",
        "        let ok = x == dunce::canonicalize(y).unwrap(); // trailing comment",
    ] {
        assert!(mentions_dunce(line), "missed: {line}");
    }
    for line in [
        "// dunce strips the prefix only when it can",
        "    /// `dunce::canonicalize` is for display",
        "    let p = crate::pathsafe::ResolvedPath::resolve(&x)?;",
    ] {
        assert!(!mentions_dunce(line), "flagged: {line}");
    }
}
