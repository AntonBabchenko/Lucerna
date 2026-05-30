//! Integration tests for logs::* testable pieces.
//!
//! `list_log_files` / `latest_crash` need a `tauri::AppHandle`, which
//! integration tests cannot construct. We exercise `read_with_cap`,
//! `assert_under_allowed_roots`, and the public IPC types from
//! outside the crate to confirm the surface is well-formed.

use lucerna_lib::logs::files::{assert_under_allowed_roots, LogFileMeta, LogSource};
use lucerna_lib::logs::read::{read_with_cap, DEFAULT_CAP_BYTES, MAX_CAP_BYTES, MIN_CAP_BYTES};
use std::path::PathBuf;

#[test]
fn read_with_cap_plain_under_returns_full() {
    let tmp = tempfile::tempdir().unwrap();
    let f = tmp.path().join("a.log");
    std::fs::write(&f, b"hello").unwrap();
    let s = read_with_cap(&f, MIN_CAP_BYTES).expect("read");
    assert_eq!(s, "hello");
}

#[test]
fn read_with_cap_plain_over_returns_tail() {
    let tmp = tempfile::tempdir().unwrap();
    let f = tmp.path().join("big.log");
    let body = "Z".repeat(200 * 1024);
    std::fs::write(&f, body.as_bytes()).unwrap();
    let s = read_with_cap(&f, MIN_CAP_BYTES).expect("read");
    assert_eq!(s.len(), MIN_CAP_BYTES as usize);
}

#[test]
fn assert_under_allowed_roots_rejects_outside() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("inside");
    let outside = tmp.path().join("outside.txt");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&outside, b"x").unwrap();
    let err = assert_under_allowed_roots(&outside, &[root]).unwrap_err();
    assert!(
        format!("{err}").contains("not in allowed log roots"),
        "got: {err}",
    );
}

#[test]
fn cap_constants_are_consistent() {
    // Sanity: ordering across the three constants.
    assert!(MIN_CAP_BYTES < DEFAULT_CAP_BYTES);
    assert!(DEFAULT_CAP_BYTES < MAX_CAP_BYTES);
}

#[test]
fn log_file_meta_constructs() {
    // Type-shape smoke test from outside the crate.
    let meta = LogFileMeta {
        path: "C:/tmp/a.log".into(),
        name: "a.log".into(),
        source: LogSource::Game,
        size_bytes: 100.0,
        modified_unix_ms: 1_700_000_000_000.0,
    };
    assert_eq!(meta.name, "a.log");
    assert_eq!(meta.source, LogSource::Game);
    let _: PathBuf = meta.path.into();
}
