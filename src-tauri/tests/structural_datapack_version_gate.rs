//! Structural guard: every client datapack WRITER refuses on a Minecraft with
//! no data-pack system (spec 2026-09-24 §4 U2, amendment A25).
//!
//! Data packs arrived in 1.13. On a 1.12.2 instance a written pack is inert:
//! the library accepts it, a world lists it as enabled, and the game reads none
//! of it. The UI hides the surface there, but its IPC-error fallback is
//! deliberately permissive ("uncertainty must not hide the feature"). The
//! refusal that keeps a permissive UI from writing is therefore
//! `commands/datapacks.rs`'s `require_datapack_support(&app, &id)?`. It reads a
//! fresh `instance.json` and asks `datapacks::compat::require_support`.
//!
//! Rules, one test each:
//!   1. every fn in `VERSION_GATED` calls `require_datapack_support(` on a code
//!      line of its body, AFTER its maintenance gate `guard(&` (A7: the
//!      running/maintenance gate first, then the version gate);
//!   2. every `#[tauri::command]` in the file that opens with `guard(&` is in
//!      `VERSION_GATED` or in `VERSION_EXEMPT` with a reason, so a new writer
//!      cannot land without a decision;
//!   3. the helper itself reads the instance and asks compat, because a helper
//!      that returns `Ok(())` is not a gate;
//!   4. both lists name real commands, and no name is on both.
//!
//! Whole-line `//` comments are not scanned: a comment is not a gate. This is a
//! separate file from `structural_maintenance_gate.rs` on purpose (A25). That
//! guard pins one gate across many files; this one pins a second gate in one
//! file.

use std::fs;
use std::path::Path;

const FILE: &str = "commands/datapacks.rs";
const VERSION_GATE: &str = "require_datapack_support(";
const MAINTENANCE_GATE: &str = "guard(&";
const HELPER: &str = "require_datapack_support";

/// (command, what it writes)
const VERSION_GATED: &[(&str, &str)] = &[
    (
        "datapacks_install_from_file",
        "installs a pack into the instance's library",
    ),
    (
        "datapacks_install_from_version",
        "downloads a pack into the instance's library",
    ),
    (
        "datapacks_update_one",
        "replaces a library pack and relinks it into worlds",
    ),
    (
        "datapacks_add_to_world",
        "links a pack into a world and enables it in level.dat",
    ),
    (
        "datapacks_set_enabled_in_world",
        "rewrites a world's level.dat pack lists",
    ),
];

/// Guarded commands deliberately NOT version-gated: (command, why).
const VERSION_EXEMPT: &[(&str, &str)] = &[
    (
        "datapacks_remove_from_library",
        "a removal: cleanup must work on any version, and leftover files are inert (A10)",
    ),
    (
        "datapacks_remove_from_world",
        "a removal: same reason (A10)",
    ),
];

fn read_lines() -> Vec<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(FILE);
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
        .lines()
        .map(str::to_string)
        .collect()
}

/// Same matcher as `structural_maintenance_gate.rs`: the name a COLUMN-0 fn
/// signature defines; `None` for indented (test / nested) fns.
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

/// First column-0 `}` at or after `sig` (sound for rustfmt'd top-level fns).
fn body_end(lines: &[String], sig: usize) -> usize {
    lines[sig..]
        .iter()
        .position(|l| l == "}")
        .map(|off| sig + off)
        .unwrap_or(lines.len() - 1)
}

fn locate_fn(lines: &[String], name: &str) -> Option<(usize, usize)> {
    let sig = lines
        .iter()
        .position(|l| top_level_fn_name(l) == Some(name))?;
    Some((sig, body_end(lines, sig)))
}

/// Every `#[tauri::command]` fn: (name, signature index, body end).
fn commands_in(lines: &[String]) -> Vec<(String, usize, usize)> {
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if !line.trim().starts_with("#[tauri::command") {
            continue;
        }
        let found = (i + 1..lines.len())
            .find_map(|j| top_level_fn_name(&lines[j]).map(|n| (n.to_string(), j)));
        if let Some((name, sig)) = found {
            out.push((name, sig, body_end(lines, sig)));
        }
    }
    out
}

/// First CODE line of the body (signature and whole-line `//` excluded)
/// containing `needle`.
fn first_code_line_with(lines: &[String], sig: usize, end: usize, needle: &str) -> Option<usize> {
    (sig + 1..=end).find(|&i| {
        let l = &lines[i];
        !l.trim_start().starts_with("//") && l.contains(needle)
    })
}

#[test]
fn every_datapack_writer_checks_the_version_after_the_maintenance_gate() {
    let lines = read_lines();
    let mut violations = Vec::new();
    for (name, why) in VERSION_GATED {
        let Some((sig, end)) = locate_fn(&lines, name) else {
            violations.push(format!(
                "{FILE} — `fn {name}` not found at column 0: renamed or removed? \
                 Update VERSION_GATED with the code ({why})."
            ));
            continue;
        };
        let Some(gate) = first_code_line_with(&lines, sig, end, VERSION_GATE) else {
            violations.push(format!(
                "{FILE}:{} — `{name}` never calls `{VERSION_GATE}` on a code line ({why})",
                sig + 1
            ));
            continue;
        };
        match first_code_line_with(&lines, sig, end, MAINTENANCE_GATE) {
            Some(g) if g < gate => {}
            Some(g) => violations.push(format!(
                "{FILE}:{} — `{name}` checks the version before its maintenance gate \
                 (line {}); the order is the gate, then the version (A7)",
                gate + 1,
                g + 1
            )),
            None => violations.push(format!(
                "{FILE}:{} — `{name}` has no `{MAINTENANCE_GATE}`; a writer opens with it",
                sig + 1
            )),
        }
    }
    assert!(
        violations.is_empty(),
        "a datapack writer does not refuse pre-1.13 instances. Call \
         `require_datapack_support(&app, &instance_id)?;` right after \
         `guard(&instance_id)?;`.\n{}",
        violations.join("\n")
    );
}

#[test]
fn every_guarded_datapack_command_is_version_gated_or_exempt() {
    let lines = read_lines();
    let listed = |n: &str| {
        VERSION_GATED.iter().any(|(g, _)| *g == n) || VERSION_EXEMPT.iter().any(|(e, _)| *e == n)
    };
    let violations: Vec<String> = commands_in(&lines)
        .into_iter()
        .filter(|(_, sig, end)| {
            first_code_line_with(&lines, *sig, *end, MAINTENANCE_GATE).is_some()
        })
        .filter(|(name, _, _)| !listed(name))
        .map(|(name, sig, _)| {
            format!(
                "{FILE}:{} — `{name}` writes (it opens with `{MAINTENANCE_GATE}`) but is in \
                 neither VERSION_GATED nor VERSION_EXEMPT",
                sig + 1
            )
        })
        .collect();
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[test]
fn the_version_gate_reads_the_instance_and_asks_compat() {
    let lines = read_lines();
    let (sig, end) = locate_fn(&lines, HELPER)
        .unwrap_or_else(|| panic!("{FILE} — `fn {HELPER}` not found at column 0"));
    assert!(
        first_code_line_with(&lines, sig, end, "read_instance(").is_some(),
        "`{HELPER}` must read a fresh instance.json (the UI's gate is permissive on error)"
    );
    assert!(
        first_code_line_with(&lines, sig, end, "compat::require_support(").is_some(),
        "`{HELPER}` must ask `datapacks::compat::require_support` — one rule, one place"
    );
}

#[test]
fn the_lists_name_real_commands_and_do_not_overlap() {
    let lines = read_lines();
    let commands: Vec<String> = commands_in(&lines).into_iter().map(|(n, _, _)| n).collect();
    for (name, _) in VERSION_GATED.iter().chain(VERSION_EXEMPT) {
        assert!(
            commands.iter().any(|c| c == name),
            "`{name}` is listed but is not a #[tauri::command] in {FILE}"
        );
    }
    for (name, _) in VERSION_GATED {
        assert!(
            !VERSION_EXEMPT.iter().any(|(e, _)| e == name),
            "`{name}` is both gated and exempt"
        );
    }
}

/// The matchers themselves, pinned on synthetic input.
#[cfg(test)]
mod matchers {
    use super::*;

    fn lines(src: &[&str]) -> Vec<String> {
        src.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_gate_named_only_in_a_comment_is_not_a_gate() {
        let l = lines(&[
            "pub async fn f(app: AppHandle, instance_id: String) -> Result<()> {",
            "    guard(&instance_id)?;",
            "    // require_datapack_support(&app, &instance_id)? is checked elsewhere.",
            "    Ok(())",
            "}",
        ]);
        assert_eq!(first_code_line_with(&l, 0, 4, VERSION_GATE), None);
    }

    #[test]
    fn the_signature_line_is_not_scanned() {
        let l = lines(&[
            "fn require_datapack_support(app: &AppHandle) -> Result<()> {",
            "    Ok(())",
            "}",
        ]);
        let (sig, end) = locate_fn(&l, HELPER).expect("found");
        assert_eq!(first_code_line_with(&l, sig, end, VERSION_GATE), None);
    }

    #[test]
    fn a_version_gate_above_the_maintenance_gate_is_found_first() {
        let l = lines(&[
            "pub async fn f(app: AppHandle, instance_id: String) -> Result<()> {",
            "    require_datapack_support(&app, &instance_id)?;",
            "    guard(&instance_id)?;",
            "}",
        ]);
        let gate = first_code_line_with(&l, 0, 3, VERSION_GATE).expect("gate");
        let maint = first_code_line_with(&l, 0, 3, MAINTENANCE_GATE).expect("maint");
        assert!(
            gate < maint,
            "the order check must see this as out of order"
        );
    }
}
