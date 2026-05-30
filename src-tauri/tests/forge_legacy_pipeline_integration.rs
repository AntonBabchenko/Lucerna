//! Regression shield for the three backend bugs surfaced during the
//! 2026-05-16 manual e2e of MC 1.7.10 + Forge 10.13.4.1614. Each test
//! pins the exact symptom that broke the launch so a future refactor
//! that re-introduces the same shape fails in `cargo test` instead of
//! at user-tap time.
//!
//! Coverage map:
//!   - bug #1 (legacy `<mc>-<fv>-<mc>` maven path quirk) — already
//!     pinned by `forge_meta_integration::list_versions_filters_to_mc`
//!     against the updated fixture; not duplicated here.
//!   - bug #2 (universal jar placement via install.path, not filePath) —
//!     `installer_resolves_universal_jar_to_maven_coord_path`.
//!   - bug #3 (libraries without downloads/url silently skipped) —
//!     `legacy_libraries_fall_back_to_mojang_libraries_server`.
//!   - bug #4 (modal loader_version null race) — UI layer, not
//!     reachable without a Svelte test harness; out of scope here.
//!
//! Tests load the real 1.7.10 installer JAR fixture downloaded by
//! `src-tauri/tests/fixtures/forge/fetch.ps1`. They gracefully skip if
//! the fixture is missing — same `load_or_skip` pattern as
//! `forge_legacy_era_integration`.

use lucerna_lib::forge::installer::legacy::{
    extract_universal_jar_bytes, extract_version_info, maven_coord_to_relative_path,
};
use lucerna_lib::versions::libraries::artifacts_to_install;
use lucerna_lib::versions::version_json::Library;

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

fn load_install_profile_or_skip() -> Option<serde_json::Value> {
    let bytes = load_installer_or_skip()?;
    let cursor = std::io::Cursor::new(&bytes);
    let mut archive = zip::ZipArchive::new(cursor).expect("zip open");
    let mut entry = archive
        .by_name("install_profile.json")
        .expect("install_profile.json entry");
    use std::io::Read;
    let mut buf = String::new();
    entry.read_to_string(&mut buf).expect("read profile");
    Some(serde_json::from_str(&buf).expect("parse profile"))
}

/// Regression for bug #2: previously the legacy installer wrote the
/// universal jar to `<libs>/<install.filePath>` (after a `maven/`
/// strip that didn't match real 1.7.10 layout), so the file landed
/// at `<libs>/forge-<raw>-universal.jar` in the libraries root. The
/// vanilla library resolver then looked at the maven-coord path
/// derived from `versionInfo.libraries[net.minecraftforge:forge:...]`,
/// found nothing, hit a 404 on maven.minecraftforge.net (the plain
/// jar isn't published — only the `-universal` classifier is), and
/// install errored with "HTTP 404 Not Found".
///
/// The fix routes placement through `install.path` (a maven
/// coordinate), and explicitly strips the `-universal` classifier
/// because `install.path` doesn't carry it. This test pins that
/// pipeline against the real installer.
#[test]
fn installer_resolves_universal_jar_to_maven_coord_path() {
    let Some(profile) = load_install_profile_or_skip() else {
        return;
    };

    // The installer carries the coord WITHOUT a classifier...
    let install_path = profile
        .get("install")
        .and_then(|i| i.get("path"))
        .and_then(|p| p.as_str())
        .expect("install.path present");
    assert_eq!(
        install_path,
        "net.minecraftforge:forge:1.7.10-10.13.4.1614-1.7.10"
    );

    // ...and a separate `filePath` for the ZIP entry name WITH classifier.
    let file_path = profile
        .get("install")
        .and_then(|i| i.get("filePath"))
        .and_then(|p| p.as_str())
        .expect("install.filePath present");
    assert_eq!(file_path, "forge-1.7.10-10.13.4.1614-1.7.10-universal.jar");

    // The resolver must use install.path, not filePath, and the result
    // must be the maven-conventional layout (g/path/a/v/a-v.jar) — no
    // `-universal` classifier in the destination name.
    let rel = maven_coord_to_relative_path(install_path).expect("resolve");
    assert_eq!(
        rel,
        "net/minecraftforge/forge/1.7.10-10.13.4.1614-1.7.10/forge-1.7.10-10.13.4.1614-1.7.10.jar"
    );

    // Sanity: the maven-coord match exactly what versionInfo.libraries
    // expects to find on disk — otherwise vanilla's ensure_libraries
    // would download from a 404 URL instead of trusting the placed file.
    let libs = profile
        .get("versionInfo")
        .and_then(|v| v.get("libraries"))
        .and_then(|l| l.as_array())
        .expect("versionInfo.libraries");
    let forge_self = libs
        .iter()
        .find(|l| {
            l.get("name")
                .and_then(|n| n.as_str())
                .map(|n| n.starts_with("net.minecraftforge:forge:"))
                .unwrap_or(false)
        })
        .expect("forge self-entry");
    assert_eq!(
        forge_self.get("name").and_then(|n| n.as_str()),
        Some(install_path),
        "versionInfo lists forge by the same maven coord as install.path"
    );
}

/// Regression for bug #3: Forge's `versionInfo.libraries` ships
/// entries with neither a `downloads.artifact` block nor a `url`.
/// Pre-fix `ensure_libraries` silently skipped them; MC failed at
/// boot with `Could not find or load main class
/// net.minecraft.launchwrapper.Launch`. The fix routes such entries
/// through a libraries.minecraft.net fallback in TOFU mode.
///
/// Six libs were affected for 1.7.10 — three Forge-only deps
/// (launchwrapper, asm-all, lzma) plus three version-overrides of
/// vanilla deps (guava:17.0, commons-lang3:3.3.2, jopt-simple:4.5)
/// where `dedupe_by_maven_coord` lets the child win and the child
/// has no download spec.
#[test]
fn legacy_libraries_fall_back_to_mojang_libraries_server() {
    let Some(profile) = load_install_profile_or_skip() else {
        return;
    };
    let libs_json = profile
        .get("versionInfo")
        .and_then(|v| v.get("libraries"))
        .and_then(|l| l.as_array())
        .expect("versionInfo.libraries");

    // The 6 entries that previously slipped through ensure_libraries.
    let want: [(&str, &str); 6] = [
        (
            "net.minecraft:launchwrapper:1.12",
            "net/minecraft/launchwrapper/1.12/launchwrapper-1.12.jar",
        ),
        (
            "org.ow2.asm:asm-all:5.0.3",
            "org/ow2/asm/asm-all/5.0.3/asm-all-5.0.3.jar",
        ),
        ("lzma:lzma:0.0.1", "lzma/lzma/0.0.1/lzma-0.0.1.jar"),
        (
            "com.google.guava:guava:17.0",
            "com/google/guava/guava/17.0/guava-17.0.jar",
        ),
        (
            "org.apache.commons:commons-lang3:3.3.2",
            "org/apache/commons/commons-lang3/3.3.2/commons-lang3-3.3.2.jar",
        ),
        (
            "net.sf.jopt-simple:jopt-simple:4.5",
            "net/sf/jopt-simple/jopt-simple/4.5/jopt-simple-4.5.jar",
        ),
    ];

    for (coord, expected_rel) in want {
        // Confirm the real installer still lists this lib without
        // download spec — if Forge ever fixes the manifest this
        // test should fail loudly so we know to drop the fallback.
        let entry_json = libs_json
            .iter()
            .find(|l| l.get("name").and_then(|n| n.as_str()) == Some(coord))
            .unwrap_or_else(|| panic!("versionInfo missing expected entry: {coord}"));
        let has_downloads = entry_json
            .get("downloads")
            .map(|d| !d.is_null())
            .unwrap_or(false);
        let has_url = entry_json
            .get("url")
            .and_then(|u| u.as_str())
            .filter(|s| !s.is_empty())
            .is_some();
        assert!(
            !has_downloads && !has_url,
            "{coord} no longer matches the legacy-Forge shape — \
             update or remove this assertion if Forge has started \
             shipping a download spec for it"
        );

        // The fallback must produce a single artifact under
        // libraries.minecraft.net, in TOFU mode.
        let lib = Library {
            name: coord.into(),
            downloads: None,
            url: None,
            rules: None,
            natives: None,
        };
        let out = artifacts_to_install(&lib, "windows", "x64");
        assert_eq!(
            out.len(),
            1,
            "expected one artifact for {coord}, got {out:?}"
        );
        let (rel, url, sha, size) = &out[0];
        assert_eq!(rel, expected_rel, "wrong relative path for {coord}");
        assert_eq!(
            url,
            &format!("https://libraries.minecraft.net/{expected_rel}"),
            "wrong fallback URL for {coord}"
        );
        assert!(sha.is_empty(), "TOFU: empty SHA expected for {coord}");
        assert_eq!(*size, 0, "TOFU: zero size expected for {coord}");
    }
}

/// Combined smoke for the legacy era path: install_profile parses,
/// era is Legacy, versionInfo extracts cleanly, and the merged
/// VersionDetails carries the inheritsFrom + mainClass + arguments
/// shape that `launch::build_argv` expects.
#[test]
fn legacy_install_pipeline_smoke_against_real_installer() {
    let Some(bytes) = load_installer_or_skip() else {
        return;
    };
    let cursor = std::io::Cursor::new(&bytes);
    let mut archive = zip::ZipArchive::new(cursor).expect("zip open");
    let mut entry = archive
        .by_name("install_profile.json")
        .expect("install_profile.json entry");
    use std::io::Read;
    let mut buf = String::new();
    entry.read_to_string(&mut buf).expect("read profile");
    let profile: serde_json::Value = serde_json::from_str(&buf).expect("parse profile");

    // Era detection on the real file → Legacy.
    assert_eq!(
        lucerna_lib::forge::installer::detect_era(&profile),
        lucerna_lib::forge::installer::Era::Legacy
    );

    // versionInfo round-trips through our parser.
    let details = extract_version_info(&profile, "1.7.10", "10.13.4.1614").expect("extract");
    assert_eq!(details.inherits_from.as_deref(), Some("1.7.10"));
    assert_eq!(details.main_class, "net.minecraft.launchwrapper.Launch");
    assert!(
        details
            .minecraft_arguments
            .as_deref()
            .unwrap_or("")
            .contains("FMLTweaker"),
        "minecraftArguments should reference FMLTweaker for legacy launch"
    );
    // Must have 18 libraries as of the 1.7.10 fixture — a sanity
    // count so a future parser regression that drops the libraries
    // array entirely fails loudly.
    assert_eq!(
        details.libraries.len(),
        18,
        "1.7.10 versionInfo has 18 libs"
    );

    // Universal jar bytes extract cleanly.
    let (entry_name, jar_bytes) =
        extract_universal_jar_bytes(&profile, &bytes, "1.7.10", "10.13.4.1614").expect("extract");
    assert_eq!(entry_name, "forge-1.7.10-10.13.4.1614-1.7.10-universal.jar");
    assert!(!jar_bytes.is_empty(), "universal jar bytes are non-empty");
    // The jar is itself a valid ZIP (Forge's universal jar is just
    // another jar). This catches a hypothetical regression where
    // extract_universal_jar_bytes returns the wrong entry.
    assert!(
        zip::ZipArchive::new(std::io::Cursor::new(&jar_bytes)).is_ok(),
        "universal jar bytes should themselves parse as a ZIP"
    );
}
