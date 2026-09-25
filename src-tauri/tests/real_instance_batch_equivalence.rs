//! Real-data acceptance instrument for the batch compatibility probe
//! (2026-09-21 spec, §8/§11): for every enabled Modrinth mod of a REAL
//! instance, compare the batch answer with the per-project listing it
//! replaces, and fail on any disagreement.
//!
//! Gated on `LUCERNA_REAL_INSTANCE_DIR` — the instance root, the folder that
//! holds `lucerna/installed-mods.json`. Unset (CI, other machines) → skipped.
//! READ-ONLY: it parses the registry itself instead of calling
//! `installed::list`, which reconciles and may write. It DOES reach
//! api.modrinth.com: one batch pair, then one listing per mod at 5 req/s —
//! about 30 s on 140 mods.
//!
//! Run (PowerShell, from src-tauri/):
//!   $env:LUCERNA_REAL_INSTANCE_DIR = "<data root>\instances\<instance>"
//!   $env:LUCERNA_REAL_MC = "1.21.1"; $env:LUCERNA_REAL_LOADER = "neoforge"
//!   cargo test --test real_instance_batch_equivalence -- --nocapture

use lucerna_lib::mods::compat::{
    batch_answer, live_availability, BatchAnswer, InstalledFile, ProbeAnswer,
};
use lucerna_lib::mods::hash_probe::HashProbeCache;
use lucerna_lib::mods::installed::{mods_dir, registry_path};
use lucerna_lib::mods::modrinth::ModrinthClient;
use lucerna_lib::mods::platform::{InstalledMod, LoaderKind, ModPlatform, ModSource};
use lucerna_lib::mods::updates::{batch_update_state, classify_update, ModUpdateState};
use sha1::{Digest, Sha1};

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|s| !s.is_empty())
}

/// An update state reduced to what must agree: its kind, and the target id.
fn state_key(s: &ModUpdateState) -> String {
    match s {
        ModUpdateState::UpToDate => "up_to_date".into(),
        ModUpdateState::UpdateAvailable { target } => format!("update:{}", target.version_id),
        ModUpdateState::Unknown => "unknown".into(),
        ModUpdateState::CheckFailed { .. } => "check_failed".into(),
    }
}

#[tokio::test]
async fn batch_answers_agree_with_the_per_project_listing() {
    let Some(root) = env("LUCERNA_REAL_INSTANCE_DIR").map(std::path::PathBuf::from) else {
        eprintln!("SKIP: LUCERNA_REAL_INSTANCE_DIR is not set");
        return;
    };
    let mc = env("LUCERNA_REAL_MC").unwrap_or_else(|| "1.21.1".into());
    let loader: LoaderKind = serde_json::from_value(serde_json::Value::String(
        env("LUCERNA_REAL_LOADER").unwrap_or_else(|| "neoforge".into()),
    ))
    .expect("LUCERNA_REAL_LOADER is a loader slug");

    let registry: serde_json::Value = serde_json::from_slice(
        &std::fs::read(registry_path(&root)).expect("read installed-mods.json"),
    )
    .expect("parse installed-mods.json");
    let mods: Vec<InstalledMod> =
        serde_json::from_value(registry["mods"].clone()).expect("a `mods` array");
    let dir = mods_dir(&root);
    let rows: Vec<(InstalledMod, String)> = mods
        .into_iter()
        .filter(|m| m.enabled && m.source == Some(ModSource::Modrinth) && m.project_id.is_some())
        .filter_map(|m| {
            let bytes = std::fs::read(dir.join(&m.filename)).ok()?;
            let h = hex::encode(Sha1::digest(&bytes));
            Some((m, h))
        })
        .collect();

    let client = ModrinthClient::new();
    let shas: Vec<String> = rows.iter().map(|(_, h)| h.clone()).collect();
    let snap = HashProbeCache::new()
        .snapshot(&client, &shas, &mc, loader)
        .await
        .expect("the batch answered");

    let (mut decided, mut declined, mut disagreements) = (0, 0, Vec::new());
    for (m, h) in &rows {
        let project = m.project_id.as_deref().unwrap_or_default();
        let file = InstalledFile {
            on_disk_sha1: Some(h.as_str()),
            registry_sha1: &m.sha1,
            registry_version_id: m.version_id.as_deref(),
        };
        let own = snap.own.get(h).and_then(Option::as_ref);
        // An absent `latest` was declined by the snapshot (an unlisted newest
        // build, spec S10): the listing decides, exactly as in production.
        let latest = snap.latest.get(h);
        let answer = match latest {
            Some(latest) => batch_answer(&file, project, &mc, loader, own, latest),
            None => BatchAnswer::Undecided,
        };
        let listing = client.versions(project, Some(&mc), Some(loader)).await;
        let expected = match &listing {
            Ok(v) => live_availability(&file, ProbeAnswer::Found(v)),
            Err(_) => live_availability(&file, ProbeAnswer::Failed),
        };
        let first = listing
            .as_ref()
            .ok()
            .and_then(|v| v.first())
            .map(|v| v.version_number.clone());
        let mut agrees = match &answer {
            BatchAnswer::Exact {
                availability,
                newest,
            } => {
                decided += 1;
                *availability == expected && *newest == first
            }
            BatchAnswer::BuildsListed { newest } => {
                decided += 1;
                first.as_deref() == Some(newest.as_str())
            }
            BatchAnswer::Undecided => {
                declined += 1;
                true
            }
        };
        // The update check, where the registry still describes the file on disk
        // (it keys the batch by the registry digest).
        if let (true, Some(current), Some(latest), Ok(versions)) = (
            m.sha1.eq_ignore_ascii_case(h),
            m.version_id.as_deref(),
            latest,
            &listing,
        ) {
            if let Some(state) = batch_update_state(current, project, &mc, loader, own, latest) {
                agrees &= state_key(&state) == state_key(&classify_update(m, versions));
            }
        }
        println!(
            "{:<4} {:<40} batch={answer:?} listing={expected:?}",
            if agrees { "ok" } else { "DIFF" },
            m.name
        );
        if !agrees {
            disagreements.push(m.name.clone());
        }
    }
    println!(
        "{} mods: {decided} decided by the batch, {declined} left to the listing",
        rows.len()
    );
    assert!(
        disagreements.is_empty(),
        "the batch disagrees with the listing for: {disagreements:?}"
    );
}
