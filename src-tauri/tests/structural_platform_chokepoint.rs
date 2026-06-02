//! Structural guard: OS-divergent primitives are confined to `platform::`.
//! `PermissionsExt` (exec bits), `WaitForInputIdle` (window detect), and
//! `libc::kill` (process signal) must not appear outside `src/platform/`, so
//! adding macOS later means editing one module — not hunting the codebase.
//! Subprocess spawning is governed separately by structural_no_raw_spawn.rs.

use std::fs;
use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read_dir src") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().map(|e| e == "rs").unwrap_or(false) {
            out.push(path);
        }
    }
}

#[test]
fn platform_primitives_confined_to_platform_module() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let platform_dir = src.join("platform");
    let needles = ["PermissionsExt", "WaitForInputIdle", "libc::kill"];

    let mut files = Vec::new();
    rust_files(&src, &mut files);

    let mut violations = Vec::new();
    for file in files {
        if file.starts_with(&platform_dir) {
            continue; // the chokepoint module is allowed these primitives
        }
        let content = fs::read_to_string(&file).expect("read rust file");
        for (i, line) in content.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue; // skip comments
            }
            for needle in needles {
                if line.contains(needle) {
                    violations.push(format!("{}:{} ({needle})", file.display(), i + 1));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "OS-divergent primitive used outside platform::\n{}",
        violations.join("\n"),
    );
}
