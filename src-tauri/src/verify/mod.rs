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

use serde::Serialize;
use specta::Type;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum VerifyCategory {
    Client,
    Libraries,
    Assets,
    Jre,
    ProfileJson,
}

#[derive(Debug, Clone, Serialize, Type)]
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
}
