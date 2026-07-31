//! The running-instance gate for datapack writes.
//!
//! Why a hard guard here when the mods commands have none: a `mods/` jar is a
//! file only Lucerna touches, but `level.dat` is a file the game owns and
//! rewrites on world save and on exit — a write while it runs is clobbered at
//! best and a corrupt save at worst. Same reasoning as
//! `servers::add_saved_server`.
//!
//! This deliberately differs from `worlds::delete_world`, which lets the OS
//! lock fail and maps errno 5/32/33 to `WorldInUse`. Both surface in this
//! feature: this gate rejects up front, and `level_dat::write_at` still maps a
//! lock failure, because `is_running` is a snapshot and the window between
//! check and write is real. It is the same window every other instance command
//! accepts; this feature does not claim to close it.

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
