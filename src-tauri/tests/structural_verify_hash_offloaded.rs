//! Structural guard: the verify pass must not hash on the async executor, and
//! must not read a whole artefact into memory to do it.
//!
//! `verify::scan::file_sha1` used to be `tokio::fs::read(path).await` followed
//! by `Sha1::digest(&bytes)`. With `hash_planned`'s `buffer_unordered(8)` that
//! occupied up to eight tokio WORKER threads with CPU work for the length of a
//! full verify, and peaked at eight times the largest artefact in memory.
//!
//! A comment saying "keep this on the blocking pool" would not survive the next
//! person simplifying two lines back into one — the same reasoning
//! `structural_no_heavy_sync_command.rs` gives for its own existence.
//!
//! Deliberately narrow: ONE file, and the absence check is scoped to the two
//! HASHING functions rather than the whole file. That scoping is not tidiness —
//! it is correctness. `verify::scan::verify_instance_report` legitimately calls
//! `tokio::fs::read` on the asset-index JSON, a small manifest it parses rather
//! than hashes, and a file-wide ban would accuse that correct line. The subject
//! here is the artefact hash, so the subject of the scan is the artefact hasher.
//!
//! Function bodies are taken to end at the first `}` in column 0 at or after
//! the signature — sound for rustfmt'd top-level fns, which is all this file
//! contains, and the same convention `structural_no_heavy_sync_command.rs`
//! uses. Named gaps, not half-matches: a whole-buffer digest spelled
//! `<Sha1 as Digest>::digest(..)` carries none of the needle text, and a
//! hashing helper added under a THIRD name would not be scanned until it is
//! added to `HASHERS` below.

use std::fs;
use std::path::Path;

/// The functions that turn an artefact's bytes into a digest. Adding a third
/// one means adding it here.
const HASHERS: &[&str] = &["async fn file_sha1(", "fn sha1_of_file("];

/// Call-site text that means "the whole artefact is in memory at once". The
/// digest needle matters as much as the read needles: streaming feeds
/// `hasher.update(&buf[..n])` and finishes with `finalize()`, so a
/// `Sha1::digest(..)` inside a hasher is a whole-buffer digest by construction.
const WHOLE_FILE: &[&str] = &["tokio::fs::read(", "fs::read_to_end(", "Sha1::digest("];

fn scan_rs() -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("verify")
        .join("scan.rs");
    fs::read_to_string(&p).expect("read verify/scan.rs")
}

/// Whole-line `//` comments are exempt: the doc comment above `file_sha1` names
/// both spellings in prose, and prose is not code.
fn is_code(line: &str) -> bool {
    !line.trim_start().starts_with("//")
}

/// Line indices belonging to the body of each function in [`HASHERS`], from its
/// signature to the first `}` in column 0 after it.
fn hasher_body_lines(lines: &[&str]) -> Vec<usize> {
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if !HASHERS.iter().any(|sig| line.contains(sig)) {
            continue;
        }
        let end = lines[i..]
            .iter()
            .position(|l| *l == "}")
            .map(|off| i + off)
            .unwrap_or(lines.len() - 1);
        out.extend(i..=end);
    }
    out
}

#[test]
fn verify_hashes_on_the_blocking_pool_not_the_async_executor() {
    let src = scan_rs();
    assert!(
        src.lines()
            .filter(|l| is_code(l))
            .any(|l| l.contains("tokio::task::spawn_blocking")),
        "verify/scan.rs must hash on the blocking pool: `buffer_unordered(8)` over \
         an executor-side hash pins eight tokio worker threads for the whole pass"
    );
}

#[test]
fn verify_never_reads_a_whole_artefact_to_hash_it() {
    let src = scan_rs();
    let lines: Vec<&str> = src.lines().collect();
    let body = hasher_body_lines(&lines);
    assert!(
        !body.is_empty(),
        "no function in HASHERS was found in verify/scan.rs — the guard has lost \
         its subject; update HASHERS to the current hasher's name"
    );
    let offenders: Vec<String> = body
        .iter()
        .filter(|i| is_code(lines[**i]))
        .filter(|i| WHOLE_FILE.iter().any(|n| lines[**i].contains(n)))
        .map(|i| format!("{}: {}", i + 1, lines[*i].trim()))
        .collect();
    assert!(
        offenders.is_empty(),
        "verify/scan.rs must stream the hash in fixed-size chunks — a whole-file \
         read peaks at CONCURRENCY x the largest artefact:\n{}",
        offenders.join("\n"),
    );
}
