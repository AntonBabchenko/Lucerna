//! Integration test for `instance_dependency_preflight`.
//!
//! Uses `dependency_preflight_for_root` directly (the thin testable core
//! extracted from the Tauri command) to avoid needing a fake `AppHandle`.
//! Builds two crafted in-memory jars, registers them in the installed-mods
//! registry via `mods::installed::add`, then asserts the resolver finds the
//! expected `VersionOutOfRange` violation.

use lucerna_lib::mods::installed;
use lucerna_lib::mods::platform::InstalledMod;
use lucerna_lib::mods::preflight::{dependency_preflight_for_root, ViolationKind};
use sha1::{Digest, Sha1};
use std::io::{Cursor, Write};
use tempfile::TempDir;
use zip::write::SimpleFileOptions;

/// Build an in-memory `.jar` (zip archive) with the given text entries.
fn make_jar(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut w = zip::ZipWriter::new(Cursor::new(&mut buf));
        for (name, body) in entries {
            w.start_file(*name, SimpleFileOptions::default()).unwrap();
            w.write_all(body.as_bytes()).unwrap();
        }
        w.finish().unwrap();
    }
    buf
}

/// Register a jar bytes slice in the instance's installed-mods registry and
/// write the jar to the `mods/` directory. Returns the hex SHA-1.
async fn register(instance_root: &std::path::Path, filename: &str, bytes: &[u8]) -> String {
    let sha1 = hex::encode(Sha1::digest(bytes));
    // Write the jar to disk so the preflight can read it.
    let mods_dir = installed::mods_dir(instance_root);
    tokio::fs::create_dir_all(&mods_dir).await.unwrap();
    tokio::fs::write(mods_dir.join(filename), bytes)
        .await
        .unwrap();
    // Register in the installed-mods registry.
    installed::add(
        instance_root,
        InstalledMod {
            filename: filename.to_string(),
            sha1: sha1.clone(),
            source: None,
            project_id: None,
            version_id: None,
            name: filename
                .strip_suffix(".jar")
                .unwrap_or(filename)
                .to_string(),
            version_number: None,
            installed_at: chrono::Utc::now().to_rfc3339(),
            enabled: true,
            enrich_attempted: false,
            requires: Vec::new(),
        },
    )
    .await
    .unwrap();
    sha1
}

// ── Tests ──────────────────────────────────────────────────────────────────

/// Headline scenario from the unit tests in `preflight.rs`, but exercised
/// end-to-end through real jars on disk and the full installed-mods registry:
///
/// - `backpacks.jar` declares `[[dependencies.backpacks]] modId="sophisticatedcore"
///   mandatory=true versionRange="[1.3.51,)"`.
/// - `core.jar` provides `sophisticatedcore` at version `1.3.50.2005`
///   (below the `[1.3.51,)` floor).
///
/// Expected: exactly one `VersionOutOfRange` for `sophisticatedcore` with
/// `installed_version == Some("1.3.50.2005")`.
#[tokio::test]
async fn version_out_of_range_detected_for_too_low_core() {
    let td = TempDir::new().unwrap();
    let root = td.path();

    // Backpacks jar: requires sophisticatedcore >= 1.3.51
    let backpacks_toml = "\
[[mods]]
modId=\"backpacks\"
version=\"3.20\"

[[dependencies.backpacks]]
    modId=\"sophisticatedcore\"
    mandatory=true
    versionRange=\"[1.3.51,)\"
    side=\"BOTH\"
";
    let backpacks_jar = make_jar(&[("META-INF/mods.toml", backpacks_toml)]);
    register(root, "backpacks-3.20.jar", &backpacks_jar).await;

    // Core jar: provides sophisticatedcore 1.3.50.2005 (too old)
    let core_toml = "\
[[mods]]
modId=\"sophisticatedcore\"
version=\"1.3.50.2005\"
";
    let core_jar = make_jar(&[("META-INF/mods.toml", core_toml)]);
    register(root, "sophisticatedcore-1.3.50.2005.jar", &core_jar).await;

    let report = dependency_preflight_for_root(root).await.unwrap();

    assert_eq!(
        report.violations.len(),
        1,
        "expected exactly 1 violation, got {:?}",
        report
            .violations
            .iter()
            .map(|v| format!("{} / {:?}", v.dep_id, v.kind))
            .collect::<Vec<_>>()
    );
    let v = &report.violations[0];
    assert_eq!(v.dep_id, "sophisticatedcore", "wrong dep_id: {}", v.dep_id);
    assert!(
        matches!(v.kind, ViolationKind::VersionOutOfRange),
        "expected VersionOutOfRange, got {:?}",
        v.kind
    );
    assert_eq!(
        v.installed_version.as_deref(),
        Some("1.3.50.2005"),
        "wrong installed_version: {:?}",
        v.installed_version
    );
    assert_eq!(v.needed, "[1.3.51,)", "wrong needed: {}", v.needed);
}

/// When both mods are installed and the core version satisfies the range,
/// no violations should be produced.
#[tokio::test]
async fn no_violation_when_core_version_satisfies_range() {
    let td = TempDir::new().unwrap();
    let root = td.path();

    let backpacks_toml = "\
[[mods]]
modId=\"backpacks\"
version=\"3.20\"

[[dependencies.backpacks]]
    modId=\"sophisticatedcore\"
    mandatory=true
    versionRange=\"[1.3.51,)\"
    side=\"BOTH\"
";
    let backpacks_jar = make_jar(&[("META-INF/mods.toml", backpacks_toml)]);
    register(root, "backpacks-3.20.jar", &backpacks_jar).await;

    // Core version 1.3.55 satisfies [1.3.51,)
    let core_toml = "\
[[mods]]
modId=\"sophisticatedcore\"
version=\"1.3.55\"
";
    let core_jar = make_jar(&[("META-INF/mods.toml", core_toml)]);
    register(root, "sophisticatedcore-1.3.55.jar", &core_jar).await;

    let report = dependency_preflight_for_root(root).await.unwrap();
    assert!(
        report.violations.is_empty(),
        "expected no violations but got: {:?}",
        report
            .violations
            .iter()
            .map(|v| v.dep_id.as_str())
            .collect::<Vec<_>>()
    );
}

/// When the required dep is completely absent, the result must be
/// `MissingRequired`.
#[tokio::test]
async fn missing_required_when_dep_absent() {
    let td = TempDir::new().unwrap();
    let root = td.path();

    let toml = "\
[[mods]]
modId=\"backpacks\"
version=\"3.20\"

[[dependencies.backpacks]]
    modId=\"sophisticatedcore\"
    mandatory=true
    versionRange=\"[1.3.51,)\"
";
    let jar = make_jar(&[("META-INF/mods.toml", toml)]);
    register(root, "backpacks-3.20.jar", &jar).await;

    let report = dependency_preflight_for_root(root).await.unwrap();
    assert_eq!(report.violations.len(), 1, "{:?}", report.violations);
    assert!(
        matches!(report.violations[0].kind, ViolationKind::MissingRequired),
        "expected MissingRequired, got {:?}",
        report.violations[0].kind
    );
    assert_eq!(report.violations[0].dep_id, "sophisticatedcore");
    assert!(report.violations[0].installed_version.is_none());
}

/// Disabled mods must not be scanned — their declared dependencies should
/// be ignored and their provided ids should not enter the provider index.
///
/// A disabled mod jar lives on disk as `<name>.jar.disabled`. The registry
/// reconciler reads the `.disabled` extension and records `enabled: false`.
/// The pre-flight skips all mods where `enabled == false`.
#[tokio::test]
async fn disabled_mods_not_scanned() {
    let td = TempDir::new().unwrap();
    let root = td.path();

    // A mod that would flag a violation — but it is disabled.
    let toml = "\
[[mods]]
modId=\"backpacks\"
version=\"3.20\"

[[dependencies.backpacks]]
    modId=\"sophisticatedcore\"
    mandatory=true
    versionRange=\"[1.3.51,)\"
";
    let jar = make_jar(&[("META-INF/mods.toml", toml)]);
    let sha1 = hex::encode(Sha1::digest(&jar));
    let mods_dir = installed::mods_dir(root);
    tokio::fs::create_dir_all(&mods_dir).await.unwrap();
    // Write as `.jar.disabled` — the reconciler treats this as enabled=false.
    tokio::fs::write(mods_dir.join("backpacks-3.20.jar.disabled"), &jar)
        .await
        .unwrap();
    // Register with enabled: false and base filename (no ".disabled" suffix).
    installed::add(
        root,
        InstalledMod {
            filename: "backpacks-3.20.jar".into(),
            sha1,
            source: None,
            project_id: None,
            version_id: None,
            name: "backpacks".into(),
            version_number: None,
            installed_at: chrono::Utc::now().to_rfc3339(),
            enabled: false,
            enrich_attempted: false,
            requires: Vec::new(),
        },
    )
    .await
    .unwrap();

    let report = dependency_preflight_for_root(root).await.unwrap();
    assert!(
        report.violations.is_empty(),
        "disabled mods must not be scanned: {:?}",
        report.violations
    );
}

/// An instance with no mods should produce an empty violations list without error.
#[tokio::test]
async fn empty_instance_produces_no_violations() {
    let td = TempDir::new().unwrap();
    let report = dependency_preflight_for_root(td.path()).await.unwrap();
    assert!(report.violations.is_empty());
}
