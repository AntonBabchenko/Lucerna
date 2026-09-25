//! The single place that turns (what is on disk, the two level.dat lists,
//! whether the game can load the pack) into a row's state and ignored
//! reason. Pure, so the whole table is unit-tested.
//!
//! Engine facts (spec §2 N.0, 26.2 `configurePackRepository`): each Enabled
//! id that is available stays selected; `Disabled` (exact `List.contains`)
//! only suppresses the auto-add of an available pack. So an id in BOTH lists
//! loads, and a Disabled entry whose case drifted from the file keeps nothing off.
//!
//! Not modelled (§0.5 A16): a pack whose `features` the world lacks is left
//! off by the game whatever these cells say; feature flags are out of scope (§0.1).

use crate::datapacks::detect::{IgnoredReason, OnDiskEntry, Presence};
use crate::datapacks::{InstalledDatapack, WorldPackState};

/// [`derive`] for a world whose two lists are known. These two functions are
/// the only code that pairs a state with an ignored reason, so
/// `state == Ignored ⟺ ignored_reason.is_some()` holds by construction (§0.2 I1).
#[must_use]
pub fn derive_listed(
    presence: Option<&Presence>,
    in_enabled: bool,
    in_disabled: bool,
    loadable: Option<bool>,
) -> (WorldPackState, Option<IgnoredReason>) {
    match presence {
        // Detection wins: the game drops the id whatever the lists say.
        Some(Presence::Unusable { reason, .. }) => (WorldPackState::Ignored, Some(*reason)),
        // §0.5 A1: the engine accepts the file and skips it.
        Some(Presence::Pack { .. }) if loadable == Some(false) => {
            (WorldPackState::Ignored, Some(IgnoredReason::NotLoadable))
        }
        Some(Presence::Pack { .. }) if in_enabled => (WorldPackState::Enabled, None),
        Some(Presence::Pack { .. }) if in_disabled => (WorldPackState::Disabled, None),
        // Present and unlisted: auto-added on the next load.
        Some(Presence::Pack { .. }) => (WorldPackState::Enabled, None),
        // Gone, but the world still asks for it (`WARN Missing data pack`).
        None if in_enabled => (WorldPackState::Orphaned, None),
        None => (WorldPackState::NotAdded, None),
    }
}

/// §0.5 A1's signature. `lists == None` = level.dat could not be read: the
/// state is unknown, except that an entry the game ignores is ignored whatever
/// level.dat says (A4).
#[must_use]
pub fn derive(
    presence: Option<&Presence>,
    lists: Option<(bool, bool)>,
    loadable: Option<bool>,
) -> (Option<WorldPackState>, Option<IgnoredReason>) {
    match (presence, lists) {
        (_, Some((in_enabled, in_disabled))) => {
            let (state, reason) = derive_listed(presence, in_enabled, in_disabled, loadable);
            (Some(state), reason)
        }
        (Some(Presence::Unusable { reason, .. }), None) => {
            (Some(WorldPackState::Ignored), Some(*reason))
        }
        (_, None) => (None, None),
    }
}

/// SEAM(G4): whether the game can load this pack's `pack.mcmeta`. §0.5 A1:
/// in batch 2 the input comes only from the registry row's recorded metadata,
/// for library rows and for world entries the vouch rule ties to the library
/// copy (`entry.vouched`). Every other entry is `None`. Until G4 lands the
/// compat verdict, nothing is known and this is always `None`. G4 replaces the
/// body (and adds the instance's format input) without changing any caller's shape.
#[must_use]
pub fn loadable_of(_row: Option<&InstalledDatapack>, _entry: Option<&OnDiskEntry>) -> Option<bool> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datapacks::detect::{IgnoredReason, Presence};
    use crate::datapacks::WorldPackState;

    const PACK: Presence = Presence::Pack { is_dir: false };
    const FOLDER: Presence = Presence::Pack { is_dir: true };
    fn listed(p: Option<&Presence>, en: bool, dis: bool) -> WorldPackState {
        derive_listed(p, en, dis, None).0
    }

    #[test]
    fn file_present_and_listed_enabled_is_enabled() {
        assert_eq!(listed(Some(&PACK), true, false), WorldPackState::Enabled);
    }
    #[test]
    fn file_present_and_unlisted_is_enabled_because_minecraft_auto_enables_it() {
        assert_eq!(listed(Some(&FOLDER), false, false), WorldPackState::Enabled);
    }
    #[test]
    fn file_present_and_listed_disabled_is_disabled() {
        assert_eq!(listed(Some(&PACK), false, true), WorldPackState::Disabled);
    }
    #[test]
    fn file_absent_and_unlisted_is_not_added() {
        assert_eq!(listed(None, false, false), WorldPackState::NotAdded);
    }
    #[test]
    fn file_absent_but_listed_enabled_is_orphaned() {
        assert_eq!(listed(None, true, false), WorldPackState::Orphaned);
    }
    #[test]
    fn file_absent_and_listed_disabled_is_not_added() {
        assert_eq!(listed(None, false, true), WorldPackState::NotAdded);
    }

    /// Inverted from `disabled_wins_when_a_name_is_in_both_lists` (spec §2
    /// N.9 #1). Engine fact (26.2 `configurePackRepository`): an Enabled id
    /// that is available stays selected; `Disabled` only suppresses the
    /// auto-add. Both lists ⇒ the pack loads.
    #[test]
    fn an_id_in_both_lists_is_enabled() {
        assert_eq!(listed(Some(&PACK), true, true), WorldPackState::Enabled);
    }

    /// Engine: the Enabled id is not available ⇒ `WARN Missing data pack`,
    /// whatever `Disabled` says.
    #[test]
    fn a_missing_id_in_both_lists_is_orphaned() {
        assert_eq!(listed(None, true, true), WorldPackState::Orphaned);
    }

    #[test]
    fn an_unusable_entry_is_ignored_whatever_the_lists_say() {
        let u = Presence::Unusable {
            is_dir: true,
            reason: IgnoredReason::FolderWithoutPackMcmeta,
        };
        for (en, dis) in [(false, false), (true, false), (false, true), (true, true)] {
            assert_eq!(
                derive_listed(Some(&u), en, dis, None),
                (
                    WorldPackState::Ignored,
                    Some(IgnoredReason::FolderWithoutPackMcmeta)
                )
            );
        }
    }

    #[test]
    fn a_pack_the_game_cannot_load_is_ignored_with_that_reason() {
        assert_eq!(
            derive(Some(&PACK), Some((true, false)), Some(false)),
            (
                Some(WorldPackState::Ignored),
                Some(IgnoredReason::NotLoadable)
            )
        );
        assert_eq!(
            derive(Some(&PACK), Some((true, false)), Some(true)),
            (Some(WorldPackState::Enabled), None)
        );
    }

    #[test]
    fn an_absent_entry_is_never_ignored_by_its_verdict() {
        assert_eq!(
            derive(None, Some((true, false)), Some(false)),
            (Some(WorldPackState::Orphaned), None)
        );
        assert_eq!(
            derive(None, Some((false, false)), Some(false)),
            (Some(WorldPackState::NotAdded), None)
        );
    }

    /// §0.5 A4: detection does not depend on level.dat.
    #[test]
    fn an_unusable_entry_is_ignored_even_when_the_lists_are_unknown() {
        let u = Presence::Unusable {
            is_dir: false,
            reason: IgnoredReason::ZipExtensionNotLowercase,
        };
        assert_eq!(
            derive(Some(&u), None, None),
            (
                Some(WorldPackState::Ignored),
                Some(IgnoredReason::ZipExtensionNotLowercase)
            )
        );
        assert_eq!(derive(Some(&PACK), None, Some(false)), (None, None));
        assert_eq!(derive(None, None, None), (None, None));
    }

    /// §0.2 I1: `state == Ignored ⟺ ignored_reason.is_some()`, over every input.
    #[test]
    fn the_reason_is_set_iff_the_state_is_ignored() {
        let reasons = [
            IgnoredReason::FolderWithoutPackMcmeta,
            IgnoredReason::FolderPackNestedInside,
            IgnoredReason::ZipExtensionNotLowercase,
            IgnoredReason::ZipWithoutPackMcmeta,
            IgnoredReason::Unreadable,
        ];
        let mut presences: Vec<Option<Presence>> = vec![None, Some(PACK), Some(FOLDER)];
        for r in reasons {
            presences.push(Some(Presence::Unusable {
                is_dir: false,
                reason: r,
            }));
            presences.push(Some(Presence::Unusable {
                is_dir: true,
                reason: r,
            }));
        }
        let lists = [
            None,
            Some((false, false)),
            Some((true, false)),
            Some((false, true)),
            Some((true, true)),
        ];
        for p in &presences {
            for l in lists {
                for loadable in [None, Some(true), Some(false)] {
                    let (state, reason) = derive(p.as_ref(), l, loadable);
                    assert_eq!(
                        state == Some(WorldPackState::Ignored),
                        reason.is_some(),
                        "{p:?} {l:?} {loadable:?}"
                    );
                }
            }
        }
    }
}
