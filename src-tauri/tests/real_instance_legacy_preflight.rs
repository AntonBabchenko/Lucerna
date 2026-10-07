//! Real-data acceptance instrument for the legacy `@Mod` annotation reader
//! (2026-10-07 spec, §5): run the dependency pre-flight over a COPY of a real
//! legacy-era instance and print what it reports.
//!
//! Gated on `LUCERNA_REAL_INSTANCE_DIR` — the instance root, the folder that
//! holds `lucerna/installed-mods.json`. Unset (CI, other machines) → skipped.
//! The registry and the jars are copied into a temp dir first, because
//! `installed::list` reconciles and may write; the real instance is only read.
//! No network.
//!
//! The assertion encodes "this instance starts", so run it only on one that does.
//!
//! Run (PowerShell, from src-tauri/):
//!   $env:LUCERNA_REAL_INSTANCE_DIR = "<data root>\instances\<instance>"
//!   $env:LUCERNA_REAL_MC = "1.12.2"
//!   cargo test --test real_instance_legacy_preflight -- --nocapture

use lucerna_lib::instances::schema::LoaderKind;
use lucerna_lib::mods::installed::{mods_dir, registry_path};
use lucerna_lib::mods::preflight::dependency_preflight_for_root;

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|s| !s.is_empty())
}

/// Copy the registry and every file of the mods folder — `.disabled` jars too.
fn copy_instance(src: &std::path::Path, dst: &std::path::Path) {
    let registry = registry_path(dst);
    std::fs::create_dir_all(registry.parent().expect("the registry has a parent")).unwrap();
    std::fs::copy(registry_path(src), &registry).unwrap();
    let (from, to) = (mods_dir(src), mods_dir(dst));
    std::fs::create_dir_all(&to).unwrap();
    for entry in std::fs::read_dir(&from).unwrap().flatten() {
        if entry.path().is_file() {
            std::fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
        }
    }
}

#[tokio::test]
async fn a_real_legacy_instance_that_starts_reports_nothing() {
    let Some(src) = env("LUCERNA_REAL_INSTANCE_DIR") else {
        eprintln!("LUCERNA_REAL_INSTANCE_DIR unset: skipped");
        return;
    };
    let mc = env("LUCERNA_REAL_MC").unwrap_or_else(|| "1.12.2".into());
    let td = tempfile::TempDir::new().unwrap();
    copy_instance(std::path::Path::new(&src), td.path());

    let started = std::time::Instant::now();
    let report = dependency_preflight_for_root(td.path(), None, LoaderKind::Forge, &mc, None)
        .await
        .unwrap();
    let elapsed = started.elapsed();
    for v in &report.violations {
        println!(
            "VIOLATION {} -> {} {:?} needed {:?} installed {:?}",
            v.dependent_name, v.dep_id, v.kind, v.needed, v.installed_version
        );
    }
    println!("unjudged: {:?}", report.unjudged);
    println!("cold scan, no cache: {elapsed:?}");
    assert!(
        report.violations.is_empty(),
        "{} violation(s)",
        report.violations.len()
    );
    assert!(report.unjudged.is_empty(), "{:?}", report.unjudged);
}
