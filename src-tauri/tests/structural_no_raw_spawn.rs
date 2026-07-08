//! Structural guard: no subprocess `Command` is constructed outside the
//! `process::` module. Every spawn must go through `process::` so the
//! set of processes the launcher runs is enumerable and documented in
//! `docs/PRINCIPLES.md` Appendix A. Guardrail, not a sandbox.
//!
//! ## Second surface — `tauri_plugin_opener`
//!
//! The opener plugin (`app.opener().open_path(..)` / `.open_url(..)`) also
//! spawns an OS process — the file manager (`explorer.exe`) or the default
//! browser — outside the `process::` chokepoint. Those spawns are documented
//! in `docs/PRINCIPLES.md` Appendix A too, so this guard also enumerates every
//! opener call site against a fixed allowlist. A NEW `open_path`/`open_url`
//! call in a file not on the allowlist fails the build, forcing the author to
//! either route it through the documented set or update Appendix A + this list.

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
            // The opener plugin's process-spawning calls. `.opener()` alone is
            // the accessor; the actual spawn is open_path/open_url.
            let spawns = line.contains(".open_path(") || line.contains(".open_url(");
            if spawns && !allowed {
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
