//! Per-instance maintenance gate: a registry of client instances whose
//! content — `saves/`, `backups/`, the datapack library, `instance.json` — is
//! being rewritten by one long operation. Today that operation is a world
//! migration.
//!
//! Why a client-instance gate exists at all (spec §4.0, amendment A5): a
//! world migration holds TWO instances for the whole operation, and on its
//! copy path that is minutes, not milliseconds. Nothing in the backend
//! otherwise refuses Play, Back up, Restore, Delete, Import, instance
//! deletion, cloning or a Minecraft-version change on either instance in
//! that window — every one of those would write into, or launch a JVM over,
//! a tree that is half-moved. The frontend operation queue cannot help: its
//! serial lane serialises only registry-run tasks, the world and instance
//! commands are direct IPC calls that know nothing of it, and the backend has
//! no way to consult it. So the claim lives here, in the backend, where every
//! refusal site can see it.
//!
//! Shape: the exact shape of `servers_runtime::maintenance` — a process-global
//! `HashSet<String>` behind a `Mutex`, an RAII [`MaintenanceGuard`] whose
//! `Drop` releases the id on every exit path (success, `?`, or a panic
//! unwinding out of blocking work), and [`maintenance_begin`] refusing a
//! double claim atomically. The two registries stay separate: an instance
//! directory name and a server id are unrelated keys, and the server gate
//! feeds Start/Restart of a server JVM while this one feeds the client launch
//! and every writer of instance content.
//!
//! Pairing with launch: `launch::spawn::start` reserves its id with
//! `claim_start` and only then re-checks the cross-cutting flags
//! (`datapacks::guard::update_in_progress`, and [`maintenance_is_active`]);
//! a migration claims both of its slots here FIRST and only then checks
//! `is_running || is_starting`. Each side sets its own flag before reading
//! the other's — Dekker-style, the same arrangement `DatapackUpdateGuard`
//! has with `claim_start` — so whichever interleaving occurs, at least one
//! side sees the other and refuses. Every other writer opens with
//! [`write_allowed`], which folds the three signals into one check so the
//! gate has exactly one definition.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use crate::error::{Error, Result};

fn registry() -> &'static Mutex<HashSet<String>> {
    static R: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashSet::new()))
}

/// RAII claim on an instance's maintenance slot. Held for the full duration
/// of the operation; `Drop` releases the id so an early error (or a panic)
/// can never leave the instance permanently unlaunchable and unwritable.
#[must_use = "dropping the guard immediately ends maintenance before the work runs"]
pub struct MaintenanceGuard {
    id: String,
}

impl Drop for MaintenanceGuard {
    fn drop(&mut self) {
        // Poison-tolerant on purpose. The lock is only ever held for a single
        // `HashSet` operation, never across caller code, so a poisoned mutex
        // can only mean a panic inside the set itself, and the set is still
        // consistent. Propagating the poison from `Drop` would panic during
        // an unwind — a double panic aborts the process and the slot would
        // never be released. Recovering the inner set is the safe direction:
        // the id is removed either way.
        registry()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.id);
    }
}

/// Atomically claim the maintenance slot for `id`. Returns `None` if another
/// operation already holds it, so the caller maps that to `InstanceBusy`
/// (same contract as `launch::spawn::start`'s `claim_start`).
pub fn maintenance_begin(id: &str) -> Option<MaintenanceGuard> {
    let inserted = registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(id.to_string());
    inserted.then(|| MaintenanceGuard { id: id.to_string() })
}

/// True iff `id`'s content is currently being rewritten under a
/// [`MaintenanceGuard`]. The launch side re-checks this after `claim_start`.
pub fn maintenance_is_active(id: &str) -> bool {
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains(id)
}

/// The single write gate for anything that touches an instance's content:
/// refuses with `InstanceBusy` when the instance is running, mid-launch
/// (`is_starting` — `is_running` stays false for the whole spawn pipeline),
/// or claimed here.
///
/// Replaces the `datapack_write_allowed(is_running(id))` shape so the gate
/// has one definition (spec §4.0). Three in-memory booleans and no I/O: there
/// is no "could not tell" state to resolve, so the fallback-discipline
/// question of direction does not arise — any signal that is true refuses.
/// It is a snapshot, exactly as `datapacks::guard` documents for its own
/// check: the window between this call and the write is real, and the
/// OS-lock mapping (errno 5/32/33 → `WorldInUse`) in the world writers still
/// covers it. A migration must NOT call this on the two ids it has just
/// claimed — it would refuse itself; it checks `is_running || is_starting`
/// directly and maps a hit to `WorldMigrateInstanceRunning`.
pub fn write_allowed(id: &str) -> Result<()> {
    if crate::launch::spawn::is_running(id)
        || crate::launch::spawn::is_starting(id)
        || maintenance_is_active(id)
    {
        return Err(Error::InstanceBusy);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn begin_marks_active_then_drop_clears() {
        let id = "inst-maint-1";
        assert!(!maintenance_is_active(id));
        let guard = maintenance_begin(id).expect("first claim succeeds");
        assert!(maintenance_is_active(id));
        drop(guard);
        assert!(!maintenance_is_active(id));
    }

    #[test]
    fn second_begin_while_active_is_refused() {
        let id = "inst-maint-2";
        let guard = maintenance_begin(id).expect("first claim succeeds");
        assert!(
            maintenance_begin(id).is_none(),
            "a concurrent second claim must be refused"
        );
        drop(guard);
        assert!(
            maintenance_begin(id).is_some(),
            "slot is reusable after release"
        );
    }

    #[test]
    fn early_error_return_clears_via_drop() {
        let id = "inst-maint-3";
        fn failing_migration(id: &str) -> std::result::Result<(), ()> {
            let _guard = maintenance_begin(id).ok_or(())?;
            Err(()) // the `?`-shaped early exit a real migration hits on I/O error
        }
        assert!(failing_migration(id).is_err());
        assert!(
            !maintenance_is_active(id),
            "an early error must not leave the instance locked"
        );
    }

    #[test]
    fn panic_unwind_clears_via_drop() {
        let id = "inst-maint-4";
        let result = std::panic::catch_unwind(|| {
            let _guard = maintenance_begin(id).expect("claim succeeds");
            panic!("blocking work panicked");
        });
        assert!(result.is_err());
        assert!(
            !maintenance_is_active(id),
            "a panic unwinding out of the work must release the slot"
        );
    }

    #[test]
    fn ids_are_independent() {
        let a = maintenance_begin("inst-maint-5a").expect("claim a");
        assert!(maintenance_is_active("inst-maint-5a"));
        assert!(!maintenance_is_active("inst-maint-5b"));
        drop(a);
    }

    #[test]
    fn write_allowed_when_unclaimed_and_not_running() {
        // Nothing has launched this id (the launch registry is empty in the
        // unit-test binary) and nothing has claimed it: all three signals
        // are false, so the gate is open.
        let id = "inst-maint-6";
        assert!(!maintenance_is_active(id));
        assert!(write_allowed(id).is_ok());
    }

    #[test]
    fn write_allowed_refuses_while_claimed_then_allows_after_release() {
        let id = "inst-maint-7";
        let guard = maintenance_begin(id).expect("first claim succeeds");
        assert!(
            matches!(write_allowed(id), Err(Error::InstanceBusy)),
            "a held claim must refuse every writer with InstanceBusy"
        );
        drop(guard);
        assert!(
            write_allowed(id).is_ok(),
            "releasing the claim must reopen the gate"
        );
    }

    #[test]
    fn both_ids_of_one_operation_are_refused_independently() {
        // A migration claims a source AND a target (spec §4.0). Each id is
        // gated on its own and released on its own — releasing one must not
        // reopen the other, and a second operation on EITHER id is refused
        // by the claim itself, not by a later check.
        let src = "inst-maint-8-src";
        let dst = "inst-maint-8-dst";
        let g_src = maintenance_begin(src).expect("claim source");
        let g_dst = maintenance_begin(dst).expect("claim target");
        assert!(matches!(write_allowed(src), Err(Error::InstanceBusy)));
        assert!(matches!(write_allowed(dst), Err(Error::InstanceBusy)));
        assert!(
            maintenance_begin(src).is_none() && maintenance_begin(dst).is_none(),
            "a second operation on either id must be refused by the claim"
        );
        drop(g_src);
        assert!(write_allowed(src).is_ok());
        assert!(
            matches!(write_allowed(dst), Err(Error::InstanceBusy)),
            "releasing the source must not release the target"
        );
        drop(g_dst);
        assert!(write_allowed(dst).is_ok());
    }
}
