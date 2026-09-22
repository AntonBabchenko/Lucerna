//! `app.json` is written through ONE chokepoint (`instances::store`), under
//! one lock, and never re-entrantly:
//!
//! 1. `write_app_json(` appears in production code only inside
//!    `src/instances/store.rs`.
//! 2. No closure passed to `update_app_json(` contains a call to
//!    `update_app_json(`, `replace_app_json(`, or any of the writer functions
//!    listed in `WRITERS` — a `std::sync::Mutex` re-locked on its own thread
//!    is not defined.
//! 3. `WRITERS` is current: every listed name has a `fn` in the tree.
//!
//! Production code = the text before the first `#[cfg(test)]` line of each
//! file (test modules sit at the bottom by convention).

use std::path::{Path, PathBuf};

const STORE: &str = "instances/store.rs";

/// Every function that ends in the chokepoint.
const WRITERS: &[&str] = &[
    "app_settings_mark_tour_completed(",
    "app_settings_patch_general(",
    "changelog_mark_seen(",
    "update_dismiss(",
    "set_active_instance(",
    "repoint_active_instance(",
    "get_active_instance(",
    "delete_instance(",
    "clear_flag(",
];

fn production(src: &str) -> &str {
    src.split("#[cfg(test)]").next().unwrap_or("")
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// The argument texts of every `update_app_json(` call: from the `(` to its
/// matching `)`, bracket-balanced.
fn closures(src: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = src[from..].find("update_app_json(") {
        let start = from + i + "update_app_json(".len();
        let mut depth = 1usize;
        let mut end = None;
        for (j, c) in src[start..].char_indices() {
            match c {
                '(' | '{' | '[' => depth += 1,
                ')' | '}' | ']' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(start + j);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(end) = end else { break };
        out.push(&src[start..end]);
        from = end;
    }
    out
}

fn violations(rel: &str, src: &str) -> Vec<String> {
    let prod = production(src);
    let mut v = Vec::new();
    if rel != STORE && prod.contains("write_app_json(") {
        v.push(format!("{rel}: write_app_json( outside the chokepoint"));
    }
    for body in closures(prod) {
        for name in ["update_app_json(", "replace_app_json("]
            .iter()
            .chain(WRITERS)
        {
            if body.contains(name) {
                v.push(format!(
                    "{rel}: an update_app_json closure calls {name} — a re-entrant lock"
                ));
            }
        }
    }
    v
}

#[test]
fn app_json_is_written_through_one_chokepoint_and_never_reentrantly() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&root, &mut files);
    let mut all = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for f in files {
        let rel = f
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let src = std::fs::read_to_string(&f).unwrap();
        for w in WRITERS {
            if production(&src).contains(&format!("fn {w}")) {
                seen.insert(*w);
            }
        }
        all.extend(violations(&rel, &src));
    }
    for w in WRITERS {
        assert!(
            seen.contains(w),
            "WRITERS lists {w} but no `fn {w}` exists — update the list"
        );
    }
    assert!(all.is_empty(), "{all:#?}");
}

#[test]
fn a_nested_writer_is_reported() {
    let src =
        "fn x() { update_app_json(&p, |af| { set_active_instance(app, id); Verdict::Write }); }";
    assert_eq!(violations("commands/x.rs", src).len(), 1);
}

#[test]
fn a_nested_update_is_reported() {
    let src = "fn x() { update_app_json(&p, |af| { update_app_json(&p, |_| Verdict::Unchanged); Verdict::Write }); }";
    assert_eq!(violations("commands/x.rs", src).len(), 1);
}

#[test]
fn a_raw_write_outside_the_store_is_reported() {
    assert_eq!(
        violations("commands/x.rs", "fn x() { write_app_json(&p, &af); }").len(),
        1
    );
    assert!(violations(STORE, "fn x() { write_app_json(&p, &af); }").is_empty());
}

#[test]
fn test_code_is_ignored() {
    let src = "fn ok() {}\n#[cfg(test)]\nmod t { fn y() { write_app_json(&p, &af); } }";
    assert!(violations("commands/x.rs", src).is_empty());
}
