//! Structural guard: the server content sidecars are rewritten under their
//! read-modify-write lock.
//!
//! `servers_runtime/installed.rs` keeps `.lucerna-installed.json` for a
//! server's `runtime/mods/` and `runtime/plugins/`; `servers_runtime/datapacks/
//! sidecar.rs` keeps the same file in a world dir for its datapacks. Both
//! rewrite it as read → mutate → rename. Browse installs several cards into one
//! server at once and the UI re-lists around them, so without the lock two
//! writers read one snapshot and the later rename erases the other's row — the
//! jar comes back with no platform identity, the pack with no provenance.
//!
//! The type system already pins LOCK HELD: `load` and `save` are methods of
//! `installed::SidecarLock`, so a read or a write without a guard does not
//! compile, and `installed::lock` is private to `servers_runtime`. What the
//! types cannot see is pinned here, over production code only (a file's scan
//! stops at its `#[cfg(test)] mod`):
//!
//!   1. NAMED, ONCE. The guard is taken as a function-level
//!      `let <name> = …lock(..);` — nothing chained onto the call. A temporary
//!      (`lock(d).load()` here, `lock(d).save(..)` there) compiles and is two
//!      critical sections, which is the defect itself; `let _ =` drops the
//!      guard on the spot; a guard bound in a nested block is released at that
//!      block's `}`, invisible to a line scan. And at most once per function —
//!      a second take is the same split.
//!   2. NO RE-ENTRY. While a function holds the guard it calls no function of
//!      either file that takes one. The lock is not re-entrant: that call would
//!      wait for its own caller forever (a debug build panics instead).
//!   3. CHOKEPOINT. No other production file under `src/` names
//!      `installed::lock`, imports `lock` from `installed`, or names
//!      `SidecarLock`. A caller holding the guard and then calling
//!      `installed::upsert` would deadlock invisibly to rule 2.
//!
//! Guardrail, not a static analyzer — same framing as
//! `structural_maintenance_gate.rs`. Named gaps, left to review:
//!
//!   - A get → edit → set across two locked calls (`sidecar::reconcile`, then
//!     `sidecar::upsert_by_filename`) is two sections. `installed::replace` is
//!     the one-section spelling of an update's row swap.
//!   - A locking function reached through a helper that takes no guard itself
//!     is invisible to rule 2. Every holder takes it directly today.
//!   - Rule 3 reads spellings, not name resolution: a module alias
//!     (`use …::installed as x;` then `x::lock(..)`) evades it. Visibility
//!     bounds the hole — `lock` is private to `servers_runtime` — and review
//!     owns the alias.
//!   - A function is located by a column-0 signature, and its body ends at the
//!     next line that is exactly `}` — sound for rustfmt'd top-level fns.
//!     Methods inside `impl` blocks are not scanned; none takes the guard.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

struct SidecarModule {
    /// Path relative to `src/`, `/`-separated.
    file: &'static str,
    /// How the other module names this one in a qualified call.
    module: &'static str,
    /// How this module spells the call that takes the guard.
    lock_call: &'static str,
    /// Functions that must be found taking the guard — non-vacuity: a renamed
    /// holder or a changed lock spelling must not pass in silence.
    holders: &'static [&'static str],
}

const MODULES: &[SidecarModule] = &[
    SidecarModule {
        file: "servers_runtime/installed.rs",
        module: "installed",
        lock_call: "lock(",
        holders: &["reconcile_on_list", "upsert", "replace", "apply_enrichment"],
    },
    SidecarModule {
        file: "servers_runtime/datapacks/sidecar.rs",
        module: "sidecar",
        lock_call: "installed::lock(",
        holders: &["reconcile", "upsert_by_filename", "forget"],
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

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
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
            Some(c) => !(is_ident(c) || c == '.' || c == ':'),
        };
        if unqualified && !code[..at].trim_end().ends_with("fn") {
            return true;
        }
        from = at + pat.len();
    }
    false
}

/// Whether `code` contains `path` (e.g. `installed::lock(`) not as the tail of
/// a longer identifier.
fn names_path(code: &str, path: &str) -> bool {
    let mut from = 0;
    while let Some(off) = code[from..].find(path) {
        let at = from + off;
        if !code[..at].chars().next_back().is_some_and(is_ident) {
            return true;
        }
        from = at + path.len();
    }
    false
}

/// Whether `code` takes the guard with `lock_call`.
fn takes_guard(code: &str, lock_call: &str) -> bool {
    match lock_call.strip_suffix('(') {
        Some(name) if !name.contains("::") => calls(code, name),
        _ => names_path(code, lock_call),
    }
}

/// A column-0 function signature's name.
fn fn_name(line: &str) -> Option<String> {
    let mut rest = line;
    if let Some(r) = rest.strip_prefix("pub(") {
        rest = r.split_once(") ")?.1;
    } else if let Some(r) = rest.strip_prefix("pub ") {
        rest = r;
    }
    let rest = rest.strip_prefix("async ").unwrap_or(rest);
    let rest = rest.strip_prefix("fn ")?;
    let name: String = rest.chars().take_while(|c| is_ident(*c)).collect();
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
        if lines[i].trim_end().ends_with('}') {
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

/// `Ok(name)` for a line that takes a well-formed guard, `Err(why)` for one that
/// takes it in a shape rule 1 rejects. Only called on lines that take one.
fn guard_binding(line: &str) -> Result<String, &'static str> {
    let c = code(line);
    let t = c.trim_start();
    let Some(rest) = t.strip_prefix("let ") else {
        return Err(
            "the guard is a temporary — bind it with `let <name> = …lock(..);` \
                    and read and write through that one binding",
        );
    };
    let rest = rest.strip_prefix("mut ").unwrap_or(rest);
    let name: String = rest.chars().take_while(|c| is_ident(*c)).collect();
    if name.is_empty() || name == "_" {
        return Err("the guard is bound to `_`, which drops it at once");
    }
    let rhs = rest.split_once('=').map(|(_, r)| r.trim()).unwrap_or("");
    if !rhs.ends_with(");") || rhs.contains(").") {
        return Err(
            "something is chained onto the lock call, so the guard is a \
                    temporary — bind the guard itself",
        );
    }
    if c.len() - t.len() != 4 {
        return Err("the guard is bound inside a nested block — take it at function level");
    }
    Ok(name)
}

/// Names of the functions in `src` that take the guard with `lock_call`.
fn locking_fns(src: &str, lock_call: &str) -> BTreeSet<String> {
    let lines = production(src);
    functions(&lines)
        .into_iter()
        .filter(|f| f.body.iter().any(|(_, l)| takes_guard(code(l), lock_call)))
        .map(|f| f.name)
        .collect()
}

/// Rules 1 and 2 for one sidecar module. `locking` pairs every module's name
/// with its locking functions.
fn violations(m: &SidecarModule, src: &str, locking: &[(&str, BTreeSet<String>)]) -> Vec<String> {
    let lines = production(src);
    let mut out = Vec::new();
    for f in functions(&lines) {
        let mut held: Option<String> = None;
        let mut takes = 0;
        for (n, line) in &f.body {
            let c = code(line);
            let at = format!("{}:{n} `{}`", m.file, f.name);
            if takes_guard(c, m.lock_call) {
                takes += 1;
                if takes == 2 {
                    out.push(format!(
                        "{at}: takes the sidecar guard a second time — one read-modify-write \
                         is one critical section"
                    ));
                }
                match guard_binding(line) {
                    Ok(name) => held = Some(name),
                    Err(why) => out.push(format!("{at}: {why}")),
                }
                continue;
            }
            if held.is_some() {
                for (module, names) in locking {
                    for name in names {
                        let same_file = *module == m.module && calls(c, name);
                        let qualified = names_path(c, &format!("{module}::{name}("));
                        if same_file || qualified {
                            out.push(format!(
                                "{at}: calls `{module}::{name}`, which takes the sidecar guard, \
                                 while holding it — a deadlock"
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

/// Rule 3 for one file outside the sidecar modules.
fn chokepoint_violations(rel: &str, src: &str) -> Vec<String> {
    let mut out = Vec::new();
    // A `use` statement is gathered up to its `;`, so a multi-line import
    // list is judged whole.
    let mut use_stmt: Option<(usize, String)> = None;
    for (i, line) in production(src).iter().enumerate() {
        let c = code(line);
        if names_path(c, "installed::lock") || names_path(c, "SidecarLock") {
            out.push(format!(
                "{rel}:{}: names the server sidecar lock outside its modules — call \
                 `installed` / `datapacks::sidecar` functions instead",
                i + 1
            ));
            continue;
        }
        let t = c.trim_start();
        let starts_use = t.starts_with("use ") || (t.starts_with("pub") && t.contains(" use "));
        if use_stmt.is_none() && starts_use {
            use_stmt = Some((i + 1, String::new()));
        }
        if let Some((start, text)) = &mut use_stmt {
            text.push_str(c);
            text.push(' ');
            if c.contains(';') {
                let imports_lock = text.contains("installed")
                    && text
                        .split(|ch: char| !is_ident(ch))
                        .any(|token| token == "lock");
                if imports_lock {
                    out.push(format!(
                        "{rel}:{start}: imports the server sidecar lock outside its modules"
                    ));
                }
                use_stmt = None;
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
fn server_sidecar_writers_hold_the_rmw_lock() {
    let src_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let sources: Vec<String> = MODULES
        .iter()
        .map(|m| fs::read_to_string(src_root.join(m.file)).expect("read sidecar module"))
        .collect();
    let locking: Vec<(&str, BTreeSet<String>)> = MODULES
        .iter()
        .zip(&sources)
        .map(|(m, s)| (m.module, locking_fns(s, m.lock_call)))
        .collect();

    let mut problems = Vec::new();
    for ((m, src), (_, found)) in MODULES.iter().zip(&sources).zip(&locking) {
        for holder in m.holders {
            if !found.contains(*holder) {
                problems.push(format!(
                    "{}: `{holder}` no longer takes the sidecar guard as `{}..)` — the scan \
                     cannot see the writer it exists for; update MODULES",
                    m.file, m.lock_call
                ));
            }
        }
        problems.extend(violations(m, src, &locking));
    }

    let mut files = Vec::new();
    rust_files(&src_root, &mut files);
    for path in files {
        let rel = relative(&src_root, &path);
        if MODULES.iter().any(|m| m.file == rel) {
            continue;
        }
        let text = fs::read_to_string(&path).expect("read source");
        problems.extend(chokepoint_violations(&rel, &text));
    }

    assert!(
        problems.is_empty(),
        "server sidecar read-modify-write lock violations:\n  {}",
        problems.join("\n  ")
    );
}

// ── the scanner's own teeth ───────────────────────────────────────────────

const TEST_MODULE: SidecarModule = SidecarModule {
    file: "servers_runtime/installed.rs",
    module: "installed",
    lock_call: "lock(",
    holders: &[],
};

fn check(src: &str) -> Vec<String> {
    let locking = vec![
        ("installed", locking_fns(src, TEST_MODULE.lock_call)),
        ("sidecar", BTreeSet::from(["forget".to_string()])),
    ];
    violations(&TEST_MODULE, src, &locking)
}

#[test]
fn a_well_formed_module_passes() {
    let src = "\
pub(in crate::servers_runtime) fn lock(dir: &Path) -> SidecarLock {
    let mut held = HELD.lock().unwrap_or_else(|p| p.into_inner());
    SidecarLock { dir: dir.to_path_buf() }
}

/// Doc prose naming lock(dir) and upsert(dir) is not a call.
pub fn upsert(jar_dir: &Path, record: ServerInstalledRecord) -> Result<()> {
    let sidecar = lock(jar_dir); // lock(jar_dir).load() in a comment is fine
    let mut records = sidecar.load()?;
    records.push(record);
    sidecar.save(&records)
}

pub fn two_phases(jar_dir: &Path) -> Result<()> {
    let first = lock(jar_dir);
    first.save(&[])?;
    drop(first);
    upsert(jar_dir, rec())
}

fn scan_dir(jar_dir: &Path) -> Result<Vec<String>> {
    upsert(jar_dir, rec())
}

#[cfg(test)]
mod tests {
    fn t() { let rows = lock(dir).load().unwrap(); upsert(dir, lock(dir).load()); }
}
";
    assert_eq!(check(src), Vec::<String>::new());
}

#[test]
fn a_temporary_guard_is_reported() {
    let chained = "\
pub fn upsert(jar_dir: &Path, record: ServerInstalledRecord) -> Result<()> {
    let mut records = lock(jar_dir).load()?;
    records.push(record);
    lock(jar_dir).save(&records)
}
";
    let found = check(chained);
    assert_eq!(found.len(), 3, "{found:#?}");
    assert!(found[0].contains("chained"), "{found:#?}");
    assert!(found[1].contains("second time"), "{found:#?}");
    assert!(found[2].contains("temporary"), "{found:#?}");
}

#[test]
fn a_discarded_or_nested_guard_is_reported() {
    let discarded = "\
pub fn upsert(jar_dir: &Path) -> Result<()> {
    let _ = lock(jar_dir);
    Ok(())
}
";
    assert!(check(discarded)[0].contains("bound to `_`"));

    let nested = "\
pub fn upsert(jar_dir: &Path) -> Result<()> {
    if ready {
        let sidecar = lock(jar_dir);
        sidecar.save(&[])?;
    }
    Ok(())
}
";
    assert!(check(nested)[0].contains("nested block"));
}

#[test]
fn re_entry_is_reported_same_file_and_qualified() {
    let src = "\
pub fn upsert(jar_dir: &Path) -> Result<()> {
    let sidecar = lock(jar_dir);
    Ok(())
}

pub fn reconcile_on_list(jar_dir: &Path) -> Result<()> {
    let sidecar = lock(jar_dir);
    upsert(jar_dir)?;
    sidecar::forget(jar_dir, \"a.zip\")
}
";
    let found = check(src);
    assert_eq!(found.len(), 2, "{found:#?}");
    assert!(found[0].contains("`installed::upsert`") && found[0].contains("deadlock"));
    assert!(found[1].contains("`sidecar::forget`"));
}

#[test]
fn the_chokepoint_reports_every_spelling_outside_the_modules() {
    let src = "\
use crate::servers_runtime::installed::{
    self,
    lock,
};
use crate::servers_runtime::datapacks::level_dat_lock;

pub fn a(dir: &Path) -> Result<()> {
    let guard = crate::servers_runtime::installed::lock(dir);
    let _unused: Option<installed::SidecarLock> = None;
    let _level = level_dat_lock().lock();
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::servers_runtime::installed::lock;
    fn t() { lock(dir).save(&[]).unwrap(); }
}
";
    let found = chokepoint_violations("servers_runtime/other.rs", src);
    assert_eq!(found.len(), 3, "{found:#?}");
    assert!(
        found[0].starts_with("servers_runtime/other.rs:1:"),
        "{found:#?}"
    );
    assert!(
        found[1].starts_with("servers_runtime/other.rs:8:"),
        "{found:#?}"
    );
    assert!(
        found[2].starts_with("servers_runtime/other.rs:9:"),
        "{found:#?}"
    );
}
