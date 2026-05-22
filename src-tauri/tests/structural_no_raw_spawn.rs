//! Structural guard: no subprocess `Command` is constructed outside the
//! `process::` module. Every spawn must go through `process::` so the
//! set of processes the launcher runs is enumerable and documented in
//! `docs/PRINCIPLES.md` Appendix A. Guardrail, not a sandbox.

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
fn no_command_constructed_outside_process_module() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let process_dir = src.join("process");
    let mut files = Vec::new();
    rust_files(&src, &mut files);

    let mut violations = Vec::new();
    for file in files {
        if file.starts_with(&process_dir) {
            continue; // the chokepoint module is allowed to build Commands
        }
        let content = fs::read_to_string(&file).expect("read rust file");
        for (i, line) in content.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue; // skip comments
            }
            if line.contains("Command::new") || line.contains("process::Command") {
                violations.push(format!("{}:{}", file.display(), i + 1));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "subprocess Command constructed outside process::\n{}",
        violations.join("\n"),
    );
}
