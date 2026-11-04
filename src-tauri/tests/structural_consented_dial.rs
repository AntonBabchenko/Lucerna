//! Structural guard: the user-consented dial tier is a single file.
//!
//! `network::consent` is the only place allowed to open a TCP connection to a
//! host the *user* supplied (`docs/PRINCIPLES.md` Part A commitment 4). This
//! guard is strictly tighter than the `TcpStream` clause it took over from
//! `structural_no_raw_http.rs`, which exempted the whole `network/` directory:
//! here exactly one file is exempt.
//!
//! `TcpListener` is a different string and deliberately unaffected — the OAuth
//! loopback (`accounts/microsoft/oauth.rs`) BINDS a loopback listener, it does
//! not dial out.
//!
//! `socket2::Socket` dials too, by a third spelling. The own-server port probe
//! (`servers_runtime/port_probe.rs`) needs it to bind with the JVM's socket
//! options and never listen, so socket2 is confined to that one file, and that
//! file must not dial (second test below).
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

/// The dial-capable socket types, matched as plain substrings. `TcpStream`
/// dials directly; tokio's `TcpSocket` (the `net` feature is already enabled
/// in Cargo.toml) builds a socket whose `.connect(..)` dials without the
/// string `TcpStream` ever appearing at the call site — same packet,
/// different spelling. `TcpListener` is deliberately NOT here — binds are
/// not dials (see module doc).
const DIAL_TYPES: &[&str] = &["TcpStream", "TcpSocket"];

/// Which dial-capable type, if any, `line` names.
fn dial_type_on(line: &str) -> Option<&'static str> {
    DIAL_TYPES.iter().copied().find(|n| line.contains(n))
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
            if let Some(needle) = dial_type_on(line) {
                if !is_consent {
                    violations.push(format!("{}:{} {needle}", file.display(), i + 1));
                }
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

/// The exact call expression `ConsentedTcp::open` must contain. Matching the
/// full argument list matters: a bare `ensure_enabled(channel` substring also
/// appears in the function's own *signature*, so a laxer check would still pass
/// after the call site was deleted — which is precisely the regression this
/// tripwire exists to catch.
const CONSENT_CALL: &str = "ensure_enabled(channel, &settings.general)?";

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
    // Anchor the search inside ConsentedTcp::open. A bare first-occurrence
    // `find` can be satisfied by any other function in the file that happens
    // to call the gate, which would let the real check inside `open` be
    // deleted with this guard still green — the regression this test exists
    // to catch.
    let open_at = content
        .find("pub async fn open(")
        .expect("ConsentedTcp::open must exist");
    let call = content[open_at..]
        .find(CONSENT_CALL)
        .map(|i| i + open_at)
        .unwrap_or_else(|| {
            panic!("ConsentedTcp::open must call the consent gate: `{CONSENT_CALL}`")
        });

    // And it must run BEFORE the socket is opened — a check that happens after
    // the dial would leak the very packet the permission is meant to prevent.
    let dial = content
        .find("TcpStream::connect")
        .expect("consent.rs is the only file allowed to dial, so it must contain the connect call");
    assert!(
        call < dial,
        "the consent check must precede TcpStream::connect (found check at {call}, dial at {dial})",
    );
}

fn port_probe_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("servers_runtime")
        .join("port_probe.rs")
}

/// A line that uses socket2, whose `Socket` can dial.
fn names_socket2(line: &str) -> bool {
    line.contains("socket2")
}

/// A line that dials: socket2's `connect` / `connect_timeout`, or any other
/// `connect` call. Matched as a bare substring, so a code line in the probe
/// that merely names it (an identifier, a trailing comment) fails too — reword
/// it; the probe has no business near the word.
fn names_a_dial_call(line: &str) -> bool {
    line.contains("connect")
}

#[test]
fn socket2_confined_to_the_bind_only_port_probe() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let probe = port_probe_path();
    assert!(
        probe.is_file(),
        "the exempt file moved; update port_probe_path() rather than drop the rule"
    );
    let mut files = Vec::new();
    rust_files(&src, &mut files);

    let mut violations = Vec::new();
    for file in files {
        let is_probe = file == probe;
        let content = fs::read_to_string(&file).expect("read rust file");
        for (i, line) in content.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue; // both words appear in doc comments
            }
            if !is_probe && names_socket2(line) {
                violations.push(format!("{}:{} socket2", file.display(), i + 1));
            }
            if is_probe && names_a_dial_call(line) {
                violations.push(format!("{}:{} connect", file.display(), i + 1));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "socket2 must stay in servers_runtime/port_probe.rs, which must never dial\n{}",
        violations.join("\n"),
    );
}

/// The matcher, pinned directly. The tree contains no `TcpSocket` today, so
/// the scan alone cannot prove the needle works — same rationale as the
/// `matchers` module in `structural_no_blind_err_swallow.rs`.
#[cfg(test)]
mod matchers {
    use super::*;

    #[test]
    fn a_tokio_tcpsocket_dial_is_a_dial() {
        // Synthetic: `TcpSocket::new_v4()?.connect(addr)` dials without the
        // string `TcpStream` at the call site.
        assert_eq!(
            dial_type_on("    let sock = tokio::net::TcpSocket::new_v4()?;"),
            Some("TcpSocket"),
        );
    }

    #[test]
    fn a_tcpstream_connect_is_still_a_dial() {
        assert_eq!(
            dial_type_on("    let stream = TcpStream::connect((host, port)).await?;"),
            Some("TcpStream"),
        );
    }

    #[test]
    fn a_listener_bind_is_not_a_dial() {
        // The OAuth loopback BINDS locally (module doc); it must stay
        // unmatched.
        assert_eq!(
            dial_type_on(r#"    let listener = TcpListener::bind("127.0.0.1:0")"#),
            None,
        );
        assert_eq!(dial_type_on("use tokio::net::TcpListener;"), None);
    }

    #[test]
    fn socket2_and_its_dial_are_matched_and_a_bind_is_not() {
        assert!(names_socket2(
            "use socket2::{Domain, Protocol, Socket, Type};"
        ));
        assert!(names_socket2("    let s = socket2::Socket::new(d, t, p)?;"));
        assert!(names_a_dial_call("    socket.connect(&addr.into())?;"));
        assert!(names_a_dial_call(
            "    socket.connect_timeout(&addr.into(), t)?;"
        ));
        assert!(!names_a_dial_call("    socket.bind(&addr.into())?;"));
    }
}
