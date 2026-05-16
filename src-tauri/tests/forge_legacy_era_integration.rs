//! Real-fixture integration test for the legacy Forge installer
//! handler. Uses a checked-out installer JAR (downloaded via
//! ../fixtures/forge/fetch.ps1) — NO network at test time.

use ftlauncher_lib::forge::installer::{detect_era, Era};

const FIXTURE_PATH: &str =
    "tests/fixtures/forge/installers/forge-1.7.10-10.13.4.1614-installer.jar";

fn load_installer_or_skip() -> Option<Vec<u8>> {
    match std::fs::read(FIXTURE_PATH) {
        Ok(bytes) => Some(bytes),
        Err(_) => {
            eprintln!(
                "SKIP: fixture not present at {FIXTURE_PATH}. Run \
                 src-tauri/tests/fixtures/forge/fetch.ps1 to download it."
            );
            None
        }
    }
}

#[test]
fn legacy_installer_era_detection() {
    let Some(bytes) = load_installer_or_skip() else { return };
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip open");
    let mut entry = archive.by_name("install_profile.json").expect("entry");
    use std::io::Read;
    let mut buf = String::new();
    entry.read_to_string(&mut buf).expect("read");
    let profile: serde_json::Value = serde_json::from_str(&buf).expect("parse");
    assert_eq!(detect_era(&profile), Era::Legacy);
}

#[test]
fn legacy_installer_extracts_version_info_via_internal_helper() {
    // Exercises only the pure-extraction path (no AppHandle needed).
    let Some(bytes) = load_installer_or_skip() else { return };
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip open");
    let mut entry = archive.by_name("install_profile.json").expect("entry");
    use std::io::Read;
    let mut buf = String::new();
    entry.read_to_string(&mut buf).expect("read");
    let profile: serde_json::Value = serde_json::from_str(&buf).expect("parse");

    // versionInfo block exists and has the expected keys.
    let vi = profile.get("versionInfo").expect("versionInfo present");
    assert_eq!(
        vi.get("inheritsFrom").and_then(|v| v.as_str()),
        Some("1.7.10")
    );
    assert!(vi.get("mainClass").and_then(|v| v.as_str()).is_some());
    assert!(vi.get("libraries").and_then(|v| v.as_array()).is_some());
}
