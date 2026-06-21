//! Pattern-matching crash log diagnoser. Scans a log file for a
//! known failure signature; returns plain-language explanation +
//! recommendation when a pattern hits. Strictly informational —
//! no automatic actions, no network, no telemetry.

pub mod engine;
pub mod fixmods;
pub mod patterns;
pub mod repair;
pub mod server;
pub mod server_mods;

use crate::error::Result;
use crate::logs::files::LogSource;
use serde::Serialize;
use sha1::{Digest, Sha1};
use specta::Type;
use std::path::Path;

/// Cap for the diagnoser's own file read. Crash reports are 10–50 kB;
/// `latest.log` can grow larger but the diagnostic signal is in the
/// tail. `logs::read::read_with_cap` already tails on overflow, so
/// the cap doubles as an "only the most recent MB matters" rule.
const DIAGNOSE_READ_CAP: u64 = 1 * 1024 * 1024;

/// A single diagnoser hit. Returned by `diagnose` and consumed
/// directly by the UI. Pattern_id is on the wire so per-pattern
/// presentation tweaks (icons, etc.) can be added later without
/// changing the protocol.
#[derive(Debug, Clone, Serialize, Type)]
pub struct Diagnosis {
    pub pattern_id: String,
    pub title: String,
    pub explanation: String,
    pub recommendation: String,
    pub matched_excerpt: String,
    /// Repair affordance tag. `Some` iff the matched pattern is in the
    /// actionable set — tells the UI to show a Fix button. The concrete
    /// plan is fetched lazily via `build_repair_plan`.
    pub repair: Option<repair::RepairKind>,
}

/// Surface state of the active instance's latest-log diagnosis.
#[derive(Debug, Clone, Copy, Serialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosisStatus {
    /// Latest log has no known problem (or there are no logs).
    None,
    /// A diagnosis with a constructible repair plan.
    Actionable,
    /// A diagnosis with guidance only (no fix button).
    Advisory,
    /// A fix was already applied for the current latest log.
    Handled,
}

/// What `diagnose_latest` returns. `diagnosis`/`path`/`signature` are absent
/// only when `status == None` with no diagnosable log.
#[derive(Debug, Clone, Serialize, Type)]
pub struct LatestDiagnosis {
    pub status: DiagnosisStatus,
    pub diagnosis: Option<Diagnosis>,
    pub path: Option<String>,
    pub signature: Option<String>,
}

/// SHA-1 hex digest of raw log content. Used as a stable identity for a
/// specific server log read, so the UI can remember that a diagnosis was
/// already acted on without depending on filesystem metadata.
///
/// Distinct from `crate::logs::files::log_signature`, which is based on
/// `LogFileMeta` (path + mtime + size) rather than content.
pub fn log_signature(content: &str) -> String {
    hex::encode(Sha1::digest(content.as_bytes()))
}

/// Pure status decision for a *found* diagnosis. I/O (reading logs, the
/// instance) happens in the command; this is the testable core.
pub fn classify_status(
    diag: &Diagnosis,
    current_heap_mb: u32,
    total_ram_mb: Option<u64>,
    signature: &str,
    handled_log_sig: Option<&str>,
) -> DiagnosisStatus {
    // Remembered: a fix was applied against this exact log.
    if handled_log_sig == Some(signature) {
        return DiagnosisStatus::Handled;
    }
    // OOM self-heals: if the heap is already at the recommended max there is
    // nothing left to raise, so treat it as handled rather than re-offer.
    if diag.pattern_id == "out-of-memory"
        && current_heap_mb >= crate::instances::memory::recommended_max_mb(total_ram_mb)
    {
        return DiagnosisStatus::Handled;
    }
    if diag.repair.is_some() {
        DiagnosisStatus::Actionable
    } else {
        DiagnosisStatus::Advisory
    }
}

/// Read the file at `path` (capped at 1 MB tail) and run the
/// pattern engine over it. Returns `Ok(None)` when no pattern
/// matches or the file is too short. File-read errors propagate
/// as `Err` — the caller (UI) treats those as best-effort failures
/// and just hides the Diagnosis section.
pub async fn diagnose(path: &Path) -> Result<Option<Diagnosis>> {
    let content = crate::logs::read::read_with_cap(path, DIAGNOSE_READ_CAP)?;
    let source_kind = engine::infer_source_from_path(path);
    Ok(engine::match_log(&content, source_kind))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logs::diagnose::repair::RepairKind;

    #[test]
    fn log_signature_stable_and_distinguishing() {
        let a = "some log content\n";
        let b = "different log content\n";
        assert_eq!(log_signature(a), log_signature(a), "same input → same sig");
        assert_ne!(
            log_signature(a),
            log_signature(b),
            "different input → different sig"
        );
    }

    fn diag(pattern: &str, repair: Option<RepairKind>) -> Diagnosis {
        Diagnosis {
            pattern_id: pattern.into(),
            title: "t".into(),
            explanation: "e".into(),
            recommendation: "r".into(),
            matched_excerpt: "x".into(),
            repair,
        }
    }

    #[test]
    fn actionable_when_repair_present_and_not_handled() {
        let d = diag("corrupt-mod-jar", Some(RepairKind::RedownloadMod));
        let s = classify_status(&d, 2048, Some(16384), "sigA", None);
        assert_eq!(s, DiagnosisStatus::Actionable);
    }

    #[test]
    fn advisory_when_no_repair() {
        let d = diag("disk-full", None);
        let s = classify_status(&d, 2048, Some(16384), "sigA", None);
        assert_eq!(s, DiagnosisStatus::Advisory);
    }

    #[test]
    fn oom_handled_when_heap_at_recommended() {
        let d = diag("out-of-memory", Some(RepairKind::RaiseHeap));
        assert_eq!(
            classify_status(&d, 12288, Some(16384), "sigA", None),
            DiagnosisStatus::Handled
        );
        assert_eq!(
            classify_status(&d, 2048, Some(16384), "sigA", None),
            DiagnosisStatus::Actionable
        );
    }

    #[test]
    fn handled_when_signature_matches_stored() {
        let d = diag("corrupt-mod-jar", Some(RepairKind::RedownloadMod));
        assert_eq!(
            classify_status(&d, 2048, Some(16384), "sigA", Some("sigA")),
            DiagnosisStatus::Handled
        );
        assert_eq!(
            classify_status(&d, 2048, Some(16384), "sigA", Some("sigOLD")),
            DiagnosisStatus::Actionable
        );
    }
}
