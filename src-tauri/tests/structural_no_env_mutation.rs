//! Structural guard: no test (or production) code mutates the process
//! environment via `std::env::set_var` / `remove_var`.
//!
//! On glibc, `setenv`/`unsetenv` can `realloc` the `environ` array while
//! another thread is inside `getenv` (e.g. `tempfile` reading `TMPDIR`) — a
//! data race that corrupts arbitrary heap under cargo's parallel test threads.
//! It caused a real, hard-to-trace `rust (ubuntu)` flake (an unrelated
//! self-contained test seeing a corrupted byte buffer). `std::env::set_var` is
//! `unsafe` as of the 2024 edition for exactly this reason.
//!
//! Test overrides now go through the in-process `test_seam` registry instead.
//! This guard keeps env mutation from creeping back in. See `docs/TESTING.md`.

use std::fs;
use std::path::{Path, PathBuf};

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
fn no_process_env_mutation_anywhere() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    rust_files(&root.join("src"), &mut files);
    rust_files(&root.join("tests"), &mut files);

    // This guard names the forbidden functions in its own assertion message, so
    // it must exempt itself; everything else is in scope (incl. other tests).
    let self_path = Path::new(file!());
    let self_name = self_path.file_name();

    let mut violations = Vec::new();
    for file in files {
        if file.file_name() == self_name {
            continue;
        }
        let content = fs::read_to_string(&file).expect("read rust file");
        for (i, line) in content.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue; // skip comments (incl. the test_seam doc-comment refs)
            }
            if line.contains("set_var") || line.contains("remove_var") {
                violations.push(format!("{}:{}", file.display(), i + 1));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "process env mutation found — use the in-process `test_seam` registry \
         instead of std::env::set_var/remove_var (see docs/TESTING.md):\n{}",
        violations.join("\n"),
    );
}
