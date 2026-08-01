//! The single place that turns (file present?, listed enabled?, listed
//! disabled?) into one state. Pure, so the whole table is unit-tested.

use crate::datapacks::WorldPackState;

#[must_use]
pub fn derive(file_present: bool, in_enabled: bool, in_disabled: bool) -> WorldPackState {
    match (file_present, in_enabled, in_disabled) {
        // Both lists name it: report the pessimistic reading.
        (true, _, true) => WorldPackState::Disabled,
        // A file Minecraft has never seen is not in either list, and Minecraft
        // enables it on the next load — so "present and unlisted" is Enabled.
        (true, _, false) => WorldPackState::Enabled,
        // The file is gone but the world still expects it: this is the state
        // that produces Minecraft's "no longer present" screen.
        (false, true, false) => WorldPackState::Orphaned,
        (false, _, _) => WorldPackState::NotAdded,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datapacks::WorldPackState;

    #[test]
    fn file_present_and_listed_enabled_is_enabled() {
        assert_eq!(derive(true, true, false), WorldPackState::Enabled);
    }

    #[test]
    fn file_present_and_unlisted_is_enabled_because_minecraft_auto_enables_it() {
        assert_eq!(derive(true, false, false), WorldPackState::Enabled);
    }

    #[test]
    fn file_present_and_listed_disabled_is_disabled() {
        assert_eq!(derive(true, false, true), WorldPackState::Disabled);
    }

    #[test]
    fn file_absent_and_unlisted_is_not_added() {
        assert_eq!(derive(false, false, false), WorldPackState::NotAdded);
    }

    #[test]
    fn file_absent_but_listed_enabled_is_orphaned() {
        assert_eq!(derive(false, true, false), WorldPackState::Orphaned);
    }

    #[test]
    fn file_absent_and_listed_disabled_is_not_added() {
        // A disabled name with no file produces no Minecraft prompt, so there is
        // nothing to repair — treat it as simply not in the world.
        assert_eq!(derive(false, false, true), WorldPackState::NotAdded);
    }

    #[test]
    fn disabled_wins_when_a_name_is_in_both_lists() {
        // Minecraft should never write this, but a hand-edited level.dat can.
        // Choosing Disabled is the conservative reading: we never report a pack
        // as active when the world might not load it.
        assert_eq!(derive(true, true, true), WorldPackState::Disabled);
    }
}
