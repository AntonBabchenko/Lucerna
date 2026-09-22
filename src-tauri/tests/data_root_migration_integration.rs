//! End-to-end verification of the data-root MOVE on real filesystem trees
//! (tempdirs), through the PRODUCTION pipeline `data_root::relocate::relocate`
//! with real file I/O. The pointer hooks write a real `data-location.json`, so
//! the assertions are about what a user's disk would look like. It never
//! touches real app data.
//!
//! This file used to carry its own copy of the pipeline. That copy deleted
//! before "writing the redirect" exactly like production did, and so could not
//! notice that production had the order wrong (STOR-01).

use lucerna_lib::data_root::redirect::{self, Redirect};
use lucerna_lib::data_root::relocate::{relocate, Hooks, Io, Outcome, Phase};
use lucerna_lib::data_root::transient::SAFE_OVERLAP;
use lucerna_lib::data_root::walk::{self, Walk};
use lucerna_lib::data_root::{looks_like_data_root, migrate, transient};
use std::path::{Path, PathBuf};
use tempfile::tempdir;

/// Lay down a realistic launcher data root.
fn seed_root(root: &Path) {
    for (rel, bytes) in [
        ("app.json", b"{}".as_slice()),
        ("instances/Default/instance.json", b"{\"id\":\"Default\"}"),
        ("instances/Default/.minecraft/options.txt", b"lang:en_us"),
        (
            "instances/Default/.minecraft/saves/World/level.dat",
            b"WORLDDATA",
        ),
        ("versions/1.20.1/1.20.1.jar", b"CLIENTJARBYTES"),
        ("libraries/net/example/lib/1.0/lib-1.0.jar", b"LIB"),
        ("servers/my-server/server.json", b"{\"id\":\"my-server\"}"),
        ("account.json", b"{\"accounts\":[]}"),
        ("logs/lucerna.log", b"launcher-log-line"),
        ("webview/EBWebView/Local State", b"{}"),
    ] {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }
}

fn read(p: &Path) -> Vec<u8> {
    std::fs::read(p).unwrap()
}

/// Run the production pipeline with a real redirect file as the pointer.
/// `new_path = None` models a reset (the redirect is removed).
fn move_root(
    current: &Path,
    target: &Path,
    redirect_file: &Path,
    new_path: Option<&Path>,
    io: &Io<'_>,
) -> Result<Outcome, String> {
    let previous = pointer(redirect_file);
    let mut on_progress = |_: Phase, _: u64| {};
    let never = || false;
    let always = || true;
    let mut commit = || -> Result<(), String> {
        match new_path {
            Some(p) => redirect::write(
                redirect_file,
                &Redirect {
                    path: p.to_path_buf(),
                },
            ),
            None => redirect::remove(redirect_file),
        }
        .map_err(|e| e.to_string())
    };
    let mut rollback = || -> Result<(), String> {
        match &previous {
            Some(r) => redirect::write(redirect_file, r),
            None => redirect::remove(redirect_file),
        }
        .map_err(|e| e.to_string())
    };
    let lands = || looks_like_data_root(target) && migrate::is_available(target);
    let mut on_switched = || {};
    let mut hooks = Hooks {
        on_progress: &mut on_progress,
        is_cancelled: &never,
        still_safe_to_switch: &always,
        commit_pointer: &mut commit,
        rollback_pointer: &mut rollback,
        lands_on_target: &lands,
        on_switched: &mut on_switched,
    };
    relocate(current, target, io, &mut hooks).map_err(|e| e.reason)
}

/// The pointer as an `Option`. These tests only ever write well-formed
/// pointers, so an unusable one is a test bug, not a case.
fn pointer(redirect_file: &Path) -> Option<redirect::Redirect> {
    match redirect::read_state(redirect_file) {
        redirect::PointerRead::Absent => None,
        redirect::PointerRead::Present(found) => Some(found),
        other => panic!("unusable pointer in a test: {other:?}"),
    }
}

fn redirect_target(redirect_file: &Path) -> Option<PathBuf> {
    pointer(redirect_file).map(|r| r.path)
}

#[test]
fn move_to_empty_target_copies_everything_and_clears_source() {
    let d = tempdir().unwrap();
    let current = d.path().join("com.lucerna.app");
    let target = d.path().join("D_drive/LucernaData");
    let redirect_file = current.join("data-location.json");
    seed_root(&current);

    let outcome = move_root(
        &current,
        &target,
        &redirect_file,
        Some(&target),
        &Io::real(),
    );
    assert_eq!(outcome, Ok(Outcome::Moved));

    assert_eq!(
        read(&target.join("instances/Default/instance.json")),
        b"{\"id\":\"Default\"}"
    );
    assert_eq!(
        read(&target.join("instances/Default/.minecraft/saves/World/level.dat")),
        b"WORLDDATA"
    );
    assert_eq!(
        read(&target.join("versions/1.20.1/1.20.1.jar")),
        b"CLIENTJARBYTES"
    );
    assert_eq!(
        read(&target.join("servers/my-server/server.json")),
        b"{\"id\":\"my-server\"}"
    );
    assert_eq!(read(&target.join("account.json")), b"{\"accounts\":[]}");
    assert!(looks_like_data_root(&target));
    assert!(
        !target.join("webview").exists(),
        "the running profile is not moved"
    );

    // The source keeps only what the running launcher owns.
    assert!(!current.join("instances").exists());
    assert!(!current.join("versions").exists());
    assert!(!current.join("account.json").exists());
    assert!(!current.join("app.json").exists());
    assert!(current.join("webview").exists());
    assert_eq!(redirect_target(&redirect_file), Some(target.clone()));
    assert!(!looks_like_data_root(&current));
}

#[test]
fn a_locked_entry_leaves_the_launcher_pointing_at_the_new_root() {
    // STOR-01. One entry of the old root cannot be removed (Explorer, antivirus,
    // the running WebView2). The move must still be a move.
    let d = tempdir().unwrap();
    let current = d.path().join("com.lucerna.app");
    let target = d.path().join("D_drive/LucernaData");
    let redirect_file = current.join("data-location.json");
    seed_root(&current);

    let locked = |p: &Path| -> std::io::Result<()> {
        if p.ends_with("libraries") {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "in use by another process",
            ))
        } else {
            lucerna_lib::data_root::relocate::real_remove_entry(p)
        }
    };
    let io = Io {
        remove_entry: &locked,
        ..Io::real()
    };
    let outcome = move_root(&current, &target, &redirect_file, Some(&target), &io);
    assert_eq!(
        outcome,
        Ok(Outcome::MovedWithLeftovers {
            entries: vec!["libraries".to_string()],
            old_root_intact: false,
        })
    );
    assert_eq!(
        redirect_target(&redirect_file),
        Some(target.clone()),
        "the next start must land on the complete copy, not on the half-deleted source"
    );
    assert!(looks_like_data_root(&target));
    assert!(!looks_like_data_root(&current));
    assert!(current.join("libraries").exists());
    assert!(!current.join("instances").exists());
}

#[test]
fn reset_into_default_with_stale_colliding_log_verifies_ok() {
    // Resetting a custom root back into the default dir, which already holds a
    // stale logs/lucerna.log of a DIFFERENT size, the redirect and launcher
    // scratch. The source-walk verify must pass, and the redirect is removed.
    let d = tempdir().unwrap();
    let current = d.path().join("custom");
    seed_root(&current);
    let target = d.path().join("default");
    let redirect_file = target.join("data-location.json");
    std::fs::create_dir_all(target.join("logs")).unwrap();
    std::fs::create_dir_all(target.join("updates")).unwrap();
    redirect::write(
        &redirect_file,
        &Redirect {
            path: current.clone(),
        },
    )
    .unwrap();
    std::fs::write(target.join("logs/lucerna.log"), b"OLD").unwrap();
    std::fs::write(target.join("updates/.keep"), b"").unwrap();

    assert!(migrate::empty_or_only_safe(&target, &SAFE_OVERLAP));

    let outcome = move_root(&current, &target, &redirect_file, None, &Io::real());
    assert_eq!(outcome, Ok(Outcome::Moved));
    assert_eq!(read(&target.join("logs/lucerna.log")), b"launcher-log-line");
    assert_eq!(
        redirect_target(&redirect_file),
        None,
        "a reset removes the redirect"
    );
    assert!(target.join("updates/.keep").exists());
    assert_eq!(
        read(&target.join("instances/Default/instance.json")),
        b"{\"id\":\"Default\"}"
    );
}

#[test]
fn a_target_that_stops_being_a_root_is_rolled_back_not_deleted_from() {
    // Audit G-6: the next start would not land on the target → the source must
    // survive byte for byte and the redirect must be what it was.
    let d = tempdir().unwrap();
    let current = d.path().join("com.lucerna.app");
    let target = d.path().join("usb/LucernaData");
    let redirect_file = current.join("data-location.json");
    seed_root(&current);

    // `rename_entry` runs right after the commit: pull the "drive" there.
    let target_for_io = target.clone();
    let unplug_then_rename = move |from: &Path, to: &Path| -> std::io::Result<()> {
        if to.ends_with(lucerna_lib::data_root::relocate::HIDDEN_APP_JSON) {
            std::fs::remove_file(target_for_io.join("app.json"))?;
        }
        std::fs::rename(from, to)
    };
    let io = Io {
        rename_entry: &unplug_then_rename,
        ..Io::real()
    };
    let outcome = move_root(&current, &target, &redirect_file, Some(&target), &io);
    assert!(outcome.is_err(), "{outcome:?}");
    assert!(
        looks_like_data_root(&current),
        "app.json is back under its own name"
    );
    assert_eq!(
        read(&current.join("versions/1.20.1/1.20.1.jar")),
        b"CLIENTJARBYTES"
    );
    assert_eq!(
        redirect_target(&redirect_file),
        None,
        "the pointer is what it was"
    );
}

#[test]
fn nested_target_is_rejected_but_sibling_is_allowed() {
    let d = tempdir().unwrap();
    let current = d.path().join("root");
    seed_root(&current);
    // A folder INSIDE current must be rejected before any copy.
    assert!(migrate::is_same_or_nested(
        &current,
        &current.join("instances/nested-target")
    ));
    assert!(!migrate::is_same_or_nested(
        &current,
        &d.path().join("elsewhere")
    ));
}

#[test]
fn verify_catches_a_truncated_file() {
    let d = tempdir().unwrap();
    let current = d.path().join("root");
    seed_root(&current);
    let target = d.path().join("target");

    let never = || false;
    let walk = Walk {
        rule: &transient::classify,
        list_dir: &walk::real_list_dir,
        copy_file: &walk::real_copy_file,
        cancelled: &never,
    };
    let mut copied = 0u64;
    walk::copy_tree(&current, &target, &walk, &mut |_| {}, &mut copied).unwrap();
    std::fs::write(target.join("versions/1.20.1/1.20.1.jar"), b"TRUNC").unwrap();
    assert!(
        walk::verify_tree(&current, &target, &walk).is_err(),
        "verify must reject a size-mismatched copy"
    );
}
