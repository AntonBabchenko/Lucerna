//! Read-only integrity verification + targeted repair of an instance's
//! core game artefacts. Isolated from `versions::install` (the working
//! install/launch path is untouched). See
//! `docs/superpowers/specs/2026-06-03-verify-repair-design.md`.

pub mod plan;
pub mod progress;
pub mod repair;
pub mod scan;
pub use progress::{VerifyPhase, VerifyProgress};
pub use repair::repair_instance_report;
pub use scan::verify_instance_report;

use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::atomic::{AtomicBool, Ordering};

/// Set while a repair is rewriting an instance's shared files. `launch::start`
/// consults this so the game is never launched on top of half-rewritten
/// library/client jars (a repair streams files over minutes; the one-shot
/// `is_running()` check at repair start can't see a launch that happens
/// mid-rewrite, so we guard the reverse direction too).
static REPAIR_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

/// True while a repair is actively rewriting files. Checked by the launch path.
pub fn repair_in_progress() -> bool {
    REPAIR_IN_PROGRESS.load(Ordering::SeqCst)
}

/// RAII guard for the repair-in-progress flag. `acquire()` returns `None` if a
/// repair is already running (rejects concurrent repairs even if the frontend
/// queue is bypassed); the flag clears on drop — panic-safe.
pub struct RepairGuard {
    _private: (),
}

impl RepairGuard {
    pub fn acquire() -> Option<Self> {
        REPAIR_IN_PROGRESS
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .ok()
            .map(|_| RepairGuard { _private: () })
    }
}

impl Drop for RepairGuard {
    fn drop(&mut self) {
        REPAIR_IN_PROGRESS.store(false, Ordering::SeqCst);
    }
}

/// Outcome of checking a single planned artefact against disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactStatus {
    Ok,
    Missing,
    /// File present but bad: SHA mismatch for hashed artefacts; unparseable
    /// for the profile JSON (which has no authoritative SHA).
    Corrupt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum VerifyCategory {
    Client,
    Libraries,
    Assets,
    Jre,
    ProfileJson,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CategoryReport {
    pub category: VerifyCategory,
    pub total: u32,
    pub ok: u32,
    pub missing: u32,
    pub corrupt: u32,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct ProblemArtifact {
    pub category: VerifyCategory,
    pub rel_path: String,
    pub expected_sha: String,
    /// `None` when the artefact is locally produced (e.g. Forge `{PATCHED}`
    /// jar) or generated (profile JSON) — repair routes these through a full
    /// `install_version` instead of `download_with_sha`.
    pub url: Option<String>,
    pub status: ArtifactStatus,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct VerifyReport {
    pub instance_id: String,
    pub effective_version_id: String,
    pub categories: Vec<CategoryReport>,
    pub problems: Vec<ProblemArtifact>,
    pub healthy: bool,
    /// `true` means the manifest/profile JSON itself is missing or unparseable,
    /// so per-file SHAs are unknowable — the report is unhealthy and repair must
    /// re-fetch/regenerate the manifest first. Naming: "recoverable" = repair
    /// can recover it, NOT "everything's fine". (true = there IS a problem.)
    pub manifest_recoverable: bool,
}

impl VerifyReport {
    /// `planned_totals`: per-category planned artefact counts. `problems`:
    /// only Missing/Corrupt artefacts. `manifest_recoverable`: the profile
    /// JSON / manifest itself was missing or unparseable.
    pub fn build(
        instance_id: String,
        effective_version_id: String,
        planned_totals: &[(VerifyCategory, u32)],
        problems: Vec<ProblemArtifact>,
        manifest_recoverable: bool,
    ) -> Self {
        let categories = aggregate_categories(planned_totals, &problems);
        let healthy = problems.is_empty() && !manifest_recoverable;
        VerifyReport {
            instance_id,
            effective_version_id,
            categories,
            problems,
            healthy,
            manifest_recoverable,
        }
    }
}

/// Persisted summary of an instance's last integrity check. Stored in
/// instance.json and surfaced on InstanceWithStatus for a passive badge.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Type)]
pub struct IntegrityStatus {
    pub healthy: bool,
    /// Unix ms of the check (f64 per the specta/u64 rule).
    pub checked_unix_ms: f64,
    pub categories: Vec<CategoryReport>,
    pub problem_count: u32,
}

impl IntegrityStatus {
    pub fn from_report(report: &VerifyReport, checked_unix_ms: f64) -> Self {
        IntegrityStatus {
            healthy: report.healthy,
            checked_unix_ms,
            categories: report.categories.clone(),
            problem_count: report.problems.len() as u32,
        }
    }
}

/// Roll planned per-category totals + the flat problem list into per-category
/// counts. `ok = total - missing - corrupt` (never underflows: problems are a
/// subset of planned artefacts).
pub fn aggregate_categories(
    planned_totals: &[(VerifyCategory, u32)],
    problems: &[ProblemArtifact],
) -> Vec<CategoryReport> {
    planned_totals
        .iter()
        .map(|&(category, total)| {
            let missing = problems
                .iter()
                .filter(|p| p.category == category && p.status == ArtifactStatus::Missing)
                .count() as u32;
            let corrupt = problems
                .iter()
                .filter(|p| p.category == category && p.status == ArtifactStatus::Corrupt)
                .count() as u32;
            CategoryReport {
                category,
                total,
                ok: total.saturating_sub(missing + corrupt),
                missing,
                corrupt,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn problem(cat: VerifyCategory, path: &str) -> ProblemArtifact {
        ProblemArtifact {
            category: cat,
            rel_path: path.into(),
            expected_sha: "abc".into(),
            url: Some("https://example/x".into()),
            status: ArtifactStatus::Missing,
        }
    }

    #[test]
    fn aggregates_counts_per_category() {
        let planned_totals = [
            (VerifyCategory::Client, 1u32),
            (VerifyCategory::Libraries, 3u32),
            (VerifyCategory::Assets, 5u32),
        ];
        let problems = vec![
            problem(VerifyCategory::Assets, "a"),
            problem(VerifyCategory::Assets, "b"),
        ];
        let cats = aggregate_categories(&planned_totals, &problems);
        let assets = cats
            .iter()
            .find(|c| c.category == VerifyCategory::Assets)
            .unwrap();
        assert_eq!(assets.total, 5);
        assert_eq!(assets.missing, 2);
        assert_eq!(assets.ok, 3);
        let client = cats
            .iter()
            .find(|c| c.category == VerifyCategory::Client)
            .unwrap();
        assert_eq!(client.ok, 1);
    }

    #[test]
    fn report_healthy_when_no_problems_and_manifest_ok() {
        let r = VerifyReport::build("i".into(), "1.20.4".into(), &[], vec![], false);
        assert!(r.healthy);
    }

    #[test]
    fn report_unhealthy_when_manifest_unrecoverable_even_without_file_problems() {
        let r = VerifyReport::build("i".into(), "1.20.4".into(), &[], vec![], true);
        assert!(!r.healthy);
        assert!(r.manifest_recoverable);
    }

    #[test]
    fn repair_guard_blocks_concurrent_and_clears_on_drop() {
        assert!(!repair_in_progress());
        let g1 = RepairGuard::acquire().expect("first acquire succeeds");
        assert!(repair_in_progress());
        // A second repair (or a launch) sees the flag set.
        assert!(RepairGuard::acquire().is_none());
        drop(g1);
        assert!(!repair_in_progress());
        // Re-acquirable once cleared.
        let g2 = RepairGuard::acquire().expect("re-acquire after drop");
        assert!(repair_in_progress());
        drop(g2);
        assert!(!repair_in_progress());
    }

    #[test]
    fn integrity_status_from_report_maps_fields() {
        let planned_totals = [
            (VerifyCategory::Client, 1u32),
            (VerifyCategory::Assets, 5u32),
        ];
        let problems = vec![
            problem(VerifyCategory::Assets, "a"),
            problem(VerifyCategory::Assets, "b"),
        ];
        let report = VerifyReport::build(
            "inst-1".into(),
            "1.20.4".into(),
            &planned_totals,
            problems,
            false,
        );
        let status = IntegrityStatus::from_report(&report, 1_700_000_000_000.0);
        assert_eq!(status.healthy, report.healthy);
        assert!(!status.healthy, "two problems → unhealthy");
        assert_eq!(status.problem_count, 2);
        assert_eq!(status.categories.len(), 2);
        assert_eq!(status.checked_unix_ms, 1_700_000_000_000.0);
    }

    #[test]
    fn integrity_status_round_trips_through_json() {
        let report = VerifyReport::build("i".into(), "1.20.4".into(), &[], vec![], false);
        let status = IntegrityStatus::from_report(&report, 42.0);
        let json = serde_json::to_string(&status).unwrap();
        let back: IntegrityStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(status, back);
    }
}
