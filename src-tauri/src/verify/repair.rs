//! Targeted repair of a verify report's problems, with a full-install
//! fallback for artefacts that can't be plain-downloaded.

use crate::error::Result;
use crate::network::download_with_sha;
use crate::verify::progress::VerifyPhase;
use crate::verify::scan::verify_instance_report;
use crate::verify::{ProblemArtifact, VerifyCategory, VerifyReport};

/// True when at least one problem cannot be fixed by a targeted download and
/// the whole install must be re-run. Asset and JRE problems are excluded —
/// they are repaired via `ensure_assets` / `ensure_jre` (driven by a full
/// install), not this gate.
pub fn needs_full_install(manifest_recoverable: bool, problems: &[ProblemArtifact]) -> bool {
    if manifest_recoverable {
        return true;
    }
    problems.iter().any(|p| {
        (matches!(
            p.category,
            VerifyCategory::Client | VerifyCategory::Libraries
        ) && p.url.is_none())
            || p.category == VerifyCategory::ProfileJson
    })
}

/// Repair the instance, then re-verify and return the post-repair report
/// together with the number of problems the PRE-repair scan found.
///
/// The caller needs that count to tell "was already healthy, nothing done"
/// (`0`) from "repaired N artefacts" — the post-repair report alone cannot
/// distinguish them, since both end healthy. Only the count crosses the
/// boundary; the IPC-visible return shape is still just the report.
pub async fn repair_instance_report(
    instance_id: &str,
    effective_id: &str,
    app: &tauri::AppHandle,
) -> Result<(VerifyReport, usize)> {
    // Always repair off a FRESH scan, never a stale client report.
    let report = verify_instance_report(instance_id, effective_id, app).await?;
    if report.healthy {
        return Ok((report, 0));
    }
    let problems_before = report.problems.len();

    emit_repairing(app, instance_id, 0, 1);

    if needs_full_install(report.manifest_recoverable, &report.problems) {
        crate::versions::install_version(effective_id, app).await?;
    } else {
        repair_targeted(&report.problems, instance_id, effective_id, app).await?;
    }

    let post = verify_instance_report(instance_id, effective_id, app).await?;
    emit_repairing(app, instance_id, 1, 1);
    Ok((post, problems_before))
}

async fn repair_targeted(
    problems: &[ProblemArtifact],
    instance_id: &str,
    effective_id: &str,
    app: &tauri::AppHandle,
) -> Result<()> {
    let downloadable: Vec<&ProblemArtifact> = problems
        .iter()
        .filter(|p| {
            matches!(
                p.category,
                VerifyCategory::Client | VerifyCategory::Libraries
            ) && p.url.is_some()
        })
        .collect();
    let total = downloadable.len() as u32;

    let mut done = 0u32;
    for prob in &downloadable {
        if let Some(url) = prob.url.as_deref() {
            let dest = absolute_dest(prob, app)?;
            download_with_sha(app, url, &dest, &prob.expected_sha, "verify").await?;
            done += 1;
            emit_repairing(app, instance_id, done, total.max(1));
        }
    }

    // Assets / JRE problems can't be plain-downloaded — drive ensure_* via a
    // full (idempotent) install.
    let needs_full = problems
        .iter()
        .any(|p| matches!(p.category, VerifyCategory::Assets | VerifyCategory::Jre));
    if needs_full {
        crate::versions::install_version(effective_id, app).await?;
    }
    Ok(())
}

fn absolute_dest(prob: &ProblemArtifact, app: &tauri::AppHandle) -> Result<std::path::PathBuf> {
    use crate::error::Error;
    let base = match prob.category {
        VerifyCategory::Libraries => {
            crate::paths::libraries_dir(app).map_err(|e| Error::io("<libraries_dir>", e))?
        }
        _ => crate::paths::versions_dir(app).map_err(|e| Error::io("<versions_dir>", e))?,
    };
    Ok(base.join(&prob.rel_path))
}

fn emit_repairing(app: &tauri::AppHandle, instance_id: &str, done: u32, total: u32) {
    use crate::verify::progress::VerifyProgress;
    use tauri_specta::Event;
    // Best-effort: a dropped event (no listener / closed window) is fine.
    let _ = VerifyProgress {
        instance_id: instance_id.to_string(),
        phase: VerifyPhase::Repairing,
        files_done: done,
        files_total: total,
        bytes_done: 0.0,
        current_category: None,
    }
    .emit(app);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verify::{ArtifactStatus, ProblemArtifact, VerifyCategory};

    fn p(cat: VerifyCategory, url: Option<&str>) -> ProblemArtifact {
        ProblemArtifact {
            category: cat,
            rel_path: "x".into(),
            expected_sha: "aa".into(),
            url: url.map(String::from),
            status: ArtifactStatus::Missing,
        }
    }

    #[test]
    fn needs_full_install_when_manifest_recoverable() {
        assert!(needs_full_install(true, &[]));
    }

    #[test]
    fn needs_full_install_when_profile_json_problem() {
        assert!(needs_full_install(
            false,
            &[p(VerifyCategory::ProfileJson, None)]
        ));
    }

    #[test]
    fn needs_full_install_when_problem_has_no_url() {
        assert!(needs_full_install(
            false,
            &[p(VerifyCategory::Libraries, None)]
        ));
    }

    #[test]
    fn targeted_when_all_problems_downloadable() {
        let probs = [
            p(VerifyCategory::Client, Some("https://c")),
            p(VerifyCategory::Libraries, Some("https://l")),
        ];
        assert!(!needs_full_install(false, &probs));
    }

    #[test]
    fn assets_and_jre_handled_separately_not_via_full_install_gate() {
        assert!(!needs_full_install(
            false,
            &[p(VerifyCategory::Assets, None)]
        ));
        assert!(!needs_full_install(false, &[p(VerifyCategory::Jre, None)]));
    }
}
