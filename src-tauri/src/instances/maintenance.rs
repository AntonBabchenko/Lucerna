//! Per-instance maintenance gate: a registry of client instances whose
//! content — `saves/`, `backups/`, `mods/`, resource and shader packs, the
//! datapack library, `pack_origin`, `instance.json` — is being rewritten, or
//! copied wholesale, by one long operation: a world migration, a modpack
//! update, a Minecraft-version mod migration, a clone, or a pack-files
//! reimport — and of the per-item writers in flight on each instance.
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
//! Shape: the shape of `servers_runtime::maintenance` — a process-global
//! set of held ids behind a `Mutex`, an RAII [`MaintenanceGuard`] whose
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
//!
//! Long writers other than the migration — the modpack update, the
//! Minecraft-version mod migration, the clone, the pack-files reimport — take
//! the same claim through [`claim_write`], the same claim-then-check order
//! spelled once, refusing with `InstanceBusy`. The migration keeps its own
//! spelling only because it claims two ids and its refusal names the instance
//! and its role.
//!
//! Per-item content writers — one mod or asset installed, updated, toggled,
//! removed or restored — take a SHARED claim through [`claim_shared_write`].
//! Reader–writer semantics, both halves under the one mutex:
//!
//! | held \ requested | exclusive claim | shared claim | `write_allowed` | launch |
//! |------------------|-----------------|--------------|-----------------|--------|
//! | exclusive        | refused         | refused      | refused         | refused |
//! | shared (n ≥ 1)   | refused         | admitted     | admitted        | admitted |
//!
//! Why shared and not the exclusive claim: the Mods browser deliberately lets
//! two installs run on one instance at once (two cards, the task registry's
//! concurrent lane), and an exclusive claim would refuse the second. Why a
//! claim and not an entry check: an install admitted before a pack update
//! starts must make that pack update refuse — only a slot the writer HOLDS is
//! visible to it. Because both halves are read and written under one lock,
//! the two claim kinds need no Dekker pairing with each other.
//!
//! Deliberately NOT consulted by a shared claim: `is_running` / `is_starting`.
//! Installing or toggling a mod for the next launch while the game runs has
//! never been refused, the loader read `mods/` at startup, and nothing in the
//! Mods views gates it; refusing it would be a behaviour change nobody decided.
//! For the same reason a shared writer leaves [`maintenance_is_active`] — and
//! so launch, `delete_backup` and [`write_allowed`] — untouched.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, MutexGuard, OnceLock};

use crate::error::{Error, Result};

/// Both halves of the gate, behind one lock so a claim of either kind is
/// checked against the other atomically.
#[derive(Default)]
struct Slots {
    /// Instances held by one long operation (the exclusive claim).
    held: HashSet<String>,
    /// In-flight per-item writers per instance. An id is present only while
    /// its count is at least one, so `contains_key` IS "a writer is in flight".
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
        // set or map operation, never across caller code, so a poisoned mutex
        // can only mean a panic inside the collection itself, and it is still
        // consistent. Propagating the poison from `Drop` would panic during
        // an unwind — a double panic aborts the process and the slot would
        // never be released. Recovering the inner value is the safe direction:
        // the id is removed either way.
        lock_slots().held.remove(&self.id);
    }
}

/// Atomically claim the maintenance slot for `id`. Returns `None` if another
/// long operation already holds it, or if a per-item writer
/// ([`claim_shared_write`]) is still in flight on it — so the caller maps that
/// to `InstanceBusy` (same contract as `launch::spawn::start`'s `claim_start`).
pub fn maintenance_begin(id: &str) -> Option<MaintenanceGuard> {
    let mut slots = lock_slots();
    if slots.held.contains(id) || slots.sharing.contains_key(id) {
        return None;
    }
    slots.held.insert(id.to_string());
    Some(MaintenanceGuard { id: id.to_string() })
}

/// True iff `id`'s content is currently being rewritten under a
/// [`MaintenanceGuard`]. The launch side re-checks this after `claim_start`.
/// A per-item writer's shared claim does not count — see the module doc.
pub fn maintenance_is_active(id: &str) -> bool {
    lock_slots().held.contains(id)
}

/// True iff ANY id holds an exclusive claim or has a shared writer in flight.
/// The data-root move refuses while this is true: a claim means something is
/// writing under the root that is about to be copied and deleted.
pub fn any_active() -> bool {
    let slots = lock_slots();
    !slots.held.is_empty() || !slots.sharing.is_empty()
}

/// RAII shared claim of a per-item content writer. Any number may be held on
/// one instance at once; while at least one is, every exclusive claim on that
/// instance is refused. `Drop` releases it on every exit path.
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

/// The gate for a per-item content writer — one mod or asset installed,
/// updated, toggled, removed or restored, or one pack-origin overlay row
/// recorded. Refuses with `InstanceBusy` while a long operation holds the
/// instance (a world migration, a modpack update, a Minecraft-version mod
/// migration, a clone, a pack-files reimport); otherwise admits the writer
/// alongside any others already in flight. The returned guard IS the
/// protection: bind it to a named local for the whole command (never
/// `let _ =`) and drop it after the last write, so a long operation that
/// starts meanwhile sees it and refuses.
///
/// Running and starting are deliberately not consulted — see the module doc.
/// A command holding this may call another command that takes it again on the
/// same id (a log repair's reinstall runs the mod install): shared claims nest.
/// It must never call anything that opens with [`claim_write`] or
/// [`maintenance_begin`] on its own id — that would refuse itself.
pub fn claim_shared_write(id: &str) -> Result<SharedWriteGuard> {
    let mut slots = lock_slots();
    if slots.held.contains(id) {
        return Err(Error::InstanceBusy);
    }
    *slots.sharing.entry(id.to_string()).or_insert(0) += 1;
    Ok(SharedWriteGuard { id: id.to_string() })
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
/// directly and maps a hit to `WorldMigrateInstanceRunning`. The same holds
/// for a long writer under [`claim_write`]: nothing it calls while holding the
/// claim may open with this gate on its own id.
pub fn write_allowed(id: &str) -> Result<()> {
    if crate::launch::spawn::is_running(id)
        || crate::launch::spawn::is_starting(id)
        || maintenance_is_active(id)
    {
        return Err(Error::InstanceBusy);
    }
    Ok(())
}

/// The gate for a LONG writer — one that holds an instance for seconds or
/// minutes (a modpack update, a Minecraft-version mod migration, a clone, a
/// pack-files reimport) rather than performing one short write. Claims the
/// maintenance slot, and only then refuses if the instance is running or
/// mid-launch. Also refused while a per-item writer ([`claim_shared_write`])
/// is still in flight on the instance. The returned guard IS the protection:
/// bind it to a named local for the whole write (never `let _ =`, which
/// releases it on the spot) and drop it after the last write.
///
/// While it is held, every short writer ([`write_allowed`]), every per-item
/// writer ([`claim_shared_write`]), a world migration (`maintenance_begin`),
/// another long writer, and a launch (`maintenance_is_active`, checked at
/// command entry and again after `claim_start`) all refuse this instance.
///
/// Order is the point — the Dekker pairing the module doc describes: launch
/// sets `starting` and then checks the claim; this sets the claim and then
/// checks `starting`/`running`, so a launch racing a long writer is refused by
/// at least one side. Checking first and claiming second would let a launch
/// slip in between. Every refusal is `InstanceBusy`, whose copy ("an
/// operation is already in progress, or the game is running") is true for
/// both causes. Three in-memory booleans and no I/O, so there is no "could not
/// tell" state; a claim taken and then refused is released by the guard's
/// `Drop` before the error leaves, so a refusal never locks the instance.
pub fn claim_write(id: &str) -> Result<MaintenanceGuard> {
    claim_write_with(id, &|id| {
        crate::launch::spawn::is_running(id) || crate::launch::spawn::is_starting(id)
    })
}

/// [`claim_write`] with the running/starting predicate injected: the launch
/// registry has no test hook, so this is the seam its refusal paths are tested
/// through. Production passes the real predicate and nothing else.
fn claim_write_with(
    id: &str,
    running_or_starting: &dyn Fn(&str) -> bool,
) -> Result<MaintenanceGuard> {
    let claim = maintenance_begin(id).ok_or(Error::InstanceBusy)?;
    if running_or_starting(id) {
        // `claim` drops on this return and releases the slot we just took.
        return Err(Error::InstanceBusy);
    }
    Ok(claim)
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

    /// Nothing running, nothing starting — the predicate the tests below pass
    /// when they are not exercising the running/starting refusal.
    fn idle(_: &str) -> bool {
        false
    }

    #[test]
    fn claim_write_holds_the_instance_against_every_other_writer_until_dropped() {
        // A modpack update (or mod migration, or clone) in flight: a short
        // writer, a world migration, a second long writer and a launch must
        // all see the instance as taken — and all see it free again after.
        let id = "inst-maint-9";
        let claim = claim_write_with(id, &idle).expect("an idle instance is claimed");
        assert!(
            maintenance_is_active(id),
            "launch's check must see the claim"
        );
        assert!(
            matches!(write_allowed(id), Err(Error::InstanceBusy)),
            "a short writer (world backup, change MC, datapack) must be refused"
        );
        assert!(
            maintenance_begin(id).is_none(),
            "a world migration must be refused"
        );
        assert!(
            matches!(claim_write_with(id, &idle), Err(Error::InstanceBusy)),
            "a second long writer (pack update vs. mod migration) must be refused"
        );
        drop(claim);
        assert!(!maintenance_is_active(id));
        assert!(write_allowed(id).is_ok());
    }

    #[test]
    fn a_refused_long_writer_leaves_the_holders_claim_in_place() {
        // The refusal must not release a claim it never took: a world
        // migration holding the slot stays protected after a pack update
        // bounces off it.
        let id = "inst-maint-10";
        let migration = maintenance_begin(id).expect("the migration claims first");
        assert!(matches!(
            claim_write_with(id, &idle),
            Err(Error::InstanceBusy)
        ));
        assert!(
            maintenance_is_active(id),
            "the refused writer must not have released the migration's slot"
        );
        drop(migration);
        assert!(!maintenance_is_active(id));
    }

    #[test]
    fn claim_write_refuses_a_running_or_starting_instance_and_releases_its_claim() {
        // A leaked claim here would leave the instance unlaunchable and
        // unwritable until the launcher restarts, all because the user tried
        // to update a pack while playing.
        let id = "inst-maint-11";
        let refused = claim_write_with(id, &|_| true);
        assert!(matches!(refused, Err(Error::InstanceBusy)));
        assert!(
            !maintenance_is_active(id),
            "a refusal after claiming must release the slot"
        );
        assert!(write_allowed(id).is_ok());
    }

    #[test]
    fn claim_write_checks_running_only_while_already_holding_the_claim() {
        // The Dekker order with `launch::start`: claim first, then look at
        // running/starting. Checked the other way round, a launch could claim
        // `starting` between our check and our claim, and both would proceed.
        let id = "inst-maint-12";
        let checked_under_claim = std::cell::Cell::new(None);
        let claim = claim_write_with(id, &|id| {
            checked_under_claim.set(Some(maintenance_is_active(id)));
            false
        })
        .expect("idle instance");
        assert_eq!(
            checked_under_claim.get(),
            Some(true),
            "the running/starting predicate must run, and run after the claim is taken"
        );
        drop(claim);
    }

    #[test]
    fn claim_write_admits_an_idle_instance_with_the_real_predicate() {
        // The unit-test binary launches nothing, so the real running/starting
        // predicate is false: the production entry point claims and releases.
        let id = "inst-maint-13";
        let claim = claim_write(id).expect("nothing runs in the test binary");
        assert!(maintenance_is_active(id));
        drop(claim);
        assert!(!maintenance_is_active(id));
    }

    #[test]
    fn shared_writers_coexist_and_only_the_last_release_reopens_the_claim() {
        // Two installs started from two Browse cards on one instance: both are
        // admitted, and a pack update stays refused until BOTH have finished —
        // releasing the first must not look like the instance is idle.
        let id = "inst-maint-14";
        let first = claim_shared_write(id).expect("the first item writer is admitted");
        let second = claim_shared_write(id).expect("a second item writer runs alongside");
        drop(first);
        assert!(
            maintenance_begin(id).is_none(),
            "one writer is still in flight — an exclusive claim must still be refused"
        );
        drop(second);
        let exclusive = maintenance_begin(id).expect("every item writer has finished");
        drop(exclusive);
    }

    #[test]
    fn a_shared_writer_is_refused_while_an_exclusive_claim_is_held() {
        // A toggle, an uninstall or a single install during a pack update, a mod
        // migration apply, a clone or a world migration.
        let id = "inst-maint-15";
        let migration = maintenance_begin(id).expect("the long operation claims first");
        assert!(matches!(claim_shared_write(id), Err(Error::InstanceBusy)));
        drop(migration);

        let pack_update = claim_write_with(id, &idle).expect("an idle instance is claimed");
        assert!(matches!(claim_shared_write(id), Err(Error::InstanceBusy)));
        drop(pack_update);

        let after = claim_shared_write(id).expect("the claim is gone, the writer is admitted");
        drop(after);
    }

    #[test]
    fn an_exclusive_claim_is_refused_while_a_shared_writer_is_in_flight() {
        // The half an entry-only check cannot give: an install admitted BEFORE the
        // pack update started must make that pack update refuse, and the refusal
        // must leave the install's slot exactly as it was.
        let id = "inst-maint-16";
        let install = claim_shared_write(id).expect("the item writer starts first");
        assert!(
            maintenance_begin(id).is_none(),
            "a world migration must be refused"
        );
        assert!(
            matches!(claim_write_with(id, &idle), Err(Error::InstanceBusy)),
            "a pack update, mod migration, clone or reimport must be refused"
        );
        assert!(
            maintenance_begin(id).is_none(),
            "the refusals above must not have released or corrupted the writer's slot"
        );
        drop(install);
        let claim = maintenance_begin(id).expect("the writer finished");
        drop(claim);
    }

    #[test]
    fn a_refused_shared_writer_leaves_nothing_behind() {
        // If the refusal still counted the writer, the instance would refuse
        // every pack update until the launcher restarts.
        let id = "inst-maint-17";
        let migration = maintenance_begin(id).expect("claimed");
        for _ in 0..3 {
            assert!(matches!(claim_shared_write(id), Err(Error::InstanceBusy)));
        }
        drop(migration);
        let claim = maintenance_begin(id).expect("three refusals left no shared count");
        drop(claim);
    }

    #[test]
    fn a_shared_writer_does_not_refuse_launch_or_short_writers() {
        // Deliberate (spec D3): installing or toggling a mod keeps Play, a world
        // backup and a datapack write available, exactly as before the item
        // writers consulted the claim. Launch and `write_allowed` read the
        // exclusive half only.
        let id = "inst-maint-18";
        let install = claim_shared_write(id).expect("admitted");
        assert!(
            !maintenance_is_active(id),
            "launch reads maintenance_is_active and must not see an item writer"
        );
        assert!(write_allowed(id).is_ok());
        drop(install);
    }

    #[test]
    fn a_panic_inside_a_shared_writer_releases_its_slot() {
        let id = "inst-maint-19";
        let result = std::panic::catch_unwind(|| {
            let _write = claim_shared_write(id).expect("admitted");
            panic!("the install panicked");
        });
        assert!(result.is_err());
        let claim = maintenance_begin(id).expect("the unwound writer released its slot");
        drop(claim);
    }

    #[test]
    fn shared_writers_on_one_instance_do_not_hold_another() {
        let install = claim_shared_write("inst-maint-20a").expect("admitted");
        let claim = maintenance_begin("inst-maint-20b").expect("a different instance is free");
        assert!(matches!(
            claim_shared_write("inst-maint-20b"),
            Err(Error::InstanceBusy)
        ));
        drop(claim);
        drop(install);
    }
    #[test]
    fn any_active_sees_an_exclusive_and_a_shared_claim() {
        let exclusive = maintenance_begin("any-active-x").expect("free id");
        assert!(any_active());
        drop(exclusive);
        let shared = claim_shared_write("any-active-s").expect("free id");
        assert!(any_active());
        drop(shared);
    }
}
