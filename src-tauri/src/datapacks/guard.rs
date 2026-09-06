//! The running-instance gate for datapack — and world — writes.
//!
//! Why a hard guard here when the mods commands have none: a `mods/` jar is a
//! file only Lucerna touches, but `level.dat` is a file the game owns and
//! rewrites on world save and on exit — a write while it runs is clobbered at
//! best and a corrupt save at worst. Same reasoning as
//! `servers::add_saved_server`.
//!
//! Where the gate is CALLED: no command calls [`datapack_write_allowed`]
//! directly. `commands::worlds`, `commands::datapacks` (through its file-local
//! `guard`) and the instance writers all open with
//! `crate::instances::maintenance::write_allowed(id)`, which answers
//! `is_running || is_starting || maintenance_is_active` with the same
//! `InstanceBusy` — one definition, so a world migration's maintenance claim
//! holds every writer off, and `tests/structural_maintenance_gate.rs` fails
//! the build for a writer that forgets it. The predicate below stays as the
//! named, tested statement of the running-state half of that rule, which
//! `l10n::options_txt` mirrors for `options.txt`. `delete_backup` is the one
//! world command outside `write_allowed` — it only touches
//! `<instance>/backups/`, never the world tree the JVM holds, so it refuses
//! for the maintenance claim alone (a Move migration relocates that set).
//!
//! This gate does not replace `worlds::delete_world`'s and `restore.rs`'s own
//! OS-lock handling, which maps errno 5/32/33 to `WorldInUse`: `is_running` is
//! a snapshot, and the window between this check and the actual write is
//! real — the same window every other instance command already accepts. This
//! gate rejects the common case up front; the OS-lock mapping still covers
//! the race.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::{Error, Result};

pub fn datapack_write_allowed(is_running: bool) -> Result<()> {
    if is_running {
        return Err(Error::InstanceBusy);
    }
    Ok(())
}

/// Set while `datapacks_update_one` is rewriting a pack across its worlds.
///
/// [`datapack_write_allowed`] above is the FORWARD gate — no datapack write
/// while the game runs — built on a one-shot `is_running` snapshot that
/// `guard`'s module doc calibrates to "the sub-second window every other
/// instance command already accepts". An update does not fit that
/// calibration: it inserts a network download plus one level.dat
/// read-modify-write per world into the window, so the user can start the
/// game halfway through. This is the REVERSE gate: `launch_instance` refuses
/// to start while an update is mid-flight — the exact shape of
/// `verify::RepairGuard`, which launch already consults for the same reason.
static UPDATE_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

/// True while a datapack update is rewriting worlds. Checked by the launch
/// path.
pub fn update_in_progress() -> bool {
    UPDATE_IN_PROGRESS.load(Ordering::SeqCst)
}

/// RAII guard for the update-in-progress flag. `acquire()` returns `None`
/// when an update is already running (rejects a concurrent second update even
/// if the frontend queue is bypassed); the flag clears on drop — panic-safe.
pub struct DatapackUpdateGuard {
    _private: (),
}

impl DatapackUpdateGuard {
    pub fn acquire() -> Option<Self> {
        UPDATE_IN_PROGRESS
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .ok()
            .map(|_| DatapackUpdateGuard { _private: () })
    }
}

impl Drop for DatapackUpdateGuard {
    fn drop(&mut self) {
        UPDATE_IN_PROGRESS.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stopped_instance_is_allowed() {
        assert!(datapack_write_allowed(false).is_ok());
    }

    #[test]
    fn a_running_instance_is_rejected() {
        assert!(matches!(
            datapack_write_allowed(true).unwrap_err(),
            Error::InstanceBusy
        ));
    }

    #[test]
    fn update_guard_is_exclusive_and_clears_on_drop() {
        // Process-global state: this is the only test that touches it, same
        // arrangement as `verify`'s RepairGuard test.
        assert!(!update_in_progress());
        let g1 = DatapackUpdateGuard::acquire().expect("first acquire succeeds");
        assert!(update_in_progress());
        assert!(
            DatapackUpdateGuard::acquire().is_none(),
            "a second concurrent update must be rejected"
        );
        drop(g1);
        assert!(!update_in_progress());
        let g2 = DatapackUpdateGuard::acquire().expect("re-acquire after drop");
        assert!(update_in_progress());
        drop(g2);
        assert!(!update_in_progress());
    }
}
