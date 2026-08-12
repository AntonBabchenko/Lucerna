//! Real-data probe: run the ACTUAL offline verdict + plan-routing logic over a
//! REAL instance's mod jars and print, per mod, why it is (in)compatible.
//!
//! Gated on the env var `LUCERNA_REAL_MODS_DIR` pointing at an instance's
//! `.minecraft/mods` directory, plus `LUCERNA_REAL_MC` / `LUCERNA_REAL_LOADER_VERSION`
//! for the instance's platform. When the var is unset (CI, other machines) the
//! tests skip — they never read a hard-coded path, so committing them is safe.
//!
//! Run:
//!   LUCERNA_REAL_MODS_DIR=".../Testingggg-228/.minecraft/mods" \
//!   LUCERNA_REAL_MC=1.21 LUCERNA_REAL_LOADER_VERSION=51.0.33 \
//!   cargo test --test real_instance_migration_probe -- --nocapture

use lucerna_lib::instances::schema::LoaderKind;
use lucerna_lib::mods::local::{descriptor_era, read_jar_manifest_deps};
use lucerna_lib::mods::mc_compat::{platform_verdict, PlatformAxis, PlatformVerdict};
use lucerna_lib::mods::migration::{
    build_migration_plan, CandidateQuery, ModMigrationInput, StrandedReason,
};
use lucerna_lib::mods::platform::{ModFile, ModSource, ModVersion};
use std::path::PathBuf;

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|s| !s.is_empty())
}

struct RealInstance {
    mods_dir: PathBuf,
    mc: String,
    loader_version: Option<String>,
}

fn real_instance() -> Option<RealInstance> {
    let dir = PathBuf::from(env("LUCERNA_REAL_MODS_DIR")?);
    if !dir.is_dir() {
        eprintln!("SKIP: LUCERNA_REAL_MODS_DIR is not a directory: {dir:?}");
        return None;
    }
    Some(RealInstance {
        mods_dir: dir,
        mc: env("LUCERNA_REAL_MC").unwrap_or_else(|| "1.21".into()),
        loader_version: env("LUCERNA_REAL_LOADER_VERSION"),
    })
}

fn verdict_for(jar_bytes: &[u8], mc: &str, loader_version: Option<&str>) -> PlatformVerdict {
    read_jar_manifest_deps(jar_bytes)
        .ok()
        .map(|man| {
            platform_verdict(
                &man,
                mc,
                LoaderKind::Forge,
                loader_version,
                descriptor_era(mc),
            )
        })
        .unwrap_or(PlatformVerdict::Unknown)
}

/// Read every real jar, print its verdict, and prove the load-bearing cases:
/// the 1.20.1 mods flag on the MC axis, and Xaero's Minimap flags on the LOADER
/// axis (it needs a newer Forge than the instance runs) — the case the plan must
/// route to `LoaderTooOld`, not a no-op reinstall.
#[test]
fn real_instance_mod_verdicts() {
    let Some(inst) = real_instance() else {
        eprintln!("SKIP real_instance_mod_verdicts: set LUCERNA_REAL_MODS_DIR to run");
        return;
    };
    println!(
        "\n=== REAL instance verdicts — mc={} forge={} ===",
        inst.mc,
        inst.loader_version.as_deref().unwrap_or("<none>")
    );

    let mut jars: Vec<PathBuf> = std::fs::read_dir(&inst.mods_dir)
        .expect("read mods dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "jar"))
        .collect();
    jars.sort();
    assert!(!jars.is_empty(), "no jars found in {:?}", inst.mods_dir);

    let mut xaero_minimap: Option<PlatformVerdict> = None;
    let (mut fits, mut mc_violated, mut loader_violated, mut unknown) = (0, 0, 0, 0);

    for jar in &jars {
        let name = jar.file_name().unwrap().to_string_lossy().to_string();
        let bytes = std::fs::read(jar).expect("read jar");
        let verdict = verdict_for(&bytes, &inst.mc, inst.loader_version.as_deref());
        let label = match &verdict {
            PlatformVerdict::Fits => {
                fits += 1;
                "FITS".to_string()
            }
            PlatformVerdict::Unknown => {
                unknown += 1;
                "UNKNOWN".to_string()
            }
            PlatformVerdict::Violated {
                axis,
                declared,
                actual,
                ..
            } => {
                match axis {
                    PlatformAxis::Minecraft => mc_violated += 1,
                    PlatformAxis::Loader => loader_violated += 1,
                }
                format!("VIOLATED[{axis:?}] declared={declared} actual={actual}")
            }
        };
        println!("  {name:52} {label}");
        if name.starts_with("xaerominimap") {
            xaero_minimap = Some(verdict);
        }
    }
    println!(
        "  -- totals: fits={fits} mc_violated={mc_violated} loader_violated={loader_violated} unknown={unknown}\n"
    );

    // Xaero's Minimap (forge-1.21.1-26.4.2) declares a Forge range the instance's
    // 51.0.33 does not satisfy → a LOADER-axis violation. This is exactly the
    // case the plan must not offer as a no-op reinstall.
    match xaero_minimap {
        Some(PlatformVerdict::Violated {
            axis: PlatformAxis::Loader,
            declared,
            actual,
            ..
        }) => {
            println!("  Xaero's Minimap: VIOLATED on the loader axis (declared {declared}, instance {actual}) — correct.");
        }
        other => panic!("expected Xaero's Minimap to be loader-axis Violated, got {other:?}"),
    }
}

/// End-to-end on the REAL Xaero's verdict: a loader-axis violation whose only
/// candidate build is the same jar already installed must land in `stranded`
/// with `LoaderTooOld` — never a no-op `replaceable` reinstall (the reported
/// "press Fix forever" loop). Uses the real jar's verdict; the candidate models
/// the loader-kind-blind query returning the identical installed build.
#[test]
fn real_xaero_loader_violation_routes_to_loader_too_old() {
    let Some(inst) = real_instance() else {
        eprintln!(
            "SKIP real_xaero_loader_violation_routes_to_loader_too_old: set LUCERNA_REAL_MODS_DIR"
        );
        return;
    };
    let xaero = std::fs::read_dir(&inst.mods_dir)
        .expect("read mods dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("xaerominimap"))
        })
        .expect("xaerominimap jar present");
    let bytes = std::fs::read(&xaero).expect("read xaero jar");
    let manifest = read_jar_manifest_deps(&bytes).expect("xaero descriptor parses");
    let verdict = platform_verdict(
        &manifest,
        &inst.mc,
        LoaderKind::Forge,
        inst.loader_version.as_deref(),
        descriptor_era(&inst.mc),
    );
    assert!(
        matches!(
            verdict,
            PlatformVerdict::Violated {
                axis: PlatformAxis::Loader,
                ..
            }
        ),
        "real Xaero's must be a loader-axis violation, got {verdict:?}"
    );
    // The version Xaero's declares it targets, straight from its real descriptor.
    let declared_mc = manifest
        .platform
        .iter()
        .find(|d| d.dep_id == "minecraft")
        .map(|d| d.range.clone());
    println!(
        "  Xaero's declares minecraft = {declared_mc:?} (instance mc = {})",
        inst.mc
    );

    // The loader-kind-blind platform query returns the newest 1.21-Forge build —
    // which for Xaero's is the identical jar already installed (there is no
    // 1.21-Forge Xaero's needing an older Forge). Model that with a same-sha
    // candidate; the plan must strand it, not offer a reinstall of itself.
    const SHA: &str = "installed-xaero-sha";
    let same_build = ModVersion {
        source: ModSource::Modrinth,
        project_id: "xaero-minimap".into(),
        version_id: "26.4.2".into(),
        name: "Xaero's Minimap".into(),
        version_number: "26.4.2".into(),
        mc_versions: vec!["1.21".into()],
        loaders: vec![LoaderKind::Forge],
        primary_file: ModFile {
            filename: "xaerominimap-forge-1.21.1-26.4.2.jar".into(),
            url: "https://example/xaero.jar".into(),
            sha1: Some(SHA.into()),
            size: 1.0,
            distribution_allowed: true,
            sha256: None,
        },
        deps: vec![],
        published_at: None,
    };

    let plan = build_migration_plan(vec![ModMigrationInput {
        sha1: SHA.into(),
        name: "Xaero's Minimap".into(),
        verdict,
        identity: Ok((ModSource::Modrinth, "xaero-minimap".into())),
        candidate: Some(CandidateQuery::Found(vec![same_build])),
        declared_mc: declared_mc.clone(),
        // The real jar is a Forge build judged on its own Forge instance —
        // the family axis is clean; this probe exercises the loader-VERSION arm.
        family_mismatch: false,
    }]);

    println!(
        "\n=== plan for real loader-violated Xaero's: replaceable={} stranded={} ===",
        plan.replaceable.len(),
        plan.stranded.len()
    );
    assert!(
        plan.replaceable.is_empty(),
        "a same-file reinstall must never be offered — got a replaceable row"
    );
    assert_eq!(
        plan.stranded.len(),
        1,
        "Xaero's must be the one stranded row"
    );
    // Stranded as LoaderTooOld, carrying the REAL version the jar declares it was
    // built for (its `minecraft` dep, "1.21.1") — so the message can name it
    // instead of promising a loader bump that does not exist for MC 1.21.
    match &plan.stranded[0].reason {
        StrandedReason::LoaderTooOld { built_for_mc } => {
            assert_eq!(
                built_for_mc.as_deref(),
                declared_mc.as_deref(),
                "the stranded reason must carry the jar's declared minecraft"
            );
            println!(
                "  → stranded with LoaderTooOld {{ built_for_mc: {built_for_mc:?} }} — honest, not a reinstall.\n"
            );
        }
        other => panic!("expected LoaderTooOld, got {other:?}"),
    }
}
