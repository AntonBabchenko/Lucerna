//! Structural guard: a `#[tauri::command]` that calls
//! `installed::reconcile_on_list` must be `async`.
//!
//! `reconcile_on_list` does a full byte read + SHA-1 of every jar in a server's
//! `mods/` or `plugins/` directory — hundreds of megabytes on a real server. A
//! Tauri command declared without `async` runs on the MAIN thread, the one
//! driving the event loop and the webview, so such a command freezes the entire
//! window for the duration of the scan. That is exactly the bug this guard was
//! written for: opening a server's Add-ons tab locked the launcher for ~2 s on
//! the first (cold hash-cache) scan.
//!
//! The fix is always the same shape, already used by five other call sites in
//! `commands/servers_runtime.rs`: make the command `async fn` and run the
//! blocking body inside `tokio::task::spawn_blocking`. A comment would not have
//! prevented the regression — copying a neighbouring handler is how the two
//! synchronous outliers came to exist in the first place.

use std::fs;
use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read_dir commands") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().map(|e| e == "rs").unwrap_or(false) {
            out.push(path);
        }
    }
}

/// `Some(true)` when `line` opens an async function definition, `Some(false)`
/// for a synchronous one, `None` when it is not a function signature at all.
fn fn_signature(line: &str) -> Option<bool> {
    let t = line.trim_start();
    if t.starts_with("pub async fn ") || t.starts_with("async fn ") {
        return Some(true);
    }
    if t.starts_with("pub fn ") || t.starts_with("fn ") {
        return Some(false);
    }
    None
}

#[test]
fn tauri_commands_calling_reconcile_on_list_are_async() {
    let commands = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("commands");
    let mut files = Vec::new();
    rust_files(&commands, &mut files);
    files.sort();

    let mut violations = Vec::new();
    for file in files {
        let content = fs::read_to_string(&file).expect("read rust file");
        let lines: Vec<&str> = content.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if line.trim() != "#[tauri::command]" {
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
            if lines[j..=end].iter().any(|l| l.contains("reconcile_on_list")) {
                violations.push(format!("{}:{}  {}", file.display(), j + 1, lines[j].trim()));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "synchronous #[tauri::command] calling reconcile_on_list runs the whole \
         scan on the main thread and freezes the window — make it `async fn` \
         with the body in tokio::task::spawn_blocking:\n{}",
        violations.join("\n"),
    );
}
