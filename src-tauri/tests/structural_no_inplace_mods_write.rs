//! Structural guard: no raw file-write primitive under `src/mods/` outside the
//! two modules that own writing. Every byte that lands in an instance's content
//! directories goes through `mods::store` (temp file + rename, link or copy);
//! every byte that lands in the content-addressed store goes through
//! `mods::cache` (temp file + rename, SHA-keyed).
//!
//! ## Why this is a build-breaking rule
//!
//! Instance mod jars are HARDLINKS to one shared physical file. Opening such a
//! path for writing — including `fs::copy`, whose destination is opened with
//! truncate — changes the bytes for EVERY instance sharing that mod, and zeroes
//! the file before the first byte is written. A plain `fs::copy` that would
//! corrupt one instance before dedup corrupts all of them after it. The only
//! safe shape is write-to-temp-then-rename, which replaces a directory entry
//! and leaves the other links intact.
//!
//! Removals are deliberately NOT covered: deleting one name cannot affect the
//! store entry or another instance's link, so the uninstall sites keep their
//! plain `remove_file`.
//!
//! Guardrail, not a sandbox — same framing as `structural_no_raw_http.rs` and
//! `structural_no_raw_spawn.rs`. Its job is to make a NEW write into instance
//! content fail the build, so the author routes it through `store` or justifies
//! an addition to the allowlist below.

use std::fs;
use std::path::{Path, PathBuf};

/// Modules under `src/mods/` allowed to call write primitives directly, with
/// the class of path each one writes. Only the first two write instance or
/// store *content*; the rest write metadata or paths outside any instance.
///
/// Adding a file here means stating which class of path it writes and why that
/// path can never be a hardlink shared between instances.
const ALLOWLIST: &[&str] = &[
    // The two owners of writing.
    "store.rs", // instance side: temp + rename, link-or-copy
    "cache.rs", // store side: temp + rename, SHA-keyed
    // Metadata and caches — JSON sidecars, never content bytes.
    "installed.rs",     // .lucerna/installed-mods.json (already temp + rename)
    "summary_cache.rs", // mod summary JSON cache
    "assets.rs",        // installed-assets registry JSON (temp + rename)
    // Paths outside any instance's content directories.
    "dep_resolve.rs", // server mods dirs — out of scope, see the session spec
    "modpack/export/assembly.rs", // the export zip being assembled
    "modpack/source/stage.rs", // downloaded pack archive staging
];

const PRIMITIVES: &[&str] = &["fs::copy(", "fs::write(", "File::create(", "OpenOptions"];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read_dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().map(|e| e == "rs").unwrap_or(false) {
            out.push(path);
        }
    }
}

#[test]
fn no_raw_write_primitive_under_mods_outside_the_owners() {
    let mods = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("mods");
    let mut files = Vec::new();
    rust_files(&mods, &mut files);

    let mut violations = Vec::new();
    for file in files {
        // Path relative to src/mods/, normalised to forward slashes so the
        // allowlist matches on every platform.
        let rel = file
            .strip_prefix(&mods)
            .expect("file under src/mods")
            .to_string_lossy()
            .replace('\\', "/");
        if ALLOWLIST.contains(&rel.as_str()) {
            continue;
        }
        let content = fs::read_to_string(&file).expect("read rust file");
        for (i, line) in content.lines().enumerate() {
            // Test modules are conventionally last in a file; their fixture
            // writes are not production writes. Stop at the first one.
            if line.trim_start().starts_with("#[cfg(test)]") {
                break;
            }
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue; // doc comments naming a primitive are fine
            }
            if PRIMITIVES.iter().any(|p| line.contains(p)) {
                violations.push(format!("{}:{}", file.display(), i + 1));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "raw write primitive under src/mods/ outside mods::store / mods::cache.\n\
         Instance mod jars are hardlinks to ONE shared physical file — writing \
         such a path in place corrupts every instance sharing that mod, and \
         truncates it to zero before the first byte lands. Route the write \
         through `mods::store::materialize` / `mods::store::place_bytes`, or add \
         the file to ALLOWLIST stating the class of path it writes:\n{}",
        violations.join("\n"),
    );
}
