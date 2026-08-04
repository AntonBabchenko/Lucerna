//! Integration test for `instance_dependency_preflight`.
//!
//! Uses `dependency_preflight_for_root` directly (the thin testable core
//! extracted from the Tauri command) to avoid needing a fake `AppHandle`.
//! Builds two crafted in-memory jars, registers them in the installed-mods
//! registry via `mods::installed::add`, then asserts the resolver finds the
//! expected `VersionOutOfRange` violation.

use lucerna_lib::instances::schema::LoaderKind;
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

/// Build an in-memory `.jar` whose entries are raw bytes — a `.class` file's
/// constant pool is not UTF-8.
fn make_jar_raw(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut w = zip::ZipWriter::new(Cursor::new(&mut buf));
        for (name, body) in entries {
            w.start_file(*name, SimpleFileOptions::default()).unwrap();
            w.write_all(body).unwrap();
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

    let report = dependency_preflight_for_root(root, LoaderKind::Forge, "1.20.1")
        .await
        .unwrap();

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

    let report = dependency_preflight_for_root(root, LoaderKind::Forge, "1.20.1")
        .await
        .unwrap();
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

    let report = dependency_preflight_for_root(root, LoaderKind::Forge, "1.20.1")
        .await
        .unwrap();
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

    let report = dependency_preflight_for_root(root, LoaderKind::Forge, "1.20.1")
        .await
        .unwrap();
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
    let report = dependency_preflight_for_root(td.path(), LoaderKind::Forge, "1.20.1")
        .await
        .unwrap();
    assert!(report.violations.is_empty());
}

/// The measured Parasites: Reloaded shape, byte-for-byte in structure.
///
/// `EnhancedVisuals_v1.4.4_mc1.12.2.jar` ships BOTH `mcmod.info` (which declares
/// no dependency) and a `META-INF/mods.toml` stamped `loaderVersion="[24,)"` —
/// a descriptor written for its 1.14+ build that Forge 1.12.2 never opens. The
/// requirement FML actually enforces lives in the `@Mod(dependencies = …)`
/// annotation and carries no range. `CreativeCore_v1.10.71_mc1.12.2.jar` ships
/// only `mcmod.info`, at version `1.10`.
///
/// Before this change the pre-flight read the wrong file and reported
/// `EnhancedVisuals … needs creativecore`, which gated Play on a pack that runs.
#[tokio::test]
async fn legacy_instance_ignores_a_mods_toml_written_for_a_newer_era() {
    let td = TempDir::new().unwrap();
    let root = td.path();

    let ev = make_jar_raw(&[
        (
            "mcmod.info",
            br#"[{"modid":"enhancedvisuals","version":"1.3","dependencies":[]}]"# as &[u8],
        ),
        (
            "META-INF/mods.toml",
            b"modLoader=\"javafml\"\nloaderVersion=\"[24,)\"\n[[mods]]\nmodId=\"enhancedvisuals\"\n\
              [[dependencies.enhancedvisuals]]\nmodId=\"creativecore\"\nmandatory=true\n\
              versionRange=\"[2.0.0,)\"\n" as &[u8],
        ),
        (
            "team/creative/EV.class",
            b"\x00\x0fenhancedvisuals\x00\x1brequired-after:creativecore\x00" as &[u8],
        ),
    ]);
    register(root, "EnhancedVisuals_v1.4.4_mc1.12.2.jar", &ev).await;

    let cc = make_jar_raw(&[(
        "mcmod.info",
        br#"[{"modid":"creativecore","name":"CreativeCore","version":"1.10"}]"# as &[u8],
    )]);
    register(root, "CreativeCore_v1.10.71_mc1.12.2.jar", &cc).await;

    let legacy = dependency_preflight_for_root(root, LoaderKind::Forge, "1.12.2")
        .await
        .unwrap();
    assert!(
        legacy.violations.is_empty(),
        "1.12.2 must read the annotation (no range) and see CreativeCore via mcmod.info, \
         not the 1.14+ mods.toml range: {:?}",
        legacy.violations
    );

    // The very same jars on a modern instance take the mods.toml path, where
    // 1.10 really is outside [2.0.0,). Pinned so the era predicate cannot be
    // satisfied by simply dropping mods.toml everywhere.
    let modern = dependency_preflight_for_root(root, LoaderKind::Forge, "1.20.1")
        .await
        .unwrap();
    assert_eq!(modern.violations.len(), 1, "{:?}", modern.violations);
    assert!(
        matches!(modern.violations[0].kind, ViolationKind::VersionOutOfRange),
        "expected VersionOutOfRange, got {:?}",
        modern.violations[0].kind
    );
}

/// Defect A: a jar shipping BOTH Forge descriptors stopped being checked at all
/// on a MinecraftForge instance, because it was parsed as `neoforge.mods.toml`
/// only and that source is (correctly) not admitted there.
#[tokio::test]
async fn a_dual_descriptor_jar_is_still_checked_on_minecraftforge() {
    let td = TempDir::new().unwrap();
    let root = td.path();

    let both = make_jar(&[
        (
            "META-INF/neoforge.mods.toml",
            "[[mods]]\nmodId=\"multi\"\n\
             [[dependencies.multi]]\nmodId=\"nf_lib\"\ntype=\"required\"\n",
        ),
        (
            "META-INF/mods.toml",
            "[[mods]]\nmodId=\"multi\"\n\
             [[dependencies.multi]]\nmodId=\"mf_lib\"\nmandatory=true\n",
        ),
    ]);
    register(root, "multi-1.0.jar", &both).await;

    let forge = dependency_preflight_for_root(root, LoaderKind::Forge, "1.20.1")
        .await
        .unwrap();
    assert_eq!(forge.violations.len(), 1, "{:?}", forge.violations);
    assert_eq!(
        forge.violations[0].dep_id, "mf_lib",
        "MinecraftForge reads mods.toml and nothing else"
    );

    // On NeoForge the same jar reports through neoforge.mods.toml, and its
    // mods.toml is shadowed — exactly one violation, the other id.
    let neo = dependency_preflight_for_root(root, LoaderKind::NeoForge, "1.20.1")
        .await
        .unwrap();
    assert_eq!(neo.violations.len(), 1, "{:?}", neo.violations);
    assert_eq!(
        neo.violations[0].dep_id, "nf_lib",
        "mods.toml must be shadowed on NeoForge, not merged"
    );
}

/// Defect B: the version a range is measured against must come from the file
/// FML opens. A 1.12.2 jar's `mods.toml` is written for its 1.14+ build.
#[tokio::test]
async fn a_legacy_provider_version_comes_from_mcmod_info_not_the_inert_mods_toml() {
    let td = TempDir::new().unwrap();
    let root = td.path();

    // CreativeCore: mcmod.info says 1.10 (what FML 1.12.2 sees), mods.toml says
    // 2.0 (written for the 1.14+ build, never opened on this instance).
    let core = make_jar(&[
        (
            "mcmod.info",
            r#"[{"modid":"creativecore","version":"1.10"}]"#,
        ),
        (
            "META-INF/mods.toml",
            "[[mods]]\nmodId=\"creativecore\"\nversion=\"2.0\"\n",
        ),
    ]);
    register(root, "CreativeCore.jar", &core).await;

    // A dependent whose annotation requires creativecore >= 2.0.
    let ev = make_jar_raw(&[
        (
            "mcmod.info",
            br#"[{"modid":"ev","version":"1.0"}]"# as &[u8],
        ),
        (
            "team/EV.class",
            b"\x00\x02ev\x00\x1brequired-after:creativecore@[2.0,)\x00" as &[u8],
        ),
    ]);
    register(root, "EnhancedVisuals.jar", &ev).await;

    let report = dependency_preflight_for_root(root, LoaderKind::Forge, "1.12.2")
        .await
        .unwrap();
    assert_eq!(report.violations.len(), 1, "{:?}", report.violations);
    assert!(
        matches!(report.violations[0].kind, ViolationKind::VersionOutOfRange),
        "1.10 is what FML sees and it is out of [2.0,); the inert mods.toml 2.0 \
         must not satisfy the range: {:?}",
        report.violations[0]
    );
    assert_eq!(
        report.violations[0].installed_version.as_deref(),
        Some("1.10")
    );
}

/// A provider declared only in a file this loader does not read is STILL
/// installed. Presence is a union — filtering it the way dependencies are
/// filtered would turn a real mod into a phantom "not installed".
#[tokio::test]
async fn a_provider_from_an_unread_descriptor_still_counts_as_installed() {
    let td = TempDir::new().unwrap();
    let root = td.path();

    // Declares itself ONLY in mods.toml, on a 1.12.2 instance where that file
    // is never opened.
    let lib = make_jar(&[(
        "META-INF/mods.toml",
        "[[mods]]\nmodId=\"somelib\"\nversion=\"1.0\"\n",
    )]);
    register(root, "somelib.jar", &lib).await;

    let dependent = make_jar_raw(&[
        (
            "mcmod.info",
            br#"[{"modid":"dep","version":"1.0"}]"# as &[u8],
        ),
        (
            "team/D.class",
            b"\x00\x03dep\x00\x18required-after:somelib\x00" as &[u8],
        ),
    ]);
    register(root, "dependent.jar", &dependent).await;

    let report = dependency_preflight_for_root(root, LoaderKind::Forge, "1.12.2")
        .await
        .unwrap();
    assert!(
        report.violations.is_empty(),
        "the jar is installed; only its VERSION is unreadable here: {:?}",
        report.violations
    );
}
