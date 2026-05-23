//! Regression shield for the modern-era install pipeline.
//! Mirrors `forge_transitional_era_integration.rs` shape.

use ftlauncher_lib::forge::installer::transitional::{
    parse_install_profile, substitute_args, DataEntry,
};
use ftlauncher_lib::forge::installer::{detect_era, Era};
use std::collections::HashMap;
use std::path::PathBuf;

const FIXTURE: &str = "tests/fixtures/forge/installers/forge-1.20.4-49.0.49-installer.jar";

fn load_or_skip() -> Option<(Vec<u8>, serde_json::Value)> {
    let bytes = match std::fs::read(FIXTURE) {
        Ok(b) => b,
        Err(_) => {
            eprintln!("SKIP: 1.20.4 fixture absent");
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
fn era_for_1204_is_modern() {
    let Some((_, profile)) = load_or_skip() else {
        return;
    };
    assert_eq!(detect_era(&profile), Era::Modern);
}

#[test]
fn install_profile_has_9_processors() {
    let Some((_, profile)) = load_or_skip() else {
        return;
    };
    let parsed = parse_install_profile(&serde_json::to_string(&profile).unwrap()).unwrap();
    assert_eq!(parsed.minecraft, "1.20.4");
    assert_eq!(parsed.spec, 1, "modern era must report spec=1");
    assert_eq!(
        parsed.processors.len(),
        9,
        "expected 9 processors per audit"
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
        ("net.minecraftforge", "ForgeAutoRenamingTool"),
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

#[test]
fn five_processors_run_on_client_side() {
    let Some((_, profile)) = load_or_skip() else {
        return;
    };
    let parsed = parse_install_profile(&serde_json::to_string(&profile).unwrap()).unwrap();
    let client_side_count = parsed
        .processors
        .iter()
        .filter(|p| match &p.sides {
            Some(sides) => sides.iter().any(|s| s == "client"),
            None => true, // missing `sides` = run on both sides
        })
        .count();
    // Server-only processors in 1.20.4-49.0.49: EXTRACT_FILES (idx 0),
    // BUNDLER_EXTRACT × 2 (idx 1-2), FART-server (idx 6) — 4 total.
    // Client runs: MCP_DATA, DOWNLOAD_MOJMAPS, MERGE_MAPPING, FART-client,
    // binarypatcher — 5 total.
    assert_eq!(
        client_side_count, 5,
        "audit table shows 4 server-only processors; client should see 5"
    );
}

#[test]
fn data_keys_match_audit() {
    let Some((_, profile)) = load_or_skip() else {
        return;
    };
    let parsed = parse_install_profile(&serde_json::to_string(&profile).unwrap()).unwrap();
    let required_keys = &[
        "MAPPINGS",
        "MAPPINGS_SHA",
        "MOJMAPS",
        "MOJMAPS_SHA",
        "MERGED_MAPPINGS",
        "MERGED_MAPPINGS_SHA",
        "MC_UNPACKED",
        "MC_UNPACKED_SHA",
        "MC_SRG",
        "MC_SRG_SHA",
        "BINPATCH",
        "PATCHED",
        "PATCHED_SHA",
    ];
    for key in required_keys {
        assert!(
            parsed.data.contains_key(*key),
            "missing data key per audit: {key}"
        );
    }
}

#[tokio::test]
async fn substitute_args_resolves_modern_data_shapes() {
    let Some((bytes, profile)) = load_or_skip() else {
        return;
    };
    let parsed = parse_install_profile(&serde_json::to_string(&profile).unwrap()).unwrap();
    let libs = PathBuf::from("/x/libraries");
    // Pull processor 8 (binarypatcher) — its --apply arg uses {BINPATCH}, the /in-jar/ shape.
    let bp = &parsed.processors[8];
    let resolved = substitute_args(
        &bp.args,
        &parsed.data,
        "client",
        &libs,
        &bytes,
        "/installer.jar",
        "/cache",
        "/mc.jar",
    )
    .await
    .expect("substitute binarypatcher args");
    // After substitution, --apply should be a real cache path (in-jar extraction).
    let apply_idx = resolved.iter().position(|a| a == "--apply").unwrap();
    let apply_value = &resolved[apply_idx + 1];
    assert!(
        apply_value.contains("client.lzma"),
        "BINPATCH placeholder should resolve to a path containing client.lzma; got {apply_value}"
    );
}

#[tokio::test]
async fn substitute_args_modern_side_data_split_uses_client() {
    let Some((_, profile)) = load_or_skip() else {
        return;
    };
    let parsed = parse_install_profile(&serde_json::to_string(&profile).unwrap()).unwrap();
    // MOJMAPS_SHA differs between client and server in 1.20.4 — verify we pick client.
    let data = parsed.data.get("MOJMAPS_SHA").expect("MOJMAPS_SHA present");
    assert_ne!(
        data.client, data.server,
        "expected modern profile to split MOJMAPS_SHA across sides"
    );
}

// Suppress unused-import warnings when fixture is absent (CI without fetch.ps1).
#[allow(dead_code)]
fn _unused_imports_workaround() {
    let _ = HashMap::<String, DataEntry>::new();
}
