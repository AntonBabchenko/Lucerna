//! Structural guard: every instance, world and datapack writer opens with the
//! maintenance gate.
//!
//! A world migration (`worlds::migrate`) holds `instances::maintenance` claims
//! on BOTH instances for the whole operation — minutes, on the copy path —
//! and relies on every other writer refusing with `InstanceBusy` while the
//! claim is held (world-migration spec §4.0, amendment A5). Nothing in the
//! frontend queue protects that: the world commands are direct IPC calls that
//! know nothing of the task registry's serial lane. The gate has ONE
//! definition, `instances::maintenance::write_allowed(id)` —
//! `is_running || is_starting || maintenance_is_active` — and this guard pins
//! that every writer actually calls it.
//!
//! Two deliberate exceptions, both checked here under their own spelling:
//!
//!   - LAUNCH must not refuse on `is_running` at command entry (a second
//!     launch of the SAME id is refused inside `launch::start` by
//!     `claim_start`), so `launch_instance` checks `maintenance_is_active`
//!     alone — and `start` checks it again right after `claim_start`,
//!     Dekker-paired with the migration's claim-then-check.
//!   - `delete_backup` only touches `<instance>/backups/`, never the world
//!     tree the JVM holds, and has always been allowed while the game runs; a
//!     Move migration relocates that set file by file, so it refuses for the
//!     maintenance claim alone.
//!
//! Why a test and not a comment: the rule "every world writer opens with the
//! gate" lived as prose in `datapacks::guard`'s module doc, listing four
//! world commands by name. `recover_stranded_world` was added later WITH the
//! gate, but nothing would have failed had it been added without — a comment
//! is not a guard. Two rules:
//!
//!   1. LISTED WRITERS. Every `(file, fn)` in `GATED` carries its required
//!      spelling on a code line of its body. A listed fn that no longer
//!      exists fails too, so the list cannot rot.
//!   2. RATCHET. In `commands/worlds.rs` and `commands/datapacks.rs` every
//!      `#[tauri::command]` either carries a `GATE_SPELLINGS` entry or is
//!      declared read-only in `READ_ONLY` with a reason. A new command in
//!      either file must choose, in code — silence fails the build. A
//!      `READ_ONLY` entry that gains a gate, or names a fn that is gone, is
//!      reported as stale. A command that TAKES the claim itself
//!      (`maintenance_begin(`, the migration command) is gated by
//!      construction: a second claim on either id is refused.
//!
//! Guardrail, not a static analyzer — same framing as
//! `structural_no_heavy_sync_command.rs`. Named gaps:
//!
//!   - ORDER. The scan is lexical: it proves the gate is CALLED, not that it
//!     runs before the first write or `.await`. Review owns ordering.
//!   - `commands/instances.rs` is NOT ratcheted — most of its commands edit
//!     `instance.json` fields a migration never reads — so an instance writer
//!     that must refuse under a claim is added to `GATED` by hand. The spec's
//!     list (§4.0) is exactly the entries below.
//!   - A gate reached through a helper this file does not name is invisible;
//!     add the helper's call-site spelling to `GATE_SPELLINGS` together with
//!     the helper, and list the helper itself in `GATED` (as `guard` is).
//!   - Whole-line `//` comments are exempt; a body whose only mention of the
//!     gate is prose therefore FAILS — which is the point.
//!   - A fn is located by a COLUMN-0 signature and its body ends at the first
//!     `}` in column 0 at or after it, so an indented test fn of the same name
//!     is never mistaken for it. Sound for rustfmt'd top-level fns, which is
//!     all this tree contains.

use std::fs;
use std::path::{Path, PathBuf};

/// The single gate: `crate::instances::maintenance::write_allowed(id)`.
const WRITE_GATE: &str = "maintenance::write_allowed(";

/// The claim-only spelling: `crate::instances::maintenance::maintenance_is_active(id)`.
/// Launch's half of the Dekker pairing, and `delete_backup`'s gate.
const ACTIVE_GATE: &str = "maintenance_is_active(";

/// A command that takes the claim itself is gated by construction — the
/// migration command's spelling, `crate::instances::maintenance::maintenance_begin(id)`.
const CLAIM_GATE: &str = "maintenance_begin(";

/// `commands/datapacks.rs` routes its commands through a file-local
/// `fn guard(instance_id)`, itself listed in `GATED` as a `WRITE_GATE` site.
const DATAPACKS_DELEGATE: &str = "guard(&";

/// Spellings a command body may carry to count as gated. Every entry is a
/// call-site prefix, so a bare mention in prose (a comment line) never counts
/// and a helper with a different name never matches by accident.
const GATE_SPELLINGS: &[&str] = &[WRITE_GATE, ACTIVE_GATE, CLAIM_GATE, DATAPACKS_DELEGATE];

/// `(path relative to src/, fn name, required spelling, why it is a writer)`.
const GATED: &[(&str, &str, &str, &str)] = &[
    (
        "commands/worlds.rs",
        "backup_world",
        WRITE_GATE,
        "zips the world tree a migration may be renaming or copying",
    ),
    (
        "commands/worlds.rs",
        "restore_backup",
        WRITE_GATE,
        "swaps or copies a world into saves/ — the directory a migration stages in",
    ),
    (
        "commands/worlds.rs",
        "delete_world",
        WRITE_GATE,
        "removes the world tree and its backup set — both of which a Move relocates",
    ),
    (
        "commands/worlds.rs",
        "world_import",
        WRITE_GATE,
        "places a new world in saves/ and may take the name a migration is about to pick",
    ),
    (
        "commands/worlds.rs",
        "recover_stranded_world",
        WRITE_GATE,
        "renames a .tmp-* stage — a migration's own live stage included — into saves/",
    ),
    (
        "commands/worlds.rs",
        "delete_backup",
        ACTIVE_GATE,
        "deletes one zip from a backup set `move_set_at` may be renaming file by file; \
         claim-only, so deleting a backup while the game runs stays allowed as before",
    ),
    (
        "commands/datapacks.rs",
        "guard",
        WRITE_GATE,
        "the file-local delegate every datapack writer calls — the gate's one spelling there",
    ),
    (
        "commands/instances.rs",
        "delete_instance",
        WRITE_GATE,
        "remove_dir_all of the instance a migration is reading from or writing into",
    ),
    (
        "commands/instances.rs",
        "rename_instance_dir",
        WRITE_GATE,
        "renames the instance root — every path a running migration holds goes stale",
    ),
    (
        "commands/instances.rs",
        "change_instance_mc",
        WRITE_GATE,
        "changes the version the migration plan's verdict was computed against",
    ),
    (
        "commands/instances.rs",
        "clone_instance",
        WRITE_GATE,
        "copies saves/ wholesale — a half-staged world would be cloned mid-copy",
    ),
    (
        "commands/instances.rs",
        "launch_instance",
        ACTIVE_GATE,
        "must not refuse on is_running (claim_start refuses a same-id relaunch inside \
         start); the maintenance claim alone is checked at command entry",
    ),
    (
        "launch/spawn.rs",
        "start",
        ACTIVE_GATE,
        "the re-check after claim_start — the launch half of the Dekker pairing with \
         the migration's claim-then-check",
    ),
];

/// Commands in the ratcheted files that touch nothing a migration moves:
/// `(path relative to src/, fn name, why it needs no gate)`.
const READ_ONLY: &[(&str, &str, &str)] = &[
    (
        "commands/worlds.rs",
        "list_worlds",
        "stats and lists; a .tmp-* stage is dot-hidden from it",
    ),
    (
        "commands/worlds.rs",
        "list_world_names",
        "sidebar listing, folder names only",
    ),
    ("commands/worlds.rs", "list_backups", "lists zips, touches none"),
    (
        "commands/worlds.rs",
        "open_saves_folder",
        "create_dir_all of saves/ then the OS opener — creates nothing a migration can lose",
    ),
    (
        "commands/worlds.rs",
        "open_backups_folder",
        "create_dir_all of backups/<world>/ then the OS opener; an empty set dir is nothing to lose",
    ),
    (
        "commands/worlds.rs",
        "list_orphaned_backup_worlds",
        "scan only",
    ),
    (
        "commands/worlds.rs",
        "list_stranded_worlds",
        "scan only — it is how a stranded stage is FOUND; recovery is the gated command",
    ),
    (
        "commands/datapacks.rs",
        "datapacks_list_library",
        "read-only view; the registry reconcile it may persist is launcher-owned metadata \
         the game never reads (see the file's module doc)",
    ),
    (
        "commands/datapacks.rs",
        "datapacks_list_for_world",
        "read-only view, same reconcile caveat",
    ),
    (
        "commands/datapacks.rs",
        "datapacks_check_updates",
        "network query over the library listing; installs nothing",
    ),
];

/// Files whose every `#[tauri::command]` must be gated or declared read-only.
const RATCHETED: &[&str] = &["commands/worlds.rs", "commands/datapacks.rs"];

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn read_lines(rel: &str) -> Vec<String> {
    let path = src_dir().join(rel);
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
        .lines()
        .map(str::to_string)
        .collect()
}

/// The name of the fn a COLUMN-0 signature line defines, for `fn`, `pub fn`,
/// `pub(crate) fn`, `pub(super) fn` and their `async` forms. `None` for
/// anything else — including an indented signature (a test fn, a closure, a
/// nested helper), so a same-named fn inside `mod tests` is never the one
/// found.
fn top_level_fn_name(line: &str) -> Option<&str> {
    if line.starts_with(char::is_whitespace) {
        return None;
    }
    let t = line
        .strip_prefix("pub(crate) ")
        .or_else(|| line.strip_prefix("pub(super) "))
        .or_else(|| line.strip_prefix("pub "))
        .unwrap_or(line);
    let t = t.strip_prefix("async ").unwrap_or(t);
    let t = t.strip_prefix("fn ")?;
    let end = t.find(|c: char| c == '(' || c == '<')?;
    Some(&t[..end])
}

/// Index of the line closing the top-level fn opened at `sig`: the first `}`
/// in column 0 at or after it. Sound for rustfmt'd top-level fns, which is all
/// this tree contains (same rule as `structural_no_heavy_sync_command.rs`).
fn body_end(lines: &[String], sig: usize) -> usize {
    lines[sig..]
        .iter()
        .position(|l| l == "}")
        .map(|off| sig + off)
        .unwrap_or(lines.len() - 1)
}

/// `(signature index, body end)` of the top-level fn named `name`.
fn locate_fn(lines: &[String], name: &str) -> Option<(usize, usize)> {
    let sig = lines
        .iter()
        .position(|l| top_level_fn_name(l) == Some(name))?;
    Some((sig, body_end(lines, sig)))
}

/// True when `line` is a `#[tauri::command]` attribute, argumented or not.
fn is_command_attr(line: &str) -> bool {
    line.trim().starts_with("#[tauri::command")
}

/// Every `#[tauri::command]` fn in `lines`: `(name, signature index, body end)`.
fn commands_in(lines: &[String]) -> Vec<(String, usize, usize)> {
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if !is_command_attr(line) {
            continue;
        }
        // Walk forward past any further attributes to the signature.
        let mut j = i + 1;
        let found = loop {
            if j >= lines.len() {
                break None;
            }
            if let Some(name) = top_level_fn_name(&lines[j]) {
                break Some((name.to_string(), j));
            }
            j += 1;
        };
        let Some((name, sig)) = found else { continue };
        out.push((name, sig, body_end(lines, sig)));
    }
    out
}

/// True when a CODE line of `lines[sig..=end]` — the signature line excluded,
/// whole-line `//` comments excluded — contains `needle`.
fn body_carries(lines: &[String], sig: usize, end: usize, needle: &str) -> bool {
    lines[sig..=end]
        .iter()
        .skip(1)
        .filter(|l| !l.trim_start().starts_with("//"))
        .any(|l| l.contains(needle))
}

#[test]
fn every_listed_writer_opens_with_the_gate() {
    let mut violations = Vec::new();
    for (rel, name, needle, why) in GATED {
        let lines = read_lines(rel);
        let Some((sig, end)) = locate_fn(&lines, name) else {
            violations.push(format!(
                "{rel} — `fn {name}` not found at column 0: renamed or removed? \
                 Update GATED together with the code (it is a writer: {why})."
            ));
            continue;
        };
        if !body_carries(&lines, sig, end, needle) {
            violations.push(format!(
                "{rel}:{} — `{name}` carries no `{needle}` on a code line of its body ({why})",
                sig + 1
            ));
        }
    }
    assert!(
        violations.is_empty(),
        "a writer the world-migration claim must hold off does not open with the \
         maintenance gate. Every instance, world and datapack writer calls \
         `crate::instances::maintenance::write_allowed(&id)?` (launch and \
         `delete_backup`: `maintenance_is_active(&id)`) on a code line of its \
         body — a comment naming the gate is not a gate. If a listed fn was \
         renamed, update GATED in the same change.\n{}",
        violations.join("\n"),
    );
}

#[test]
fn every_command_in_a_ratcheted_file_is_gated_or_declared_read_only() {
    let mut violations = Vec::new();
    for rel in RATCHETED {
        let lines = read_lines(rel);
        for (name, sig, end) in commands_in(&lines) {
            let declared_read_only = READ_ONLY
                .iter()
                .any(|(f, n, _)| f == rel && *n == name.as_str());
            if declared_read_only {
                continue;
            }
            let gated = GATE_SPELLINGS
                .iter()
                .any(|needle| body_carries(&lines, sig, end, needle));
            if !gated {
                violations.push(format!(
                    "{rel}:{} — `{name}` is neither gated nor declared read-only",
                    sig + 1
                ));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "a #[tauri::command] in a ratcheted file must choose, in code: open with \
         `crate::instances::maintenance::write_allowed(&id)?` (or the file's \
         delegate `guard(&id)?`, or take the claim with `maintenance_begin`), or \
         be declared in READ_ONLY with the reason it touches nothing a migration \
         moves.\n{}",
        violations.join("\n"),
    );
}

#[test]
fn read_only_entries_still_match() {
    let mut stale = Vec::new();
    for (rel, name, _) in READ_ONLY {
        if !RATCHETED.contains(rel) {
            stale.push(format!(
                "{rel} — not a ratcheted file; a READ_ONLY entry there means nothing"
            ));
            continue;
        }
        let lines = read_lines(rel);
        let Some((sig, end)) = locate_fn(&lines, name) else {
            stale.push(format!("{rel} — `fn {name}` no longer exists"));
            continue;
        };
        let is_command = commands_in(&lines)
            .iter()
            .any(|(n, _, _)| n.as_str() == *name);
        if !is_command {
            stale.push(format!("{rel} — `{name}` is not a #[tauri::command]"));
        }
        if GATE_SPELLINGS
            .iter()
            .any(|needle| body_carries(&lines, sig, end, needle))
        {
            stale.push(format!(
                "{rel}:{} — `{name}` now carries a gate; drop its READ_ONLY entry",
                sig + 1
            ));
        }
    }
    assert!(
        stale.is_empty(),
        "READ_ONLY entry no longer describes the code — fix the entry together \
         with the code.\n{}",
        stale.join("\n"),
    );
}

/// The matchers, pinned directly — same practice as
/// `structural_no_heavy_sync_command.rs`: the scans above only prove what the
/// tree happens to contain today; these pin the decisions the tree does not
/// exercise.
#[cfg(test)]
mod matchers {
    use super::*;

    fn lines(src: &[&str]) -> Vec<String> {
        src.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn fn_names_cover_every_top_level_spelling() {
        assert_eq!(
            top_level_fn_name("pub async fn backup_world("),
            Some("backup_world")
        );
        assert_eq!(
            top_level_fn_name("pub fn delete_backup("),
            Some("delete_backup")
        );
        assert_eq!(
            top_level_fn_name("fn guard(instance_id: &str) -> Result<(), crate::error::Error> {"),
            Some("guard")
        );
        assert_eq!(top_level_fn_name("pub(crate) fn f("), Some("f"));
        assert_eq!(
            top_level_fn_name("pub(super) async fn fetch("),
            Some("fetch")
        );
        assert_eq!(
            top_level_fn_name("pub fn generic<T>(x: T)"),
            Some("generic")
        );
        // Indented: a test fn or a nested helper, never the top-level one.
        assert_eq!(top_level_fn_name("    fn guard(x: bool) -> bool {"), None);
        assert_eq!(top_level_fn_name("    app: tauri::AppHandle,"), None);
        assert_eq!(top_level_fn_name("#[tauri::command]"), None);
        assert_eq!(top_level_fn_name("mod tests {"), None);
    }

    #[test]
    fn a_same_named_test_fn_is_not_mistaken_for_the_command() {
        let l = lines(&[
            "pub fn delete_backup(app: AppHandle) -> Result<()> {",
            "    crate::worlds::backup::delete_backup(&app)",
            "}",
            "#[cfg(test)]",
            "mod tests {",
            "    fn delete_backup() {",
            "        maintenance_is_active(\"x\");",
            "    }",
            "}",
        ]);
        let (sig, end) = locate_fn(&l, "delete_backup").expect("top-level fn found");
        assert_eq!((sig, end), (0, 2));
        assert!(!body_carries(&l, sig, end, ACTIVE_GATE));
    }

    #[test]
    fn a_gate_named_only_in_a_comment_is_not_a_gate() {
        let l = lines(&[
            "#[tauri::command]",
            "#[specta::specta]",
            "pub async fn backup_world(app: AppHandle, id: String) -> Result<()> {",
            "    // maintenance::write_allowed(&id) is checked by the caller.",
            "    crate::worlds::backup::backup_world(&app, &id).await",
            "}",
        ]);
        let (sig, end) = locate_fn(&l, "backup_world").expect("found");
        assert!(!body_carries(&l, sig, end, WRITE_GATE));
    }

    #[test]
    fn a_gate_on_a_code_line_counts_even_with_a_trailing_comment() {
        let l = lines(&[
            "pub async fn f(id: String) -> Result<()> {",
            "    crate::instances::maintenance::write_allowed(&id)?; // A5",
            "    Ok(())",
            "}",
        ]);
        let (sig, end) = locate_fn(&l, "f").expect("found");
        assert!(body_carries(&l, sig, end, WRITE_GATE));
    }

    #[test]
    fn the_signature_line_is_not_scanned() {
        // A fn NAMED after the gate carries nothing by name alone.
        let l = lines(&[
            "pub fn maintenance_is_active(id: &str) -> bool {",
            "    false",
            "}",
        ]);
        let (sig, end) = locate_fn(&l, "maintenance_is_active").expect("found");
        assert!(!body_carries(&l, sig, end, ACTIVE_GATE));
    }

    #[test]
    fn commands_are_found_past_intervening_attributes() {
        let l = lines(&[
            "#[tauri::command]",
            "#[specta::specta]",
            "#[allow(clippy::too_many_arguments)]",
            "pub async fn a(app: AppHandle) -> Result<()> {",
            "    Ok(())",
            "}",
            "",
            "fn not_a_command() {}",
            "",
            "#[tauri::command(rename_all = \"snake_case\")]",
            "pub fn b(app: AppHandle) -> Result<()> {",
            "    Ok(())",
            "}",
        ]);
        assert_eq!(
            commands_in(&l),
            vec![("a".to_string(), 3, 5), ("b".to_string(), 10, 12)]
        );
    }

    #[test]
    fn a_new_ungated_command_in_a_ratcheted_file_would_fail() {
        // The ratchet's decision on a synthetic file: one gated directly, one
        // through the datapacks delegate, one that takes the claim itself,
        // and one that nobody gated or declared.
        let l = lines(&[
            "#[tauri::command]",
            "pub async fn gated(id: String) -> Result<()> {",
            "    crate::instances::maintenance::write_allowed(&id)?;",
            "    Ok(())",
            "}",
            "#[tauri::command]",
            "pub async fn delegated(id: String) -> Result<()> {",
            "    guard(&id)?;",
            "    Ok(())",
            "}",
            "#[tauri::command]",
            "pub async fn claims(id: String) -> Result<()> {",
            "    let _claim = crate::instances::maintenance::maintenance_begin(&id)",
            "        .ok_or(crate::error::Error::InstanceBusy)?;",
            "    Ok(())",
            "}",
            "#[tauri::command]",
            "pub async fn silent(id: String) -> Result<()> {",
            "    crate::worlds::something_that_writes(&id)",
            "}",
        ]);
        let ungated: Vec<String> = commands_in(&l)
            .into_iter()
            .filter(|(_, sig, end)| {
                !GATE_SPELLINGS
                    .iter()
                    .any(|n| body_carries(&l, *sig, *end, n))
            })
            .map(|(name, _, _)| name)
            .collect();
        assert_eq!(ungated, vec!["silent".to_string()]);
    }
}
