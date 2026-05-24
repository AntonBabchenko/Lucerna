//! Pattern-matching crash log diagnoser. Scans a log file for a
//! known failure signature; returns plain-language explanation +
//! recommendation when a pattern hits. Strictly informational —
//! no automatic actions, no network, no telemetry.

pub mod engine;
pub mod patterns;

use crate::error::Result;
use crate::logs::files::LogSource;
use serde::Serialize;
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
