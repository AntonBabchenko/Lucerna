//! Patcher subsystem integration smoke. Exercises run_processor for
//! each of the 4 transitional-era processors. Golden-fixture tests
//! skip gracefully if fixtures aren't present.

use lucerna_lib::forge::patcher::{run_processor, ProcessorContext};
use std::io::Write;
use tempfile::tempdir;

#[tokio::test]
async fn unknown_processor_returns_unsupported() {
    let dir = tempdir().unwrap();
    let ctx = ProcessorContext {
        classpath: vec![],
        cache_dir: dir.path().to_path_buf(),
        java_bin: None,
    };
    let err = run_processor("com.example:unknown:1.0", vec![], &ctx)
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        lucerna_lib::error::Error::ForgeUnsupportedProcessor { .. }
    ));
}

#[tokio::test]
async fn installertools_extract_files() {
    let dir = tempdir().unwrap();
    let zip_path = dir.path().join("in.zip");
    let out_dir = dir.path().join("out");
    let f = std::fs::File::create(&zip_path).unwrap();
    let mut w = zip::ZipWriter::new(f);
    let opts: zip::write::FileOptions<()> = zip::write::FileOptions::default();
    w.start_file::<_, ()>("data/x.txt", opts).unwrap();
    w.write_all(b"payload").unwrap();
    w.finish().unwrap();
    let ctx = ProcessorContext {
        classpath: vec![],
        cache_dir: dir.path().to_path_buf(),
        java_bin: None,
    };
    let args = vec![
        "--task".into(),
        "EXTRACT_FILES".into(),
        "--archive".into(),
        zip_path.display().to_string(),
        "--from".into(),
        "data/x.txt".into(),
        "--output".into(),
        out_dir.display().to_string(),
    ];
    run_processor("net.minecraftforge:installertools:1.3.0", args, &ctx)
        .await
        .expect("extract");
    assert_eq!(
        std::fs::read_to_string(out_dir.join("x.txt")).unwrap(),
        "payload"
    );
}

#[tokio::test]
async fn jarsplitter_routes_classes() {
    let dir = tempdir().unwrap();
    let in_path = dir.path().join("in.jar");
    let slim = dir.path().join("slim.jar");
    let extra = dir.path().join("extra.jar");
    let srg = dir.path().join("m.tsrg");
    std::fs::write(&srg, "net/known/A net/known/A\n").unwrap();
    let f = std::fs::File::create(&in_path).unwrap();
    let mut w = zip::ZipWriter::new(f);
    let opts: zip::write::FileOptions<()> = zip::write::FileOptions::default();
    w.start_file::<_, ()>("net/known/A.class", opts).unwrap();
    w.write_all(b"a").unwrap();
    w.start_file::<_, ()>("net/other/B.class", opts).unwrap();
    w.write_all(b"b").unwrap();
    w.finish().unwrap();
    let ctx = ProcessorContext {
        classpath: vec![],
        cache_dir: dir.path().to_path_buf(),
        java_bin: None,
    };
    let args = vec![
        "--input".into(),
        in_path.display().to_string(),
        "--slim".into(),
        slim.display().to_string(),
        "--extra".into(),
        extra.display().to_string(),
        "--srg".into(),
        srg.display().to_string(),
    ];
    run_processor("net.minecraftforge:jarsplitter:1.1.4", args, &ctx)
        .await
        .expect("split");
    assert!(slim.exists() && extra.exists());
}

#[tokio::test]
async fn specialsource_golden_or_skip() {
    let input = "tests/fixtures/specialsource/golden/input.jar";
    if !std::path::Path::new(input).exists() {
        eprintln!("SKIP: specialsource golden absent");
        return;
    }
    let dir = tempdir().unwrap();
    let out = dir.path().join("out.jar");
    let ctx = ProcessorContext {
        classpath: vec![],
        cache_dir: dir.path().to_path_buf(),
        java_bin: None,
    };
    let args = vec![
        "--in-jar".into(),
        input.into(),
        "--out-jar".into(),
        out.display().to_string(),
        "--srg-in".into(),
        "tests/fixtures/specialsource/golden/mappings.tsrg".into(),
    ];
    run_processor("net.md-5:SpecialSource:1.8.5", args, &ctx)
        .await
        .expect("specialsource");
    assert!(out.exists());
}

#[tokio::test]
async fn binarypatcher_golden_or_skip() {
    let clean = "tests/fixtures/binarypatcher/clean.jar";
    if !std::path::Path::new(clean).exists() {
        eprintln!("SKIP: binarypatcher golden absent");
        return;
    }
    let dir = tempdir().unwrap();
    let out = dir.path().join("out.jar");
    let ctx = ProcessorContext {
        classpath: vec![],
        cache_dir: dir.path().to_path_buf(),
        java_bin: None,
    };
    let args = vec![
        "--clean".into(),
        clean.into(),
        "--output".into(),
        out.display().to_string(),
        "--apply".into(),
        "tests/fixtures/binarypatcher/patches.lzma".into(),
    ];
    run_processor("net.minecraftforge:binarypatcher:1.0.12", args, &ctx)
        .await
        .expect("binarypatcher");
    assert!(out.exists());
}

// ── Phase 3 additions ─────────────────────────────────────────────────────────

#[tokio::test]
async fn installertools_merge_mapping_classes_only() {
    let dir = tempdir().unwrap();
    let left = dir.path().join("left.tsrg");
    let right = dir.path().join("right.txt");
    let out = dir.path().join("merged.tsrg");
    std::fs::write(&left, "a net/srg/A\nb net/srg/B\n").unwrap();
    std::fs::write(&right, "com.example.Named -> a:\n    int n -> q\n").unwrap();

    let ctx = ProcessorContext {
        classpath: vec![],
        cache_dir: dir.path().to_path_buf(),
        java_bin: None,
    };
    let args = vec![
        "--task".into(),
        "MERGE_MAPPING".into(),
        "--left".into(),
        left.display().to_string(),
        "--right".into(),
        right.display().to_string(),
        "--output".into(),
        out.display().to_string(),
        "--classes".into(),
        "--reverse-right".into(),
    ];
    run_processor("net.minecraftforge:installertools:1.4.0", args, &ctx)
        .await
        .expect("merge");
    let merged = std::fs::read_to_string(&out).unwrap();
    assert!(merged.contains("a com/example/Named"));
    assert!(merged.contains("b net/srg/B"));
}

#[tokio::test]
async fn installertools_bundler_extract_returns_server_only_error() {
    let dir = tempdir().unwrap();
    let ctx = ProcessorContext {
        classpath: vec![],
        cache_dir: dir.path().to_path_buf(),
        java_bin: None,
    };
    let args = vec!["--task".into(), "BUNDLER_EXTRACT".into()];
    let err = run_processor("net.minecraftforge:installertools:1.4.0", args, &ctx)
        .await
        .unwrap_err();
    match err {
        lucerna_lib::error::Error::ForgeUnsupportedProcessor { coord } => {
            assert!(coord.contains("BUNDLER_EXTRACT"));
            assert!(coord.contains("server-only") || coord.contains("client install"));
        }
        other => panic!("expected ForgeUnsupportedProcessor, got {other:?}"),
    }
}

#[tokio::test]
async fn fart_rejects_missing_required_args() {
    let dir = tempdir().unwrap();
    let ctx = ProcessorContext {
        classpath: vec![],
        cache_dir: dir.path().to_path_buf(),
        java_bin: None,
    };
    let args = vec!["--ann-fix".into()];
    let err = run_processor("net.minecraftforge:ForgeAutoRenamingTool:1.0.6", args, &ctx)
        .await
        .unwrap_err();
    match err {
        lucerna_lib::error::Error::ForgePatcherFailed { processor, details } => {
            assert_eq!(processor, "fart");
            assert!(details.contains("missing --input"));
        }
        other => panic!("expected ForgePatcherFailed, got {other:?}"),
    }
}
