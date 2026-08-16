//! Structural guard: no heavy work in a synchronous `#[tauri::command]`.
//!
//! A Tauri command declared without `async` runs on the MAIN thread — the one
//! driving the event loop and the webview — so everything it does freezes the
//! whole window for its duration. The founding case (`reconcile_on_list`, a
//! full byte read + SHA-1 of every jar in a server's `mods/` dir) locked the
//! launcher for ~2 s on a cold cache. The fix is always the same shape, used
//! by `server_backup_create` (commands/servers_runtime.rs),
//! `data_root_size_bytes` (commands/data_location.rs) and
//! `screenshot_thumbnail` (commands/screenshots.rs): make the command
//! `async fn` and run the blocking body inside `tokio::task::spawn_blocking`,
//! keeping cheap validation / `is_running` guards in front of the offload.
//!
//! This file generalizes the retired `structural_no_sync_reconcile.rs`, which
//! had three measured blind spots:
//!
//!   1. The attribute match was exact equality against `#[tauri::command]`,
//!      so any argumented form (`#[tauri::command(rename_all = "…")]`) was
//!      skipped silently. Matching is now prefix-based (`is_command_attr`).
//!   2. Only the literal `reconcile_on_list` was hunted. `NEEDLES` now
//!      carries the tree's known heavy-work call-site spellings, each
//!      measured against the tree before adoption.
//!   3. Only `src/commands/` was scanned. The scan now anchors on the
//!      attribute anywhere under `src/`.
//!
//! Guardrail, not a static analyzer — same framing as
//! `structural_no_raw_http.rs`. Residual gaps are NAMED, not half-matched:
//!
//!   - WRAPPERS. A sync command whose body delegates the heavy work to a
//!     helper or module fn carries none of the needle text. Still-sync
//!     examples in this tree: `delete_backup` (commands/worlds.rs) → a
//!     `worlds::` helper, and `server_list` (commands/servers_runtime.rs) →
//!     a status scan at mount. This is the shape that hid `delete_instance`
//!     (→ `instances::delete_instance` → `remove_dir_all`), `server_delete`
//!     (→ `remove_firewall_rules_on_delete` → UAC-elevated netsh) and
//!     `delete_world` until #395 converted all three — an audit found them,
//!     not a guard. A lexical body scan cannot see through a call: review
//!     owns wrappers, and the chokepoint guards
//!     (`structural_no_raw_spawn.rs`, `structural_no_raw_http.rs`) own the
//!     primitives.
//!   - Whole-line `//` comments are exempt; a TRAILING comment naming a
//!     needle after real code still flags. Block comments are not parsed
//!     (none exist in command bodies today).
//!   - An unqualified import (`use crate::process::x;` + bare `x()`) evades
//!     the `crate::process::` needle. Every call site today is fully
//!     qualified; review owns the import spelling.
//!   - The body is taken to end at the first `}` in column 0 at or after the
//!     signature — sound for rustfmt'd top-level fns, which is all this tree
//!     contains.
//!   - The signature line itself is NOT scanned, so a command NAME containing
//!     a needle (`server_export_zip`) never flags by name alone — only a
//!     body line does.

use std::fs;
use std::path::{Path, PathBuf};

/// Heavy-work call-site text and why it is heavy. A synchronous command whose
/// body carries any of these freezes the window for the duration of the work.
///
/// Measured against the tree before adoption (2026-08-16). Candidates that
/// fired on legitimately-cheap sync commands were DROPPED rather than
/// allowlisted:
///
///   - `read_to_string` — a 100% false-positive rate when measured: its only
///     sync-command hit was `server_connectivity` reading a <2 KiB
///     `server.properties` (that command became `async` in this same branch,
///     for its `ipconfig` spawn — not for the read). Every freeze-scale read
///     in the tree lives behind helpers this guard cannot see anyway.
///   - `.output()` — unreachable text in a command body:
///     `structural_no_raw_spawn.rs` already forbids raw subprocess spawns
///     outside `process::`. The reachable spelling is the `crate::process::`
///     wrapper call, which IS a needle below.
const NEEDLES: &[(&str, &str)] = &[
    (
        "reconcile_on_list(",
        "full byte read + SHA-1 of every jar in the directory",
    ),
    (
        "extract_zip",
        "unpacks an archive — GB-scale for a real modpack or world",
    ),
    ("export_zip(", "walks and zips a potentially GB-scale tree"),
    (
        "remove_dir_all(",
        "recursive tree delete — seconds on a real instance or server dir",
    ),
    (
        "copy_into_runtime(",
        "recursive tree copy of a server runtime",
    ),
    (
        "dir_size",
        "recursive size walk — tens of thousands of files on a real data root",
    ),
    (
        "image::",
        "decode/resize/encode of a full-resolution image — 100-300 ms of pure CPU",
    ),
    (
        "crate::process::",
        "spawns a subprocess and waits for it (ipconfig / netsh / UAC elevation)",
    ),
];

/// Scheduled exceptions: (path relative to `src/`, the exact trimmed
/// violating line, why it is allowed). LINE-SCOPED on purpose — same shape
/// and reasoning as `structural_no_blind_err_swallow.rs`: a whole-file entry
/// would shield every future command in the file. Empty today; the
/// stale-check test below removes entries together with the code they excuse.
const ALLOWLIST: &[(&str, &str, &str)] = &[];

/// True when `line` is a `#[tauri::command]` attribute, with or without
/// arguments (`#[tauri::command(rename_all = "snake_case")]`). The retired
/// guard compared with `==` and silently skipped every argumented form — one
/// attribute argument switched the guard off for that command.
fn is_command_attr(line: &str) -> bool {
    line.trim().starts_with("#[tauri::command")
}

/// `Some(true)` when `line` opens an async function definition, `Some(false)`
/// for a synchronous one, `None` when it is not a function signature at all.
/// Handles `pub`, `pub(crate)` and private spellings.
fn fn_signature(line: &str) -> Option<bool> {
    let t = line.trim_start();
    let t = t.strip_prefix("pub(crate) ").unwrap_or(t);
    let t = t.strip_prefix("pub ").unwrap_or(t);
    if t.starts_with("async fn ") {
        return Some(true);
    }
    if t.starts_with("fn ") {
        return Some(false);
    }
    None
}

/// First needle found on a body line, with its reason. Whole-line `//`
/// comments are exempt — prose naming a heavy primitive is documentation,
/// not a call (`delete_instance` explains its running-guard in terms of what
/// `remove_dir_all` would do without calling it). A trailing comment after
/// real code is NOT exempt; keep needle prose on its own comment line.
fn needle_on_code_line(line: &str) -> Option<(&'static str, &'static str)> {
    if line.trim_start().starts_with("//") {
        return None;
    }
    NEEDLES
        .iter()
        .find(|(needle, _)| line.contains(needle))
        .map(|(needle, why)| (*needle, *why))
}

/// Violations in one file: `(signature_line_idx, needle, why, offending_idx)`
/// for every synchronous command whose body carries a needle. Pure so the
/// matchers module can feed it synthetic files.
fn scan_lines(lines: &[&str]) -> Vec<(usize, &'static str, &'static str, usize)> {
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if !is_command_attr(line) {
            continue;
        }
        // Walk forward past any further attributes to the signature.
        let mut j = i + 1;
        let is_async = loop {
            if j >= lines.len() {
                break None;
            }
            if let Some(is_async) = fn_signature(lines[j]) {
                break Some(is_async);
            }
            j += 1;
        };
        let Some(is_async) = is_async else { continue };
        if is_async {
            continue;
        }
        // The body of a rustfmt'd top-level fn ends at the first `}` in
        // column 0 at or after the signature line.
        let end = lines[j..]
            .iter()
            .position(|l| *l == "}")
            .map(|off| j + off)
            .unwrap_or(lines.len() - 1);
        // `.skip(1)` excludes the signature line: a command whose NAME
        // contains a needle must not flag by name alone.
        for (k, body_line) in lines[j..=end].iter().enumerate().skip(1) {
            if let Some((needle, why)) = needle_on_code_line(body_line) {
                out.push((j, needle, why, j + k));
            }
        }
    }
    out
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read_dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().map(|e| e == "rs").unwrap_or(false) {
            out.push(path);
        }
    }
}

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Path relative to `src/`, forward-slashed so the allowlist matches on every
/// platform.
fn rel_of(file: &Path, src: &Path) -> String {
    file.strip_prefix(src)
        .expect("file under src/")
        .to_string_lossy()
        .replace('\\', "/")
}

#[test]
fn tauri_commands_doing_heavy_work_are_async() {
    let src = src_dir();
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    files.sort();

    let mut violations = Vec::new();
    for file in &files {
        let rel = rel_of(file, &src);
        let content = fs::read_to_string(file).expect("read rust file");
        let lines: Vec<&str> = content.lines().collect();
        for (sig, needle, why, at) in scan_lines(&lines) {
            let allowlisted = ALLOWLIST
                .iter()
                .any(|(f, l, _)| *f == rel && lines[at].trim() == *l);
            if allowlisted {
                continue;
            }
            violations.push(format!(
                "{}:{} — `{}` ({}) in sync command opened at line {}: {}",
                rel,
                at + 1,
                needle,
                why,
                sig + 1,
                lines[sig].trim(),
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "synchronous #[tauri::command] doing heavy work runs it on the MAIN \
         thread and freezes the whole window for the duration.\n\
         \n\
         Fix shape (canonical examples: `server_backup_create` in \
         commands/servers_runtime.rs, `data_root_size_bytes` in \
         commands/data_location.rs, `screenshot_thumbnail` in \
         commands/screenshots.rs): make the command `async fn` and run the \
         blocking body inside `tokio::task::spawn_blocking`, keeping cheap \
         validation / `is_running` guards in front of the offload. The \
         sync→async conversion is TS-invisible (empty bindings.ts diff) as \
         long as the doc comment does not change.\n\
         \n\
         If the flagged line really is cheap in context, add a line-scoped \
         ALLOWLIST entry with the reason — do not weaken the needle.\n{}",
        violations.join("\n"),
    );
}

#[test]
fn allowlist_entries_still_match() {
    let src = src_dir();
    let mut stale = Vec::new();
    for (rel, line, _) in ALLOWLIST {
        let path = src.join(rel);
        let found = fs::read_to_string(&path)
            .map(|c| c.lines().any(|l| l.trim() == *line))
            .unwrap_or(false);
        if !found {
            stale.push(format!("{rel} — {line}"));
        }
    }
    assert!(
        stale.is_empty(),
        "ALLOWLIST entry no longer matches anything — the code it excused has \
         been fixed or moved. Delete the entry.\n{}",
        stale.join("\n"),
    );
}

/// The matchers, pinned directly — same practice as
/// `structural_no_blind_err_swallow.rs`: the scan above only proves what the
/// tree happens to contain today; these pin the decisions the tree does not
/// exercise, including the two blind spots the retired guard shipped with.
#[cfg(test)]
mod matchers {
    use super::*;

    #[test]
    fn the_attribute_matches_argumented_forms() {
        assert!(is_command_attr("#[tauri::command]"));
        // The retired guard's `==` comparison skipped this spelling silently.
        assert!(is_command_attr(
            "#[tauri::command(rename_all = \"snake_case\")]"
        ));
        assert!(is_command_attr("    #[tauri::command]"));
        assert!(!is_command_attr("#[specta::specta]"));
        assert!(!is_command_attr("// #[tauri::command] in prose"));
    }

    #[test]
    fn signatures_cover_all_visibility_spellings() {
        assert_eq!(fn_signature("pub async fn f("), Some(true));
        assert_eq!(fn_signature("pub fn f("), Some(false));
        assert_eq!(fn_signature("async fn f("), Some(true));
        assert_eq!(fn_signature("fn f("), Some(false));
        assert_eq!(fn_signature("pub(crate) fn f("), Some(false));
        assert_eq!(fn_signature("pub(crate) async fn f("), Some(true));
        assert_eq!(fn_signature("    app: tauri::AppHandle,"), None);
    }

    #[test]
    fn a_needle_in_a_sync_body_is_flagged() {
        let lines = [
            "#[tauri::command]",
            "#[specta::specta]",
            "pub fn zip_it(app: AppHandle, id: String) -> Result<()> {",
            "    crate::servers_runtime::transfer::export_zip(&p.runtime, &dest)",
            "}",
        ];
        let hits = scan_lines(&lines);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].1, "export_zip(");
        assert_eq!(hits[0].3, 3);
    }

    #[test]
    fn an_argumented_attribute_no_longer_hides_a_sync_command() {
        // End-to-end pin for retired blind spot 1: same body, argumented attr.
        let lines = [
            "#[tauri::command(rename_all = \"snake_case\")]",
            "#[specta::specta]",
            "pub fn purge(app: AppHandle) -> Result<()> {",
            "    std::fs::remove_dir_all(&path)?;",
            "    Ok(())",
            "}",
        ];
        assert_eq!(scan_lines(&lines).len(), 1);
    }

    #[test]
    fn an_async_command_is_exempt_even_with_needles_in_the_body() {
        let lines = [
            "#[tauri::command]",
            "#[specta::specta]",
            "pub async fn data_root_size_bytes(app: AppHandle) -> Result<f64> {",
            "    let size = tokio::task::spawn_blocking(move || crate::data_root::migrate::dir_size(&root))",
            "        .await",
            "}",
        ];
        assert!(scan_lines(&lines).is_empty());
    }

    #[test]
    fn intervening_attributes_are_walked_to_the_signature() {
        let lines = [
            "#[tauri::command]",
            "#[specta::specta]",
            "#[allow(clippy::too_many_arguments)]",
            "pub fn f(app: AppHandle) -> Result<()> {",
            "    crate::process::firewall_rule_present(&name);",
            "}",
        ];
        assert_eq!(scan_lines(&lines).len(), 1);
    }

    #[test]
    fn a_whole_line_comment_naming_a_needle_is_not_a_call() {
        // Synthetic, in the shape of the real `delete_instance` guard prose
        // (commands/instances.rs — `async` today): a running-check explained
        // in terms of `remove_dir_all(..)`. Without the comment exemption any
        // sync command documenting a heavy primitive would false-positive on
        // its own documentation.
        let lines = [
            "#[tauri::command]",
            "#[specta::specta]",
            "pub fn delete_instance(app: tauri::AppHandle, id: String) -> Result<(), Error> {",
            "    // A live JVM holds locks, so remove_dir_all(&dir) here would corrupt it.",
            "    if crate::launch::spawn::is_running(&id) {",
            "        return Err(crate::error::Error::InstanceBusy);",
            "    }",
            "    crate::instances::delete_instance(&app, &id)",
            "}",
        ];
        // NOTE this also documents the WRAPPER gap: the delegated call on the
        // last body line is genuinely heavy and carries no needle text.
        assert!(scan_lines(&lines).is_empty());
    }

    #[test]
    fn a_trailing_comment_after_code_is_not_exempt() {
        assert!(needle_on_code_line("    fs::remove_dir_all(&p)?; // heavy").is_some());
        assert!(needle_on_code_line("    let x = 1; // then remove_dir_all( later").is_some());
        assert!(needle_on_code_line("    // remove_dir_all(&p) in prose").is_none());
    }

    #[test]
    fn the_signature_line_is_not_scanned() {
        // A sync command NAMED after a heavy op it does not perform (it only
        // flips a cancel flag) must not flag on its own name.
        let lines = [
            "#[tauri::command]",
            "#[specta::specta]",
            "pub fn cancel_export_zip(id: String) -> Result<()> {",
            "    crate::servers_runtime::upload_control::upload_cancel(&id);",
            "    Ok(())",
            "}",
        ];
        assert!(scan_lines(&lines).is_empty());
    }

    #[test]
    fn dir_size_covers_both_tree_spellings() {
        assert!(needle_on_code_line("    crate::data_root::migrate::dir_size(&root)").is_some());
        assert!(needle_on_code_line("    dir_size_bytes(&mc2.join(\"saves\"))").is_some());
    }
}
