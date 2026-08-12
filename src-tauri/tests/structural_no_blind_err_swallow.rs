//! Structural guard: no blind swallow of a failure.
//!
//! Two rules, both distilled from `docs/PRINCIPLES.md` B.7 and the "Fallback
//! discipline" section of `CLAUDE.md`:
//!
//!   A. An `Err` arm whose body is empty. "An error happened and we do nothing"
//!      is either a missed `NotFound` discrimination or a fail-open. If doing
//!      nothing really is right, say why inside the block.
//!   B. A discarded `Result` from an fs `rename` or `write` — via `let _ =` or a
//!      trailing `.ok()` — with no justification comment.
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

/// Scheduled exceptions: (path relative to `src/`, the exact violating line
/// content, why it is allowed and who removes it).
///
/// LINE-SCOPED on purpose. A whole-file entry — the shape
/// `structural_no_inplace_mods_write.rs` uses — would also disable rule A for
/// the entire file including its test module, and the PR that fixes this line
/// rewrites the whole surrounding function under that shield.
const ALLOWLIST: &[(&str, &str, &str)] = &[(
    "worlds/restore.rs",
    "let _ = std::fs::rename(&tmp_path, &world_path);",
    "The restore rollback swallow — the defect this whole rule was written to catch, \
         tracked by docs/superpowers/specs/2026-08-12-world-restore-rollback-design.md. The \
         follow-up PR rewrites this block and MUST delete this entry; \
         `allowlist_entries_still_match` below turns red if it does not.",
)];

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

/// A justification must sit AT the discard: trailing the offending line, or on
/// the nearest non-blank line above it, which must be a `//` comment.
///
/// An earlier draft accepted a comment anywhere within 8 lines of the enclosing
/// function, on the theory that a short "best-effort" function documents its own
/// discards. Running it disproved the theory: in `worlds/restore.rs` the rollback
/// carries a comment two lines up saying *what* it does ("Roll back: nuke
/// whatever the move left…"), and that was enough to excuse the swallowed
/// `rename` — the exact defect this whole rule was written to catch.
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
}
