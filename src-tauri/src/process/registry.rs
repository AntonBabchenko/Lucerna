//! Generic keyed process registry shared by client launch and server runtime.
//!
//! Encapsulates the two concurrency lessons `servers_runtime` learned the hard
//! way:
//!  - **TOCTOU:** two rapid `start(id)` must not both pass the "is it running?"
//!    gate and spawn two processes — a `StartClaim` reserves the id across the
//!    spawn.
//!  - **ABA:** a force-kill + immediate restart can insert a NEW pid under the
//!    same id before the OLD exit watcher fires; removal must be pid-matched so
//!    the stale watcher never evicts the live entry.
//!
//! Lock order is fixed: `running` first, then `starting`.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

struct Entry<T> {
    pid: u32,
    data: T,
}

pub struct ProcessRegistry<T> {
    running: Mutex<HashMap<String, Entry<T>>>,
    starting: Mutex<HashSet<String>>,
}

/// RAII claim on an in-flight start. Held from the top of `start()` until the
/// running entry is inserted (or an early-return error). `Drop` releases the id.
pub struct StartClaim<'r, T> {
    registry: &'r ProcessRegistry<T>,
    id: String,
}

impl<'r, T> StartClaim<'r, T> {
    /// Release the claim now, removing the id from `starting`. Call after the
    /// running entry is inserted so the reservation isn't held for the rest of
    /// the caller's scope. (Equivalent to letting the claim drop.)
    pub fn commit(self) {}
}

impl<'r, T> Drop for StartClaim<'r, T> {
    fn drop(&mut self) {
        self.registry
            .starting
            .lock()
            .expect("registry starting set poisoned")
            .remove(&self.id);
    }
}

impl<T> ProcessRegistry<T> {
    pub fn new() -> Self {
        Self {
            running: Mutex::new(HashMap::new()),
            starting: Mutex::new(HashSet::new()),
        }
    }

    /// Reserve `id`. Returns `None` if the id is already running OR already
    /// starting (caller maps that to an "already running" error).
    pub fn claim_start(&self, id: &str) -> Option<StartClaim<'_, T>> {
        let running = self.running.lock().expect("registry running poisoned");
        if running.contains_key(id) {
            return None;
        }
        let mut starting = self.starting.lock().expect("registry starting poisoned");
        if !starting.insert(id.to_string()) {
            return None;
        }
        Some(StartClaim {
            registry: self,
            id: id.to_string(),
        })
    }

    /// Insert the live entry. Call `claim.commit()` after this.
    pub fn insert(&self, id: &str, pid: u32, data: T) {
        self.running
            .lock()
            .expect("registry running poisoned")
            .insert(id.to_string(), Entry { pid, data });
    }

    /// Remove the entry for `id` only if its pid still matches `pid`
    /// (ABA-safe). Returns true iff an entry was removed.
    pub fn remove_if_pid(&self, id: &str, pid: u32) -> bool {
        let mut running = self.running.lock().expect("registry running poisoned");
        if running.get(id).map(|e| e.pid) == Some(pid) {
            running.remove(id);
            true
        } else {
            false
        }
    }

    /// Unconditionally remove `id`, returning its pid. NOT ABA-safe: call only
    /// when you hold proof (e.g. a just-checked `pid_of`) that no intervening
    /// `claim_start` could have replaced the entry; otherwise use `remove_if_pid`.
    pub fn remove(&self, id: &str) -> Option<u32> {
        self.running
            .lock()
            .expect("registry running poisoned")
            .remove(id)
            .map(|e| e.pid)
    }

    pub fn is_running(&self, id: &str) -> bool {
        self.running
            .lock()
            .expect("registry running poisoned")
            .contains_key(id)
    }

    /// True iff `id` is mid-start: claimed by [`Self::claim_start`] but not
    /// yet inserted as running. `is_running` is false for the whole spawn
    /// pipeline (the `running` map is only populated after the JVM process
    /// exists), so a gate that must also refuse during that window checks
    /// both.
    pub fn is_starting(&self, id: &str) -> bool {
        self.starting
            .lock()
            .expect("registry starting poisoned")
            .contains(id)
    }

    pub fn is_any_running(&self) -> bool {
        !self
            .running
            .lock()
            .expect("registry running poisoned")
            .is_empty()
    }

    pub fn pid_of(&self, id: &str) -> Option<u32> {
        self.running
            .lock()
            .expect("registry running poisoned")
            .get(id)
            .map(|e| e.pid)
    }
}

impl<T: Clone> ProcessRegistry<T> {
    /// Snapshot of `(id, pid, data)` for every running entry. Owned so callers
    /// never hold the lock across a kill.
    pub fn snapshot(&self) -> Vec<(String, u32, T)> {
        self.running
            .lock()
            .expect("registry running poisoned")
            .iter()
            .map(|(id, e)| (id.clone(), e.pid, e.data.clone()))
            .collect()
    }
}

impl<T> Default for ProcessRegistry<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_is_exclusive_and_releases_on_drop() {
        let reg: ProcessRegistry<()> = ProcessRegistry::new();
        let c = reg.claim_start("a").expect("first claim succeeds");
        assert!(reg.claim_start("a").is_none(), "second claim rejected");
        drop(c);
        let c2 = reg.claim_start("a").expect("claim succeeds after release");
        drop(c2);
    }

    #[test]
    fn claim_blocked_while_running() {
        let reg: ProcessRegistry<()> = ProcessRegistry::new();
        let c = reg.claim_start("a").unwrap();
        reg.insert("a", 100, ());
        c.commit();
        assert!(
            reg.claim_start("a").is_none(),
            "running id cannot be reclaimed"
        );
        assert!(reg.is_running("a"));
        assert!(reg.is_any_running());
    }

    #[test]
    fn remove_if_pid_is_aba_safe() {
        let reg: ProcessRegistry<()> = ProcessRegistry::new();
        reg.insert("a", 100, ());
        // A stale watcher for the OLD pid must not evict a restarted entry.
        reg.remove("a");
        reg.insert("a", 200, ());
        assert!(!reg.remove_if_pid("a", 100), "stale pid must not remove");
        assert!(reg.is_running("a"), "live entry survives stale removal");
        assert!(reg.remove_if_pid("a", 200), "matching pid removes");
        assert!(!reg.is_running("a"));
        assert!(!reg.is_any_running());
    }

    #[test]
    fn snapshot_lists_all_running() {
        let reg: ProcessRegistry<u32> = ProcessRegistry::new();
        reg.insert("a", 1, 2048);
        reg.insert("b", 2, 4096);
        let mut snap = reg.snapshot();
        snap.sort_by_key(|(id, _, _)| id.clone());
        assert_eq!(snap, vec![("a".into(), 1, 2048), ("b".into(), 2, 4096)]);
    }
}
