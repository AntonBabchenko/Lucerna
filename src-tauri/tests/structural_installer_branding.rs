//! Structural guard: the NSIS installer must stay branded + localized and must
//! not build the (never-published) MSI. Fails the build if a branding key is
//! dropped or `msi` is re-added to bundle.targets.
//! See docs/superpowers/specs/2026-07-08-installer-styling-design.md.

use std::fs;
use std::path::Path;

fn conf() -> serde_json::Value {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json");
    let raw = fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()));
    serde_json::from_str(&raw).expect("tauri.conf.json is valid JSON")
}

#[test]
fn nsis_is_branded_and_localized() {
    let c = conf();
    let nsis = c
        .pointer("/bundle/windows/nsis")
        .and_then(|n| n.as_object())
        .expect("bundle.windows.nsis object present");

    for key in [
        "installerIcon",
        "headerImage",
        "sidebarImage",
        "displayLanguageSelector",
    ] {
        assert!(
            nsis.contains_key(key),
            "bundle.windows.nsis.{key} must be set"
        );
    }
    assert_eq!(
        nsis.get("displayLanguageSelector")
            .and_then(|v| v.as_bool()),
        Some(true),
        "displayLanguageSelector must be true"
    );

    let langs: Vec<&str> = nsis
        .get("languages")
        .and_then(|l| l.as_array())
        .expect("nsis.languages array present")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(langs.contains(&"English"), "languages must include English");
    assert!(langs.contains(&"Russian"), "languages must include Russian");
}

#[test]
fn bundle_does_not_build_msi() {
    let c = conf();
    let targets = c
        .pointer("/bundle/targets")
        .expect("bundle.targets present");
    let arr = targets
        .as_array()
        .expect("bundle.targets must be an explicit array (not \"all\"), so msi can be excluded");
    let names: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
    assert!(
        !names.contains(&"msi"),
        "bundle.targets must not include \"msi\" (never published)"
    );
    assert!(
        names.contains(&"nsis"),
        "bundle.targets must still include \"nsis\""
    );
}
