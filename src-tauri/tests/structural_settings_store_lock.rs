//! Structural guard: the settings-sync shared store is used only under its
//! lock, and the lock is held, never nested.
//!
//! `settings_sync::store::StoreLock` already makes unlocked access a compile
//! error (`load` / `save` are guard methods) and holding it across `.await`
//! one too (`!Send`). What the types cannot see is pinned here, over
//! production code only (a file's scan stops at its `#[cfg(test)]`):
//!
//!   1. CHOKEPOINT. No production file outside `src/settings_sync/` names
//!      `StoreLock`, `store::lock(` or `store_dir(`: callers go through the
//!      module's API, which takes the lock itself.
//!   2. NAMED. Every `store::lock(` in `src/settings_sync/` is the right-hand
//!      side of `let <name> = …;` (not `let _`), so the guard lives for the
//!      function, and a function takes it at most once.
//!   3. NO RE-ENTRY. A function that takes the lock calls no other function
//!      of the module that takes it: the lock is not re-entrant (a debug
//!      build panics; a release build would wait forever).
//!   4. ONE PATH. `.join("shared")` appears only in `settings_sync/store.rs`.
//!
//! Guardrail, not a static analyzer (same framing as
//! `structural_server_sidecar_lock.rs`). Named gaps: a lock-taking function
//! reached through a helper that takes no lock itself is invisible to rule 3;
//! functions are found by column-0 `fn`/`pub fn`/`pub(crate) fn` signatures
//! and end at the next column-0 `}` (rustfmt'd top-level fns only).

use std::fs;
use std::path::{Path, PathBuf};

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

/// The file's lines up to its `#[cfg(test)]` module.
fn production(src: &str) -> Vec<&str> {
    src.lines()
        .take_while(|l| !l.trim_start().starts_with("#[cfg(test)]"))
        .collect()
}

fn code(line: &str) -> &str {
    let t = line.trim_start();
    if t.starts_with("//") {
        return "";
    }
    line.split("//").next().unwrap_or(line)
}

fn rel(p: &Path) -> String {
    p.strip_prefix(src_dir())
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/")
}

fn fn_name(line: &str) -> Option<String> {
    let t = line
        .strip_prefix("pub(crate) ")
        .or_else(|| line.strip_prefix("pub "))
        .unwrap_or(line);
    let t = t.strip_prefix("fn ")?;
    let end = t.find(|c: char| !(c.is_alphanumeric() || c == '_'))?;
    Some(t[..end].to_string())
}

/// `(name, body lines)` of each column-0 function.
fn functions<'a>(lines: &[&'a str]) -> Vec<(String, Vec<&'a str>)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if let Some(name) = fn_name(lines[i]) {
            let end = (i..lines.len())
                .find(|&j| lines[j] == "}")
                .unwrap_or(lines.len() - 1);
            out.push((name, lines[i + 1..=end].to_vec()));
            i = end + 1;
        } else {
            i += 1;
        }
    }
    out
}

#[test]
fn the_settings_store_is_held_by_name_never_nested_and_only_inside_settings_sync() {
    let mut files = Vec::new();
    rust_files(&src_dir(), &mut files);
    let mut violations = Vec::new();
    let mut holders: Vec<(String, String)> = Vec::new();
    let mut module_fns: Vec<(String, String, Vec<String>)> = Vec::new();

    for path in &files {
        let r = rel(path);
        let text = fs::read_to_string(path).unwrap();
        let lines = production(&text);
        let inside = r.starts_with("settings_sync/");
        for (n, line) in lines.iter().enumerate() {
            let c = code(line);
            if !inside
                && (c.contains("StoreLock")
                    || c.contains("store::lock(")
                    || c.contains("store_dir("))
            {
                violations.push(format!(
                    "{r}:{} — the settings store outside settings_sync",
                    n + 1
                ));
            }
            if c.contains(".join(\"shared\")") && r != "settings_sync/store.rs" {
                violations.push(format!(
                    "{r}:{} — the shared store path built outside store.rs",
                    n + 1
                ));
            }
        }
        if !inside {
            continue;
        }
        for (name, body) in functions(&lines) {
            let locking: Vec<&&str> = body
                .iter()
                .filter(|l| code(l).contains("store::lock("))
                .collect();
            if locking.len() > 1 {
                violations.push(format!("{r} — `{name}` takes the store lock twice"));
            }
            for l in &locking {
                let t = code(l).trim_start();
                let named = t
                    .strip_prefix("let ")
                    .map(|rest| rest.trim_start_matches("mut "))
                    .and_then(|rest| rest.split([' ', '=', ':']).next())
                    .is_some_and(|b| !b.is_empty() && b != "_");
                if !named {
                    violations.push(format!(
                        "{r} — `{name}` does not bind the store lock to a named local: {}",
                        t.trim()
                    ));
                }
            }
            if !locking.is_empty() {
                holders.push((r.clone(), name.clone()));
            }
            module_fns.push((
                r.clone(),
                name,
                body.iter().map(|l| code(l).to_string()).collect(),
            ));
        }
    }

    let holder_names: Vec<&str> = holders.iter().map(|(_, n)| n.as_str()).collect();
    for (r, name, body) in &module_fns {
        if !holder_names.contains(&name.as_str()) {
            continue;
        }
        for other in &holder_names {
            if other == name {
                continue;
            }
            let call = format!("{other}(");
            if body.iter().any(|l| {
                l.match_indices(&call).any(|(i, _)| {
                    i == 0 || // A method call (`.set_kind(`) is another function.
                    !l[..i].ends_with(|c: char| c.is_alphanumeric() || c == '_' || c == '.')
                })
            }) {
                violations.push(format!(
                    "{r} — `{name}` holds the store lock and calls `{other}`, which takes it too"
                ));
            }
        }
    }

    // Non-vacuity: the known holders are found, so a renamed module or a
    // broken scan cannot pass silently.
    for expected in ["run_all", "state", "set_kind"] {
        assert!(
            holder_names.contains(&expected),
            "expected `{expected}` to take the settings store lock; found {holder_names:?}"
        );
    }
    assert!(
        violations.is_empty(),
        "the settings-sync store must be used through its lock (see this guard's doc):\n{}",
        violations.join("\n")
    );
}
