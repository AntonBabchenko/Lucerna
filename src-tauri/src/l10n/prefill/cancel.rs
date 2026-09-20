//! Which instances have an AI pre-fill run in flight, and the flag that
//! cancels each. A run takes a claim on start ([`try_begin`]) and holds it
//! until it returns; dropping the claim de-registers the instance. [`cancel`]
//! flips the flag; the batch loop polls it between batches. [`is_active`]
//! powers the "a pre-fill is already running" listing, and a refused
//! [`try_begin`] is the start-guard (`Error::L10nPrefillBusy`).
//!
//! The process-wide instance of [`CancelRegistry`] for pre-fill runs. Same
//! type as `servers_runtime::upload_control`, so the two cancellable
//! long-running operations behave identically. The claim's atomicity and its
//! release on every exit path — a panic included — are properties of that type
//! and are tested there, on private instances.

use std::sync::OnceLock;

use crate::cancel_registry::{CancelClaim, CancelRegistry};

fn registry() -> &'static CancelRegistry {
    static R: OnceLock<CancelRegistry> = OnceLock::new();
    R.get_or_init(CancelRegistry::default)
}

/// Claim the pre-fill run for `instance_id`. `None` while one is already in
/// flight on it. The claim carries the run's cancel flag and de-registers the
/// instance when dropped: bind it to a named local for the whole run.
pub fn try_begin(instance_id: &str) -> Option<CancelClaim<'static>> {
    registry().try_begin(instance_id)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn the_public_functions_reach_one_process_wide_registry() {
        // Goes through the process-wide registry, which it shares with every
        // other test in this binary. So it uses an id nothing else uses and
        // asserts only what holds whatever a parallel test does. In particular
        // it never asserts `!any_active()`: that is true only if no other
        // thread has a run registered, which no test here can know
        // (`docs/TESTING.md`, "Process-global registries in unit tests").
        let id = "inst-process-wide";
        let claim = try_begin(id).expect("nothing else uses this id");
        assert!(is_active(id));
        assert!(any_active(), "our own run is registered, so this holds");
        assert!(
            try_begin(id).is_none(),
            "a second run on the instance must be refused"
        );
        cancel(id);
        assert!(claim.flag().load(Ordering::SeqCst));
        drop(claim);
        assert!(
            !is_active(id),
            "dropping the claim must de-register, or the instance stays busy"
        );
    }

    #[test]
    fn a_private_registry_cannot_see_a_process_wide_run() {
        // A run held in the process-wide registry stands in for a parallel
        // sibling frozen mid-flight; a registry of our own must not see it.
        // Were the registries ever a front for shared state, every
        // whole-registry assertion in `cancel_registry`'s tests would go back
        // to failing now and then — this one fails every time instead. The
        // claim is what makes it so: with no run held, shared state would look
        // just as empty.
        let _claim = try_begin("inst-isolation").expect("nothing else uses this id");
        assert!(!CancelRegistry::default().any_active());
    }
}
