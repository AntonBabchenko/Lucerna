//! Per-server maintenance gate: a registry of servers whose `runtime/` tree is
//! being rewritten wholesale (backup restore, import commit).
//! `maintenance_is_active` powers the Start/Restart guard the same way
//! `upload_control::upload_is_active` powers the upload one: a Start click
//! while a restore is mid-flight would launch a JVM over a half-written tree
//! and tear the world.
//!
//! Unlike `upload_control` there is no cancel flag to hand out — the only
//! state is membership — so the begin/end pair is folded into an RAII guard:
//! [`maintenance_begin`] atomically claims the slot (refusing a double claim,
//! mirroring `runtime::claim_start`), and dropping the returned
//! [`MaintenanceGuard`] releases it on every exit path — success, `?`, or a
//! panic unwinding out of the blocking work.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

fn registry() -> &'static Mutex<HashSet<String>> {
    static R: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashSet::new()))
}

/// RAII claim on a server's maintenance slot. Held for the full duration of
/// the `runtime/` rewrite; `Drop` releases the id so an early error (or a
/// panic) can never leave the server permanently un-startable.
#[must_use = "dropping the guard immediately ends maintenance before the work runs"]
pub struct MaintenanceGuard {
    id: String,
}

impl Drop for MaintenanceGuard {
    fn drop(&mut self) {
        registry()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.id);
    }
}

/// Atomically claim the maintenance slot for `id`. Returns `None` if a
/// restore/import is already rewriting this server, so the caller maps that
/// to `ServerMaintenanceInProgress` (same contract as `runtime::claim_start`).
pub fn maintenance_begin(id: &str) -> Option<MaintenanceGuard> {
    let inserted = registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(id.to_string());
    inserted.then(|| MaintenanceGuard { id: id.to_string() })
}

/// True iff `id`'s `runtime/` is currently being rewritten by a
/// restore/import.
pub fn maintenance_is_active(id: &str) -> bool {
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn begin_marks_active_then_drop_clears() {
        let id = "maint-1";
        assert!(!maintenance_is_active(id));
        let guard = maintenance_begin(id).expect("first claim succeeds");
        assert!(maintenance_is_active(id));
        drop(guard);
        assert!(!maintenance_is_active(id));
    }

    #[test]
    fn second_begin_while_active_is_refused() {
        let id = "maint-2";
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
        let id = "maint-3";
        fn failing_restore(id: &str) -> Result<(), ()> {
            let _guard = maintenance_begin(id).ok_or(())?;
            Err(()) // the `?`-shaped early exit a real restore hits on I/O error
        }
        assert!(failing_restore(id).is_err());
        assert!(
            !maintenance_is_active(id),
            "an early error must not leave the server locked"
        );
    }

    #[test]
    fn panic_unwind_clears_via_drop() {
        let id = "maint-4";
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
        let a = maintenance_begin("maint-5a").expect("claim a");
        assert!(maintenance_is_active("maint-5a"));
        assert!(!maintenance_is_active("maint-5b"));
        drop(a);
    }
}
