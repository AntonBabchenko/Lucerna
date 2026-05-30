//! Regression shield for the transitional-era install pipeline.

use lucerna_lib::forge::installer::transitional::{
    parse_install_profile, substitute_args, DataEntry,
};
use lucerna_lib::forge::installer::{detect_era, Era};
use std::collections::HashMap;
use std::path::PathBuf;

const FIXTURE: &str = "tests/fixtures/forge/installers/forge-1.16.5-36.2.42-installer.jar";

fn load_or_skip() -> Option<(Vec<u8>, serde_json::Value)> {
    let bytes = match std::fs::read(FIXTURE) {
        Ok(b) => b,
        Err(_) => {
            eprintln!("SKIP: 1.16.5 fixture absent");
            return None;
        }
    };
    let cursor = std::io::Cursor::new(bytes.clone());
    let mut archive = zip::ZipArchive::new(cursor).expect("zip");
    let mut entry = archive
        .by_name("install_profile.json")
        .expect("install_profile");
    use std::io::Read;
    let mut buf = String::new();
    entry.read_to_string(&mut buf).expect("read");
    let value: serde_json::Value = serde_json::from_str(&buf).unwrap();
    Some((bytes, value))
}

#[test]
fn era_for_1165_is_transitional() {
    let Some((_, profile)) = load_or_skip() else {
        return;
    };
    assert_eq!(detect_era(&profile), Era::Transitional);
}

#[test]
fn install_profile_has_4_processors() {
    let Some((_, profile)) = load_or_skip() else {
        return;
    };
    let parsed = parse_install_profile(&serde_json::to_string(&profile).unwrap()).unwrap();
    assert_eq!(parsed.minecraft, "1.16.5");
    assert_eq!(
        parsed.processors.len(),
        4,
        "expected 4 processors per audit"
    );
}

#[test]
fn every_processor_coord_recognised() {
    let Some((_, profile)) = load_or_skip() else {
        return;
    };
    let parsed = parse_install_profile(&serde_json::to_string(&profile).unwrap()).unwrap();
    let recognised = &[
        ("net.minecraftforge", "installertools"),
        ("net.minecraftforge", "jarsplitter"),
        ("net.md-5", "SpecialSource"),
        ("net.minecraftforge", "binarypatcher"),
    ];
    for p in &parsed.processors {
        let parts: Vec<&str> = p.jar.split(':').collect();
        let (g, a) = (parts[0], parts[1]);
        assert!(
            recognised.iter().any(|(rg, ra)| *rg == g && *ra == a),
            "unrecognised processor coord: {} — extend run_processor",
            p.jar
        );
    }
}

#[tokio::test]
async fn substitute_args_reserved_placeholders() {
    let data: HashMap<String, DataEntry> = HashMap::new();
    let libs = PathBuf::from("/x/libraries");
    let raw = vec![
        "{ROOT}".into(),
        "{INSTALLER}".into(),
        "{SIDE}".into(),
        "{MINECRAFT_JAR}".into(),
    ];
    let out = substitute_args(
        &raw,
        &data,
        "client",
        &libs,
        b"",
        "/inst.jar",
        "/cache",
        "/mc.jar",
    )
    .await
    .unwrap();
    assert_eq!(
        out,
        vec![
            "/cache".to_string(),
            "/inst.jar".into(),
            "client".into(),
            "/mc.jar".into()
        ]
    );
}

#[tokio::test]
async fn substitute_args_quoted_hex_literal() {
    let mut data = HashMap::new();
    data.insert(
        "HASH".into(),
        DataEntry {
            client: "'deadbeef'".into(),
            server: "'cafebabe'".into(),
        },
    );
    let libs = PathBuf::from("/x/libraries");
    let out = substitute_args(
        &vec!["{HASH}".into()],
        &data,
        "client",
        &libs,
        b"",
        "/i",
        "/c",
        "/m",
    )
    .await
    .unwrap();
    assert_eq!(out, vec!["deadbeef".to_string()]);
}
