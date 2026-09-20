//! A registry of cancellable long-running runs, keyed by the id they run on —
//! the AI pre-fill of one instance (`l10n::prefill::cancel`), the SFTP upload
//! of one server (`servers_runtime::upload_control`). Each of those is a thin
//! process-wide `static` over one [`CancelRegistry`], so the two behave
//! identically because they are the same code, not because two copies agree.
//!
//! A run takes a [`CancelClaim`] with [`CancelRegistry::try_begin`] and polls
//! the claim's flag; [`CancelRegistry::cancel`] flips it. Two properties are the
//! point of the type, and both used to be missing:
//!
//!  - **The claim is atomic.** "Is a run registered?" and "register this one"
//!    are one step under one lock. As two steps, two starts landing together
//!    both passed the check, and the second replaced the first's flag — two
//!    runs on one id, the first no longer cancellable.
//!  - **The claim releases itself.** `Drop` de-registers the id on every exit
//!    path: success, `?`, and a panic unwinding out of the run. A hand-written
//!    "end" after the `.await` is skipped by an unwind, and the id then stays
//!    busy until the launcher restarts — for an upload, that is a server which
//!    can no longer be started.
//!
//! There is deliberately no way to register without a claim: a second way in
//! could replace a claimed entry, and that claim's `Drop` would then remove
//! the replacement. With one way in, an id belongs to at most one claim at a
//! time, which is what makes removing by id in `Drop` exact.
//!
//! An owned type rather than a bare `static` for the reason `docs/TESTING.md`
//! gives under "Process-global registries in unit tests": [`any_active`] is a
//! claim about the WHOLE registry, which only a test holding a private
//! instance can make.
//!
//! [`any_active`]: CancelRegistry::any_active

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

#[derive(Default)]
pub struct CancelRegistry {
    flags: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

/// One registered run. Hold it for the whole run — bind it to a named local
/// before the first `.await`, never `let _ =`, which releases it on the spot.
#[must_use = "dropping the claim de-registers the run before it has started"]
pub struct CancelClaim<'r> {
    registry: &'r CancelRegistry,
    id: String,
    flag: Arc<AtomicBool>,
}

impl CancelClaim<'_> {
    /// The flag the run polls; `true` once cancellation has been requested.
    pub fn flag(&self) -> &Arc<AtomicBool> {
        &self.flag
    }
}

impl Drop for CancelClaim<'_> {
    fn drop(&mut self) {
        // Runs during an unwind too, so it must not panic: a double panic
        // aborts the process. `lock` recovers a poisoned mutex instead.
        self.registry.lock().remove(&self.id);
    }
}

impl CancelRegistry {
    /// Poison-tolerant: the lock is only ever held for one map operation,
    /// never across caller code, so a poisoned mutex still guards a
    /// consistent map.
    fn lock(&self) -> MutexGuard<'_, HashMap<String, Arc<AtomicBool>>> {
        self.flags.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Register a run on `id` with a fresh, un-cancelled flag. `None` while
    /// another run holds `id` — the caller maps that to its own "busy" error.
    pub fn try_begin(&self, id: &str) -> Option<CancelClaim<'_>> {
        let mut flags = self.lock();
        if flags.contains_key(id) {
            return None;
        }
        let flag = Arc::new(AtomicBool::new(false));
        flags.insert(id.to_string(), flag.clone());
        Some(CancelClaim {
            registry: self,
            id: id.to_string(),
            flag,
        })
    }

    /// Request cancellation of the run on `id` (no-op if none).
    pub fn cancel(&self, id: &str) {
        if let Some(flag) = self.lock().get(id) {
            flag.store(true, Ordering::SeqCst);
        }
    }

    /// True iff a run is currently registered on `id`.
    pub fn is_active(&self, id: &str) -> bool {
        self.lock().contains_key(id)
    }

    /// True iff ANY run is currently registered.
    pub fn any_active(&self) -> bool {
        !self.lock().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::sync::atomic::AtomicUsize;
    use std::sync::Barrier;

    #[test]
    fn claim_cancel_release_is_scoped_to_one_id() {
        let reg = CancelRegistry::default();
        let a = reg.try_begin("run-a").expect("a is free");
        let b = reg.try_begin("run-b").expect("b is free");
        assert!(reg.is_active("run-a"));
        reg.cancel("run-a");
        assert!(a.flag().load(Ordering::SeqCst));
        assert!(
            !b.flag().load(Ordering::SeqCst),
            "cancelling A must not stop B"
        );
        drop(a);
        assert!(!reg.is_active("run-a"));
        assert!(reg.is_active("run-b"), "releasing A must not release B");
    }

    #[test]
    fn cancelling_an_unknown_id_is_a_no_op() {
        CancelRegistry::default().cancel("never-started");
    }

    #[test]
    fn any_active_sees_a_run_on_any_id() {
        let reg = CancelRegistry::default();
        assert!(!reg.any_active());
        let first = reg.try_begin("run-any").expect("free");
        assert!(reg.any_active());
        let second = reg.try_begin("run-other").expect("free");
        drop(first);
        assert!(reg.any_active(), "a run on another id is still in flight");
        drop(second);
        assert!(!reg.any_active());
    }

    #[test]
    fn a_second_claim_is_refused_and_leaves_the_holders_flag_alone() {
        // The two halves of the old check-then-register window: the second run
        // must not start, and it must not take the first run's place in the
        // registry either — that left the first run impossible to cancel.
        let reg = CancelRegistry::default();
        let holder = reg.try_begin("run-dup").expect("first claim succeeds");
        assert!(
            reg.try_begin("run-dup").is_none(),
            "a second run on a claimed id must be refused"
        );
        assert!(!holder.flag().load(Ordering::SeqCst));
        reg.cancel("run-dup");
        assert!(
            holder.flag().load(Ordering::SeqCst),
            "the registered flag must still be the holder's own"
        );
    }

    #[test]
    fn an_early_return_releases_the_claim() {
        fn failing_run(reg: &CancelRegistry) -> Result<(), ()> {
            let _claim = reg.try_begin("run-early").ok_or(())?;
            Err(()) // the `?`-shaped exit a real run takes on a transport error
        }
        let reg = CancelRegistry::default();
        assert!(failing_run(&reg).is_err());
        assert!(
            !reg.is_active("run-early"),
            "a failed run must not leave its id busy"
        );
    }

    #[test]
    fn a_panic_releases_the_claim() {
        // The exit a hand-written `end()` after the `.await` never covered: the
        // unwind skips it, and the id stays busy until the launcher restarts.
        let reg = CancelRegistry::default();
        let result = catch_unwind(AssertUnwindSafe(|| {
            let _claim = reg.try_begin("run-panic").expect("free");
            panic!("the run panicked");
        }));
        assert!(result.is_err());
        assert!(!reg.is_active("run-panic"));
        assert!(!reg.any_active());
        assert!(reg.try_begin("run-panic").is_some(), "the id is reusable");
    }

    #[test]
    fn a_refused_claim_leaves_nothing_behind() {
        let reg = CancelRegistry::default();
        let holder = reg.try_begin("run-refused").expect("free");
        for _ in 0..3 {
            assert!(reg.try_begin("run-refused").is_none());
        }
        drop(holder);
        assert!(!reg.any_active(), "three refusals must not have registered");
        assert!(reg.try_begin("run-refused").is_some());
    }

    #[test]
    fn a_rerun_after_a_cancelled_run_does_not_start_cancelled() {
        let reg = CancelRegistry::default();
        let first = reg.try_begin("run-again").expect("free");
        reg.cancel("run-again");
        assert!(first.flag().load(Ordering::SeqCst));
        drop(first);
        let second = reg.try_begin("run-again").expect("released by the drop");
        assert!(!second.flag().load(Ordering::SeqCst));
    }

    #[test]
    fn only_one_of_many_simultaneous_claims_wins() {
        // Check and insert are one step under one lock. Every contender keeps
        // whatever it won until all have tried, so a correct registry yields
        // exactly one winner in every round, whatever the scheduler does; one
        // that looks first and inserts second sooner or later yields two.
        const CONTENDERS: usize = 8;
        const ROUNDS: usize = 200;
        let reg = CancelRegistry::default();
        for round in 0..ROUNDS {
            let winners = AtomicUsize::new(0);
            let go = Barrier::new(CONTENDERS);
            let all_tried = Barrier::new(CONTENDERS);
            std::thread::scope(|scope| {
                for _ in 0..CONTENDERS {
                    scope.spawn(|| {
                        go.wait();
                        let claim = reg.try_begin("run-contended");
                        if claim.is_some() {
                            winners.fetch_add(1, Ordering::SeqCst);
                        }
                        all_tried.wait();
                        drop(claim);
                    });
                }
            });
            assert_eq!(winners.load(Ordering::SeqCst), 1, "round {round}");
            assert!(!reg.any_active(), "round {round}: the winner released");
        }
    }
}
