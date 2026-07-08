//! Structural guard: the Windows NSIS installer MUST stay per-user
//! (`bundle.windows.nsis.installMode = "currentUser"`), so a self-update
//! applies WITHOUT a UAC elevation prompt. Flipping to `perMachine`/`both`
//! would add an admin prompt to every update (and hit documented Tauri
//! elevation-relaunch bugs). Guardrail, not a sandbox.
//!
//! See docs/superpowers/specs/2026-07-08-seamless-windows-update-design.md.

use std::fs;
use std::path::Path;

#[test]
fn nsis_install_mode_is_current_user() {
    let conf_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json");
    let raw = fs::read_to_string(&conf_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", conf_path.display()));
    let conf: serde_json::Value =
        serde_json::from_str(&raw).expect("tauri.conf.json is valid JSON");

    let mode = conf
        .get("bundle")
        .and_then(|b| b.get("windows"))
        .and_then(|w| w.get("nsis"))
        .and_then(|n| n.get("installMode"))
        .and_then(|m| m.as_str());

    assert_eq!(
        mode,
        Some("currentUser"),
        "bundle.windows.nsis.installMode must be \"currentUser\" (per-user, no-UAC \
         updates); found {mode:?}. Changing it to perMachine/both reintroduces a UAC \
         prompt on every update.",
    );
}
