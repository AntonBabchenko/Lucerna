//! Integration test for `instance_dependency_preflight`.
//!
//! Uses `dependency_preflight_for_root` directly (the thin testable core
//! extracted from the Tauri command) to avoid needing a fake `AppHandle`.
//! Builds two crafted in-memory jars, registers them in the installed-mods
//! registry via `mods::installed::add`, then asserts the resolver finds the
//! expected `VersionOutOfRange` violation.

use lucerna_lib::instances::schema::LoaderKind;
use lucerna_lib::mods::installed;
use lucerna_lib::mods::jar_scan_cache::ScanCache;
use lucerna_lib::mods::platform::InstalledMod;
use lucerna_lib::mods::preflight::{dependency_preflight_for_root, parse_instance, ViolationKind};
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

/// A javac-compiled class from `tests/fixtures/legacy_mod/classes/` — real
/// constant-pool layout, length bytes included (see that directory's README).
macro_rules! class {
    ($path:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/legacy_mod/classes/",
            $path
        ))
        .as_slice()
    };
}

/// `mcmod.info` naming one mod at one version.
fn mcmod(modid: &str, version: &str) -> Vec<u8> {
    format!(r#"[{{"modid":"{modid}","version":"{version}"}}]"#).into_bytes()
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

    let report = dependency_preflight_for_root(root, None, LoaderKind::Forge, "1.20.1", None)
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

    let report = dependency_preflight_for_root(root, None, LoaderKind::Forge, "1.20.1", None)
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

    let report = dependency_preflight_for_root(root, None, LoaderKind::Forge, "1.20.1", None)
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

/// A disabled mod never declares requirements: the loader never reads a
/// `.jar.disabled`, so nothing it requires can stop the launch.
///
/// The pre-flight does read the jar — so a requirement only a disabled mod
/// could meet is reported as `RequiredDisabled` rather than plain "missing" —
/// but never reads its declarations as requirements, and a disabled jar still
/// satisfies nothing (its ids never enter the provider index).
///
/// A disabled mod jar lives on disk as `<name>.jar.disabled`. The registry
/// reconciler reads the `.disabled` extension and records `enabled: false`.
#[tokio::test]
async fn disabled_mods_never_declare_requirements() {
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

    let report = dependency_preflight_for_root(root, None, LoaderKind::Forge, "1.20.1", None)
        .await
        .unwrap();
    assert!(
        report.violations.is_empty(),
        "a disabled mod's requirements must never be judged: {:?}",
        report.violations
    );
}

/// An instance with no mods should produce an empty violations list without error.
#[tokio::test]
async fn empty_instance_produces_no_violations() {
    let td = TempDir::new().unwrap();
    let report = dependency_preflight_for_root(td.path(), None, LoaderKind::Forge, "1.20.1", None)
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
            class!("fixture/EnhancedVisuals.class"),
        ),
    ]);
    register(root, "EnhancedVisuals_v1.4.4_mc1.12.2.jar", &ev).await;

    let cc = make_jar_raw(&[(
        "mcmod.info",
        br#"[{"modid":"creativecore","name":"CreativeCore","version":"1.10"}]"# as &[u8],
    )]);
    register(root, "CreativeCore_v1.10.71_mc1.12.2.jar", &cc).await;

    let legacy = dependency_preflight_for_root(root, None, LoaderKind::Forge, "1.12.2", None)
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
    let modern = dependency_preflight_for_root(root, None, LoaderKind::Forge, "1.20.1", None)
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

    let forge = dependency_preflight_for_root(root, None, LoaderKind::Forge, "1.20.1", None)
        .await
        .unwrap();
    assert_eq!(forge.violations.len(), 1, "{:?}", forge.violations);
    assert_eq!(
        forge.violations[0].dep_id, "mf_lib",
        "MinecraftForge reads mods.toml and nothing else"
    );

    // On NeoForge the same jar reports through neoforge.mods.toml, and its
    // mods.toml is shadowed — exactly one violation, the other id.
    let neo = dependency_preflight_for_root(root, None, LoaderKind::NeoForge, "1.20.1", None)
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
        ("team/EV.class", class!("fixture/Ev.class")),
    ]);
    register(root, "EnhancedVisuals.jar", &ev).await;

    let report = dependency_preflight_for_root(root, None, LoaderKind::Forge, "1.12.2", None)
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
        ("team/D.class", class!("fixture/Dep.class")),
    ]);
    register(root, "dependent.jar", &dependent).await;

    let report = dependency_preflight_for_root(root, None, LoaderKind::Forge, "1.12.2", None)
        .await
        .unwrap();
    assert!(
        report.violations.is_empty(),
        "the jar is installed; only its VERSION is unreadable here: {:?}",
        report.violations
    );
}

/// The report carries what the pack is still waiting for, so the launch gate can
/// tell "you assembled this wrong" from "this pack fills itself in on first run".
#[tokio::test]
async fn the_report_carries_pack_completion_when_the_helper_is_present() {
    let td = TempDir::new().unwrap();
    let root = td.path();

    let plain = dependency_preflight_for_root(root, None, LoaderKind::Forge, "1.20.1", None)
        .await
        .unwrap();
    assert!(
        plain.pack_completion.is_none(),
        "an instance with no helper reports nothing"
    );

    let cfg = root.join(".minecraft").join("config");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(
        cfg.join("missing_mods_checker.json"),
        r#"[{"displayName":"Balm","pattern":"balm.jar","destination":"mods"}]"#,
    )
    .unwrap();

    let with = dependency_preflight_for_root(root, None, LoaderKind::Forge, "1.20.1", None)
        .await
        .unwrap();
    let c = with.pack_completion.expect("helper present");
    assert_eq!(c.total, 1);
    assert_eq!(c.outstanding.len(), 1);
    assert_eq!(c.outstanding[0].display_name, "Balm");
}

// ── the jar-scan cache ─────────────────────────────────────────────────────

/// THE WIRING RED. Red on pre-cache code for the plainest possible reason:
/// `jar_scan_cache` had no call site, so the file was never created and the
/// `expect` below panics. Green once the pre-flight writes through it — and the
/// four assertions pin WHAT it wrote, which is where the honesty lives.
#[tokio::test]
async fn the_preflight_stores_what_it_parsed_under_the_jars_on_disk_digest() {
    let td = TempDir::new().unwrap();
    let root = td.path();
    let cache = root.join("mods-cache").join("jar-scans.json");

    let dependent = make_jar(&[(
        "META-INF/mods.toml",
        "[[mods]]\nmodId=\"alpha\"\nversion=\"1.0\"\n\n\
         [[dependencies.alpha]]\n    modId=\"absent_mod\"\n    mandatory=true\n    \
         versionRange=\"[1.0,)\"\n    side=\"BOTH\"\n",
    )]);
    let sha = register(root, "alpha.jar", &dependent).await;

    assert!(!cache.exists(), "nothing has scanned yet");
    let report =
        dependency_preflight_for_root(root, Some(&cache), LoaderKind::Forge, "1.20.1", None)
            .await
            .unwrap();
    assert_eq!(report.violations.len(), 1, "{:?}", report.violations);

    let stored = ScanCache::load(&cache);
    let hit = stored
        .get(&sha)
        .expect("the scanned jar is cached under its digest");
    assert!(hit.manifest.is_some(), "the manifest the pre-flight read");
    assert!(hit.jij_provided.is_some(), "the JIJ pass it also ran");
    assert!(
        hit.legacy.is_none(),
        "a modern-era scan never opened the annotation and must not claim it did"
    );
    assert!(hit.meta.is_none(), "and never ran the compat scan's reader");
}

/// A modern-era scan must not teach a legacy-era scan that a jar needs nothing.
///
/// The `@Mod(dependencies = …)` annotation is read ONLY on the legacy era, so a
/// record written while scanning a 1.20.1 instance has never looked at it. If
/// that absence were stored as a fact, the 1.12.2 pass below would read it back
/// and report zero violations for a jar that genuinely requires a mod nobody
/// installed. Fixture shape copied from
/// `a_legacy_provider_version_comes_from_mcmod_info_not_the_inert_mods_toml`.
#[tokio::test]
async fn a_modern_scan_does_not_teach_the_legacy_scan_that_a_jar_needs_nothing() {
    let td = TempDir::new().unwrap();
    let root = td.path();
    let cache = root.join("mods-cache").join("jar-scans.json");

    let ev = make_jar_raw(&[
        (
            "mcmod.info",
            br#"[{"modid":"ev","version":"1.0"}]"# as &[u8],
        ),
        ("team/EV.class", class!("fixture/EvPlain.class")),
    ]);
    register(root, "EnhancedVisuals.jar", &ev).await;

    // Warm the cache from a MODERN instance: the annotation is never read.
    let modern =
        dependency_preflight_for_root(root, Some(&cache), LoaderKind::Forge, "1.20.1", None)
            .await
            .unwrap();
    assert!(
        modern.violations.is_empty(),
        "the modern era does not enforce the annotation: {:?}",
        modern.violations
    );

    // The same jar, on the era that DOES enforce it, through the same cache.
    let legacy =
        dependency_preflight_for_root(root, Some(&cache), LoaderKind::Forge, "1.12.2", None)
            .await
            .unwrap();
    assert_eq!(
        legacy.violations.len(),
        1,
        "the legacy scan must read the annotation itself, not inherit the modern scan's silence: {:?}",
        legacy.violations
    );
    assert_eq!(legacy.violations[0].dep_id, "creativecore");
}

/// Replacing a jar's bytes in place must change the answer, even though the
/// registry still carries the digest of the jar the launcher installed
/// (`installed::reconcile` step 2 keeps it on purpose). This is why the key is
/// `installed::on_disk_sha1` and not `InstalledMod::sha1`.
#[tokio::test]
async fn replacing_a_jars_bytes_in_place_invalidates_its_cached_scan() {
    let td = TempDir::new().unwrap();
    let root = td.path();
    let cache = root.join("mods-cache").join("jar-scans.json");

    let first = make_jar(&[(
        "META-INF/mods.toml",
        "[[mods]]\nmodId=\"alpha\"\nversion=\"1.0\"\n\n\
         [[dependencies.alpha]]\n    modId=\"needs_one\"\n    mandatory=true\n    \
         versionRange=\"[1.0,)\"\n    side=\"BOTH\"\n",
    )]);
    register(root, "alpha.jar", &first).await;
    let a = dependency_preflight_for_root(root, Some(&cache), LoaderKind::Forge, "1.20.1", None)
        .await
        .unwrap();
    assert_eq!(a.violations[0].dep_id, "needs_one");

    // Same filename, different bytes — and a DIFFERENT LENGTH on purpose: the
    // digest shortcut in `installed` is keyed by (mtime, size), and a same-size
    // rewrite inside one mtime tick is invisible to the whole registry, not just
    // to this cache.
    let second = make_jar(&[(
        "META-INF/mods.toml",
        "[[mods]]\nmodId=\"alpha\"\nversion=\"1.0\"\n\n\
         [[dependencies.alpha]]\n    modId=\"needs_two_and_then_some\"\n    mandatory=true\n    \
         versionRange=\"[1.0,)\"\n    side=\"BOTH\"\n",
    )]);
    tokio::fs::write(installed::mods_dir(root).join("alpha.jar"), &second)
        .await
        .unwrap();

    let b = dependency_preflight_for_root(root, Some(&cache), LoaderKind::Forge, "1.20.1", None)
        .await
        .unwrap();
    assert_eq!(
        b.violations[0].dep_id, "needs_two_and_then_some",
        "the cache must follow the bytes on disk, not the registry's expectation"
    );
}

// ── the legacy @Mod annotation (spec 2026-10-07) ───────────────────────────
//
// Every dependent fixture except MobDis starts with `required-after:forge@[…];`
// — what real 1.12.2 mods do — so main's printable-run reader loses only that
// clause and still sees the requirement under test; each dependent jar ships an
// `mcmod.info`, so main's own-id guard is not what decides it.

/// The report: OreLib's annotation says `3.6.0.1`, its `mcmod.info`
/// `1.12.2-3.6.0.1`. FML compares the first; comparing the second flagged a pack
/// that starts.
#[tokio::test]
async fn legacy_annotation_version_is_what_a_requirement_is_measured_against() {
    let td = TempDir::new().unwrap();
    let root = td.path();
    let orelib = make_jar_raw(&[
        ("mcmod.info", &mcmod("orelib", "1.12.2-3.6.0.1")),
        (
            "org/orecruncher/LibBase.class",
            class!("fixture/OreLib.class"),
        ),
    ]);
    register(root, "OreLib-1.12.2-3.6.0.1.jar", &orelib).await;
    let ds = make_jar_raw(&[
        ("mcmod.info", &mcmod("dsurround", "1.12.2-3.6.1.0")),
        (
            "org/orecruncher/dsurround/ModBase.class",
            class!("fixture/DynamicSurroundings.class"),
        ),
    ]);
    register(root, "DynamicSurroundings-1.12.2-3.6.1.0.jar", &ds).await;

    let report = dependency_preflight_for_root(root, None, LoaderKind::Forge, "1.12.2", None)
        .await
        .unwrap();
    assert!(report.violations.is_empty(), "{:?}", report.violations);
}

/// No `version` in the annotation: FML reads `version.properties` →
/// `<modid>.version` before `mcmod.info` (BoP's `mcmod.info` says `7.0.1`).
#[tokio::test]
async fn legacy_version_properties_comes_before_mcmod_info() {
    let td = TempDir::new().unwrap();
    let root = td.path();
    let bop = make_jar_raw(&[
        ("mcmod.info", &mcmod("biomesoplenty", "7.0.1")),
        (
            "version.properties",
            b"biomesoplenty.version=7.0.1.2445\n".as_slice(),
        ),
        (
            "biomesoplenty/core/BiomesOPlenty.class",
            class!("fixture/BopNoVersion.class"),
        ),
    ]);
    register(root, "BiomesOPlenty-1.12.2-7.0.1.2445-universal.jar", &bop).await;
    let dt = make_jar_raw(&[
        ("mcmod.info", &mcmod("dynamictreesbop", "1.12.2-1.5.2")),
        (
            "dynamictreesbop/DynamicTreesBOP.class",
            class!("fixture/DtBop.class"),
        ),
    ]);
    register(root, "DynamicTreesBOP-1.12.2-1.5.2.jar", &dt).await;

    let report = dependency_preflight_for_root(root, None, LoaderKind::Forge, "1.12.2", None)
        .await
        .unwrap();
    assert!(report.violations.is_empty(), "{:?}", report.violations);
}

/// The authority runs both ways: `mcmod.info` can name a HIGHER version than
/// the one FML compares (measured: futuremc 0.2.15 vs 0.2.6). Stripping a
/// Minecraft prefix would not catch this; reading the annotation does.
#[tokio::test]
async fn legacy_annotation_beats_a_higher_mcmod_info() {
    let td = TempDir::new().unwrap();
    let root = td.path();
    let fut = make_jar_raw(&[
        ("mcmod.info", &mcmod("futuremc", "0.2.15")),
        (
            "thedarkcolour/futuremc/FutureMC.class",
            class!("fixture/FutureMc.class"),
        ),
    ]);
    register(root, "future-mc-0.2.15.jar", &fut).await;
    let dep = make_jar_raw(&[
        ("mcmod.info", &mcmod("needsfuture", "1.0")),
        ("x/NeedsFuture.class", class!("fixture/NeedsFuture.class")),
    ]);
    register(root, "needsfuture.jar", &dep).await;

    let report = dependency_preflight_for_root(root, None, LoaderKind::Forge, "1.12.2", None)
        .await
        .unwrap();
    assert_eq!(report.violations.len(), 1, "{:?}", report.violations);
    assert!(matches!(
        report.violations[0].kind,
        ViolationKind::VersionOutOfRange
    ));
    assert_eq!(
        report.violations[0].installed_version.as_deref(),
        Some("0.2.6")
    );
}

/// A dependency string of 33–126 bytes: the length byte before it is printable,
/// and main's reader glued it onto the first clause (`&required-after:…`) and
/// dropped it. MobDismemberment's real string is exactly that, and its only one.
#[tokio::test]
async fn legacy_first_clause_survives_a_printable_length_byte() {
    let td = TempDir::new().unwrap();
    let root = td.path();
    let mob = make_jar_raw(&[
        ("mcmod.info", &mcmod("mobdismemberment", "1.12.2-7.0.0")),
        (
            "me/ichun/mods/mobdismemberment/MobDismemberment.class",
            class!("fixture/MobDis.class"),
        ),
    ]);
    register(root, "MobDismemberment-1.12.2-7.0.0.jar", &mob).await;

    let report = dependency_preflight_for_root(root, None, LoaderKind::Forge, "1.12.2", None)
        .await
        .unwrap();
    assert_eq!(report.violations.len(), 1, "{:?}", report.violations);
    assert!(matches!(
        report.violations[0].kind,
        ViolationKind::MissingRequired
    ));
    assert_eq!(report.violations[0].dep_id, "ichunutil");
}

fn drp_medieval() -> Vec<u8> {
    // Ships no mcmod.info at all — main's own-id guard never read it.
    make_jar_raw(&[(
        "drpmedieval/DRPMedievalMain.class",
        class!("fixture/DrpMedieval.class"),
    )])
}

/// PIN (green on main too): an annotation-only library is installed. Once the
/// dependent's requirement is read, `drpcore` must count as present through its
/// annotation alone — or every annotation-only library becomes a phantom.
#[tokio::test]
async fn legacy_annotation_only_library_counts_as_installed() {
    let td = TempDir::new().unwrap();
    let root = td.path();
    let core = make_jar_raw(&[("drpcore/DRPCoreMain.class", class!("fixture/DrpCore.class"))]);
    register(root, "drpcore-1.12.2-0.4.8.jar", &core).await;
    register(root, "drpmedieval-1.12.2-0.3.6.jar", &drp_medieval()).await;

    let report = dependency_preflight_for_root(root, None, LoaderKind::Forge, "1.12.2", None)
        .await
        .unwrap();
    assert!(report.violations.is_empty(), "{:?}", report.violations);
}

/// The same dependent without its library: a jar with no `mcmod.info` still
/// declares requirements FML enforces.
#[tokio::test]
async fn legacy_requirement_of_a_jar_without_mcmod_info_is_checked() {
    let td = TempDir::new().unwrap();
    let root = td.path();
    register(root, "drpmedieval-1.12.2-0.3.6.jar", &drp_medieval()).await;

    let report = dependency_preflight_for_root(root, None, LoaderKind::Forge, "1.12.2", None)
        .await
        .unwrap();
    assert_eq!(report.violations.len(), 1, "{:?}", report.violations);
    assert_eq!(report.violations[0].dep_id, "drpcore");
}

/// `@version@` is what FML compares when a build forgot to substitute it. The
/// comparison cannot be judged, so the pre-flight stays silent — and must NOT
/// fall back to `mcmod.info`'s `1.0`, which FML never uses for this mod.
#[tokio::test]
async fn legacy_placeholder_version_does_not_fall_back_to_mcmod_info() {
    let td = TempDir::new().unwrap();
    let root = td.path();
    let sun = make_jar_raw(&[
        ("mcmod.info", &mcmod("mobsunscreen", "1.0")),
        ("x/Sunscreen.class", class!("fixture/Sunscreen.class")),
    ]);
    register(root, "mobsunscreen.jar", &sun).await;
    let dep = make_jar_raw(&[
        ("mcmod.info", &mcmod("needssun", "1.0")),
        (
            "x/NeedsSunscreen.class",
            class!("fixture/NeedsSunscreen.class"),
        ),
    ]);
    register(root, "needssun.jar", &dep).await;

    let report = dependency_preflight_for_root(root, None, LoaderKind::Forge, "1.12.2", None)
        .await
        .unwrap();
    assert!(report.violations.is_empty(), "{:?}", report.violations);
}

/// `"useDependencyInformation": true` in `mcmod.info` makes FML take that mod's
/// requirements from `mcmod.info` — unless the annotation says
/// `useMetadata = false` (FMLModContainer.java:218).
#[tokio::test]
async fn legacy_use_dependency_information_hands_requirements_to_mcmod_info() {
    let info =
        br#"[{"modid":"udidep","version":"1.0","useDependencyInformation":true}]"#.as_slice();

    let td = TempDir::new().unwrap();
    let udi = make_jar_raw(&[
        ("mcmod.info", info),
        ("u/UdiDep.class", class!("fixture/UdiDep.class")),
    ]);
    register(td.path(), "udidep.jar", &udi).await;
    let report = dependency_preflight_for_root(td.path(), None, LoaderKind::Forge, "1.12.2", None)
        .await
        .unwrap();
    assert!(
        report.violations.is_empty(),
        "FML ignores the annotation here: {:?}",
        report.violations
    );

    let td2 = TempDir::new().unwrap();
    let forced = make_jar_raw(&[
        ("mcmod.info", info),
        ("u/UdiDep.class", class!("fixture/UdiDepExplicit.class")),
    ]);
    register(td2.path(), "udidep.jar", &forced).await;
    let report = dependency_preflight_for_root(td2.path(), None, LoaderKind::Forge, "1.12.2", None)
        .await
        .unwrap();
    assert_eq!(report.violations.len(), 1, "{:?}", report.violations);
    assert_eq!(report.violations[0].dep_id, "absentlib");
}

/// A `@Mod` class that will not parse is "could not tell" — exactly like a zip
/// that will not open: the jar is unjudged, and nothing is cached for it, so the
/// next scan tries again instead of believing an emptiness nobody measured.
#[tokio::test]
async fn legacy_unreadable_mod_class_is_unjudged_and_never_cached() {
    let td = TempDir::new().unwrap();
    let root = td.path();
    let cache = root.join("mods-cache").join("jar-scans.json");
    let full = class!("fixture/EnhancedVisuals.class");
    let broken = make_jar_raw(&[
        ("mcmod.info", &mcmod("enhancedvisuals", "1.3")),
        (
            "team/creative/enhancedvisuals/EnhancedVisuals.class",
            &full[..full.len() - 8],
        ),
    ]);
    let sha = register(root, "EnhancedVisuals.jar", &broken).await;

    let report =
        dependency_preflight_for_root(root, Some(&cache), LoaderKind::Forge, "1.12.2", None)
            .await
            .unwrap();
    assert_eq!(
        report.unjudged,
        vec![sha.clone()],
        "{:?}",
        report.violations
    );
    assert!(report.violations.is_empty(), "{:?}", report.violations);
    assert!(
        ScanCache::load(&cache).get(&sha).is_none(),
        "a failure is never frozen into the cache"
    );
}

/// Removal impact reads the same providers: removing an annotation-only library
/// names the mod that needs it.
#[tokio::test]
async fn legacy_annotation_only_library_has_a_removal_impact() {
    let td = TempDir::new().unwrap();
    let root = td.path();
    let core = make_jar_raw(&[("drpcore/DRPCoreMain.class", class!("fixture/DrpCore.class"))]);
    let core_sha = register(root, "drpcore-1.12.2-0.4.8.jar", &core).await;
    let medieval_sha = register(root, "drpmedieval-1.12.2-0.3.6.jar", &drp_medieval()).await;

    let parsed = parse_instance(root, None, LoaderKind::Forge, "1.12.2", None)
        .await
        .unwrap();
    let impact = parsed
        .removal_impact(&std::collections::HashSet::from([core_sha]))
        .unwrap();
    let dependents: Vec<&str> = impact.dependents.iter().map(|d| d.sha1.as_str()).collect();
    assert_eq!(dependents, vec![medieval_sha.as_str()]);
}
