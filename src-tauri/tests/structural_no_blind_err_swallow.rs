//! Structural guard: no blind swallow of a failure.
//!
//! Four rules, all distilled from `docs/PRINCIPLES.md` B.7 and the "Fallback
//! discipline" section of `CLAUDE.md`:
//!
//!   A. An `Err` arm whose body is empty. "An error happened and we do nothing"
//!      is either a missed `NotFound` discrimination or a fail-open. If doing
//!      nothing really is right, say why inside the block.
//!   B. A discarded `Result` from an fs `rename` or `write` — via `let _ =` or a
//!      trailing `.ok()` — with no justification comment.
//!   D. A one-line `let Ok(..) = <fs read> .. else {` whose body answers
//!      `return Ok(..)` — rule A's blind arm in let-else clothing: every
//!      failure, `NotFound` and "could not tell" alike, is laundered into a
//!      success answer. No comment escape; the line-scoped ALLOWLIST is the
//!      only hatch. A wrapped initializer or a tuple `let (Ok(a), Ok(b))` is
//!      invisible to this rule — named gaps, not half-matches.
//!   E. `.unwrap_or_default()` chained onto an fs read on the SAME line — a
//!      permission failure becomes indistinguishable from an absent file.
//!      Escape hatch: rule B's justification comment at the call. The chained
//!      multi-line spelling is a named gap left to review.
//!
//! (There is no rule C. A `Path::exists()` ratchet was measured and rejected —
//! every lexable enforcement form fails against this tree; review owns that
//! question.)
//!
//! Removals are deliberately NOT covered. `let _ = fs::remove_file(&tmp)` after a
//! temp-and-rename write is a legitimate idiom appearing ~42 times in this
//! backend; demanding a comment on each would produce 42 restatements of
//! "best-effort cleanup" and train authors to write the comment without reading
//! it. `rename` and `write` are different: they CHANGE STATE, so discarding one
//! means the change may not have happened and nobody will ever know. Same
//! reasoning, same exclusion, as `structural_no_inplace_mods_write.rs`.
//!
//! This guard enforces a strict SUBSET of "Fallback discipline" question 4.
//! A swallowed removal on a recovery path, and a promised-but-missing log, are
//! both real defects this guard cannot see. That is review's job — which is why
//! the lifecycle's code-review step names the section.
//!
//! Guardrail, not a sandbox — same framing as `structural_no_raw_http.rs`.

use std::fs;
use std::path::{Path, PathBuf};

/// Call-site text for the two state-changing primitives. Matching on the `fs::`
/// prefix covers `std::fs::`, `tokio::fs::` and a bare `fs::` alias alike,
/// because all three carry it at the call site.
const STATE_CHANGING: &[&str] = &["fs::rename(", "fs::write("];

/// Call-site text for the read-side primitives, matched on the `fs::` prefix
/// for the same reason as `STATE_CHANGING`: `std::fs::`, `tokio::fs::` and a
/// bare `fs::` alias all carry it at the call site. `fs::File::open(` is
/// included because it is a read acquisition with the same error surface.
const READ_PRIMITIVES: &[&str] = &[
    "fs::read(",
    "fs::read_to_string(",
    "fs::read_dir(",
    "fs::metadata(",
    "fs::symlink_metadata(",
    "fs::try_exists(",
    "fs::File::open(",
];

/// How far into an `else { .. }` body the success-return scan looks. Every
/// body in the tree today is a single statement; 8 lines is headroom, not a
/// promise — a body that buries `return Ok(..)` deeper evades the rule, and
/// the module doc says so.
const ELSE_BODY_SCAN_LINES: usize = 8;

/// Scheduled exceptions: (path relative to `src/`, the exact violating line
/// content, why it is allowed and who removes it).
///
/// LINE-SCOPED on purpose. A whole-file entry — the shape
/// `structural_no_inplace_mods_write.rs` uses — would also disable rule A for
/// the entire file including its test module, and the PR that fixes this line
/// rewrites the whole surrounding function under that shield.
const ALLOWLIST: &[(&str, &str, &str)] = &[];

/// True for an UNGUARDED `Err(<pat>) => {}` / `=> {},` / `=> ()` / `=> (),` —
/// an error arm whose body does nothing. A comment inside the block makes it
/// non-empty, which is exactly the escape hatch.
///
/// An arm carrying a match guard is exempt, because a guard IS the
/// discrimination this rule asks for:
///
/// ```ignore
/// Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
/// ```
///
/// says "this one identified error is genuinely nothing to do about", which is
/// the opposite of a blind swallow. Flagging it would penalise the exact form
/// the rule exists to encourage — `datapacks/world_link/migrate.rs` is cited in
/// the spec as the model precisely because it writes this next to an unguarded
/// arm that is a real fail-open.
fn is_empty_err_arm(line: &str) -> bool {
    let t = line.trim();
    if !t.starts_with("Err(") {
        return false;
    }
    let Some((pattern, body)) = t.split_once("=>") else {
        return false;
    };
    if pattern.contains(" if ") {
        return false;
    }
    matches!(body.trim().trim_end_matches(','), "{}" | "()")
}

/// True for a discarded `Result` from an fs rename/write.
fn discards_state_change(line: &str) -> bool {
    if !STATE_CHANGING.iter().any(|p| line.contains(p)) {
        return false;
    }
    let t = line.trim_start();
    t.starts_with("let _ =") || line.contains(".ok();")
}

/// True when `line` is a one-line `let Ok(..) = <fs read> .. else {` — the
/// let-else form routes EVERY read failure, `NotFound` and "could not tell"
/// alike, into its else branch. That is rule A's blind `Err(_)` arm in
/// different clothing, which is exactly how the fail-open in
/// `datapacks/world_link/mutate.rs` escaped rule A.
///
/// Only the one-line spelling is matched. rustfmt keeps `else {` on the
/// `let` line whenever the initializer fits — all 57 fs let-else sites in
/// the tree do today (measured); a wrapped initializer puts the fs call and
/// the `else` on different lines and this rule cannot see it. The tuple form
/// `let (Ok(a), Ok(b)) = .. else` is likewise invisible. Both gaps are named
/// here rather than half-matched.
fn is_fs_read_let_else(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("let Ok(")
        && READ_PRIMITIVES.iter().any(|p| line.contains(p))
        && line.contains("else {")
}

/// True when the else block opened at `idx` answers with SUCCESS —
/// `return Ok(..)`. A fallible signature promises "I will tell you when I
/// could not tell"; answering `Ok` out of ignorance is the honesty inversion
/// of Fallback discipline Q2/Q3. `continue`, bare `return;`, and plain-value
/// returns from infallible helpers are deliberately NOT flagged (~53 sites
/// today): an infallible signature is visible at the call site, a laundered
/// `Ok` is not. A let-else body must diverge, so `return Ok(` is the
/// complete lexical signature of the flagged shape.
///
/// There is NO comment escape hatch for this rule. The founding defect
/// (`mutate.rs:26`) already carried `// nothing there — free to place` — a
/// comment that states the fail-open as fact. A comment hatch would excuse
/// it on day one, making the guard a laundering mechanism at birth (the
/// concern the fallback spec §7 names). The escape is the line-scoped
/// ALLOWLIST, which demands a reason string and is stale-checked — or,
/// better, an actual `NotFound` discrimination.
fn else_body_returns_ok(lines: &[&str], idx: usize) -> bool {
    if lines[idx].contains("return Ok(") {
        return true; // fully inline: `else { return Ok(None) };`
    }
    for line in lines.iter().skip(idx + 1).take(ELSE_BODY_SCAN_LINES) {
        let t = line.trim_start();
        if t.starts_with('}') {
            return false; // body closed without a success return
        }
        if t.contains("return Ok(") {
            return true;
        }
    }
    false
}

/// True for `.unwrap_or_default()` chained onto an fs read on the SAME line —
/// "could not read" and "absent" both become the empty value, and downstream
/// code cannot tell a first run from a permission failure. The worst case is
/// read-modify-write: parse the defaulted empty string, edit one key, write
/// the whole file back — a transient read failure silently erases every other
/// key the user set.
///
/// The CHAINED spelling — `.ok()` on one line, `.and_then(..)` and
/// `.unwrap_or_default()` on later lines — is NOT matched: line-based
/// scanning cannot pair them, and those chains collapse parse errors too,
/// which is a different question. Four such sites exist today
/// (`l10n/prefill/cache.rs`, `servers_runtime/{backup,quarantine,transfer}.rs`);
/// they are review's job, same as the removals exclusion above.
///
/// Escape hatch: rule B's — a justification comment trailing the line or on
/// the line above (`is_justified`).
fn defaults_a_read(line: &str) -> bool {
    line.contains(".unwrap_or_default()") && READ_PRIMITIVES.iter().any(|p| line.contains(p))
}

/// A justification must sit AT the discard: trailing the offending line, or on
/// the nearest non-blank line above it, which must be a `//` comment.
///
/// An earlier draft accepted a comment anywhere within 8 lines of the enclosing
/// function, on the theory that a short "best-effort" function documents its own
/// discards. Running it disproved the theory. At the time, `worlds/restore.rs`
/// held a rollback whose comment two lines up said *what* it did ("Roll back:
/// nuke whatever the move left…"), and that was enough to excuse the swallowed
/// `rename` — the exact defect this rule was written to catch. (That code is
/// gone: the restore reorder replaced it with a `match` on both renames. The
/// lesson is why the rule is what it is, not a description of the current tree.)
///
/// There is no syntactic way to tell "this explains what the code does" from
/// "this explains why discarding is correct". A `//` line merely NEAR a discard
/// says nothing; only one written at it does.
fn is_justified(lines: &[&str], idx: usize) -> bool {
    let own = lines[idx];
    if own.contains("//") && !own.trim_start().starts_with("//") {
        return true; // trailing comment on the discard itself
    }
    for j in (0..idx).rev() {
        let t = lines[j].trim_start();
        if t.is_empty() {
            continue; // a blank line between comment and call is fine
        }
        return t.starts_with("//");
    }
    false
}

/// `STATE_CHANGING` matches the qualified call text only. A bare
/// `use std::fs::rename;` would let `rename(a, b)` slip past with no `fs::`
/// substring anywhere. So importing one of these names unqualified from an `fs`
/// module is itself a violation: keep the `fs::` prefix and the guard keeps
/// working. This catches nothing today — it is a ratchet.
fn imports_a_primitive_unqualified(line: &str) -> bool {
    let t = line.trim_start();
    if !t.starts_with("use ") || !t.contains("fs::") {
        return false;
    }
    let tail = t.rsplit("fs::").next().unwrap_or("");
    ["rename", "write"].iter().any(|item| {
        tail.split(|c: char| !c.is_alphanumeric() && c != '_')
            .any(|w| w == *item)
    })
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
fn no_blind_err_swallow_or_unjustified_state_change_discard() {
    let src = src_dir();
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    files.sort();

    let mut violations = Vec::new();
    for file in &files {
        let rel = rel_of(file, &src);
        let content = fs::read_to_string(file).expect("read rust file");
        let lines: Vec<&str> = content.lines().collect();
        // NOTE: unlike `structural_no_inplace_mods_write.rs`, this guard does NOT
        // stop at the first `#[cfg(test)]`. Test code that swallows a rename is
        // not more acceptable, the escape hatch costs one line, and one real
        // `.ok()` site lives inside a test module.
        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue; // prose naming a primitive is fine
            }
            let allowlisted = ALLOWLIST
                .iter()
                .any(|(f, l, _)| *f == rel && line.trim() == *l);
            if allowlisted {
                continue;
            }
            let bad = if is_empty_err_arm(line) {
                Some("empty Err arm")
            } else if discards_state_change(line) && !is_justified(&lines, i) {
                Some("discarded fs rename/write")
            } else if is_fs_read_let_else(line) && else_body_returns_ok(&lines, i) {
                Some("fs read let-else answering Ok out of ignorance")
            } else if defaults_a_read(line) && !is_justified(&lines, i) {
                Some("fs read defaulted without justification")
            } else if imports_a_primitive_unqualified(line) {
                Some("unqualified fs rename/write import")
            } else {
                None
            };
            if let Some(why) = bad {
                violations.push(format!("{}:{} — {} — {}", rel, i + 1, why, line.trim()));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "blind failure swallow (see `## Fallback discipline` in CLAUDE.md).\n\
         \n\
         `empty Err arm`: an error arm that does nothing is a missed discrimination \
         or a fail-open. Best: add a match guard naming the error you mean — \
         `Err(e) if e.kind() == std::io::ErrorKind::NotFound => {{}}` — which this \
         guard treats as correct. Otherwise put the reason INSIDE the block:\n\
         \x20    Err(_) => {{\n\
         \x20        // why doing nothing is right here\n\
         \x20    }}\n\
         A comment ABOVE the arm does not satisfy this rule: it is too easy to \
         satisfy by accident with an unrelated neighbouring comment.\n\
         \n\
         `discarded fs rename/write`: the state change may not have happened and \
         nobody will know. Handle the error, or justify the discard in a comment \
         trailing the line, or a few lines above it inside the same function.\n\
         \n\
         `fs read let-else answering Ok out of ignorance`: `let Ok(x) = <fs read> \
         else {{ return Ok(..) }}` treats every failure — NotFound and `could not \
         tell` alike — as a success answer. Discriminate: match on the error and \
         keep `Ok` for `NotFound` only. If collapsing really is right, add a \
         line-scoped ALLOWLIST entry with the reason — there is deliberately no \
         comment escape for this rule.\n\
         \n\
         `fs read defaulted without justification`: `.unwrap_or_default()` on an \
         fs read makes a permission failure indistinguishable from an absent \
         file. Handle the error, or justify the default in a comment at the call \
         (trailing, or the line above).\n\
         \n{}",
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
        "ALLOWLIST entry no longer matches anything — the code it excused has been \
         fixed or moved. Delete the entry.\n{}",
        stale.join("\n"),
    );
}

/// The matchers, pinned directly.
///
/// The scan above only proves what the tree happens to contain today. These pin
/// the decisions the tree does not exercise — the spellings nobody has written
/// yet, and the two forms that must NOT be flagged. Written after the first real
/// run reported 21 sites, 13 of them accusing correct code.
#[cfg(test)]
mod matchers {
    use super::*;

    #[test]
    fn an_empty_err_arm_matches_every_spelling() {
        assert!(is_empty_err_arm("Err(_) => {}"));
        assert!(is_empty_err_arm("    Err(_e) => {}"));
        assert!(is_empty_err_arm("Err(..) => {},"));
        assert!(is_empty_err_arm("Err(_) => (),"));
    }

    #[test]
    fn a_match_guard_is_discrimination_and_must_not_be_flagged() {
        assert!(!is_empty_err_arm(
            "Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}"
        ));
    }

    #[test]
    fn a_comment_inside_the_block_is_the_escape_hatch() {
        assert!(!is_empty_err_arm(
            "Err(_) => { /* deliberate, and here is why */ }"
        ));
    }

    #[test]
    fn discards_cover_both_spellings_and_all_three_qualifications() {
        assert!(discards_state_change("let _ = std::fs::rename(&a, &b);"));
        assert!(discards_state_change(
            "let _ = tokio::fs::write(&a, b).await;"
        ));
        assert!(discards_state_change("fs::write(&a, b).ok();"));
        // Removals are out of scope by design, and a handled Result is not a discard.
        assert!(!discards_state_change("let _ = std::fs::remove_file(&a);"));
        assert!(!discards_state_change("std::fs::rename(&a, &b)?;"));
    }

    #[test]
    fn a_justification_must_sit_at_the_discard() {
        assert!(is_justified(
            &["// why", "let _ = std::fs::write(a, b);"],
            1
        ));
        assert!(is_justified(&["let _ = std::fs::write(a, b); // why"], 0));
        assert!(is_justified(
            &["// why", "", "let _ = std::fs::write(a, b);"],
            2
        ));

        // The restore-rollback shape. The comment describes WHAT the block does,
        // and sits two lines above the discard — under the original 8-line
        // lookback this excused the defect the whole rule was written to catch.
        let rollback = [
            "// Roll back: put the original back, bubble the original error.",
            "let _ = std::fs::remove_dir_all(&world_path);",
            "let _ = std::fs::rename(&tmp_path, &world_path);",
        ];
        assert!(!is_justified(&rollback, 2));
    }

    #[test]
    fn an_unqualified_import_of_a_primitive_is_itself_a_violation() {
        assert!(imports_a_primitive_unqualified("use std::fs::rename;"));
        assert!(imports_a_primitive_unqualified(
            "use tokio::fs::{self, write};"
        ));
        assert!(!imports_a_primitive_unqualified("use std::fs;"));
        assert!(!imports_a_primitive_unqualified(
            "use std::fs::remove_file;"
        ));
    }

    #[test]
    fn a_read_let_else_answering_ok_is_flagged() {
        // The shape that escaped rule A in `datapacks/world_link/mutate.rs`:
        // any metadata failure reads as "nothing there — free to place", then
        // materialize renames over the destination unconditionally.
        let lines = [
            "    let Ok(dest_meta) = tokio::fs::metadata(dest).await else {",
            "        return Ok(None); // nothing there — free to place",
            "    };",
        ];
        assert!(is_fs_read_let_else(lines[0]));
        assert!(else_body_returns_ok(&lines, 0));

        // The fully inline spelling, and a bare `fs::` alias.
        let inline = ["    let Ok(m) = fs::metadata(&p) else { return Ok(None) };"];
        assert!(is_fs_read_let_else(inline[0]));
        assert!(else_body_returns_ok(&inline, 0));
    }

    #[test]
    fn a_listing_let_else_returning_a_plain_default_is_not_flagged() {
        // ~53 read-only listing fallbacks in the tree look like this. An
        // infallible signature is visible to the caller, so they are not the
        // honesty inversion this rule targets.
        let lines = [
            "    let Ok(rd) = std::fs::read_dir(&saves_dir) else {",
            "        return Vec::new();",
            "    };",
        ];
        assert!(is_fs_read_let_else(lines[0]));
        assert!(!else_body_returns_ok(&lines, 0));

        // `continue` in a loop body, inline (instances/import/model.rs shape).
        let cont = ["        let Ok(rd) = fs::read_dir(&d) else { continue };"];
        assert!(!else_body_returns_ok(&cont, 0));
    }

    #[test]
    fn a_non_fs_let_else_is_out_of_scope() {
        // Parse and path-helper fallbacks answer a different question; only
        // the filesystem boundary is this rule's business.
        assert!(!is_fs_read_let_else(
            "    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {"
        ));
        assert!(!is_fs_read_let_else(
            "    let Ok(path) = crate::paths::app_file(app) else {"
        ));
    }

    #[test]
    fn a_defaulted_fs_read_is_flagged_across_qualifications() {
        // The read-modify-write shape (commands/servers_runtime.rs:295): a
        // transient read failure parses as an empty properties file, one key
        // is set, and the write-back erases everything else.
        assert!(defaults_a_read(
            "    let raw = std::fs::read_to_string(&props_path).unwrap_or_default();"
        ));
        assert!(defaults_a_read(
            "    Ok(std::fs::read_to_string(&path).unwrap_or_default())"
        ));
        assert!(defaults_a_read(
            "    let bytes = tokio::fs::read(&p).await.unwrap_or_default();"
        ));
    }

    #[test]
    fn only_same_line_fs_defaults_are_in_scope() {
        // The chained spelling is a NAMED gap, not a match — the
        // `.unwrap_or_default()` line carries no fs text.
        assert!(!defaults_a_read("        .unwrap_or_default()"));
        // A defaulted parse with no fs call on the line is out of scope.
        assert!(!defaults_a_read(
            "    let cfg: Config = serde_json::from_str(&s).unwrap_or_default();"
        ));
    }

    #[test]
    fn a_justified_defaulted_read_is_the_escape_hatch() {
        // Same rule as B: the reason must sit AT the call.
        assert!(is_justified(
            &[
                "// sysfs probe: an absent vendor file is an unknown adapter,",
                "// which the label match below already renders as-is.",
                "let vendor = std::fs::read_to_string(dev.join(\"vendor\")).unwrap_or_default();",
            ],
            2
        ));
        assert!(is_justified(
            &["let raw = std::fs::read_to_string(&p).unwrap_or_default(); // seeded below on first run"],
            0
        ));
    }
}
