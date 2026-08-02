//! Per-instance cancellation flags for an in-flight pre-fill run. A run
//! registers a flag on start (`begin`) and de-registers on return (`end`).
//! `cancel` flips the flag; the batch loop polls it between batches.
//! `is_active` powers the "a pre-fill is already running" start-guard
//! (`Error::L10nPrefillBusy`).
//!
//! Same shape as `servers_runtime::upload_control` — deliberately, so the two
//! cancellable long-running operations behave identically.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

fn registry() -> &'static Mutex<HashMap<String, Arc<AtomicBool>>> {
    static R: OnceLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Register a fresh cancel flag (false) for `instance_id`, replacing any stale
/// entry — a re-run must not inherit the previous run's cancellation.
pub fn begin(instance_id: &str) -> Arc<AtomicBool> {
    let flag = Arc::new(AtomicBool::new(false));
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(instance_id.to_string(), flag.clone());
    flag
}

/// Request cancellation of `instance_id`'s in-flight run (no-op if none).
pub fn cancel(instance_id: &str) {
    if let Some(flag) = registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(instance_id)
    {
        flag.store(true, Ordering::SeqCst);
    }
}

/// True iff a pre-fill run is currently registered for `instance_id`.
pub fn is_active(instance_id: &str) -> bool {
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains_key(instance_id)
}

/// De-register `instance_id` (always called when the run returns, success or
/// not).
pub fn end(instance_id: &str) {
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(instance_id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn begin_cancel_end_is_scoped_to_one_instance() {
        let a = begin("inst-a");
        let b = begin("inst-b");
        assert!(is_active("inst-a"));
        cancel("inst-a");
        assert!(a.load(Ordering::SeqCst));
        assert!(!b.load(Ordering::SeqCst), "cancelling A must not stop B");
        end("inst-a");
        end("inst-b");
        assert!(!is_active("inst-a"));
    }

    #[test]
    fn cancelling_an_unknown_instance_is_a_no_op() {
        cancel("never-started");
    }

    #[test]
    fn begin_replaces_a_stale_flag_so_a_rerun_does_not_start_cancelled() {
        let first = begin("inst-c");
        cancel("inst-c");
        assert!(first.load(Ordering::SeqCst));
        let second = begin("inst-c");
        assert!(!second.load(Ordering::SeqCst));
        end("inst-c");
    }
}
