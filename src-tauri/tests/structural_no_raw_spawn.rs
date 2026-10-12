//! Structural guard: no subprocess `Command` is constructed outside the
//! `process::` module. Every spawn must go through `process::` so the
//! set of processes the launcher runs is enumerable and documented in
//! `docs/PRINCIPLES.md` Appendix A. Guardrail, not a sandbox.
//!
//! ## Second surface — `tauri_plugin_opener`
//!
//! The opener plugin (`app.opener().open_path(..)` / `.open_url(..)` /
//! `.reveal_item_in_dir(..)`) also spawns an OS process — the file manager
//! (`explorer.exe`) or the default browser — outside the `process::`
//! chokepoint. `docs/PRINCIPLES.md` Appendix A documents these spawns under
//! two process classes: the OS file manager (the folder-open and reveal rows)
//! and the default browser row, which also covers the frontend `openUrl`
//! surface gated separately by `tools/check-opener-calls.mjs`. This guard
//! enumerates every backend opener call site against a fixed allowlist. A NEW
//! opener call in a file not on the allowlist fails the build, forcing the
//! author to either route it through the documented set or update Appendix A
//! + this list.
//!
//! ## Third surface — the `trash` crate
//!
//! On macOS the `trash` crate's default `DeleteMethod::Finder` builds
//! `Command::new("osascript")` inside the dependency to ask Finder to delete —
//! a spawn the first test cannot see, absent from Appendix A, and one that can
//! raise an Automation permission prompt. `platform/recycle_bin.rs` is the one
//! file allowed to name the crate, and it selects `DeleteMethod::NsFileManager`
//! (in-process `trashItemAtURL`). A call anywhere else would get the default
//! back, so it fails the build.

use std::fs;
use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read_dir src") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().map(|e| e == "rs").unwrap_or(false) {
            out.push(path);
        }
    }
}

/// True for the opener plugin's process-spawning calls. `.opener()` alone is
/// the accessor; the spawns are `open_path` (file manager on a directory),
/// `open_url` (default browser), and `reveal_item_in_dir` (file manager with
/// the item pre-selected — the same explorer.exe spawn as `open_path`).
fn is_opener_spawn(line: &str) -> bool {
    line.contains(".open_path(")
        || line.contains(".open_url(")
        || line.contains(".reveal_item_in_dir(")
}

#[test]
fn no_command_constructed_outside_process_module() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let process_dir = src.join("process");
    let mut files = Vec::new();
    rust_files(&src, &mut files);

    let mut violations = Vec::new();
    for file in files {
        if file.starts_with(&process_dir) {
            continue; // the chokepoint module is allowed to build Commands
        }
        let content = fs::read_to_string(&file).expect("read rust file");
        for (i, line) in content.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue; // skip comments
            }
            if line.contains("Command::new") || line.contains("process::Command") {
                violations.push(format!("{}:{}", file.display(), i + 1));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "subprocess Command constructed outside process::\n{}",
        violations.join("\n"),
    );
}

/// Enumerated allowlist of files that legitimately invoke the opener plugin
/// (each spawn is documented in `docs/PRINCIPLES.md` Appendix A). Paths are
/// relative to `src/` and use `/` separators (normalised below for Windows).
///
/// Adding a NEW opener call site means: document the spawned process in
/// Appendix A, then add the file here. That keeps the opener surface as
/// enumerable as the `Command` surface.
const OPENER_ALLOWLIST: &[&str] = &[
    // Folder-open commands — spawn the OS file manager (explorer.exe et al.).
    "commands/data_location.rs",
    "commands/logs.rs",
    "commands/instance_import_cmds.rs",
    "commands/servers_runtime.rs",
    "commands/instances.rs",
    "commands/worlds.rs",
    "commands/screenshots.rs",
    "instances/mod.rs",
    // Microsoft sign-in — opens the auth URL in the default browser.
    "accounts/microsoft/mod.rs",
];

#[test]
fn opener_spawns_only_from_allowlisted_files() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);

    let mut violations = Vec::new();
    for file in files {
        // Path relative to src/, normalised to forward slashes so the
        // allowlist matches on every platform.
        let rel = file
            .strip_prefix(&src)
            .expect("file under src")
            .to_string_lossy()
            .replace('\\', "/");
        let allowed = OPENER_ALLOWLIST.contains(&rel.as_str());

        let content = fs::read_to_string(&file).expect("read rust file");
        for (i, line) in content.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") || trimmed.starts_with("//!") {
                continue; // skip comments (incl. doc mentions of open_path)
            }
            if is_opener_spawn(line) && !allowed {
                violations.push(format!("{}:{}", file.display(), i + 1));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "tauri_plugin_opener spawn outside the documented allowlist \
         (document the process in docs/PRINCIPLES.md Appendix A and add the \
         file to OPENER_ALLOWLIST):\n{}",
        violations.join("\n"),
    );
}

/// The one file allowed to name the `trash` crate (relative to `src/`).
const TRASH_CHOKEPOINT: &str = "platform/recycle_bin.rs";

/// True for a reference to the external `trash` crate. Matched by its API
/// names, not by a bare `trash::` path: Lucerna's own `mods::trash` module is
/// reached as `trash::parse_token(..)` after `use super::trash`, and must not
/// match. `trash::delete` also covers `delete_all`. Any import of the crate is
/// itself a hit — `use trash::..`, `use trash;`, `use trash as t;` — so an
/// alias cannot hide a call from the API-name needles.
fn names_trash_crate(line: &str) -> bool {
    line.contains("trash::delete")
        || line.contains("TrashContext")
        || line.contains("trash::os_limited")
        || imports_trash_crate(line)
}

/// `use trash`, `use ::trash` or `extern crate trash`, followed by anything
/// but more of an identifier (`use trash_bin::..` is another crate).
fn imports_trash_crate(line: &str) -> bool {
    ["use trash", "use ::trash", "extern crate trash"]
        .iter()
        .any(|pat| {
            line.match_indices(pat).any(|(i, _)| {
                !line[i + pat.len()..].starts_with(|c: char| c.is_alphanumeric() || c == '_')
            })
        })
}

/// Files under `src/` that name the `trash` crate on a non-comment line,
/// other than the chokepoint itself. Returns `path:line` strings.
fn trash_crate_uses_outside_chokepoint(src: &Path) -> Vec<String> {
    let mut files = Vec::new();
    rust_files(src, &mut files);
    let mut violations = Vec::new();
    for file in files {
        let rel = file
            .strip_prefix(src)
            .expect("file under src")
            .to_string_lossy()
            .replace('\\', "/");
        if rel == TRASH_CHOKEPOINT {
            continue;
        }
        let content = fs::read_to_string(&file).expect("read rust file");
        for (i, line) in content.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue; // skip comments (incl. doc mentions of the crate)
            }
            if names_trash_crate(line) {
                violations.push(format!("{}:{}", file.display(), i + 1));
            }
        }
    }
    violations
}

#[test]
fn trash_crate_called_only_from_the_recycle_bin_chokepoint() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let violations = trash_crate_uses_outside_chokepoint(&src);
    assert!(
        violations.is_empty(),
        "the `trash` crate named outside src/{TRASH_CHOKEPOINT}: its macOS default \
         spawns `osascript`, a process docs/PRINCIPLES.md does not list. Call \
         `crate::platform::recycle_bin::to_recycle_bin` (through `off_thread`) \
         instead:\n{}",
        violations.join("\n"),
    );
}

/// True when production code (the lines before `#[cfg(test)]`, comments
/// skipped) selects `DeleteMethod::NsFileManager`. The test module is cut
/// off: the chokepoint's macOS unit test names the same variant, and must not
/// satisfy this pin on its own.
fn production_selects_ns_file_manager(content: &str) -> bool {
    content
        .lines()
        .take_while(|line| !line.trim_start().starts_with("#[cfg(test)]"))
        .any(|line| {
            !line.trim_start().starts_with("//") && line.contains("DeleteMethod::NsFileManager")
        })
}

/// The chokepoint must actually select the in-process method — a text pin
/// that runs on every OS. The behaviour itself is pinned on macOS by the
/// chokepoint's own unit test.
#[test]
fn the_recycle_bin_chokepoint_selects_ns_file_manager() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(TRASH_CHOKEPOINT);
    let content =
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let pinned = production_selects_ns_file_manager(&content);
    assert!(
        pinned,
        "{} must set `DeleteMethod::NsFileManager` on macOS: the `trash` crate's \
         default asks Finder through an `osascript` process",
        path.display(),
    );
}

/// The matcher, pinned directly. The one `reveal_item_in_dir` call in the
/// tree sits in an allowlisted file, so the scan alone cannot prove the
/// needle works. Same rationale as the `matchers` module in
/// `structural_no_blind_err_swallow.rs`.
#[cfg(test)]
mod matchers {
    use super::*;

    #[test]
    fn reveal_item_in_dir_is_a_spawn() {
        // The commands/screenshots.rs:96 shape: the receiver sits on the
        // previous line, the call starts its own.
        assert!(is_opener_spawn("        .reveal_item_in_dir(&path)"));
    }

    #[test]
    fn open_path_and_open_url_are_spawns() {
        // Real shapes: commands/worlds.rs:128 and accounts/microsoft/mod.rs:42.
        assert!(is_opener_spawn(
            "        .open_path(dir.to_string_lossy().to_string(), None::<&str>)"
        ));
        assert!(is_opener_spawn(
            "    if let Err(e) = app.opener().open_url(&auth_url, None::<&str>) {"
        ));
    }

    #[test]
    fn the_accessor_alone_is_not_a_spawn() {
        assert!(!is_opener_spawn("    app.opener()"));
    }

    #[test]
    fn the_trash_crate_is_named() {
        // Real shapes: screenshots/mod.rs and mods/asset_folder.rs before the
        // move, plus the forms a new call could take.
        for line in [
            "    trash::delete(&path).map_err(|e| Error::io(path.display().to_string(), e))?;",
            "        use trash::macos::{DeleteMethod, TrashContextExtMacos};",
            "        let mut ctx = trash::TrashContext::default();",
            "    ::trash::delete_all(&paths)?;",
            "use trash::{delete, TrashContext};",
            "    let items = trash::os_limited::list()?;",
            // An alias would hide `t::delete(..)` from the API-name needles,
            // so the import itself is the hit.
            "use trash as t;",
            "use trash;",
            "pub use trash::delete;",
            "extern crate trash;",
        ] {
            assert!(names_trash_crate(line), "should match: {line}");
        }
    }

    #[test]
    fn another_crate_whose_name_starts_with_trash_is_not_it() {
        assert!(!names_trash_crate("use trash_bin::Bin;"));
        assert!(!names_trash_crate("extern crate trashcan;"));
    }

    #[test]
    fn lucernas_own_trash_module_is_not_the_crate() {
        // Real shapes from mods/txn.rs (through `use super::trash`),
        // commands/mods.rs, and the module's own imports.
        for line in [
            "        if trash::parse_token(&name).is_none() {",
            "    let token = trash::new_token(now);",
            "    let outcome = crate::mods::trash::restore(&inst_root, &token).await?;",
            "    let trashed = crate::mods::trash::uninstall_to_trash(",
            "use super::trash;",
        ] {
            assert!(!names_trash_crate(line), "should not match: {line}");
        }
    }

    #[test]
    fn only_production_code_satisfies_the_ns_file_manager_pin() {
        let in_tests_only = "fn context() -> TrashContext {\n    TrashContext::new()\n}\n\
             #[cfg(test)]\nmod tests {\n    \
             assert!(matches!(m, DeleteMethod::NsFileManager));\n}\n";
        assert!(!production_selects_ns_file_manager(in_tests_only));
        let commented = "// ctx.set_delete_method(DeleteMethod::NsFileManager);\n";
        assert!(!production_selects_ns_file_manager(commented));
        let real = "        ctx.set_delete_method(DeleteMethod::NsFileManager);\n";
        assert!(production_selects_ns_file_manager(real));
    }
}
