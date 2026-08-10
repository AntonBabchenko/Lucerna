//! The reported repro, as a gate.
//!
//! Instance `Testingggg-228` was 1.21.11 / Forge, changed to 1.20.1 / Forge
//! 47.4.10. Six jars built for 1.21.11 stayed in `mods/` and FML died at
//! pre-load. These are those six jars' real `META-INF/mods.toml` descriptors.
//!
//! ALL SIX MUST FLAG. Only two declare a `minecraft` range that excludes
//! 1.20.1; three declare no `minecraft` block at all and a fourth declares a
//! bare Maven spec, which is a soft requirement the loader accepts. They are
//! caught by the loader axis, which is why this feature has two.

use lucerna_lib::instances::schema::LoaderKind;
use lucerna_lib::mods::local::{read_jar_manifest_deps, DescriptorEra};
use lucerna_lib::mods::mc_compat::{platform_verdict, PlatformVerdict};
use std::io::{Cursor, Write};
use zip::write::SimpleFileOptions;

const FIXTURES: &[(&str, &str)] = &[
    (
        "biomesoplenty",
        include_str!("fixtures/mc_migration/biomesoplenty.mods.toml"),
    ),
    (
        "terrablender",
        include_str!("fixtures/mc_migration/terrablender.mods.toml"),
    ),
    (
        "glitchcore",
        include_str!("fixtures/mc_migration/glitchcore.mods.toml"),
    ),
    (
        "xaeroworldmap",
        include_str!("fixtures/mc_migration/xaeroworldmap.mods.toml"),
    ),
    (
        "xaerominimap",
        include_str!("fixtures/mc_migration/xaerominimap.mods.toml"),
    ),
    (
        "openpartiesandclaims",
        include_str!("fixtures/mc_migration/openpartiesandclaims.mods.toml"),
    ),
];

fn jar_with_mods_toml(toml: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut w = zip::ZipWriter::new(Cursor::new(&mut buf));
        w.start_file("META-INF/mods.toml", SimpleFileOptions::default())
            .expect("zip entry starts");
        w.write_all(toml.as_bytes()).expect("zip entry writes");
        w.finish().expect("zip finishes");
    }
    buf
}

#[test]
fn all_six_stale_jars_flag_on_the_downgraded_instance() {
    let mut unflagged = Vec::new();
    for (name, toml) in FIXTURES {
        let jar = jar_with_mods_toml(toml);
        let manifest = read_jar_manifest_deps(&jar).expect("fixture descriptor parses");
        let verdict = platform_verdict(
            &manifest,
            "1.20.1",
            LoaderKind::Forge,
            Some("47.4.10"),
            DescriptorEra::Modern,
        );
        if !matches!(verdict, PlatformVerdict::Violated { .. }) {
            unflagged.push(format!("{name}: {verdict:?}"));
        }
    }
    assert!(
        unflagged.is_empty(),
        "every stale jar must flag; these did not: {unflagged:#?}"
    );
}

#[test]
fn the_same_six_are_silent_on_the_instance_they_were_built_for() {
    // The other half of the gate: no false positives on a healthy 1.21.11 instance.
    for (name, toml) in FIXTURES {
        let jar = jar_with_mods_toml(toml);
        let manifest = read_jar_manifest_deps(&jar).expect("fixture descriptor parses");
        let verdict = platform_verdict(
            &manifest,
            "1.21.11",
            LoaderKind::Forge,
            Some("61.0.8"),
            DescriptorEra::Modern,
        );
        assert!(
            !matches!(verdict, PlatformVerdict::Violated { .. }),
            "{name} must not flag on the instance it was built for, got {verdict:?}"
        );
    }
}

#[test]
fn an_instance_without_a_loader_version_goes_silent_rather_than_wrong() {
    // `raw_minecraft.rs:49` writes `loader_version: None` on a real import.
    // Three of the six declare no `minecraft` block, so the feature is blind
    // there — a recorded property, not a surprise.
    // Looked up by name, not by index: this test depends on the fixture having
    // no `minecraft` block, and an index pin would silently start asserting
    // against a different jar if `FIXTURES` were ever reordered.
    let &(name, toml) = FIXTURES
        .iter()
        .find(|(n, _)| *n == "biomesoplenty")
        .expect("the biomesoplenty fixture is present");
    let jar = jar_with_mods_toml(toml);
    let manifest = read_jar_manifest_deps(&jar).expect("fixture descriptor parses");
    assert_eq!(
        platform_verdict(
            &manifest,
            "1.20.1",
            LoaderKind::Forge,
            None,
            DescriptorEra::Modern
        ),
        PlatformVerdict::Unknown,
        "{name} with no instance loader version"
    );
}
