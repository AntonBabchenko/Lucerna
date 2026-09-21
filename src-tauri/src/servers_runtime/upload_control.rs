//! Per-server upload control: which servers have an SFTP upload in flight, and
//! the flag that cancels each. An upload takes a claim on start
//! ([`upload_try_begin`]) and holds it until it returns; dropping the claim
//! de-registers the server. [`upload_cancel`] flips the flag; the transfer
//! loop polls it. [`upload_is_active`] powers the "is an upload in flight?"
//! start-guard (#F).
//!
//! The process-wide instance of [`CancelRegistry`] for uploads — the same type
//! `l10n::prefill::cancel` is, so the two behave identically. The claim's
//! atomicity and its release on every exit path are tested there.
//!
//! Why the release matters more here than anywhere: while a server is
//! registered, `server_start` and `server_restart` refuse it. A registration
//! that outlived its upload would be a server that cannot be started until the
//! launcher restarts.

use std::sync::OnceLock;

use crate::cancel_registry::{CancelClaim, CancelRegistry};

fn registry() -> &'static CancelRegistry {
    static R: OnceLock<CancelRegistry> = OnceLock::new();
    R.get_or_init(CancelRegistry::default)
}

/// Claim the upload of server `id`. `None` while one is already in flight.
/// The claim carries the upload's cancel flag and de-registers the server when
/// dropped: bind it to a named local for the whole upload.
pub fn upload_try_begin(id: &str) -> Option<CancelClaim<'static>> {
    registry().try_begin(id)
}

/// Request cancellation of `id`'s in-flight upload (no-op if none).
pub fn upload_cancel(id: &str) {
    registry().cancel(id);
}

/// True iff an upload is currently registered for `id`.
pub fn upload_is_active(id: &str) -> bool {
    registry().is_active(id)
}

/// True iff ANY upload is currently registered.
pub fn upload_any_active() -> bool {
    registry().any_active()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    // These go through the process-wide registry, shared with every other test
    // in this binary: each uses an id of its own and asserts per-id state only.

    #[test]
    fn a_claim_marks_active_then_its_drop_clears() {
        let id = "ctl-1";
        assert!(!upload_is_active(id));
        let claim = upload_try_begin(id).expect("nothing else uses this id");
        assert!(upload_is_active(id));
        assert!(
            upload_try_begin(id).is_none(),
            "a second upload of the server must be refused"
        );
        drop(claim);
        assert!(!upload_is_active(id));
    }

    #[test]
    fn cancel_sets_the_flag() {
        let id = "ctl-2";
        let claim = upload_try_begin(id).expect("nothing else uses this id");
        assert!(!claim.flag().load(Ordering::SeqCst));
        upload_cancel(id);
        assert!(claim.flag().load(Ordering::SeqCst));
    }

    #[test]
    fn cancel_absent_is_noop() {
        upload_cancel("ctl-absent-never-registered"); // must not panic
    }
    #[test]
    fn upload_any_active_sees_a_claim() {
        let claim = upload_try_begin("any-active-u").expect("free id");
        assert!(upload_any_active());
        drop(claim);
    }
}
