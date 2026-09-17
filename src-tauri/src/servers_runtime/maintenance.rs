//! Per-server maintenance gate: a registry of servers whose `runtime/` tree is
//! being rewritten wholesale (backup restore, import commit), and of the
//! per-item content writers in flight on each server.
//!
//! `maintenance_is_active` powers the Start/Restart guard the same way
//! `upload_control::upload_is_active` powers the upload one: a Start click
//! while a restore is mid-flight would launch a JVM over a half-written tree
//! and tear the world.
//!
//! # Why there are two kinds of claim
//!
//! The exclusive claim alone was not enough. A restore is
//! `remove_dir_all(runtime/)` + `extract_zip` — minutes on a GB-scale server —
//! and `runtime/` is also where every mod jar, plugin jar, datapack,
//! `server.properties` and the `.lucerna-installed.json` sidecar live. Until
//! this module grew its second half, nothing but `server_start` /
//! `server_restart` ever READ the claim, and nothing but the restore and the
//! two import paths ever took one. Both directions were open:
//!
//!   - A mod install, plugin install, update or hash-enrichment checked only
//!     `is_running` and was admitted mid-restore, writing a jar or the sidecar
//!     into a tree being deleted and re-extracted. Opening the Add-ons tab is
//!     enough on its own: it auto-fires `server_enrich_mods` whenever a listed
//!     jar has no provenance.
//!   - A writer already in flight was invisible, so a restore starting on top
//!     of one could not refuse — a snapshot check cannot fix that, because only
//!     a slot the writer HOLDS is visible to an operation that begins later.
//!
//! So per-item writers take a SHARED claim ([`claim_shared_write`]) and long
//! rewrites take the EXCLUSIVE one ([`maintenance_begin`] / [`try_begin`]).
//! Both halves live under one mutex, so the two kinds are checked against each
//! other atomically and need no Dekker pairing between themselves:
//!
//! | held \ requested | exclusive | shared | [`not_under_maintenance`] | Start/Restart |
//! |------------------|-----------|--------|---------------------------|---------------|
//! | exclusive        | refused   | refused| refused                   | refused       |
//! | shared (n >= 1)  | refused   | admitted| admitted                 | admitted      |
//!
//! Shared rather than exclusive because the server Mods browser deliberately
//! runs two installs on one server at once (`ServerModBrowser.svelte`: "installs
//! can overlap without clobbering each other's busy state"); an exclusive claim
//! would refuse the second and change shipped behaviour.
//!
//! Start/Restart read [`maintenance_is_active`], which is the exclusive half
//! ONLY — a mod install does not block Start, exactly as today. Whether it
//! should is a separate question from this one and is not decided here.
//!
//! # What this gate is NOT
//!
//! Unlike the client twin's `instances::maintenance::write_allowed`, nothing
//! here folds in `is_running` / `is_starting`. The server writers' running
//! policy is heterogeneous and already decided per command — `server.properties`
//! is editable while the server runs, mods are not, the datapack commands also
//! refuse while starting — and this module does not touch it.
//! [`not_under_maintenance`] is ONE TERM, composed with each command's own
//! check; it is named so that it cannot be mistaken for the whole gate.
//!
//! # Guards
//!
//! There is no cancel flag to hand out — the only state is membership — so each
//! begin/end pair is folded into an RAII guard whose `Drop` releases the slot on
//! every exit path: success, `?`, or a panic unwinding out of the blocking work.
//! Bind one to a NAMED local for the whole write; `let _ = claim_shared_write(…)`
//! drops it on the spot and protects nothing.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, MutexGuard, OnceLock};

use crate::error::{Error, Result};

/// Both halves of the gate, behind one lock so a claim of either kind is
/// checked against the other atomically.
#[derive(Default)]
struct Slots {
    /// Servers whose `runtime/` is being rewritten wholesale (the exclusive
    /// claim): a backup restore, or an import commit laying down a fresh tree.
    held: HashSet<String>,
    /// In-flight per-item content writers per server. An id is present only
    /// while its count is at least one, so `contains_key` IS "a writer is in
    /// flight".
    sharing: HashMap<String, usize>,
}

fn registry() -> &'static Mutex<Slots> {
    static R: OnceLock<Mutex<Slots>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(Slots::default()))
}

/// Poison-tolerant on purpose (see [`MaintenanceGuard`]'s `Drop`): the lock is
/// only ever held for a single map/set operation, never across caller code, so
/// a poisoned mutex cannot mean an inconsistent `Slots`, and a guard's `Drop`
/// must never panic during an unwind.
fn lock_slots() -> MutexGuard<'static, Slots> {
    registry().lock().unwrap_or_else(|e| e.into_inner())
}

/// Why an exclusive claim was refused. Computed under the one lock together
/// with the refusal itself, so the reason the user is shown is the reason that
/// actually blocked them and not a re-read that may have changed meanwhile
/// (fallback discipline: honesty).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blocked {
    /// Another restore or import commit already holds this server.
    Maintenance,
    /// A per-item content writer (install, update, toggle, enrichment) is
    /// still in flight on this server.
    ContentWrite,
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
        // Poison-tolerant on purpose. The lock is only ever held for a single
        // set or map operation, never across caller code, so a poisoned mutex
        // can only mean a panic inside the collection itself and it is still
        // consistent. Propagating the poison from `Drop` would panic during an
        // unwind — a double panic aborts the process and the slot would never
        // be released. Recovering the inner value is the safe direction: the id
        // is removed either way.
        lock_slots().held.remove(&self.id);
    }
}

/// RAII shared claim of a per-item content writer. Any number may be held on
/// one server at once; while at least one is, every exclusive claim on that
/// server is refused. `Drop` releases it on every exit path.
#[must_use = "dropping the guard immediately ends the write's claim before the work runs"]
pub struct SharedWriteGuard {
    id: String,
}

impl Drop for SharedWriteGuard {
    fn drop(&mut self) {
        // Poison-tolerant for the reason `MaintenanceGuard::drop` gives. The
        // key is always present here: a guard exists only after its increment
        // and only its own drop decrements it — and were it absent, there would
        // be nothing to release.
        let mut slots = lock_slots();
        let remaining = slots.sharing.get_mut(&self.id).map(|count| {
            *count = count.saturating_sub(1);
            *count
        });
        if remaining == Some(0) {
            // The last writer on this id: remove the key, so `contains_key`
            // keeps meaning "a writer is in flight".
            slots.sharing.remove(&self.id);
        }
    }
}

/// Atomically claim the maintenance slot for `id`, or say what blocked it.
/// Refused while another restore/import holds the server, and refused while a
/// per-item content writer ([`claim_shared_write`]) is still in flight on it —
/// the direction that was missing: a restore must not start on top of a mod
/// install that is still downloading into `runtime/mods/`.
pub fn try_begin(id: &str) -> std::result::Result<MaintenanceGuard, Blocked> {
    let mut slots = lock_slots();
    if slots.held.contains(id) {
        return Err(Blocked::Maintenance);
    }
    if slots.sharing.contains_key(id) {
        return Err(Blocked::ContentWrite);
    }
    slots.held.insert(id.to_string());
    Ok(MaintenanceGuard { id: id.to_string() })
}

/// Atomically claim the maintenance slot for `id`. Returns `None` if a
/// restore/import is already rewriting this server, or if a per-item content
/// writer is still in flight on it, so the caller maps that to
/// `ServerMaintenanceInProgress` (same contract as `runtime::claim_start`).
/// Callers that must tell the two causes apart in the message they show take
/// [`try_begin`] instead.
pub fn maintenance_begin(id: &str) -> Option<MaintenanceGuard> {
    try_begin(id).ok()
}

/// True iff `id`'s `runtime/` is currently being rewritten by a
/// restore/import. A per-item writer's shared claim does not count — see the
/// module doc.
pub fn maintenance_is_active(id: &str) -> bool {
    lock_slots().held.contains(id)
}

/// The maintenance term of the server write gate, for a writer that performs
/// ONE short write (a rename, a single `fs::write`) rather than holding the
/// server across downloads. Refuses with `ServerMaintenanceInProgress` while a
/// restore or import is rewriting `runtime/`.
///
/// **This is not the whole gate.** It folds in neither `is_running` nor
/// `is_starting`: each command keeps its own running policy, which differs
/// between them by design. Call it *alongside* that check, not instead of it.
///
/// One in-memory bool and no I/O, so there is no "could not tell" state to
/// resolve. It is a snapshot, exactly as the client twin documents for its own
/// short-writer gate: the window between this call and the write is real and
/// accepted for a write measured in microseconds. A writer that spans an
/// `.await` takes [`claim_shared_write`] instead, which closes it.
pub fn not_under_maintenance(id: &str) -> Result<()> {
    if maintenance_is_active(id) {
        return Err(Error::ServerMaintenanceInProgress { id: id.to_string() });
    }
    Ok(())
}

/// The gate for a per-item content writer that holds the server across
/// `.await`s — one mod or plugin installed, updated or removed, one datapack
/// written, one hash-enrichment pass. Refuses with
/// `ServerMaintenanceInProgress` while a restore or import holds the server;
/// otherwise admits the writer alongside any others already in flight.
///
/// The returned guard IS the protection in the other direction: bind it to a
/// named local for the whole command (never `let _ =`) and hold it past the
/// last write, so a restore that starts meanwhile sees it and refuses.
///
/// Running and starting are deliberately not consulted here — see the module
/// doc. A command holding this may call another that takes it again on the same
/// id: shared claims nest. It must never call anything that opens with
/// [`maintenance_begin`] on its own id — that would refuse itself.
pub fn claim_shared_write(id: &str) -> Result<SharedWriteGuard> {
    let mut slots = lock_slots();
    if slots.held.contains(id) {
        return Err(Error::ServerMaintenanceInProgress { id: id.to_string() });
    }
    *slots.sharing.entry(id.to_string()).or_insert(0) += 1;
    Ok(SharedWriteGuard { id: id.to_string() })
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
        fn failing_restore(id: &str) -> std::result::Result<(), ()> {
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

    // --- Ordering A: a restore must refuse on top of a content writer -------

    #[test]
    fn exclusive_claim_is_refused_while_a_shared_writer_is_in_flight() {
        // The Add-ons install that is still downloading when the user switches
        // to Backups and presses Restore.
        let id = "maint-6";
        let install = claim_shared_write(id).expect("an install is admitted on an idle server");
        assert!(
            maintenance_begin(id).is_none(),
            "a restore must not start on top of an in-flight content writer"
        );
        assert_eq!(
            try_begin(id).err(),
            Some(Blocked::ContentWrite),
            "and the refusal must name the content writer, not a phantom restore"
        );
        drop(install);
        let restore = maintenance_begin(id).expect("the install finished; the restore may proceed");
        drop(restore);
    }

    #[test]
    fn a_second_restore_is_told_it_is_maintenance_not_a_content_write() {
        let id = "maint-7";
        let first = maintenance_begin(id).expect("first restore claims");
        assert_eq!(try_begin(id).err(), Some(Blocked::Maintenance));
        drop(first);
    }

    #[test]
    fn every_shared_writer_must_finish_before_an_exclusive_claim_is_admitted() {
        let id = "maint-8";
        let a = claim_shared_write(id).expect("first install");
        let b = claim_shared_write(id).expect("the browser runs two installs at once");
        drop(a);
        assert!(
            maintenance_begin(id).is_none(),
            "one writer finishing must not release the other's protection"
        );
        drop(b);
        assert!(
            maintenance_begin(id).is_some(),
            "the count reaching zero releases the server"
        );
    }

    // --- Ordering B: a content writer must refuse under a restore ----------

    #[test]
    fn shared_claim_and_snapshot_are_refused_under_an_exclusive_claim() {
        // The enrichment that fires automatically when the user opens the
        // Add-ons tab while a restore is running.
        let id = "maint-9";
        let restore = maintenance_begin(id).expect("the restore claims first");
        assert!(
            claim_shared_write(id).is_err(),
            "an install/enrichment must refuse while runtime/ is being replaced"
        );
        assert!(
            not_under_maintenance(id).is_err(),
            "and so must a short writer (a toggle, a properties write)"
        );
        drop(restore);
        assert!(claim_shared_write(id).is_ok());
        assert!(not_under_maintenance(id).is_ok());
    }

    #[test]
    fn a_refused_shared_claim_leaves_no_residue() {
        // A refusal must not increment the count: if it did, the server would
        // stay unclaimable for a restore forever after one refused install.
        let id = "maint-10";
        let restore = maintenance_begin(id).expect("claim");
        for _ in 0..3 {
            assert!(claim_shared_write(id).is_err());
        }
        drop(restore);
        assert!(
            maintenance_begin(id).is_some(),
            "three refusals must leave no shared count behind"
        );
    }

    // --- The shared claim must stay invisible to Start ---------------------

    #[test]
    fn a_shared_writer_does_not_make_maintenance_active() {
        // Start/Restart read `maintenance_is_active`. Installing a mod has
        // never blocked Start and must not begin to here.
        let id = "maint-11";
        let write = claim_shared_write(id).expect("claim");
        assert!(
            !maintenance_is_active(id),
            "Start reads maintenance_is_active and must not see an item writer"
        );
        assert!(
            not_under_maintenance(id).is_ok(),
            "nor may one short writer refuse another"
        );
        drop(write);
    }

    // --- Shared guard release paths ---------------------------------------

    #[test]
    fn shared_claim_clears_on_early_error_and_on_panic() {
        let id = "maint-12";
        fn failing_install(id: &str) -> Result<()> {
            let _write = claim_shared_write(id)?;
            Err(Error::ServerContentStale) // the `?`-shaped exit a download error hits
        }
        assert!(failing_install(id).is_err());
        assert!(
            maintenance_begin(id).is_some(),
            "an early error must release the shared slot"
        );

        let id = "maint-13";
        let result = std::panic::catch_unwind(|| {
            let _write = claim_shared_write(id).expect("claim");
            panic!("the install panicked");
        });
        assert!(result.is_err());
        assert!(
            maintenance_begin(id).is_some(),
            "a panic unwinding out of the write must release the shared slot"
        );
    }

    #[test]
    fn shared_claims_are_per_server() {
        let a = claim_shared_write("maint-14a").expect("claim a");
        assert!(
            maintenance_begin("maint-14b").is_some(),
            "a writer on one server must not block a restore of another"
        );
        drop(a);
    }
}
