//! End-to-end verification of the data-root MIGRATION on real filesystem trees
//! (tempdirs). Composes the same public functions `commands::set_data_location`
//! uses, in the same order (same/nested reject -> reparse reject -> copy ->
//! verify -> delete-old-except-redirect), and asserts the data-moving / reset /
//! reject behaviour. This is the manual "move / reset / nested / verify"
//! checklist, automated and safe — it never touches real app data.

use lucerna_lib::data_root::migrate;
use std::path::Path;
use tempfile::tempdir;

const REDIRECT: &str = "data-location.json";
const SAFE_OVERLAP: [&str; 3] = ["data-location.json", "logs", "updates"];

/// Lay down a realistic launcher data root.
fn seed_root(root: &Path) {
    for (rel, bytes) in [
        ("instances/Default/instance.json", b"{\"id\":\"Default\"}".as_slice()),
        ("instances/Default/.minecraft/options.txt", b"lang:en_us"),
        ("instances/Default/.minecraft/saves/World/level.dat", b"WORLDDATA"),
        ("versions/1.20.1/1.20.1.jar", b"CLIENTJARBYTES"),
        ("libraries/net/example/lib/1.0/lib-1.0.jar", b"LIB"),
        ("servers/my-server/server.json", b"{\"id\":\"my-server\"}"),
        ("account.json", b"{\"accounts\":[]}"),
        ("logs/lucerna.log", b"launcher-log-line"),
    ] {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }
}

/// Replicates the migration pipeline of `commands::set_data_location`: reject a
/// same/nested or reparse-point-containing source, copy the tree (skipping the
/// top-level redirect), verify completeness, then delete the old top-level
/// entries except the redirect. Returns Err with a short tag on rejection.
fn run_migration(current: &Path, target: &Path) -> Result<u64, String> {
    if migrate::is_same_or_nested(current, target) {
        return Err("nested".into());
    }
    if migrate::contains_reparse_point(current).map_err(|e| e.to_string())? {
        return Err("symlink".into());
    }
    let redirect_name = std::ffi::OsString::from(REDIRECT);
    let skip = move |p: &Path| p.as_os_str() == redirect_name;

    let mut copied = 0u64;
    migrate::copy_tree(current, target, &skip, &mut |_| {}, &mut copied)
        .map_err(|e| e.to_string())?;
    migrate::verify_copy(current, target, &skip).map_err(|e| e.to_string())?;

    // Delete old data: every top-level entry of `current` except the redirect.
    for entry in std::fs::read_dir(current).unwrap().flatten() {
        if entry.file_name() == REDIRECT {
            continue;
        }
        let p = entry.path();
        if p.is_dir() {
            std::fs::remove_dir_all(&p).unwrap();
        } else {
            std::fs::remove_file(&p).unwrap();
        }
    }
    Ok(copied)
}

fn read(p: &Path) -> Vec<u8> {
    std::fs::read(p).unwrap()
}

#[test]
fn move_to_empty_target_copies_everything_and_clears_source() {
    let d = tempdir().unwrap();
    let current = d.path().join("com.lucerna.app");
    let target = d.path().join("D_drive/LucernaData");
    seed_root(&current);

    run_migration(&current, &target).expect("clean move");

    // Every file present in the target with identical content.
    assert_eq!(read(&target.join("instances/Default/instance.json")), b"{\"id\":\"Default\"}");
    assert_eq!(read(&target.join("instances/Default/.minecraft/saves/World/level.dat")), b"WORLDDATA");
    assert_eq!(read(&target.join("versions/1.20.1/1.20.1.jar")), b"CLIENTJARBYTES");
    assert_eq!(read(&target.join("servers/my-server/server.json")), b"{\"id\":\"my-server\"}");
    assert_eq!(read(&target.join("account.json")), b"{\"accounts\":[]}");

    // Source is cleared (no redirect existed in it, so nothing remains).
    assert!(!current.join("instances").exists());
    assert!(!current.join("versions").exists());
    assert!(!current.join("account.json").exists());
}

#[test]
fn reset_into_default_with_stale_colliding_log_verifies_ok() {
    // The regression the source-walk verify fixed: resetting a custom root back
    // into the default dir, which already holds a stale logs/lucerna.log of a
    // DIFFERENT size that the copy overwrites. A whole-dir size delta would
    // false-fail; the source-walk verify must pass.
    let d = tempdir().unwrap();
    let current = d.path().join("custom"); // the relocated root being moved back
    seed_root(&current); // its logs/lucerna.log = "launcher-log-line"

    let target = d.path().join("default");
    std::fs::create_dir_all(target.join("logs")).unwrap();
    std::fs::write(target.join(REDIRECT), b"{\"path\":\"...\"}").unwrap(); // bootstrap redirect
    std::fs::write(target.join("logs/lucerna.log"), b"OLD").unwrap(); // stale, different size
    std::fs::write(target.join("updates/.keep"), b"").unwrap_or_else(|_| {
        std::fs::create_dir_all(target.join("updates")).unwrap();
        std::fs::write(target.join("updates/.keep"), b"").unwrap();
    });

    // Reset accepts a target that is empty-or-only-safe-overlap.
    assert!(migrate::empty_or_only_safe(&target, &SAFE_OVERLAP));

    run_migration(&current, &target).expect("reset verifies despite overwritten stale log");

    // The overwritten log now matches the source; the redirect file is untouched.
    assert_eq!(read(&target.join("logs/lucerna.log")), b"launcher-log-line");
    assert_eq!(read(&target.join(REDIRECT)), b"{\"path\":\"...\"}");
    assert_eq!(read(&target.join("instances/Default/instance.json")), b"{\"id\":\"Default\"}");
}

#[test]
fn nested_target_is_rejected_but_sibling_is_allowed() {
    let d = tempdir().unwrap();
    let current = d.path().join("root");
    seed_root(&current);

    // A folder INSIDE current must be rejected (else the delete loop would wipe
    // both source and the fresh copy).
    let nested = current.join("instances/nested-target");
    assert_eq!(run_migration(&current, &nested), Err("nested".into()));

    // A sibling on a notional other drive is fine.
    let sibling = d.path().join("elsewhere");
    assert!(!migrate::is_same_or_nested(&current, &sibling));
}

#[test]
fn verify_copy_catches_a_truncated_file() {
    let d = tempdir().unwrap();
    let current = d.path().join("root");
    seed_root(&current);
    let target = d.path().join("target");

    let skip = |_: &Path| false;
    let mut copied = 0u64;
    migrate::copy_tree(&current, &target, &skip, &mut |_| {}, &mut copied).unwrap();
    // Corrupt one copied file (simulate a partial/interrupted copy).
    std::fs::write(target.join("versions/1.20.1/1.20.1.jar"), b"TRUNC").unwrap();

    assert!(
        migrate::verify_copy(&current, &target, &skip).is_err(),
        "verify must reject a size-mismatched copy"
    );
}
