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

/// True when `line` names the SSH stack. Matched on the bare crate name the
/// way `structural_no_raw_http.rs` matches bare `reqwest`: a path-qualified
/// needle (`russh::`) lets `use russh as ssh;` re-export the whole crate
/// under a name this guard never looks for. The bare substring also covers
/// `russh_sftp`. Full comment lines are skipped by the caller, same as the
/// http guard; a trailing comment naming the crate on a code line would flag
/// — the same accepted exposure bare `reqwest` has (zero such lines today).
fn names_ssh_crate(line: &str) -> bool {
    line.contains("russh")
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
            if names_ssh_crate(line) {
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

/// The matcher, pinned directly — the alias spelling exists nowhere in the
/// tree, so the scan alone cannot prove it is caught. Same rationale as the
/// `matchers` module in `structural_no_blind_err_swallow.rs`.
#[cfg(test)]
mod matchers {
    use super::*;

    #[test]
    fn an_aliasing_import_is_matched() {
        // `use russh as ssh;` re-exports the crate under a name the old
        // `russh::` needle never saw.
        assert!(names_ssh_crate("use russh as ssh;"));
        assert!(names_ssh_crate("use russh_sftp as sftp;"));
    }

    #[test]
    fn qualified_paths_still_match() {
        // Real shapes from servers_runtime/transfer.rs (the exempt chokepoint).
        assert!(names_ssh_crate(
            "    let config = Arc::new(russh::client::Config::default());"
        ));
        assert!(names_ssh_crate(
            "    let sftp = russh_sftp::client::SftpSession::new(channel.into_stream())"
        ));
    }

    #[test]
    fn plain_ssh_prose_is_not_the_crate() {
        assert!(!names_ssh_crate(
            "    // upload over SSH to the user's OWN server"
        ));
        assert!(!names_ssh_crate("fn ssh_upload_worker() {}"));
    }
}
