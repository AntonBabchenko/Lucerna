//! Structural guard: no raw HTTP client outside the `network::` module.
//! Every outbound request must go through the `network::` chokepoint so
//! the host allowlist is unbypassable. This is a guardrail against
//! accidental network code, not a sandbox — see the same reasoning in
//! `tools/check-no-network-calls.mjs`.
//!
//! Raw TCP dialing used to be checked here too, exempting the whole
//! `network/` directory. That clause now lives in
//! `structural_consented_dial.rs`, which is strictly tighter: it exempts the
//! single file `network/consent.rs` and additionally bans `UdpSocket`
//! everywhere.

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
fn no_http_client_outside_network_module() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let network_dir = src.join("network");
    let mut files = Vec::new();
    rust_files(&src, &mut files);

    let mut violations = Vec::new();
    for file in files {
        if file.starts_with(&network_dir) {
            continue; // the chokepoint module is allowed to use reqwest
        }
        let content = fs::read_to_string(&file).expect("read rust file");
        for (i, line) in content.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue; // skip comments — `reqwest` is named in several
            }
            // `TcpStream` is covered by structural_consented_dial.rs.
            if line.contains("reqwest") {
                violations.push(format!("{}:{}", file.display(), i + 1));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "HTTP client used outside network::\n{}",
        violations.join("\n"),
    );
}
