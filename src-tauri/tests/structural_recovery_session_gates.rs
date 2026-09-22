//! Structural guard: commands whose effect ESCAPES a recovery session's
//! throwaway root, or loses user content with it, refuse first.
//!
//! When the configured data folder cannot be used, the launcher runs a
//! recovery session on a throwaway root (`data_root::recovery`): whatever any
//! command writes under the data root is thrown away at exit, so containment —
//! not a list — is what keeps the OS-default folder clean. Preferences are
//! therefore ACCEPTED in such a session and merely said to be temporary; a
//! refusal would freeze the UI language and the theme, because the frontend's
//! save contract is "roll back on a failed write".
//!
//! Containment does not cover everything, though, and what it does not cover
//! is listed here:
//!
//!   - ACCOUNTS. Sign-in writes tokens to the OS KEYRING, which no data root
//!     contains. The `account.json` entry naming them would vanish with the
//!     session and orphan the secrets.
//!   - SKIN LIBRARY. User content, silently lost with the session dir.
//!   - `install_version`. Hundreds of MB downloaded into a directory that is
//!     deleted at exit — and the ungated twin of `install_instance`, which has
//!     carried the gate since the fallback was introduced.
//!
//! `reject_if_root_unusable` also refuses during and after a data-folder move,
//! which is the right answer for every one of them.
//!
//! Why a test and not a comment: the gate on `install_instance` next to an
//! ungated `install_version` is what a comment-only rule produces. A listed fn
//! that no longer exists fails too, so the list cannot rot.
//!
//! Guardrail, not a static analyzer: the scan is lexical. It proves the gate
//! is CALLED on a code line of the body before the body's first `.await`, not
//! that no early return precedes it.

use std::fs;
use std::path::{Path, PathBuf};

const GATE: &str = "reject_if_root_unusable(";

/// `(file under src/, fn, why it is listed)`.
const GATED: &[(&str, &str, &str)] = &[
    (
        "commands/accounts.rs",
        "begin_microsoft_signin",
        "writes tokens to the OS keyring; account.json vanishes with the session",
    ),
    (
        "commands/accounts.rs",
        "refresh_microsoft_account",
        "rewrites both keyring slots and account.json",
    ),
    (
        "commands/accounts.rs",
        "add_offline_account",
        "an account that disappears at exit",
    ),
    (
        "commands/accounts.rs",
        "set_active_account",
        "a choice that disappears at exit",
    ),
    (
        "commands/accounts.rs",
        "remove_account",
        "deletes keyring entries by an id read from a throwaway account.json",
    ),
    (
        "commands/skin_library.rs",
        "skin_library_save",
        "user content lost with the session dir",
    ),
    (
        "commands/skin_library.rs",
        "skin_library_update",
        "user content lost with the session dir",
    ),
    (
        "commands/skin_library.rs",
        "skin_library_delete",
        "same store as save / update",
    ),
    (
        "commands/versions.rs",
        "install_version",
        "a large download into a directory deleted at exit; twin of install_instance",
    ),
];

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

/// The name of the fn a COLUMN-0 signature line defines.
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

/// `(signature index, index of the closing column-0 brace)`.
fn locate_fn(lines: &[String], name: &str) -> Option<(usize, usize)> {
    let sig = lines
        .iter()
        .position(|l| top_level_fn_name(l) == Some(name))?;
    let end = lines[sig..]
        .iter()
        .position(|l| l == "}")
        .map(|off| sig + off)
        .unwrap_or(lines.len() - 1);
    Some((sig, end))
}

fn is_code(line: &str) -> bool {
    !line.trim_start().starts_with("//")
}

/// What is wrong with `name`'s body, if anything.
fn verdict(lines: &[String], name: &str) -> Option<String> {
    let Some((sig, end)) = locate_fn(lines, name) else {
        return Some("no such top-level fn (renamed or removed? update GATED)".into());
    };
    let body = sig + 1..=end;
    let gate = body
        .clone()
        .find(|&i| is_code(&lines[i]) && lines[i].contains(GATE));
    let first_await = body
        .clone()
        .find(|&i| is_code(&lines[i]) && lines[i].contains(".await"));
    match (gate, first_await) {
        (None, _) => Some(format!("does not call `{GATE}…)`")),
        (Some(g), Some(a)) if g > a => Some(format!(
            "calls the gate on line {} — AFTER its first `.await` on line {}",
            g + 1,
            a + 1
        )),
        _ => None,
    }
}

#[test]
fn every_command_that_escapes_a_recovery_session_refuses_first() {
    let mut problems = Vec::new();
    for (file, name, why) in GATED {
        let lines = read_lines(file);
        if let Some(problem) = verdict(&lines, name) {
            problems.push(format!(
                "{file}::{name} {problem}\n      listed because: {why}"
            ));
        }
    }
    assert!(
        problems.is_empty(),
        "commands that must refuse in a recovery session (and during a data move):\n  {}",
        problems.join("\n  ")
    );
}

/// The scanner, pinned directly: a guard that cannot fail proves nothing.
#[cfg(test)]
mod matchers {
    use super::*;

    fn lines(src: &str) -> Vec<String> {
        src.lines().map(str::to_string).collect()
    }

    #[test]
    fn a_gated_command_passes() {
        let src = "pub async fn f(app: AppHandle) -> Result<()> {\n    crate::data_root::reject_if_root_unusable(&app)?;\n    work().await\n}";
        assert_eq!(verdict(&lines(src), "f"), None);
    }

    #[test]
    fn an_ungated_command_fails() {
        let src = "pub async fn f(app: AppHandle) -> Result<()> {\n    work().await\n}";
        assert!(verdict(&lines(src), "f").unwrap().contains("does not call"));
    }

    #[test]
    fn a_gate_that_only_appears_in_a_comment_fails() {
        let src = "pub fn f(app: AppHandle) -> Result<()> {\n    // reject_if_root_unusable(&app)?;\n    Ok(())\n}";
        assert!(verdict(&lines(src), "f").is_some());
    }

    #[test]
    fn a_gate_after_the_first_await_fails() {
        let src = "pub async fn f(app: AppHandle) -> Result<()> {\n    fetch().await?;\n    crate::data_root::reject_if_root_unusable(&app)?;\n    Ok(())\n}";
        assert!(verdict(&lines(src), "f").unwrap().contains("AFTER"));
    }

    #[test]
    fn a_missing_fn_fails_so_the_list_cannot_rot() {
        assert!(verdict(&lines("pub fn g() {}\n"), "f")
            .unwrap()
            .contains("no such top-level fn"));
    }

    #[test]
    fn a_same_named_fn_inside_a_test_module_is_not_the_one_found() {
        let src = "mod tests {\n    fn f() {\n        reject_if_root_unusable(&app);\n    }\n}";
        assert!(verdict(&lines(src), "f")
            .unwrap()
            .contains("no such top-level fn"));
    }
}
