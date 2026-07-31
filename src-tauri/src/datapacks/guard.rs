//! The running-instance gate for datapack — and, now, world — writes.
//!
//! Why a hard guard here when the mods commands have none: a `mods/` jar is a
//! file only Lucerna touches, but `level.dat` is a file the game owns and
//! rewrites on world save and on exit — a write while it runs is clobbered at
//! best and a corrupt save at worst. Same reasoning as
//! `servers::add_saved_server`.
//!
//! `commands::worlds`'s `delete_world`, `restore_backup`, `backup_world` and
//! `world_import` open with this exact same gate: refusing a datapack toggle
//! on a live world while cheerfully deleting or overwriting that same world
//! would be indefensible. `delete_backup` is the one exception — it only
//! touches `<instance>/backups/`, never the world tree the JVM holds, so it
//! stays unguarded.
//!
//! This gate does not replace `worlds::delete_world`'s and `restore.rs`'s own
//! OS-lock handling, which maps errno 5/32/33 to `WorldInUse`: `is_running` is
//! a snapshot, and the window between this check and the actual write is
//! real — the same window every other instance command already accepts. This
//! gate rejects the common case up front; the OS-lock mapping still covers
//! the race.

use crate::error::{Error, Result};

pub fn datapack_write_allowed(is_running: bool) -> Result<()> {
    if is_running {
        return Err(Error::InstanceBusy);
    }
    Ok(())
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
}
