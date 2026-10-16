//! Structural guard: every way an instance comes into being decides its
//! settings-sync membership (spec 2026-10-15 §B7, rev. 4 §13.1).
//!
//! `instances::create_instance` is called from exactly the files listed in
//! `CREATORS`; each creation command calls `join_new_instance` with its
//! origin, and the launcher import deliberately does not (an instance from
//! another launcher brings its own settings). A new caller of
//! `create_instance` fails here until it is given an origin — or a reason.
//!
//! Two writers bypass `create_instance` and are pinned by name: the clone
//! (`instances/clone.rs`, whose command passes `Origin::Clone`), and the
//! first-run seed (`instances/migrate.rs`), which writes default settings —
//! sync off — together with its instance, so it never joins.

use std::fs;
use std::path::{Path, PathBuf};

/// `(file under src/, why it creates an instance)`.
const CREATORS: &[(&str, &str)] = &[
    ("commands/instances.rs", "the New instance command"),
    (
        "mods/modpack/import.rs",
        "modpack import (its command joins)",
    ),
    (
        "servers_runtime/to_instance.rs",
        "a client made from a server (its command joins)",
    ),
    (
        "instances/import/pipeline.rs",
        "import from another launcher: never joins",
    ),
];

/// `(file, command fn, required spelling)`.
const JOINS: &[(&str, &str, &str)] = &[
    ("commands/instances.rs", "create_instance", "Origin::New"),
    ("commands/instances.rs", "clone_instance", "Origin::Clone"),
    (
        "commands/modpack_cmds.rs",
        "modpack_import",
        "Origin::ModpackImport",
    ),
    (
        "commands/servers_runtime.rs",
        "server_create_client_instance",
        "Origin::FromServer",
    ),
];

/// `(file, command fn)` that must NOT join.
const NEVER_JOINS: &[(&str, &str)] = &[("commands/instance_import_cmds.rs", "launcher_import_run")];

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let p = entry.unwrap().path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
}

fn production(src: &str) -> String {
    src.lines()
        .take_while(|l| !l.trim_start().starts_with("#[cfg(test)]"))
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The body of the column-0 `pub fn` / `pub async fn` named `name`.
fn body(src: &str, name: &str) -> Option<String> {
    let lines: Vec<&str> = src.lines().collect();
    let start = lines.iter().position(|l| {
        l.starts_with(&format!("pub fn {name}("))
            || l.starts_with(&format!("pub async fn {name}("))
            || l.starts_with(&format!("pub fn {name}<"))
    })?;
    let end = (start..lines.len()).find(|&j| lines[j] == "}")?;
    Some(lines[start..=end].join("\n"))
}

#[test]
fn every_creation_path_decides_its_sync_membership() {
    let mut files = Vec::new();
    rust_files(&src_dir(), &mut files);
    let mut callers: Vec<String> = files
        .iter()
        .filter(|p| {
            let text = production(&fs::read_to_string(p).unwrap());
            text.lines().any(|l| {
                l.contains("instances::create_instance(") && !l.contains("fn create_instance")
            })
        })
        .map(|p| {
            p.strip_prefix(src_dir())
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    callers.sort();
    let mut known: Vec<String> = CREATORS.iter().map(|(f, _)| f.to_string()).collect();
    known.sort();
    assert_eq!(
        callers, known,
        "a new caller of instances::create_instance must decide the new instance's \
         settings-sync membership: call commands::join_new_instance with its Origin in \
         the command, or say here why it never joins"
    );

    for (file, func, spelling) in JOINS {
        let src = fs::read_to_string(src_dir().join(file)).unwrap();
        let b = body(&src, func).unwrap_or_else(|| panic!("{file}: `{func}` not found"));
        assert!(
            b.contains("join_new_instance(") && b.contains(spelling),
            "{file}: `{func}` must call join_new_instance with {spelling}"
        );
    }
    for (file, func) in NEVER_JOINS {
        let src = fs::read_to_string(src_dir().join(file)).unwrap();
        let b = body(&src, func).unwrap_or_else(|| panic!("{file}: `{func}` not found"));
        assert!(
            !b.contains("join_new_instance("),
            "{file}: `{func}` imports from another launcher, which brings its own settings"
        );
    }
}
