//! Structural guard: the mod and asset registries are rewritten under their
//! read-modify-write lock.
//!
//! `mods/installed.rs` (`installed-mods.json`) and `mods/assets.rs`
//! (`installed-assets.json`) rewrite their registry as read → mutate → write,
//! with `.await`s in between. Tauri runs commands concurrently — Browse installs
//! several mods into one instance at once, and ~21 commands reconcile through
//! `installed::list` — so two writers read one snapshot and the later rename
//! erases the earlier change: an install's row is lost, and the next reconcile
//! re-adopts its jar as a manual mod. `mods::registry_lock` serialises them.
//!
//! Three rules, over production code only (the scan of a file stops at its
//! `#[cfg(test)] mod`):
//!
//!   1. LOCK HELD. A function that calls its registry's WRITE primitive holds a
//!      guard at every READ and WRITE primitive call. A guard is a
//!      function-level `let <name> = registry_lock::lock(..)`; `let _ =` drops
//!      it on the spot and is rejected, and `drop(<name>)` releases it. A guard
//!      bound inside a nested block is rejected rather than modelled: its
//!      release at the block's `}` is invisible to a line scan.
//!   2. NO RE-ENTRY. While a guard is held, no function of either registry that
//!      takes a guard is called. Tokio's mutex is not re-entrant — that call
//!      would wait for its own caller forever.
//!   3. CHOKEPOINT. `registry_lock::lock(` appears in no other production file
//!      under `src/`. A caller outside that held the guard and then called a
//!      registry function would deadlock the same way, invisibly to rule 2.
//!
//! Guardrail, not a static analyzer — same framing as
//! `structural_maintenance_gate.rs`. Named gaps, left to review:
//!
//!   - A COMMAND-level get → edit → set across two registry calls is two
//!     locked sections, not one. `installed::update_pack_origin` is the
//!     one-section spelling for the pack origin.
//!   - A guard reached through a helper, or a primitive called through an
//!     alias, is invisible. Both registries spell them directly today.
//!   - A function is located by a column-0 signature, and its body ends at the
//!     next line that is exactly `}` — sound for rustfmt'd top-level fns.

use std::fs;
use std::path::{Path, PathBuf};

/// The call-site text of taking the guard.
const LOCK_CALL: &str = "registry_lock::lock(";

/// The module that defines the lock — its own `lock(` is not a use of it.
const LOCK_MODULE: &str = "mods/registry_lock.rs";

struct Registry {
    /// Path relative to `src/`.
    file: &'static str,
    /// How the OTHER registry names this one in a qualified call.
    module: &'static str,
    /// Unqualified names of the functions that read the whole registry file.
    reads: &'static [&'static str],
    /// Unqualified name of the function that replaces the registry file.
    write: &'static str,
}

const REGISTRIES: &[Registry] = &[
    Registry {
        file: "mods/installed.rs",
        module: "installed",
        reads: &["read_or_empty"],
        write: "write",
    },
    Registry {
        file: "mods/assets.rs",
        module: "assets",
        reads: &["list_all"],
        write: "write_all",
    },
];

/// Lines of `src` before its test module.
fn production(src: &str) -> Vec<&str> {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if line.trim_start().starts_with("#[cfg(test)]") {
            let next = lines[i + 1..].iter().find(|l| !l.trim().is_empty());
            if next.is_some_and(|l| l.trim_start().starts_with("mod ")) {
                break;
            }
        }
        out.push(*line);
    }
    out
}

/// `line` without its `//` comment. A `//` right after a `:` is a URL, not a
/// comment.
fn code(line: &str) -> &str {
    let mut from = 0;
    while let Some(off) = line[from..].find("//") {
        let at = from + off;
        if at == 0 || line.as_bytes()[at - 1] != b':' {
            return &line[..at];
        }
        from = at + 2;
    }
    line
}

/// Whether `code` calls the free function `name` unqualified — not a method
/// (`.name(`), not a path (`fs::name(`), not a longer identifier, not a
/// definition.
fn calls(code: &str, name: &str) -> bool {
    let pat = format!("{name}(");
    let mut from = 0;
    while let Some(off) = code[from..].find(&pat) {
        let at = from + off;
        let unqualified = match code[..at].chars().next_back() {
            None => true,
            Some(c) => !(c.is_alphanumeric() || c == '_' || c == '.' || c == ':'),
        };
        if unqualified && !code[..at].trim_end().ends_with("fn") {
            return true;
        }
        from = at + pat.len();
    }
    false
}

/// A column-0 function signature's name.
fn fn_name(line: &str) -> Option<String> {
    let mut rest = line;
    for vis in ["pub(crate) ", "pub(super) ", "pub "] {
        if let Some(r) = rest.strip_prefix(vis) {
            rest = r;
            break;
        }
    }
    let rest = rest.strip_prefix("async ").unwrap_or(rest);
    let rest = rest.strip_prefix("fn ")?;
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

struct Func<'a> {
    name: String,
    /// `(1-based line number, line)` for every line after the signature line,
    /// up to and excluding the closing `}`.
    body: Vec<(usize, &'a str)>,
}

fn functions<'a>(lines: &[&'a str]) -> Vec<Func<'a>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let Some(name) = fn_name(lines[i]) else {
            i += 1;
            continue;
        };
        if lines[i].trim_end().ends_with("{}") {
            out.push(Func { name, body: vec![] });
            i += 1;
            continue;
        }
        let mut body = Vec::new();
        let mut j = i + 1;
        while j < lines.len() && lines[j] != "}" {
            body.push((j + 1, lines[j]));
            j += 1;
        }
        out.push(Func { name, body });
        i = j + 1;
    }
    out
}

/// `Some(Ok(name))` for a line that takes a well-formed guard, `Some(Err(why))`
/// for one that takes it in a shape this guard cannot follow, `None` for a line
/// that takes no guard.
fn guard_taken(line: &str) -> Option<Result<String, &'static str>> {
    let c = code(line);
    if !c.contains(LOCK_CALL) {
        return None;
    }
    let t = c.trim_start();
    let Some(rest) = t.strip_prefix("let ") else {
        return Some(Err("the guard is not bound with `let`"));
    };
    let rest = rest.strip_prefix("mut ").unwrap_or(rest);
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() || name == "_" {
        return Some(Err("the guard is bound to `_`, which drops it at once"));
    }
    if c.len() - t.len() != 4 {
        return Some(Err(
            "the guard is bound inside a nested block — take it at function level",
        ));
    }
    Some(Ok(name))
}

/// Names of the functions in `src` that take a guard.
fn locking_fns(src: &str) -> Vec<String> {
    let lines = production(src);
    functions(&lines)
        .into_iter()
        .filter(|f| f.body.iter().any(|(_, l)| guard_taken(l).is_some()))
        .map(|f| f.name)
        .collect()
}

/// Rule 1 and rule 2 for one registry file. `locking` pairs each registry's
/// `module` with its locking function names.
fn violations(reg: &Registry, src: &str, locking: &[(&str, Vec<String>)]) -> Vec<String> {
    let lines = production(src);
    let mut out = Vec::new();
    for f in functions(&lines) {
        let writes = f.body.iter().any(|(_, l)| calls(code(l), reg.write));
        let mut held: Option<String> = None;
        for (n, line) in &f.body {
            let c = code(line);
            let at = format!("{}:{n} `{}`", reg.file, f.name);
            match guard_taken(line) {
                Some(Ok(name)) => held = Some(name),
                Some(Err(why)) => out.push(format!("{at}: {why}")),
                None => {}
            }
            if writes {
                for prim in reg.reads.iter().chain(std::iter::once(&reg.write)) {
                    if calls(c, prim) && held.is_none() {
                        out.push(format!(
                            "{at} calls `{prim}` without holding the registry guard"
                        ));
                    }
                }
            }
            if held.is_some() {
                for (module, names) in locking {
                    for name in names {
                        let same_file = *module == reg.module && calls(c, name);
                        let qualified = c.contains(&format!("{module}::{name}("));
                        if same_file || qualified {
                            out.push(format!(
                                "{at} calls `{module}::{name}`, which takes a registry \
                                 guard, while holding one — a deadlock"
                            ));
                        }
                    }
                }
            }
            if let Some(name) = &held {
                if c.contains(&format!("drop({name})")) {
                    held = None;
                }
            }
        }
    }
    out
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read src dir").flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn relative(src_root: &Path, path: &Path) -> String {
    path.strip_prefix(src_root)
        .expect("under src/")
        .to_string_lossy()
        .replace('\\', "/")
}

#[test]
fn registry_writers_hold_the_rmw_lock() {
    let src_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let sources: Vec<String> = REGISTRIES
        .iter()
        .map(|r| fs::read_to_string(src_root.join(r.file)).expect("read registry source"))
        .collect();
    let locking: Vec<(&str, Vec<String>)> = REGISTRIES
        .iter()
        .zip(&sources)
        .map(|(r, s)| (r.module, locking_fns(s)))
        .collect();

    let mut problems = Vec::new();

    // Non-vacuity: the scan must still see the writers it exists for. A renamed
    // primitive or a changed lock spelling would otherwise pass in silence.
    for (reg, src) in REGISTRIES.iter().zip(&sources) {
        let lines = production(src);
        let writers: Vec<String> = functions(&lines)
            .into_iter()
            .filter(|f| f.body.iter().any(|(_, l)| calls(code(l), reg.write)))
            .map(|f| f.name)
            .collect();
        if writers.len() < 2 {
            problems.push(format!(
                "{}: found {} function(s) calling `{}` — the scan no longer sees the \
                 registry's writers; update REGISTRIES",
                reg.file,
                writers.len(),
                reg.write
            ));
        }
        problems.extend(violations(reg, src, &locking));
    }

    // Rule 3.
    let mut files = Vec::new();
    rust_files(&src_root, &mut files);
    for path in files {
        let rel = relative(&src_root, &path);
        if rel == LOCK_MODULE || REGISTRIES.iter().any(|r| r.file == rel) {
            continue;
        }
        let text = fs::read_to_string(&path).expect("read source");
        for (i, line) in production(&text).iter().enumerate() {
            if code(line).contains(LOCK_CALL) {
                problems.push(format!(
                    "{rel}:{}: takes the registry guard outside the registry modules — \
                     call the registry's own function instead",
                    i + 1
                ));
            }
        }
    }

    assert!(
        problems.is_empty(),
        "registry read-modify-write lock violations:\n  {}",
        problems.join("\n  ")
    );
}

// ── the scanner's own teeth ───────────────────────────────────────────────

const TEST_REGISTRY: Registry = Registry {
    file: "mods/installed.rs",
    module: "installed",
    reads: &["read_or_empty"],
    write: "write",
};

fn check(src: &str) -> Vec<String> {
    let locking = vec![
        ("installed", locking_fns(src)),
        ("assets", vec!["add".into()]),
    ];
    violations(&TEST_REGISTRY, src, &locking)
}

#[test]
fn a_well_formed_registry_passes() {
    let src = "\
async fn write(root: &Path, s: &OnDisk) -> Result<(), Error> {
    fs::write(&tmp, &bytes).await?; // the primitive itself: `write(` is qualified
    Ok(())
}

pub async fn add(root: &Path, m: InstalledMod) -> Result<(), Error> {
    let _guard = registry_lock::lock(&registry_path(root)).await;
    // write(root) in prose is not a call
    let mut state = read_or_empty(root).await?;
    state.mods.push(m);
    write(root, &state).await
}

pub async fn backfill(root: &Path) -> Result<(), Error> {
    let guard = registry_lock::lock(&registry_path(root)).await;
    let state = read_or_empty(root).await?;
    drop(guard);
    let names = resolve(state).await;
    let _guard = registry_lock::lock(&registry_path(root)).await;
    let mut state = read_or_empty(root).await?;
    write(root, &state).await
}

pub async fn read_only(root: &Path) -> Result<OnDisk, Error> {
    read_or_empty(root).await
}

#[cfg(test)]
mod tests {
    async fn t() { write(root, &s).await.unwrap(); add(root, m).await.unwrap(); }
}
";
    assert_eq!(check(src), Vec::<String>::new());
}

#[test]
fn a_writer_without_a_guard_is_reported() {
    let src = "\
pub async fn set_enabled(root: &Path) -> Result<(), Error> {
    let mut state = read_or_empty(root).await?;
    write(root, &state).await
}
";
    let v = check(src);
    assert_eq!(v.len(), 2, "{v:#?}");
    assert!(v[0].contains("`read_or_empty` without holding"), "{v:#?}");
    assert!(v[1].contains("`write` without holding"), "{v:#?}");
}

#[test]
fn a_guard_taken_after_the_read_or_released_before_the_write_is_reported() {
    let src = "\
pub async fn late(root: &Path) -> Result<(), Error> {
    let mut state = read_or_empty(root).await?;
    let _guard = registry_lock::lock(&registry_path(root)).await;
    write(root, &state).await
}

pub async fn early(root: &Path) -> Result<(), Error> {
    let guard = registry_lock::lock(&registry_path(root)).await;
    let mut state = read_or_empty(root).await?;
    drop(guard);
    write(root, &state).await
}
";
    let v = check(src);
    assert_eq!(v.len(), 2, "{v:#?}");
    assert!(v[0].contains("`late`") && v[0].contains("`read_or_empty`"));
    assert!(v[1].contains("`early`") && v[1].contains("`write`"));
}

#[test]
fn a_guard_bound_to_underscore_or_in_a_nested_block_is_reported() {
    let src = "\
pub async fn dropped(root: &Path) -> Result<(), Error> {
    let _ = registry_lock::lock(&registry_path(root)).await;
    Ok(())
}

pub async fn nested(root: &Path) -> Result<(), Error> {
    if true {
        let _guard = registry_lock::lock(&registry_path(root)).await;
    }
    Ok(())
}
";
    let v = check(src);
    assert_eq!(v.len(), 2, "{v:#?}");
    assert!(v[0].contains("bound to `_`"), "{v:#?}");
    assert!(v[1].contains("nested block"), "{v:#?}");
}

#[test]
fn calling_a_locking_function_under_the_guard_is_reported() {
    let src = "\
pub async fn add(root: &Path, m: InstalledMod) -> Result<(), Error> {
    let _guard = registry_lock::lock(&registry_path(root)).await;
    let mut state = read_or_empty(root).await?;
    write(root, &state).await
}

pub async fn list(root: &Path) -> Result<(), Error> {
    let _guard = registry_lock::lock(&registry_path(root)).await;
    add(root, m).await?;
    crate::mods::assets::add(root, a).await?;
    state.mods.remove(0);
    Ok(())
}
";
    let v = check(src);
    assert_eq!(v.len(), 2, "{v:#?}");
    assert!(v[0].contains("`installed::add`"), "{v:#?}");
    assert!(v[1].contains("`assets::add`"), "{v:#?}");
}
