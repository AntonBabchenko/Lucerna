//! Mod-install matrix e2e: for each (loader x popular MC) combo, install N
//! randomly-sampled COMPATIBLE mods from live Modrinth and assert the
//! install/dependency pipeline succeeds (right version chosen, deps resolved,
//! jars on disk, no errors). The install analogue of loader_matrix_e2e.rs.
//!
//! Gated behind #[ignore] — real network + downloads. Run:
//!   cargo test --manifest-path src-tauri/Cargo.toml \
//!     --test mods_matrix_e2e -- --ignored --nocapture
//! Subset overrides for a fast single-combo run:
//!   LUCERNA_MOD_MATRIX_MC=1.21.1 LUCERNA_MOD_MATRIX_LOADER=fabric \
//!   LUCERNA_MOD_MATRIX_N=3 cargo test --test mods_matrix_e2e -- --ignored --nocapture

use lucerna_lib::instances::schema::LoaderKind;

const MC_VERSIONS_DEFAULT: &[&str] = &["1.16.5", "1.18.2", "1.20.1", "1.20.4", "1.21.1", "1.21.8"];

fn mc_versions() -> Vec<String> {
    match std::env::var("LUCERNA_MOD_MATRIX_MC") {
        Ok(s) if !s.trim().is_empty() => s.split(',').map(|s| s.trim().to_string()).collect(),
        _ => MC_VERSIONS_DEFAULT.iter().map(|s| s.to_string()).collect(),
    }
}

/// NeoForge applies to MC >= 1.20.1 (fork point). Mirrors loader_matrix_e2e.
fn neoforge_applies(mc: &str) -> bool {
    let p: Vec<u32> = mc.split('.').filter_map(|s| s.parse().ok()).collect();
    let (maj, min, pat) = (
        p.first().copied().unwrap_or(0),
        p.get(1).copied().unwrap_or(0),
        p.get(2).copied().unwrap_or(0),
    );
    if maj != 1 {
        return maj > 1;
    }
    if min != 20 {
        return min > 20;
    }
    pat >= 1
}

/// Mod loaders for a combo (Vanilla excluded — vanilla can't take mods).
/// Fabric/Quilt from 1.16; Forge from 1.6; NeoForge per neoforge_applies.
fn loaders_for(mc: &str) -> Vec<LoaderKind> {
    let p: Vec<u32> = mc.split('.').filter_map(|s| s.parse().ok()).collect();
    let mm = (
        p.first().copied().unwrap_or(0),
        p.get(1).copied().unwrap_or(0),
    );
    let mut out = Vec::new();
    if mm >= (1, 16) {
        out.push(LoaderKind::Fabric);
        out.push(LoaderKind::Quilt);
    }
    if mm >= (1, 6) {
        out.push(LoaderKind::Forge);
    }
    if neoforge_applies(mc) {
        out.push(LoaderKind::NeoForge);
    }
    out
}

/// Apply the LUCERNA_MOD_MATRIX_LOADER subset filter (csv of loader names).
fn loader_enabled(l: LoaderKind) -> bool {
    let want = match std::env::var("LUCERNA_MOD_MATRIX_LOADER") {
        Ok(s) if !s.trim().is_empty() => s,
        _ => return true,
    };
    let name = match l {
        LoaderKind::Fabric => "fabric",
        LoaderKind::Quilt => "quilt",
        LoaderKind::Forge => "forge",
        LoaderKind::NeoForge => "neoforge",
        LoaderKind::Vanilla => "vanilla",
    };
    want.split(',').any(|w| w.trim().eq_ignore_ascii_case(name))
}

use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::SeedableRng;

/// Deterministically sample up to `n` items from `pool` using `seed`.
/// Returns all of `pool` (shuffled) when `pool.len() <= n`. Same seed +
/// same pool => same sample, so a failing combo reproduces with the logged seed.
fn seeded_sample<T: Clone>(pool: &[T], n: usize, seed: u64) -> Vec<T> {
    let mut idx: Vec<usize> = (0..pool.len()).collect();
    let mut rng = StdRng::seed_from_u64(seed);
    idx.shuffle(&mut rng);
    idx.into_iter().take(n).map(|i| pool[i].clone()).collect()
}

#[cfg(test)]
mod config_tests {
    use super::*;
    #[test]
    fn neoforge_gated_at_1_20_1() {
        assert!(!neoforge_applies("1.20.0"));
        assert!(neoforge_applies("1.20.1"));
        assert!(neoforge_applies("1.21.8"));
        assert!(!neoforge_applies("1.16.5"));
    }
    #[test]
    fn loaders_for_modern_includes_all_four() {
        let ls = loaders_for("1.21.1");
        assert!(ls.contains(&LoaderKind::Fabric));
        assert!(ls.contains(&LoaderKind::Quilt));
        assert!(ls.contains(&LoaderKind::Forge));
        assert!(ls.contains(&LoaderKind::NeoForge));
        assert!(!ls.contains(&LoaderKind::Vanilla));
    }
    #[test]
    fn loaders_for_legacy_excludes_fabric_and_neoforge() {
        let ls = loaders_for("1.12.2");
        assert!(ls.contains(&LoaderKind::Forge));
        assert!(!ls.contains(&LoaderKind::Fabric));
        assert!(!ls.contains(&LoaderKind::NeoForge));
    }
}

#[cfg(test)]
mod sample_tests {
    use super::*;
    #[test]
    fn sample_is_deterministic_for_a_seed() {
        let pool: Vec<u32> = (0..100).collect();
        let a = seeded_sample(&pool, 20, 42);
        let b = seeded_sample(&pool, 20, 42);
        assert_eq!(a, b, "same seed -> same sample");
        assert_eq!(a.len(), 20);
    }
    #[test]
    fn different_seeds_differ() {
        let pool: Vec<u32> = (0..100).collect();
        assert_ne!(seeded_sample(&pool, 20, 1), seeded_sample(&pool, 20, 2));
    }
    #[test]
    fn returns_all_when_pool_smaller_than_n() {
        let pool: Vec<u32> = (0..5).collect();
        let got = seeded_sample(&pool, 20, 7);
        assert_eq!(got.len(), 5);
        let mut sorted = got.clone();
        sorted.sort();
        assert_eq!(sorted, pool);
    }
}

use lucerna_lib::mods::install::{install_one, ModInstallPhase, ProgressFn};
use lucerna_lib::mods::installed;
use lucerna_lib::mods::modrinth::ModrinthClient;
use lucerna_lib::mods::platform::{ModPlatform, ModSearchQuery, ModSort, ModSource};
use std::path::Path;

/// Per-mod result within a combo.
#[derive(Debug)]
enum ModOutcome {
    Installed {
        project_id: String,
        version_id: String,
        deps: usize,
    },
    Skipped {
        project_id: String,
        reason: String,
    }, // no compatible version / distribution disabled
    Failed {
        project_id: String,
        error: String,
    }, // a real pipeline error
}

struct ComboReport {
    mc: String,
    loader: LoaderKind,
    seed: u64,
    outcomes: Vec<ModOutcome>,
}

impl ComboReport {
    fn installed(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|o| matches!(o, ModOutcome::Installed { .. }))
            .count()
    }
    fn skipped(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|o| matches!(o, ModOutcome::Skipped { .. }))
            .count()
    }
    fn failed(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|o| matches!(o, ModOutcome::Failed { .. }))
            .count()
    }
}

fn noop_progress() -> ProgressFn {
    Box::new(|_p: ModInstallPhase, _a: u64, _b: Option<u64>| {})
}

/// Page Modrinth search (facet-filtered to mc+loader) into a pool of up to `cap` summaries.
async fn fetch_pool(
    client: &ModrinthClient,
    mc: &str,
    loader: LoaderKind,
    cap: usize,
) -> Result<Vec<lucerna_lib::mods::platform::ModSummary>, String> {
    let mut pool = Vec::new();
    let page_size = 100u32;
    let mut offset = 0u32;
    loop {
        let q = ModSearchQuery {
            source: ModSource::Modrinth,
            query: String::new(),
            mc_version: Some(mc.to_string()),
            loader: Some(loader),
            sort: ModSort::Downloads,
            page_size,
            offset,
        };
        let page = client
            .search(&q)
            .await
            .map_err(|e| format!("search: {e:?}"))?;
        let total = page.total;
        let n_hits = page.hits.len();
        pool.extend(page.hits);
        offset += page_size;
        // Stop on: enough collected, server says we're past the end, or this
        // page came back empty (catalogue inconsistency / true end).
        if pool.len() >= cap || offset >= total || n_hits == 0 {
            break;
        }
    }
    pool.truncate(cap);
    Ok(pool)
}

/// Run one (mc, loader) combo end to end.
async fn run_combo(mc: &str, loader: LoaderKind, n: usize, seed: u64) -> ComboReport {
    let mut report = ComboReport {
        mc: mc.to_string(),
        loader,
        seed,
        outcomes: Vec::new(),
    };
    let tmp = tempfile::tempdir().expect("tempdir");
    let data_dir = tmp.path().join("data");
    let instance_root = tmp.path().join("instance");
    std::fs::create_dir_all(&data_dir).unwrap();
    std::fs::create_dir_all(&instance_root).unwrap();
    let client = ModrinthClient::new();
    let progress = noop_progress();

    let cap = (n * 10).max(200);
    let pool = match fetch_pool(&client, mc, loader, cap).await {
        Ok(p) => p,
        Err(e) => {
            report.outcomes.push(ModOutcome::Failed {
                project_id: "<search>".into(),
                error: e,
            });
            return report;
        }
    };
    let sample = seeded_sample(&pool, n, seed);

    for summary in sample {
        let pid = summary.project_id.clone();
        let versions = match client.versions(&pid, Some(mc), Some(loader)).await {
            Ok(v) => v,
            Err(e) => {
                report.outcomes.push(ModOutcome::Failed {
                    project_id: pid,
                    error: format!("versions: {e:?}"),
                });
                continue;
            }
        };
        // Modrinth returns a project's versions newest-first, so the first
        // (mc, loader)-filtered entry is the version the browser would install.
        let Some(primary) = versions.into_iter().next() else {
            report.outcomes.push(ModOutcome::Skipped {
                project_id: pid,
                reason: "no compatible version".into(),
            });
            continue;
        };
        if !primary.primary_file.distribution_allowed {
            report.outcomes.push(ModOutcome::Skipped {
                project_id: pid,
                reason: "distribution disabled".into(),
            });
            continue;
        }
        let plan = match client.resolve_deps(&primary, mc, loader).await {
            Ok(p) => p,
            Err(e) => {
                report.outcomes.push(ModOutcome::Failed {
                    project_id: pid,
                    error: format!("resolve: {e:?}"),
                });
                continue;
            }
        };
        let primary_vid = primary.version_id.clone();
        if let Err(e) = install_one(&data_dir, &instance_root, primary, &progress).await {
            report.outcomes.push(ModOutcome::Failed {
                project_id: pid,
                error: format!("install primary: {e:?}"),
            });
            continue;
        }
        let mut dep_count = 0usize;
        let mut dep_failed: Option<(String, String)> = None;
        for dep in plan.required {
            let dep_pid = dep.version.project_id.clone();
            // A required dep the author marked non-distributable can't be
            // downloaded by anyone — record it as a visible SKIP rather than
            // silently dropping it, so a missing required dependency shows in
            // the log instead of vanishing behind the primary's (+N deps).
            if !dep.version.primary_file.distribution_allowed {
                report.outcomes.push(ModOutcome::Skipped {
                    project_id: dep_pid,
                    reason: format!("required dep of {pid}: distribution disabled"),
                });
                continue;
            }
            match install_one(&data_dir, &instance_root, dep.version, &progress).await {
                Ok(_) => dep_count += 1,
                Err(e) => {
                    dep_failed = Some((dep_pid, format!("install dep of {pid}: {e:?}")));
                    break;
                }
            }
        }
        if let Some((dep_pid, err)) = dep_failed {
            report.outcomes.push(ModOutcome::Failed {
                project_id: dep_pid,
                error: err,
            });
            continue;
        }
        report.outcomes.push(ModOutcome::Installed {
            project_id: pid,
            version_id: primary_vid,
            deps: dep_count,
        });
    }

    // Guard against a silent false-green: a non-empty pool that installs
    // nothing (every sample entry skipped) means the run exercised the
    // pipeline on zero mods. Treat that as a combo failure, not a pass.
    if !pool.is_empty() && report.installed() == 0 && report.failed() == 0 {
        report.outcomes.push(ModOutcome::Failed {
            project_id: "<combo>".into(),
            error: format!(
                "pool had {} mods but installed 0 ({} skipped) — nothing exercised",
                pool.len(),
                report.skipped()
            ),
        });
        return report;
    }

    let registry = installed::list(&instance_root).await.unwrap_or_default();
    let mods_dir = installed::mods_dir(&instance_root);
    for o in &report.outcomes {
        if let ModOutcome::Installed { version_id, .. } = o {
            let row = registry
                .iter()
                .find(|r| r.version_id.as_deref() == Some(version_id.as_str()))
                .unwrap_or_else(|| {
                    panic!(
                        "[{} {:?}] installed version {} missing from registry (seed {})",
                        mc, loader, version_id, seed
                    )
                });
            assert!(
                mods_dir.join(&row.filename).exists(),
                "[{} {:?}] jar {} not on disk (seed {})",
                mc,
                loader,
                row.filename,
                seed
            );
        }
    }
    report
}

use std::io::Write;

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(default)
}
fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(default)
}

fn log_dir() -> std::path::PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("mod-install-matrix-logs");
    std::fs::create_dir_all(&dir).expect("create log dir");
    dir
}

fn write_combo_log(r: &ComboReport) {
    let fname = format!("{}_{:?}.log", r.mc.replace('.', "_"), r.loader);
    let path = log_dir().join(fname);
    let mut f = std::fs::File::create(&path).expect("combo log");
    writeln!(
        f,
        "combo: MC {} loader {:?} seed {}",
        r.mc, r.loader, r.seed
    )
    .ok();
    writeln!(
        f,
        "installed {} skipped {} failed {}",
        r.installed(),
        r.skipped(),
        r.failed()
    )
    .ok();
    for o in &r.outcomes {
        match o {
            ModOutcome::Installed {
                project_id,
                version_id,
                deps,
            } => writeln!(f, "  OK   {project_id} -> {version_id} (+{deps} deps)").ok(),
            ModOutcome::Skipped { project_id, reason } => {
                writeln!(f, "  SKIP {project_id}: {reason}").ok()
            }
            ModOutcome::Failed { project_id, error } => {
                writeln!(f, "  FAIL {project_id}: {error}").ok()
            }
        };
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "real network + downloads; run manually with --ignored"]
async fn mod_install_matrix() {
    let n = env_usize("LUCERNA_MOD_MATRIX_N", 20);
    let base_seed = env_u64("LUCERNA_MOD_MATRIX_SEED", 0xC0FFEE);

    let mut reports: Vec<ComboReport> = Vec::new();
    let mut combo_idx: u64 = 0;
    for mc in mc_versions() {
        for loader in loaders_for(&mc) {
            if !loader_enabled(loader) {
                continue;
            }
            let seed = base_seed.wrapping_add(combo_idx);
            combo_idx += 1;
            eprintln!("=== MC {mc} {loader:?} (seed {seed}) ===");
            let report = run_combo(&mc, loader, n, seed).await;
            eprintln!(
                "    installed {} skipped {} failed {}",
                report.installed(),
                report.skipped(),
                report.failed()
            );
            write_combo_log(&report);
            reports.push(report);
        }
    }

    eprintln!("\n| MC | Loader | seed | installed | skipped | failed |");
    eprintln!("|---|---|---|---|---|---|");
    let mut total_failed = 0usize;
    for r in &reports {
        total_failed += r.failed();
        eprintln!(
            "| {} | {:?} | {} | {} | {} | {} |",
            r.mc,
            r.loader,
            r.seed,
            r.installed(),
            r.skipped(),
            r.failed()
        );
    }
    eprintln!("\nlogs: {}", log_dir().display());
    assert_eq!(
        total_failed, 0,
        "{} mod install(s) failed across the matrix — see per-combo logs",
        total_failed
    );
}
