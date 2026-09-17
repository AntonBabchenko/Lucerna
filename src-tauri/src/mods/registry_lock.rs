//! One async mutex per registry FILE, held across each read-modify-write of it.
//!
//! `mods::installed` (`installed-mods.json`) and `mods::assets`
//! (`installed-assets.json`) rewrite their registry as read → mutate → write,
//! with `.await`s in between. Tauri runs commands concurrently: Browse installs
//! several mods into one instance at once, and ~21 commands reconcile through
//! `installed::list`. Without this lock two of them read the same snapshot and
//! the later rename erases the earlier change — an install's row is lost, and
//! the next reconcile re-adopts its jar as a manual mod with no platform
//! identity, no update tracking and no `requires` edges.
//!
//! The maintenance claim does not cover this: its SHARED form deliberately lets
//! per-item writers run together, and reconcile-on-read is ungated.
//!
//! Keyed by the registry FILE, not the instance: the two registries of one
//! instance never contend, and no function holds both. `datapacks::registry`
//! guards its own file with its own lock for the same defect.
//!
//! Rules for holders, pinned by `tests/structural_registry_rmw_lock.rs`:
//!
//! - Only the registry modules take it, as a named function-level binding.
//! - Inside, only the lock-free primitives run (`read_or_empty`, `migrate`,
//!   `reconcile`, `write`, `list_all`, `write_all`). Tokio's mutex is not
//!   re-entrant, so calling another locking registry function would wait on its
//!   own caller forever.
//! - Never across network I/O or a caller-supplied future.
//!
//! Lock order: this lock, then `installed`'s per-jar hash gate. Nothing holding
//! a hash gate calls into a registry, so the order cannot invert.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

use tokio::sync::OwnedMutexGuard;

/// Never pruned: one small `Arc` per registry file this process has touched,
/// bounded by the number of instances — the same growth, and the same bound, as
/// `installed`'s `HASH_GATES`.
static LOCKS: LazyLock<Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// The map key for `path`. `components()` folds the spellings that name one file
/// lexically — `/` vs `\` on Windows, a doubled separator, a trailing separator,
/// an interior `.` — so they share one mutex. It does not resolve case, symlinks
/// or a `\\?\` prefix; every production caller builds the instance root through
/// `paths::instance_dir`, so those spellings never diverge in practice.
fn key(path: &Path) -> PathBuf {
    path.components().collect()
}

/// This file's mutex, created on first use.
fn gate(path: &Path) -> Arc<tokio::sync::Mutex<()>> {
    // A poisoned map is still a valid map: the only code that runs under this
    // std lock is an entry lookup and an `Arc` clone, which cannot leave it
    // half-updated. The std lock is never held across an await.
    let mut locks = LOCKS.lock().unwrap_or_else(|p| p.into_inner());
    Arc::clone(
        locks
            .entry(key(path))
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
    )
}

/// Wait for exclusive use of `registry_file`. The returned guard owns its mutex,
/// so the future stays `Send` and the guard can outlive this call; bind it to a
/// NAMED local — `let _ =` would release it immediately.
pub(crate) async fn lock(registry_file: &Path) -> OwnedMutexGuard<()> {
    gate(registry_file).lock_owned().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn spellings_of_one_path_share_one_mutex() {
        let plain = gate(Path::new(
            "registry-lock-test/inst/lucerna/installed-mods.json",
        ));
        for spelling in [
            "registry-lock-test/inst/lucerna/installed-mods.json/",
            "registry-lock-test//inst/lucerna/installed-mods.json",
            "registry-lock-test/inst/./lucerna/installed-mods.json",
        ] {
            assert!(
                Arc::ptr_eq(&plain, &gate(Path::new(spelling))),
                "{spelling} must map to the same mutex"
            );
        }
        let other = gate(Path::new("registry-lock-test/inst/installed-assets.json"));
        assert!(!Arc::ptr_eq(&plain, &other));
    }

    #[tokio::test]
    async fn a_held_lock_blocks_its_own_file_and_no_other() {
        let mods = Path::new("registry-lock-test/held/lucerna/installed-mods.json");
        let assets = Path::new("registry-lock-test/held/installed-assets.json");

        let held = lock(mods).await;
        assert!(
            tokio::time::timeout(Duration::from_millis(50), lock(mods))
                .await
                .is_err(),
            "a second holder of the same file must wait"
        );
        assert!(
            tokio::time::timeout(Duration::from_secs(5), lock(assets))
                .await
                .is_ok(),
            "another registry file must not wait on this one"
        );

        drop(held);
        assert!(
            tokio::time::timeout(Duration::from_secs(5), lock(mods))
                .await
                .is_ok(),
            "released, the file's lock is available again"
        );
    }
}
