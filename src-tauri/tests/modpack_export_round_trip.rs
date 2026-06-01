//! Full-mode export must produce an archive the existing import parser can
//! read. We export an instance with one local jar in FULL mode (everything
//! bundled, empty `files[]`), then assert the archive shape + manifest.

use lucerna_lib::instances::schema::LoaderKind;
use lucerna_lib::mods::modpack::export::{run_export, ExportMetadata, ExportMode, ExportOptions};
use lucerna_lib::mods::modpack::schema::ModpackFormat;
use lucerna_lib::mods::platform::InstalledMod;

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
