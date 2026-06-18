//! Structural guard: no raw SSH/SFTP client construction outside the
//! `servers_runtime::transfer` module. The single sanctioned outbound SSH
//! channel — a user-initiated upload to the user's OWN server — lives there
//! and nowhere else (see docs/PRINCIPLES.md). This is a guardrail against
//! accidental SSH/SFTP code, not a sandbox — same reasoning as
//! `structural_no_raw_http.rs` and `structural_no_raw_spawn.rs`.

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
fn no_ssh_sftp_client_outside_transfer_module() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);

    let mut violations = Vec::new();
    for file in files {
        // The transfer module is the sole sanctioned SSH/SFTP chokepoint.
        if file.ends_with(Path::new("servers_runtime/transfer.rs")) {
            continue;
        }
        let content = fs::read_to_string(&file).expect("read rust file");
        for (i, line) in content.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue; // skip comments — these crates are named in several
            }
            if line.contains("russh::") || line.contains("russh_sftp") {
                violations.push(format!("{}:{}", file.display(), i + 1));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "SSH/SFTP client used outside servers_runtime::transfer\n{}",
        violations.join("\n"),
    );
}
