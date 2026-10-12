//! Structural guard: `NetworkFailure::Other` is produced only by
//! `network::failure`.
//!
//! The UI words every network failure by its typed cause and sends the user to
//! Logs for exactly one of them, `Other` ("the cause is not recognised; see
//! Logs"). That pointer is only honest if a line was written there, and only
//! `network::failure`'s constructors guarantee it: each logs the whole cause
//! chain as it builds the error. An `Other` built anywhere else would point the
//! user at a log that says nothing.

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
fn network_failure_other_is_built_only_by_the_logging_constructors() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let allowed = src.join("network").join("failure.rs");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    assert!(
        files.iter().any(|f| f == &allowed),
        "network/failure.rs moved — update this guard"
    );

    let mut violations = Vec::new();
    for file in files {
        if file == allowed {
            continue;
        }
        let content = fs::read_to_string(&file).expect("read rust file");
        for (i, line) in content.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            if line.contains("NetworkFailure::Other") {
                violations.push(format!("{}:{}", file.display(), i + 1));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "NetworkFailure::Other built outside network/failure.rs — build it with \
         network::failure::unexpected (which logs), or name the cause:\n{}",
        violations.join("\n"),
    );
}
