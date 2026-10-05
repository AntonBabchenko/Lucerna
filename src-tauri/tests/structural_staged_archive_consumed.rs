//! Structural guard: the commands that consume a staged modpack archive
//! release it, however they return.
//!
//! `modpack_fetch_to_temp` stages a pack version under
//! `<temp>/lucerna/modpack/` and hands its path to the webview, which passes it
//! on to `modpack_import` or `modpack_apply_update`. Each of those opens with
//! `let <name> = crate::mods::modpack::source::stage::ConsumeGuard::new(&app,
//! &<path>);` — the guard removes the staged file when it drops
//! (`mods::modpack::source::stage` has the lifecycle):
//!
//! - its FIRST statement, so a refusal before anything is read releases the
//!   file too;
//! - bound to a NAMED local, because `let _ = ConsumeGuard::new(..)` drops the
//!   guard on the spot — the archive would be gone before the command reads
//!   it — and `#[must_use]` does not fire on an explicit `let _`.
//!
//! Why a test and not a comment: without the binding nothing fails. No test
//! reaches a command without an `AppHandle`, and the day-old sweep hides the
//! leak a day at a time — the pile-up this binding fixed. A command that
//! starts consuming a staged path joins `CONSUMERS` in the same change; a
//! listed fn that no longer exists fails, so the list cannot rot.
//!
//! Lexical, in the shape of `structural_maintenance_gate.rs`: a fn is found by
//! its column-0 signature, its body starts after the first line ending in `{`
//! and ends at the first column-0 `}`. Whole-line `//` comments are skipped,
//! and the first statement is read across the lines rustfmt may wrap it over.

use std::fs;
use std::path::Path;

/// The file holding every consumer.
const FILE: &str = "commands/modpack_cmds.rs";

/// `(command, the parameter holding the staged path)`.
const CONSUMERS: &[(&str, &str)] = &[
    ("modpack_import", "path"),
    ("modpack_apply_update", "mrpack_path"),
];

fn read_lines() -> Vec<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(FILE);
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
        .lines()
        .map(str::to_string)
        .collect()
}

/// The body lines of the column-0 `pub async fn <name>(`.
fn body<'a>(lines: &'a [String], name: &str) -> Option<&'a [String]> {
    let signature = format!("pub async fn {name}(");
    let sig = lines.iter().position(|l| l.starts_with(&signature))?;
    let open = (sig..lines.len()).find(|&i| lines[i].trim_end().ends_with('{'))?;
    let end = (open + 1..lines.len()).find(|&i| lines[i].starts_with('}'))?;
    Some(&lines[open + 1..end])
}

/// The body's first statement: its code lines up to the first one ending in
/// `;` or `{`, joined by a space.
fn first_statement(body: &[String]) -> Option<String> {
    let mut parts = Vec::new();
    for line in body.iter().map(|l| l.trim()) {
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        parts.push(line);
        if line.ends_with(';') || line.ends_with('{') {
            break;
        }
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// Why `statement` is not a named binding of the guard over `param`, if it is
/// not one. Read with every space removed, so rustfmt's wrapping (and the
/// trailing comma it adds) changes nothing.
fn binding_problem(statement: &str, param: &str) -> Option<String> {
    let compact = statement
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .replace(",)", ")");
    let call = format!("::ConsumeGuard::new(&app,&{param})");
    if !compact.contains(&call) {
        return Some(format!("does not call `ConsumeGuard::new(&app, &{param})`"));
    }
    let Some(rest) = compact.strip_prefix("let") else {
        return Some("does not bind the guard with `let`".into());
    };
    let rest = rest.strip_prefix("mut").unwrap_or(rest);
    let binding = &rest[..rest.find(['=', ':']).unwrap_or(rest.len())];
    if binding.is_empty() || binding == "_" {
        return Some(format!(
            "binds the guard to `{binding}`, which drops it, and the archive, at once"
        ));
    }
    None
}

#[test]
fn every_consumer_binds_the_staged_archive_guard_first() {
    let lines = read_lines();
    let mut problems = Vec::new();
    for (name, param) in CONSUMERS {
        let Some(body) = body(&lines, name) else {
            problems.push(format!(
                "{name}: no `pub async fn {name}(` in src/{FILE} — renamed or moved? Update CONSUMERS"
            ));
            continue;
        };
        match first_statement(body) {
            None => problems.push(format!("{name}: empty body")),
            Some(statement) => {
                if let Some(why) = binding_problem(&statement, param) {
                    problems.push(format!("{name}: its first statement `{statement}` {why}"));
                }
            }
        }
    }
    assert!(
        problems.is_empty(),
        "A command consuming a staged modpack archive must open with \
         `let _staged = crate::mods::modpack::source::stage::ConsumeGuard::new(&app, &<path>);` \
         (see mods::modpack::source::stage):\n{}",
        problems.join("\n")
    );
}

#[test]
fn the_reading_names_a_wrong_binding() {
    let bound =
        "let _staged = crate::mods::modpack::source::stage::ConsumeGuard::new(&app, &path);";
    assert_eq!(binding_problem(bound, "path"), None);
    let discarded = "let _ = crate::mods::modpack::source::stage::ConsumeGuard::new(&app, &path);";
    assert!(binding_problem(discarded, "path").is_some());
    let typed = "let _: ConsumeGuard = crate::mods::modpack::source::stage::ConsumeGuard::new(&app, &path);";
    assert!(binding_problem(typed, "path").is_some());
    let unbound = "crate::data_root::reject_if_root_unusable(&app)?;";
    assert!(binding_problem(unbound, "path").is_some());
    let other_path =
        "let _staged = crate::mods::modpack::source::stage::ConsumeGuard::new(&app, &x);";
    assert!(binding_problem(other_path, "path").is_some());
    let wrapped = [
        "    let _staged = crate::mods::modpack::source::stage::ConsumeGuard::new(".to_string(),
        "        &app,".to_string(),
        "        &mrpack_path,".to_string(),
        "    );".to_string(),
    ];
    let statement = first_statement(&wrapped).unwrap();
    assert_eq!(
        binding_problem(&statement, "mrpack_path"),
        None,
        "{statement}"
    );
}
