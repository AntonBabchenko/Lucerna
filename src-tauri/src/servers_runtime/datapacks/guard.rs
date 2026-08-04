//! The two-way launch guard for server datapack mutations.

use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};

use crate::error::{Error, Result};

/// The forward gate's decision, as a pure function of the two flags so the
/// whole table is unit-testable without a live registry.
///
/// `is_running` alone is NOT enough: it reads only the live-process map,
/// which `start()` populates AFTER resolving Java and possibly downloading a
/// JRE. During that window the server is unambiguously being started and a
/// datapack write would race the JVM's first world load.
///
/// Returns a plain `bool`, not a `Result` — an error is the caller's to build,
/// because only the caller knows the server id that belongs in it.
#[must_use]
pub fn write_allowed(is_running: bool, is_starting: bool) -> bool {
    !is_running && !is_starting
}

/// The gate every mutating datapack command opens with.
pub fn gate(server_id: &str) -> Result<()> {
    let rt = crate::servers_runtime::runtime::is_running(server_id);
    let starting = crate::servers_runtime::runtime::is_starting(server_id);
    if !write_allowed(rt, starting) {
        return Err(Error::ServerAlreadyRunning {
            id: server_id.to_string(),
        });
    }
    Ok(())
}

/// Servers whose datapacks are being updated right now.
///
/// This is the REVERSE half of the pair: `gate` above is a one-shot snapshot
/// calibrated for sub-second commands, and an update inserts a network
/// download plus a `level.dat` read-modify-write into its window, so the user
/// can hit Start halfway through. Per-server rather than global — updating
/// server A must not block starting server B.
static UPDATING: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// True while a datapack update is rewriting this server's world. Checked by
/// `runtime::start` immediately after it claims the start slot.
pub fn update_in_progress(server_id: &str) -> bool {
    UPDATING
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .contains(server_id)
}

/// RAII claim on the update-in-progress set. `acquire` returns `None` when an
/// update of the same server is already running (so a bypassed frontend queue
/// cannot start a second one); the claim clears on drop, panic included.
pub struct UpdateGuard {
    id: String,
}

impl UpdateGuard {
    pub fn acquire(server_id: &str) -> Option<Self> {
        let mut set = UPDATING.lock().unwrap_or_else(|p| p.into_inner());
        if !set.insert(server_id.to_string()) {
            return None;
        }
        Some(UpdateGuard {
            id: server_id.to_string(),
        })
    }
}

impl Drop for UpdateGuard {
    fn drop(&mut self) {
        UPDATING
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&self.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gate_refuses_a_running_or_starting_server_and_admits_a_stopped_one() {
        assert!(write_allowed(false, false));
        assert!(!write_allowed(true, false));
        // The hole the audit found: `is_running` is still false during the
        // whole of `start()`'s Java resolution and JRE download.
        assert!(!write_allowed(false, true));
    }

    #[test]
    fn the_update_set_is_per_server_and_releases_on_drop() {
        let a = UpdateGuard::acquire("srv-a").expect("first claim on A succeeds");
        assert!(update_in_progress("srv-a"));
        // Per-server, not global: updating A must not block starting B.
        assert!(!update_in_progress("srv-b"));
        assert!(
            UpdateGuard::acquire("srv-a").is_none(),
            "a second concurrent update of the same server must be refused"
        );
        assert!(
            UpdateGuard::acquire("srv-b").is_some(),
            "a different server must still be claimable"
        );
        drop(a);
        assert!(!update_in_progress("srv-a"));
    }
}
