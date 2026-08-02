//! What a run reports: the shapes that cross IPC, plus the phase vocabulary
//! [`PrefillProgress`] is allowed to carry.

use serde::Serialize;
use specta::Type;

use crate::l10n::prefill::role::UiRole;
use crate::l10n::prefill::verify::RejectReason;

/// Why one machine-produced string was refused. The reason is the typed
/// [`RejectReason`], never a `Debug` string, so the UI can localise it — the
/// same shape `Error::L10nTranslationInvalid` already uses for `FormatError`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Rejected {
    pub id: String,
    pub reason: RejectReason,
}

/// Progress tick. `phase` is a stable lowercase token the UI maps to copy —
/// `scanning`, `free`, `name`, `prose`, `other`, `applying`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PrefillProgress {
    pub done: u32,
    pub total: u32,
    pub phase: String,
}

pub(super) const PHASE_SCANNING: &str = "scanning";
pub(super) const PHASE_FREE: &str = "free";
pub(super) const PHASE_APPLYING: &str = "applying";

pub(super) fn phase_of(role: UiRole) -> &'static str {
    match role {
        UiRole::Name => "name",
        UiRole::Prose => "prose",
        UiRole::Other => "other",
    }
}

/// What a finished run did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    /// KEYS written — the number the coverage report will move by.
    pub written: u32,
    /// Distinct units answered from a previous run's cache. Counted in units
    /// rather than keys, matching `PrefillEstimate::from_cache`, so the
    /// estimate and the summary are comparable.
    pub from_cache: u32,
    /// Distinct units vanilla Minecraft already translated verbatim.
    pub from_glossary: u32,
    /// Distinct units the verifier refused twice. Their keys were not written
    /// and the mod's own English still shows.
    pub rejected: u32,
    pub cancelled: bool,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    /// False when any completion reported no usage — a local model typically
    /// reports none. The UI must then show nothing rather than a fake zero: a
    /// run that cost nothing and a run whose cost is unknown are different
    /// claims.
    pub usage_known: bool,
    /// The pack rebuild is best-effort. False means the rebuild itself failed
    /// (`pack_rebuild_error` says how); the strings are still on disk and the
    /// editor's Apply button can ship them.
    pub pack_rebuilt: bool,
    pub pack_rebuild_error: Option<String>,
    /// Set when the run stopped early — a provider failure, or a namespace
    /// store that could not be written. The run is still REPORTED rather than
    /// thrown away: everything before the failure was verified, paid for and
    /// flushed to disk, and the pack is rebuilt around it. A user who rotates
    /// their key at batch 900 of 1000 keeps 900 batches. `None` means the run
    /// finished, which includes a cancelled one.
    pub failed: Option<String>,
}

impl RunSummary {
    pub(super) fn new() -> Self {
        Self {
            written: 0,
            from_cache: 0,
            from_glossary: 0,
            rejected: 0,
            cancelled: false,
            // Starts true and only ever falls: a run that made no model call
            // at all genuinely cost zero tokens, and that is known, not
            // unknown.
            usage_known: true,
            prompt_tokens: 0,
            completion_tokens: 0,
            pack_rebuilt: false,
            pack_rebuild_error: None,
            failed: None,
        }
    }
}

/// Counts cross IPC as `u32`. Clamping keeps a degenerate instance honest
/// instead of wrapping to a small number that would read as good news.
pub(super) fn clamp_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------
    // IPC shape
    // -----------------------------------------------------------------

    #[test]
    fn the_run_summary_crosses_ipc_in_camel_case() {
        // `PrefillEstimate` is already camelCase; a summary in snake_case
        // would hand the same dialog two naming conventions.
        let json = serde_json::to_string(&RunSummary::new()).expect("plain data");
        for field in [
            "\"fromCache\"",
            "\"fromGlossary\"",
            "\"usageKnown\"",
            "\"promptTokens\"",
            "\"packRebuilt\"",
            "\"packRebuildError\"",
        ] {
            assert!(json.contains(field), "missing {field} in {json}");
        }
        assert!(!json.contains('_'), "snake_case leaked into IPC: {json}");
    }
}
