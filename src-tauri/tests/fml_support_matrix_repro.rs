//! The All Of Create report, as a gate.
//!
//! A NeoForge 21.1.235 / Minecraft 1.21.1 modpack ships seven jars whose
//! `minecraft` range excludes 1.21.1 under plain Maven reading — `[1.21,1.21.1)`
//! ×5, `[1.21]`, `[1.20.1,1.21.1)`. NeoForge loads every one: FML's
//! `VersionSupportMatrix` also accepts 1.21 on 1.21.1. These are those jars'
//! real `neoforge.mods.toml` dependency declarations.
//!
//! NONE MAY FLAG — not in the platform verdict, not in the launch pre-flight
//! that gates Play. And the same jars must still flag where the loader would
//! refuse them, or the gate would pass by going blind.

use lucerna_lib::instances::schema::LoaderKind;
use lucerna_lib::mods::local::{read_jar_manifest_deps, DescriptorEra};
use lucerna_lib::mods::mc_compat::{platform_verdict, PlatformVerdict};
use lucerna_lib::mods::preflight::{resolve, ParsedMod, ProviderIndex, Violation};
use std::io::{Cursor, Write};
use zip::write::SimpleFileOptions;

const FIXTURES: &[(&str, &str)] = &[
    (
        "explorerscompass",
        include_str!("fixtures/fml_support_matrix/explorerscompass.neoforge.mods.toml"),
    ),
    (
        "naturescompass",
        include_str!("fixtures/fml_support_matrix/naturescompass.neoforge.mods.toml"),
    ),
    (
        "createendertransmission",
        include_str!("fixtures/fml_support_matrix/createendertransmission.neoforge.mods.toml"),
    ),
    (
        "iris",
        include_str!("fixtures/fml_support_matrix/iris.neoforge.mods.toml"),
    ),
    (
        "jei",
        include_str!("fixtures/fml_support_matrix/jei.neoforge.mods.toml"),
    ),
    (
        "rainbowcompound",
        include_str!("fixtures/fml_support_matrix/rainbowcompound.neoforge.mods.toml"),
    ),
    (
        "create_sophback_compat",
        include_str!("fixtures/fml_support_matrix/create_sophback_compat.neoforge.mods.toml"),
    ),
];

const MC: &str = "1.21.1";
const NEOFORGE: &str = "21.1.235";

fn jar_with_neoforge_toml(toml: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut w = zip::ZipWriter::new(Cursor::new(&mut buf));
        w.start_file("META-INF/neoforge.mods.toml", SimpleFileOptions::default())
            .expect("zip entry starts");
        w.write_all(toml.as_bytes()).expect("zip entry writes");
        w.finish().expect("zip finishes");
    }
    buf
}

fn parsed_fixtures() -> Vec<ParsedMod> {
    FIXTURES
        .iter()
        .map(|(name, toml)| ParsedMod {
            sha1: (*name).to_string(),
            name: (*name).to_string(),
            manifest: read_jar_manifest_deps(&jar_with_neoforge_toml(toml))
                .expect("fixture descriptor parses"),
        })
        .collect()
}

fn violated_on(mc: &str, neoforge: &str) -> Vec<String> {
    parsed_fixtures()
        .iter()
        .filter_map(|m| {
            let v = platform_verdict(
                &m.manifest,
                mc,
                LoaderKind::NeoForge,
                Some(neoforge),
                DescriptorEra::Modern,
            );
            matches!(v, PlatformVerdict::Violated { .. }).then(|| format!("{}: {v:?}", m.name))
        })
        .collect()
}

#[test]
fn none_of_the_seven_flags_on_the_pack_they_ship_in() {
    let flagged = violated_on(MC, NEOFORGE);
    assert!(
        flagged.is_empty(),
        "NeoForge loads all seven; flagged anyway: {flagged:#?}"
    );
}

#[test]
fn the_launch_preflight_raises_no_platform_mismatch_for_them() {
    let mods = parsed_fixtures();
    let index = ProviderIndex::build(&mods, &[], LoaderKind::NeoForge, DescriptorEra::Modern);
    let mismatches: Vec<Violation> = resolve(
        &mods,
        &index,
        LoaderKind::NeoForge,
        DescriptorEra::Modern,
        MC,
        Some(NEOFORGE),
    )
    .into_iter()
    .filter(|v| matches!(v, Violation::PlatformMismatch { .. }))
    .collect();
    assert!(
        mismatches.is_empty(),
        "a mismatch here gates Play: {mismatches:#?}"
    );
}

#[test]
fn the_same_seven_still_flag_where_the_loader_would_refuse_them() {
    // Minecraft 1.21.4 carries no matrix entry, and every declared range ends
    // at or below 1.21.1 — FML refuses all seven there.
    assert_eq!(violated_on("1.21.4", "21.4.150").len(), FIXTURES.len());
}
