//! Full-mode export must produce an archive the existing import parser can
//! read. We export an instance with one local jar in FULL mode (everything
//! bundled, empty `files[]`), then assert the archive shape + manifest.

use lucerna_lib::instances::schema::LoaderKind;
use lucerna_lib::mods::modpack::export::{run_export, ExportMetadata, ExportMode, ExportOptions};
use lucerna_lib::mods::modpack::schema::ModpackFormat;
use lucerna_lib::mods::platform::{InstalledMod, ModSource};

fn local_jar(filename: &str) -> InstalledMod {
    InstalledMod {
        filename: filename.into(),
        sha1: "0".repeat(40),
        source: None,
        project_id: None,
        version_id: None,
        name: filename.into(),
        version_number: None,
        installed_at: "2026-01-01T00:00:00Z".into(),
        enabled: true,
        enrich_attempted: false,
        requires: Vec::new(),
    }
}

fn archive_names(path: &std::path::Path) -> Vec<String> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    (0..zip.len())
        .map(|i| zip.by_index(i).unwrap().name().to_string())
        .collect()
}

fn opts(format: ModpackFormat, mode: ExportMode, bundle_shas: Vec<String>) -> ExportOptions {
    ExportOptions {
        format,
        mode,
        include_config: false,
        include_resourcepacks: false,
        include_shaderpacks: false,
        include_worlds: false,
        bundle_shas,
        metadata: ExportMetadata {
            name: "T".into(),
            version: "1.0.0".into(),
            author: String::new(),
            summary: String::new(),
        },
    }
}

#[tokio::test]
async fn full_mode_export_round_trips_through_import_parser() {
    let td = tempfile::tempdir().unwrap();
    let root = td.path();
    let mods_dir = root.join(".minecraft").join("mods");
    std::fs::create_dir_all(&mods_dir).unwrap();
    std::fs::write(mods_dir.join("local.jar"), b"localbytes").unwrap();

    let opts = ExportOptions {
        format: ModpackFormat::Modrinth,
        mode: ExportMode::Full,
        include_config: false,
        include_resourcepacks: false,
        include_shaderpacks: false,
        include_worlds: false,
        bundle_shas: vec![],
        metadata: ExportMetadata {
            name: "RoundTrip".into(),
            version: "1.0.0".into(),
            author: String::new(),
            summary: String::new(),
        },
    };

    let dest = td.path().join("out.mrpack");
    let mods = vec![local_jar("local.jar")];
    run_export(
        root,
        "1.21.1",
        LoaderKind::Fabric,
        Some("0.16.0"),
        &mods,
        &opts,
        &dest,
        &|_p| {},
    )
    .await
    .unwrap();

    assert!(dest.exists(), "archive written");

    let mut zip = zip::ZipArchive::new(std::fs::File::open(&dest).unwrap()).unwrap();
    let names: Vec<String> = (0..zip.len())
        .map(|i| zip.by_index(i).unwrap().name().to_string())
        .collect();
    assert!(names.iter().any(|n| n == "modrinth.index.json"));
    assert!(names.iter().any(|n| n == "overrides/mods/local.jar"));

    use std::io::Read;
    let mut idx = String::new();
    zip.by_name("modrinth.index.json")
        .unwrap()
        .read_to_string(&mut idx)
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&idx).unwrap();
    assert_eq!(v["files"].as_array().unwrap().len(), 0);
    assert_eq!(v["dependencies"]["fabric-loader"], "0.16.0");
}

#[tokio::test]
async fn lightweight_bundles_unresolvable_only_when_chosen() {
    let td = tempfile::tempdir().unwrap();
    let root = td.path();
    let mods_dir = root.join(".minecraft").join("mods");
    std::fs::create_dir_all(&mods_dir).unwrap();
    std::fs::write(mods_dir.join("local.jar"), b"x").unwrap();
    let m = local_jar("local.jar");
    let sha = m.sha1.clone();
    let mods = vec![m];

    // Skip: bundle_shas empty -> jar NOT in archive.
    let dest_skip = root.join("skip.mrpack");
    run_export(
        root,
        "1.21.1",
        LoaderKind::Fabric,
        Some("0.16.0"),
        &mods,
        &opts(ModpackFormat::Modrinth, ExportMode::Lightweight, vec![]),
        &dest_skip,
        &|_p| {},
    )
    .await
    .unwrap();
    assert!(!archive_names(&dest_skip)
        .iter()
        .any(|n| n == "overrides/mods/local.jar"));

    // Bundle: sha in bundle_shas -> jar IS in archive.
    let dest_keep = root.join("keep.mrpack");
    run_export(
        root,
        "1.21.1",
        LoaderKind::Fabric,
        Some("0.16.0"),
        &mods,
        &opts(ModpackFormat::Modrinth, ExportMode::Lightweight, vec![sha]),
        &dest_keep,
        &|_p| {},
    )
    .await
    .unwrap();
    assert!(archive_names(&dest_keep)
        .iter()
        .any(|n| n == "overrides/mods/local.jar"));
}

#[tokio::test]
async fn cf_format_non_numeric_ids_fall_back_to_bundle() {
    let td = tempfile::tempdir().unwrap();
    let root = td.path();
    let mods_dir = root.join(".minecraft").join("mods");
    std::fs::create_dir_all(&mods_dir).unwrap();
    std::fs::write(mods_dir.join("cf.jar"), b"y").unwrap();
    let mut m = local_jar("cf.jar");
    m.source = Some(ModSource::Curseforge);
    m.project_id = Some("not-a-number".into());
    m.version_id = Some("also-bad".into());
    let mods = vec![m];

    let dest = root.join("cf.zip");
    run_export(
        root,
        "1.20.1",
        LoaderKind::Forge,
        Some("47.2.0"),
        &mods,
        &opts(ModpackFormat::Curseforge, ExportMode::Lightweight, vec![]),
        &dest,
        &|_p| {},
    )
    .await
    .unwrap();
    let names = archive_names(&dest);
    assert!(names.iter().any(|n| n == "manifest.json"));
    // Non-numeric CF ids cannot be referenced -> jar bundled under overrides.
    assert!(names.iter().any(|n| n == "overrides/mods/cf.jar"));
}

#[tokio::test]
async fn cf_format_numeric_ids_are_referenced_not_bundled() {
    // The inverse of cf_format_non_numeric_ids_fall_back_to_bundle: when a
    // CurseForge mod carries valid numeric project/file ids, it is referenced
    // in manifest.json (projectID/fileID) and NOT copied into overrides. This
    // path takes no network — the CF branch only parses the ids.
    let td = tempfile::tempdir().unwrap();
    let root = td.path();
    let mods_dir = root.join(".minecraft").join("mods");
    std::fs::create_dir_all(&mods_dir).unwrap();
    std::fs::write(mods_dir.join("jei.jar"), b"cfbytes").unwrap();
    let mut m = local_jar("jei.jar");
    m.source = Some(ModSource::Curseforge);
    m.project_id = Some("238222".into());
    m.version_id = Some("4712788".into());
    let mods = vec![m];

    let dest = root.join("cf.zip");
    run_export(
        root,
        "1.20.1",
        LoaderKind::Forge,
        Some("47.2.0"),
        &mods,
        &opts(ModpackFormat::Curseforge, ExportMode::Lightweight, vec![]),
        &dest,
        &|_p| {},
    )
    .await
    .unwrap();

    let names = archive_names(&dest);
    assert!(names.iter().any(|n| n == "manifest.json"));
    // Referenced, not bundled.
    assert!(
        !names.iter().any(|n| n == "overrides/mods/jei.jar"),
        "numeric-id CF mod must be referenced, not bundled"
    );

    use std::io::Read;
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&dest).unwrap()).unwrap();
    let mut manifest = String::new();
    zip.by_name("manifest.json")
        .unwrap()
        .read_to_string(&mut manifest)
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&manifest).unwrap();
    let files = v["files"].as_array().unwrap();
    assert_eq!(files.len(), 1, "one referenced file");
    assert_eq!(files[0]["projectID"], 238222);
    assert_eq!(files[0]["fileID"], 4712788);
}

#[tokio::test]
async fn includes_resourcepacks_shaderpacks_and_worlds_as_overrides() {
    // Exercises the include_resourcepacks / include_shaderpacks / include_worlds
    // branches (the existing content test only flips include_config).
    let td = tempfile::tempdir().unwrap();
    let root = td.path();
    let mc = root.join(".minecraft");
    std::fs::create_dir_all(mc.join("mods")).unwrap();
    std::fs::write(mc.join("mods").join("local.jar"), b"z").unwrap();
    std::fs::create_dir_all(mc.join("resourcepacks")).unwrap();
    std::fs::write(mc.join("resourcepacks").join("pack.zip"), b"rp").unwrap();
    std::fs::create_dir_all(mc.join("shaderpacks")).unwrap();
    std::fs::write(mc.join("shaderpacks").join("shader.zip"), b"sh").unwrap();
    std::fs::create_dir_all(mc.join("saves").join("world")).unwrap();
    std::fs::write(mc.join("saves").join("world").join("level.dat"), b"lvl").unwrap();
    let mods = vec![local_jar("local.jar")];

    let mut o = opts(ModpackFormat::Modrinth, ExportMode::Full, vec![]);
    o.include_resourcepacks = true;
    o.include_shaderpacks = true;
    o.include_worlds = true;
    let dest = root.join("full.mrpack");
    run_export(
        root,
        "1.21.1",
        LoaderKind::Fabric,
        Some("0.16.0"),
        &mods,
        &o,
        &dest,
        &|_p| {},
    )
    .await
    .unwrap();

    let names = archive_names(&dest);
    assert!(names
        .iter()
        .any(|n| n == "overrides/resourcepacks/pack.zip"));
    assert!(names
        .iter()
        .any(|n| n == "overrides/shaderpacks/shader.zip"));
    assert!(names.iter().any(|n| n == "overrides/saves/world/level.dat"));
}

#[tokio::test]
async fn includes_selected_content_dirs_as_overrides() {
    let td = tempfile::tempdir().unwrap();
    let root = td.path();
    let mc = root.join(".minecraft");
    std::fs::create_dir_all(mc.join("mods")).unwrap();
    std::fs::write(mc.join("mods").join("local.jar"), b"z").unwrap();
    std::fs::create_dir_all(mc.join("config").join("sodium")).unwrap();
    std::fs::write(mc.join("config").join("sodium").join("options.json"), b"{}").unwrap();
    let mods = vec![local_jar("local.jar")];

    let mut o = opts(ModpackFormat::Modrinth, ExportMode::Full, vec![]);
    o.include_config = true;
    let dest = root.join("full.mrpack");
    run_export(
        root,
        "1.21.1",
        LoaderKind::Fabric,
        Some("0.16.0"),
        &mods,
        &o,
        &dest,
        &|_p| {},
    )
    .await
    .unwrap();
    let names = archive_names(&dest);
    assert!(names
        .iter()
        .any(|n| n == "overrides/config/sodium/options.json"));
}
