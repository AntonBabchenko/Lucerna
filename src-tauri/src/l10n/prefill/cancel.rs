//! Per-instance cancellation flags for an in-flight pre-fill run. A run
//! registers a flag on start (`begin`) and de-registers on return (`end`).
//! `cancel` flips the flag; the batch loop polls it between batches.
//! `is_active` powers the "a pre-fill is already running" start-guard
//! (`Error::L10nPrefillBusy`).
//!
//! Same shape as `servers_runtime::upload_control` — deliberately, so the two
//! cancellable long-running operations behave identically.
//!
//! The flags live in an owned [`Registry`] and the public functions delegate
//! to one process-wide instance of it — the arrangement `launch::spawn` has
//! with `process::registry::ProcessRegistry`. The split is there for
//! [`any_active`]: it is a claim about the WHOLE registry, and `cargo test`
//! runs this crate's tests on parallel threads inside one process, so "nothing
//! is registered" can only be asserted on an instance no other test can reach.
//! Folding the struct back into a bare `static` brings the intermittent test
//! failure back (`docs/TESTING.md`, "Process-global registries in unit tests").

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

#[derive(Default)]
struct Registry {
    flags: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl Registry {
    /// Poison-tolerant: the lock is only ever held for one map operation,
    /// never across caller code, so a poisoned mutex still guards a
    /// consistent map.
    fn lock(&self) -> MutexGuard<'_, HashMap<String, Arc<AtomicBool>>> {
        self.flags.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn begin(&self, instance_id: &str) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        self.lock().insert(instance_id.to_string(), flag.clone());
        flag
    }

    fn cancel(&self, instance_id: &str) {
        if let Some(flag) = self.lock().get(instance_id) {
            flag.store(true, Ordering::SeqCst);
        }
    }

    fn is_active(&self, instance_id: &str) -> bool {
        self.lock().contains_key(instance_id)
    }

    fn any_active(&self) -> bool {
        !self.lock().is_empty()
    }

    fn end(&self, instance_id: &str) {
        self.lock().remove(instance_id);
    }
}

fn registry() -> &'static Registry {
    static R: OnceLock<Registry> = OnceLock::new();
    R.get_or_init(Registry::default)
}

/// Register a fresh cancel flag (false) for `instance_id`, replacing any stale
/// entry — a re-run must not inherit the previous run's cancellation.
pub fn begin(instance_id: &str) -> Arc<AtomicBool> {
    registry().begin(instance_id)
}

/// Request cancellation of `instance_id`'s in-flight run (no-op if none).
pub fn cancel(instance_id: &str) {
    registry().cancel(instance_id);
}

/// True iff a pre-fill run is currently registered for `instance_id`.
pub fn is_active(instance_id: &str) -> bool {
    registry().is_active(instance_id)
}

/// True iff ANY pre-fill run is currently registered — for gates protecting
/// the GLOBAL override store rather than one instance's pack: a run for any
/// instance writes `<lang>/<namespace>.json` files every instance shares, so
/// share-import (and slice 2's delete) must refuse while one is in flight.
pub fn any_active() -> bool {
    registry().any_active()
}

/// De-register `instance_id` (always called when the run returns, success or
/// not).
pub fn end(instance_id: &str) {
    registry().end(instance_id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn begin_cancel_end_is_scoped_to_one_instance() {
        let reg = Registry::default();
        let a = reg.begin("inst-a");
        let b = reg.begin("inst-b");
        assert!(reg.is_active("inst-a"));
        reg.cancel("inst-a");
        assert!(a.load(Ordering::SeqCst));
        assert!(!b.load(Ordering::SeqCst), "cancelling A must not stop B");
        reg.end("inst-a");
        reg.end("inst-b");
        assert!(!reg.is_active("inst-a"));
    }

    #[test]
    fn cancelling_an_unknown_instance_is_a_no_op() {
        Registry::default().cancel("never-started");
    }

    #[test]
    fn begin_replaces_a_stale_flag_so_a_rerun_does_not_start_cancelled() {
        let reg = Registry::default();
        let first = reg.begin("inst-c");
        reg.cancel("inst-c");
        assert!(first.load(Ordering::SeqCst));
        let second = reg.begin("inst-c");
        assert!(!second.load(Ordering::SeqCst));
        reg.end("inst-c");
    }

    #[test]
    fn any_active_sees_a_run_on_any_instance() {
        // A registry of its own: "nothing is registered" is a claim about the
        // whole registry, which a test sharing the process-wide one with its
        // parallel siblings cannot make.
        let reg = Registry::default();
        assert!(!reg.any_active());
        let _first = reg.begin("inst-any");
        assert!(reg.any_active());
        let _second = reg.begin("inst-other");
        reg.end("inst-any");
        assert!(
            reg.any_active(),
            "a run on another instance is still in flight"
        );
        reg.end("inst-other");
        assert!(!reg.any_active());
    }

    #[test]
    fn the_public_functions_reach_one_process_wide_registry() {
        // The one test that goes through the process-wide registry, which it
        // shares with every other test in this binary. So it uses an id nothing
        // else uses and asserts only what holds whatever a parallel test does.
        // In particular it never asserts `!any_active()`: that is true only if
        // no other thread has a run registered, which no test here can know.
        let id = "inst-process-wide";
        let flag = begin(id);
        assert!(is_active(id));
        assert!(any_active(), "our own run is registered, so this holds");
        cancel(id);
        assert!(flag.load(Ordering::SeqCst));
        end(id);
        assert!(
            !is_active(id),
            "`end` must de-register, or the instance stays busy"
        );
    }

    #[test]
    fn a_private_registry_cannot_see_a_process_wide_run() {
        // What the tests above rest on. A run held in the process-wide registry
        // stands in for a parallel sibling frozen mid-flight; a registry of our
        // own must not see it. Were `Registry` ever a front for shared state,
        // the emptiness assertions above would go back to failing now and then
        // — this one fails every time instead.
        let id = "inst-isolation";
        let _flag = begin(id);
        assert!(!Registry::default().any_active());
        end(id);
    }
}
