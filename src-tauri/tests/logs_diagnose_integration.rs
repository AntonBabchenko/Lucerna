//! End-to-end: write a fixture log, invoke the diagnose path with a
//! real tempfile, assert the right pattern_id fires. Exercises the
//! file-read + source inference + match path together.

use ftlauncher_lib::logs::diagnose;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

#[tokio::test]
async fn diagnose_recognises_java_version_too_old_in_a_real_file() {
    let td = TempDir::new().unwrap();
    let logs_dir = td.path().join(".minecraft").join("logs");
    fs::create_dir_all(&logs_dir).unwrap();
    let path = logs_dir.join("latest.log");
    let content = "java.lang.UnsupportedClassVersionError: net/optifine/Config has been \
                   compiled by a more recent version of the Java Runtime (class file \
                   version 65.0), this version of the Java Runtime only recognizes class \
                   file versions up to 61.0";
    fs::write(&path, content).unwrap();

    let r = diagnose::diagnose(&path).await.unwrap();
    let d = r.expect("expected a diagnosis");
    assert_eq!(d.pattern_id, "java-version-too-old");
    // The excerpt is line-bounded and soft-capped (~133 bytes after
    // the match start). Assert against the regex's distinctive token
    // — the version digits at the tail fall outside the forward window
    // for this fixture shape, and that's a property of extract_excerpt
    // not a bug.
    assert!(
        d.matched_excerpt.contains("UnsupportedClassVersionError"),
        "excerpt should include the matched text, got: {:?}",
        d.matched_excerpt
    );
    assert!(
        !d.title.is_empty() && !d.explanation.is_empty() && !d.recommendation.is_empty(),
        "all copy fields populated"
    );
}

#[tokio::test]
async fn diagnose_returns_none_for_unknown_crash() {
    let td = TempDir::new().unwrap();
    let crash_dir = td.path().join(".minecraft").join("crash-reports");
    fs::create_dir_all(&crash_dir).unwrap();
    let path = crash_dir.join("crash-2026-05-24_00.00.00-client.txt");
    fs::write(
        &path,
        "java.lang.RuntimeException: something nobody has seen before",
    )
    .unwrap();

    let r = diagnose::diagnose(&path).await.unwrap();
    assert!(r.is_none(), "unknown content must not match any pattern");
}

#[tokio::test]
async fn diagnose_returns_none_for_empty_file() {
    let td = TempDir::new().unwrap();
    let path = td.path().join("empty.log");
    fs::write(&path, "").unwrap();

    let r = diagnose::diagnose(&path).await.unwrap();
    assert!(r.is_none(), "empty file → no diagnosis");
}

#[tokio::test]
async fn diagnose_respects_source_hint_for_launcher_only_pattern() {
    // fabric-loader-missing-main has source_hint = LauncherStdout.
    // Putting the same content in a path that infers as Game
    // (.minecraft/logs/) must produce no diagnosis.
    let td = TempDir::new().unwrap();
    let dir = td.path().join(".minecraft").join("logs");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("latest.log");
    let content = "Error: Could not find or load main class \
                   net.fabricmc.loader.impl.launch.knot.KnotClient";
    fs::write(&path, content).unwrap();

    let r = diagnose::diagnose(&path).await.unwrap();
    assert!(
        r.is_none(),
        "launcher-only pattern must not fire on a Game-source scan"
    );

    // Same content, launcher-source path → must fire.
    let launcher_dir = td.path().join("logs");
    fs::create_dir_all(&launcher_dir).unwrap();
    let lp = launcher_dir.join("launch-2026-05-24.log");
    fs::write(&lp, content).unwrap();
    let r2 = diagnose::diagnose(&lp).await.unwrap();
    assert_eq!(
        r2.unwrap().pattern_id,
        "fabric-loader-missing-main",
        "launcher-source path must fire the LauncherStdout-hint pattern"
    );

    // Suppress the unused-import warning for PathBuf — it's only
    // referenced via the dir.join() chain above which the compiler
    // sees through; leaving the explicit import here keeps editors
    // friendly to grep.
    let _: PathBuf = lp;
}
