//! Cross-instance apply targets: which instances could usefully receive the
//! current translations for a language, and what state their pack is in.
//!
//! The state enum is deliberately NOT `options_txt::PackState`. That type
//! already crosses IPC on `InstanceCoverage::pack_state` with a different
//! variant set, and a second same-named type would collide in the generated
//! TypeScript bindings. `PresentNotEnabled` maps onto `NotApplied` here,
//! because re-running Apply is what resolves it — which is the only thing this
//! enum's consumer can act on.
//!
//! Everything here is a pure function over values the caller has already
//! gathered. That is the point: whether the game is running and whether an AI
//! pre-fill is writing both come from process-global registries with no test
//! seam, so keeping the DECISION separate from the SOURCING is what makes the
//! rules testable at all.

use serde::Serialize;

use crate::l10n::pack_format::ApplyGate;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ApplyTargetState {
    Current,
    Outdated,
    NotApplied,
    NotApplicable,
}

/// One instance's row in the offer dialog, and the source of the Overview
/// badge for the active instance.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ApplyTarget {
    pub instance_id: String,
    pub name: String,
    /// This instance has at least one mod whose namespace we hold overrides
    /// for. Without it, applying would install a pack this instance's mods
    /// never read.
    pub covered: bool,
    pub state: ApplyTargetState,
    /// A DIFFERENT language's Lucerna pack is applied here. Exactly one
    /// Lucerna pack exists per instance, so applying would REPLACE it — the
    /// dialog has to say so, or a bulk action silently overwrites a
    /// deliberate per-instance language choice.
    pub applied_other_lang: Option<String>,
    pub is_running: bool,
    pub prefill_active: bool,
    pub candidate: bool,
    pub actionable: bool,
}

/// Classify one instance.
///
/// `disk` is the installed pack's bytes if a pack file exists; `rebuild` is
/// what a fresh build produces now (`None` = nothing left to ship for this
/// language). `enabled_in_options` is whether `options.txt` currently lists the
/// pack. The gate is checked first and short-circuits: an unknown format cannot
/// produce a rebuild at all, so hashing against `None` would misreport it as
/// outdated.
pub fn classify(
    disk: Option<&[u8]>,
    rebuild: Option<&[u8]>,
    enabled_in_options: bool,
    gate: ApplyGate,
) -> ApplyTargetState {
    match gate {
        ApplyGate::UnknownFormat | ApplyGate::TooOld => return ApplyTargetState::NotApplicable,
        ApplyGate::Ready => {}
    }
    match (disk, rebuild) {
        (None, _) => ApplyTargetState::NotApplied,
        // A pack is installed but there is nothing to rebuild: every override
        // for this language was reverted. Applying sweeps the stale pack.
        (Some(_), None) => ApplyTargetState::Outdated,
        // Byte-identical but not listed in options.txt — the modpack-update
        // wipe. Inactive in game, and re-applying re-enables it.
        (Some(_), Some(_)) if !enabled_in_options => ApplyTargetState::NotApplied,
        (Some(d), Some(fresh)) if d == fresh => ApplyTargetState::Current,
        (Some(_), Some(_)) => ApplyTargetState::Outdated,
    }
}

/// Whether the offer dialog should list this instance at all.
///
/// `empty_rebuild_with_pack` carries the one case that is offered despite
/// `covered` being false: the store holds nothing for this language any more,
/// yet a generated pack is still installed and still shipping the deleted
/// translations into the game.
pub fn candidacy(covered: bool, state: ApplyTargetState, empty_rebuild_with_pack: bool) -> bool {
    matches!(
        state,
        ApplyTargetState::Outdated | ApplyTargetState::NotApplied
    ) && (covered || empty_rebuild_with_pack)
}

/// Whether a listed row can be ticked right now. A candidate whose game is
/// running, or whose store files an AI pre-fill run is currently writing, is
/// shown disabled with a reason rather than hidden — an instance the user
/// expected to see must not silently vanish from the list.
pub fn actionable(candidate: bool, is_running: bool, prefill_active: bool) -> bool {
    candidate && !is_running && !prefill_active
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::l10n::pack_format::ApplyGate;

    #[test]
    fn classify_covers_every_state() {
        let disk = b"pack".as_slice();
        let fresh = b"pack".as_slice();
        let other = b"different".as_slice();

        // The gate is decided before anything else — including before any
        // attempt to hash, since an unknown format cannot produce a rebuild.
        assert_eq!(
            classify(None, None, false, ApplyGate::UnknownFormat),
            ApplyTargetState::NotApplicable
        );
        assert_eq!(
            classify(Some(disk), Some(fresh), true, ApplyGate::TooOld),
            ApplyTargetState::NotApplicable
        );

        // No pack file at all.
        assert_eq!(
            classify(None, Some(fresh), false, ApplyGate::Ready),
            ApplyTargetState::NotApplied
        );

        // Byte-identical but NOT listed in options.txt — the modpack-update
        // wipe. Reporting `Current` here would tell the user their
        // translations are live when the game never loads them.
        assert_eq!(
            classify(Some(disk), Some(fresh), false, ApplyGate::Ready),
            ApplyTargetState::NotApplied
        );

        assert_eq!(
            classify(Some(disk), Some(fresh), true, ApplyGate::Ready),
            ApplyTargetState::Current
        );
        assert_eq!(
            classify(Some(disk), Some(other), true, ApplyGate::Ready),
            ApplyTargetState::Outdated
        );

        // Store emptied for this language while a pack is still installed:
        // there is nothing to rebuild, so applying sweeps the stale pack.
        assert_eq!(
            classify(Some(disk), None, true, ApplyGate::Ready),
            ApplyTargetState::Outdated
        );
    }

    #[test]
    fn candidacy_offers_work_and_the_empty_rebuild_sweep() {
        assert!(candidacy(true, ApplyTargetState::Outdated, false));
        assert!(candidacy(true, ApplyTargetState::NotApplied, false));
        assert!(!candidacy(true, ApplyTargetState::Current, false));
        assert!(!candidacy(true, ApplyTargetState::NotApplicable, false));

        // Nothing is covered any more — which is exactly why the leftover pack
        // needs sweeping, so this one IS offered.
        assert!(candidacy(false, ApplyTargetState::Outdated, true));
        assert!(!candidacy(false, ApplyTargetState::Outdated, false));
    }

    #[test]
    fn actionable_excludes_busy_instances() {
        assert!(actionable(true, false, false));
        assert!(!actionable(true, true, false), "the game is running");
        assert!(!actionable(true, false, true), "a pre-fill run is writing");
        assert!(!actionable(false, false, false), "not a candidate at all");
    }
}
