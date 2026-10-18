//! The shared store, `<data_root>/shared/settings/`, and its one door.
//!
//! Every read and write of a shared file goes through a [`StoreLock`], the
//! shape of `servers_runtime::installed::SidecarLock`: `load` / `save` are
//! methods of the guard, so unlocked access does not compile, and the guard
//! is `!Send`, so holding it across an `.await` does not compile either.
//! Each change keeps the three versions before it in `backups/` — a safety
//! net for a bad capture (no restore UI yet).

use std::collections::HashMap;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, LazyLock, Mutex};
use std::thread::ThreadId;

use super::failure::{self, At};
use super::Kind;
use crate::error::Result;

/// How many earlier versions of each shared file are kept.
pub const BACKUPS: u32 = 3;

/// `<data_root>/shared/settings`.
pub fn store_dir(data_root: &Path) -> PathBuf {
    data_root.join("shared").join("settings")
}

static HELD: LazyLock<Mutex<HashMap<PathBuf, ThreadId>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static FREED: Condvar = Condvar::new();

/// Exclusive use of the shared store.
#[must_use = "the store is held only while this guard lives — bind it to a named local"]
pub struct StoreLock {
    dir: PathBuf,
    key: PathBuf,
    _not_send: PhantomData<*const ()>,
}

/// Wait for exclusive use of `data_root`'s shared store.
///
/// Std, not tokio: every holder is synchronous (a sync point runs on the
/// blocking pool or in the exit watcher's blocking step). Poison is recovered:
/// the map holds a lookup, an insert and a remove, which cannot leave it
/// half-updated. The wait loop re-checks after every wake-up.
pub fn lock(data_root: &Path) -> StoreLock {
    let dir = store_dir(data_root);
    let key: PathBuf = dir.components().collect();
    let me = std::thread::current().id();
    let mut held = HELD.lock().unwrap_or_else(|p| p.into_inner());
    loop {
        let Some(owner) = held.get(&key).copied() else {
            break;
        };
        // Waiting on yourself never ends: a debug build fails instead.
        if cfg!(debug_assertions) && owner == me {
            drop(held);
            panic!(
                "re-entrant lock of {}: this thread already holds it",
                key.display()
            );
        }
        held = FREED.wait(held).unwrap_or_else(|p| p.into_inner());
    }
    held.insert(key.clone(), me);
    StoreLock {
        dir,
        key,
        _not_send: PhantomData,
    }
}

impl Drop for StoreLock {
    fn drop(&mut self) {
        HELD.lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&self.key);
        FREED.notify_all();
    }
}

impl StoreLock {
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn file(&self, kind: Kind) -> PathBuf {
        self.dir.join(kind.store_file())
    }

    /// The shared file's bytes; `None` when there is no shared copy yet.
    pub fn load(&self, kind: Kind) -> Result<Option<Vec<u8>>> {
        super::fsio::read_if_present(At::shared(kind), &self.file(kind))
    }

    /// Replace the shared file. Nothing happens when `bytes` equal what is
    /// there; otherwise the old versions rotate into `backups/` first, and a
    /// rotation that fails fails the save with the current file untouched.
    pub fn save(&self, kind: Kind, bytes: &[u8]) -> Result<()> {
        let current = self.load(kind)?;
        if current.as_deref() == Some(bytes) {
            return Ok(());
        }
        if let Some(current) = current {
            self.rotate(kind, &current)?;
        }
        super::fsio::write_atomic(At::shared(kind), &self.file(kind), bytes)
    }

    /// `.2 → .3`, `.1 → .2`, then the current version → `.1`.
    fn rotate(&self, kind: Kind, current: &[u8]) -> Result<()> {
        let backups = self.dir.join("backups");
        let at = |n: u32| backups.join(format!("{}.{n}", kind.store_file()));
        for n in (1..BACKUPS).rev() {
            match std::fs::rename(at(n), at(n + 1)) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(failure::io(At::shared(kind), &at(n), e)),
            }
        }
        super::fsio::write_atomic(At::shared(kind), &at(1), current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backup(root: &Path, n: u32) -> Option<Vec<u8>> {
        std::fs::read(
            store_dir(root)
                .join("backups")
                .join(format!("servers.dat.{n}")),
        )
        .ok()
    }

    #[test]
    fn nothing_shared_yet_is_none() {
        let td = tempfile::tempdir().unwrap();
        let store = lock(td.path());
        assert_eq!(store.load(Kind::Servers).unwrap(), None);
    }

    #[test]
    fn a_save_reads_back() {
        let td = tempfile::tempdir().unwrap();
        let store = lock(td.path());
        store.save(Kind::History, b"/time set day").unwrap();
        assert_eq!(
            store.load(Kind::History).unwrap().as_deref(),
            Some(&b"/time set day"[..])
        );
    }

    #[test]
    fn three_earlier_versions_are_kept() {
        let td = tempfile::tempdir().unwrap();
        let store = lock(td.path());
        for v in ["a", "b", "c", "d", "e"] {
            store.save(Kind::Servers, v.as_bytes()).unwrap();
        }
        assert_eq!(
            store.load(Kind::Servers).unwrap().as_deref(),
            Some(&b"e"[..])
        );
        assert_eq!(backup(td.path(), 1).as_deref(), Some(&b"d"[..]));
        assert_eq!(backup(td.path(), 2).as_deref(), Some(&b"c"[..]));
        assert_eq!(backup(td.path(), 3).as_deref(), Some(&b"b"[..]));
        assert_eq!(backup(td.path(), 4), None);
    }

    #[test]
    fn an_unchanged_save_rotates_nothing() {
        let td = tempfile::tempdir().unwrap();
        let store = lock(td.path());
        store.save(Kind::Servers, b"a").unwrap();
        store.save(Kind::Servers, b"b").unwrap();
        store.save(Kind::Servers, b"b").unwrap();
        assert_eq!(backup(td.path(), 1).as_deref(), Some(&b"a"[..]));
        assert_eq!(backup(td.path(), 2), None);
    }

    #[test]
    fn a_second_holder_waits_for_the_first() {
        let td = tempfile::tempdir().unwrap();
        let root = td.path().to_path_buf();
        let first = lock(&root);
        let (tx, rx) = std::sync::mpsc::channel();
        let r = root.clone();
        let waiter = std::thread::spawn(move || {
            let _second = lock(&r);
            tx.send(()).unwrap();
        });
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(rx.try_recv().is_err(), "the second lock waits");
        drop(first);
        rx.recv_timeout(std::time::Duration::from_secs(5))
            .expect("the second lock is granted once the first is dropped");
        waiter.join().unwrap();
    }

    #[test]
    #[should_panic(expected = "re-entrant")]
    fn locking_twice_on_one_thread_is_caught_in_debug() {
        let td = tempfile::tempdir().unwrap();
        let _a = lock(td.path());
        let _b = lock(td.path());
    }
}
