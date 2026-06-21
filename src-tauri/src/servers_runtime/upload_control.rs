//! Per-server upload control: a registry of cancellation flags. An upload
//! registers a flag on start (`upload_begin`) and de-registers on return
//! (`upload_end`). `upload_cancel` flips the flag; the transfer loop polls it.
//! `upload_is_active` powers the "is an upload in flight?" start-guard (#F).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

fn registry() -> &'static Mutex<HashMap<String, Arc<AtomicBool>>> {
    static R: OnceLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Register a fresh cancel flag (false) for `id`, replacing any stale entry.
pub fn upload_begin(id: &str) -> Arc<AtomicBool> {
    let flag = Arc::new(AtomicBool::new(false));
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(id.to_string(), flag.clone());
    flag
}

/// Request cancellation of `id`'s in-flight upload (no-op if none).
pub fn upload_cancel(id: &str) {
    if let Some(flag) = registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(id)
    {
        flag.store(true, Ordering::SeqCst);
    }
}

/// True iff an upload is currently registered for `id`.
pub fn upload_is_active(id: &str) -> bool {
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains_key(id)
}

/// De-register `id` (always called when the upload returns, success or not).
pub fn upload_end(id: &str) {
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn begin_marks_active_then_end_clears() {
        let id = "ctl-1";
        assert!(!upload_is_active(id));
        let _flag = upload_begin(id);
        assert!(upload_is_active(id));
        upload_end(id);
        assert!(!upload_is_active(id));
    }

    #[test]
    fn cancel_sets_the_flag() {
        let id = "ctl-2";
        let flag = upload_begin(id);
        assert!(!flag.load(Ordering::SeqCst));
        upload_cancel(id);
        assert!(flag.load(Ordering::SeqCst));
        upload_end(id);
    }

    #[test]
    fn cancel_absent_is_noop() {
        upload_cancel("ctl-absent-never-registered"); // must not panic
    }
}
