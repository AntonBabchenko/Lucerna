//! Structural guard: the user-consented dial tier is a single file.
//!
//! `network::consent` is the only place allowed to open a TCP connection to a
//! host the *user* supplied (`docs/PRINCIPLES.md` Part A commitment 4). This
//! guard is strictly tighter than the `TcpStream` clause it took over from
//! `structural_no_raw_http.rs`, which exempted the whole `network/` directory:
//! here exactly one file is exempt.
//!
//! `TcpListener` is a different string and deliberately unaffected — the OAuth
//! loopback (`accounts/microsoft/oauth.rs`) and the own-server port probe
//! (`servers_runtime/preflight.rs`) BIND locally, they do not dial out.
//!
//! Like the sibling guards, this is a guardrail against accidental network
//! code, not a sandbox.

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

fn consent_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("network")
        .join("consent.rs")
}

#[test]
fn tcp_dialing_and_udp_confined_to_the_consent_module() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let consent = consent_path();
    let mut files = Vec::new();
    rust_files(&src, &mut files);

    let mut violations = Vec::new();
    for file in files {
        let is_consent = file == consent;
        let content = fs::read_to_string(&file).expect("read rust file");
        for (i, line) in content.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue; // these types are named in several doc comments
            }
            if line.contains("TcpStream") && !is_consent {
                violations.push(format!("{}:{} TcpStream", file.display(), i + 1));
            }
            // No raw UDP anywhere: pre-empts a hand-rolled DNS/SRV resolver
            // reaching arbitrary hosts outside the consent tier.
            if line.contains("UdpSocket") {
                violations.push(format!("{}:{} UdpSocket", file.display(), i + 1));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "outbound socket used outside network::consent\n{}",
        violations.join("\n"),
    );
}

#[test]
fn consent_module_still_gates_on_the_setting() {
    // Tripwire: deleting the consent check must fail the build rather than
    // silently turn the channel into an always-on one.
    let content = fs::read_to_string(consent_path()).expect("read consent.rs");
    assert!(
        content.contains("read_app_json"),
        "consent.rs must read the settings file to check consent",
    );
    assert!(
        content.contains("ConsentedChannelDisabled"),
        "consent.rs must refuse a channel whose permission is off",
    );
    assert!(
        content.contains("ensure_enabled(channel"),
        "ConsentedTcp::open must call the consent gate",
    );
}
