//! In-process test seams replacing the `LUCERNA_*` env-var overrides the
//! test suite used to redirect production URLs and the network allowlist at
//! wiremock servers.
//!
//! ## Why this exists
//!
//! Installing those overrides with `std::env::set_var` mutates the process
//! `environ` array. Under cargo's parallel test threads on glibc, a
//! `setenv`/`unsetenv` can `realloc` `environ` while another thread is inside
//! `getenv` (e.g. `tempfile` reading `TMPDIR`, or tokio's runtime) — a data
//! race that reads freed memory and can corrupt arbitrary heap. That is the
//! root cause of the `rust (ubuntu)` flake where a fully self-contained,
//! no-network test (`mods::modpack::overrides::tests::extracts_normal_file`)
//! saw a corrupted byte buffer: it was an innocent bystander whose heap was
//! clobbered by a sibling test's `set_var`. `std::env::set_var` is `unsafe`
//! as of the 2024 edition for exactly this reason. See the post-mortem in
//! `docs/TESTING.md`.
//!
//! ## How it works
//!
//! Tests call [`scope`] with the overrides they need; it returns an RAII
//! guard that installs them in a process-local table and reverts on drop. The
//! guard holds a process-wide lock, so concurrent test scopes are serialized
//! and a test's overrides stay live for its whole body (closing the
//! reader-vs-mutator hole the old `test_env_lock()` left open).
//!
//! Production reads go through [`resolve`], keyed by the same env-var name the
//! seam replaces. When a scope is active it returns the scope value; otherwise
//! it falls back to the original `std::env::var`, so production behavior —
//! including any operator-set env override — is unchanged. A relaxed atomic
//! short-circuits the table lookup, so the production path adds nothing but
//! one atomic load before the existing `env::var` call it already performed.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, RwLock};

/// Active-scope flag. `false` in production → [`resolve`] skips the table and
/// behaves exactly like the old `env::var` read.
static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Installed overrides, keyed by the env-var name they replace. A short linear
/// scan (≤ ~14 keys) beats a map here and is `const`-constructible, so no
/// `OnceLock`/`Lazy` and no new dependency.
static SEAMS: RwLock<Vec<(&'static str, String)>> = RwLock::new(Vec::new());

/// Serializes test scopes — and, via [`crate::test_env_lock`], every other
/// test that touches a process-global (e.g. the in-memory CurseForge keyring's
/// `TEST_KEY`). One mutex = one serialization domain, exactly as before the env
/// vars moved in-process, so a global set under this lock can't be cleared by a
/// concurrent test. Held for the lifetime of the returned [`SeamScope`].
static SCOPE_LOCK: Mutex<()> = Mutex::new(());

/// Acquire the shared process-wide test serialization lock directly, without
/// installing any override. Backs `crate::test_env_lock()` so seam scopes and
/// non-env global-state tests (keyring, etc.) serialize on the *same* mutex.
#[cfg(test)]
pub(crate) fn serialization_lock() -> MutexGuard<'static, ()> {
    SCOPE_LOCK.lock().unwrap_or_else(|p| p.into_inner())
}

/// Resolve a test-seam override by the env-var name it replaces. Returns the
/// active scope's value, else the process env var (so an operator-set override
/// still works in production), else `None`.
pub(crate) fn resolve(key: &'static str) -> Option<String> {
    if ACTIVE.load(Ordering::Acquire) {
        let seams = SEAMS.read().unwrap_or_else(|p| p.into_inner());
        if let Some((_, value)) = seams.iter().find(|(k, _)| *k == key) {
            return Some(value.clone());
        }
    }
    std::env::var(key).ok()
}

/// RAII guard returned by [`scope`]; reverts the installed overrides on drop.
#[must_use = "the overrides revert as soon as this guard is dropped — bind it to a named variable for the test body"]
pub struct SeamScope {
    _lock: MutexGuard<'static, ()>,
}

impl Drop for SeamScope {
    fn drop(&mut self) {
        // Clear the flag before the table so a reader that observes the flag
        // still races at worst to an empty table (→ env fallback), never to a
        // freed allocation. The lock releases last, after both are reset.
        ACTIVE.store(false, Ordering::Release);
        SEAMS.write().unwrap_or_else(|p| p.into_inner()).clear();
    }
}

/// Install the given `(env-var name, value)` overrides for the lifetime of the
/// returned guard. Test-only. Serialized process-wide: a second caller blocks
/// until the prior scope's guard is dropped, so each test's overrides stay
/// live (and uncontended) for its whole body.
///
/// Bind the guard to a named variable — `let _seam = …;`, never `let _ = …;` —
/// or the overrides revert immediately.
///
/// ```ignore
/// let _seam = crate::test_seam::scope(&[
///     ("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost"),
///     ("LUCERNA_MC_LOGIN_URL_OVERRIDE", &server.uri()),
/// ]);
/// ```
pub fn scope(overrides: &[(&'static str, &str)]) -> SeamScope {
    let lock = SCOPE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    {
        let mut seams = SEAMS.write().unwrap_or_else(|p| p.into_inner());
        seams.clear();
        seams.extend(
            overrides
                .iter()
                .map(|(key, value)| (*key, (*value).to_string())),
        );
    }
    ACTIVE.store(true, Ordering::Release);
    SeamScope { _lock: lock }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Each test uses a UNIQUE key set by no other test, so reads taken outside
    // the test's own scope are race-free even though `ACTIVE`/`SEAMS` are
    // process-global and these self-tests run on parallel threads: a concurrent
    // scope holding a different key resolves *this* key to None via the env
    // fallback. (Reusing one key across both tests would itself be a flake.)

    #[test]
    fn resolve_returns_scope_value_then_reverts() {
        const K: &str = "LUCERNA_TEST_SEAM_SELFTEST_REVERTS";
        // No scope holds K → falls back to env (unset) → None.
        assert_eq!(resolve(K), None);
        {
            // While we hold the guard, SCOPE_LOCK keeps any other scope out, so
            // the table is exactly our K.
            let _seam = scope(&[(K, "seam-value")]);
            assert_eq!(resolve(K).as_deref(), Some("seam-value"));
        }
        // Guard dropped → table cleared, flag down → back to env fallback.
        assert_eq!(resolve(K), None);
    }

    #[test]
    fn unknown_key_falls_back_when_scope_active() {
        const PRESENT: &str = "LUCERNA_TEST_SEAM_SELFTEST_PRESENT";
        let _seam = scope(&[(PRESENT, "x")]);
        // A key not installed in the active scope falls through to env::var,
        // which is unset here → None (not a panic, not the other key's value).
        assert_eq!(resolve("LUCERNA_TEST_SEAM_SELFTEST_ABSENT"), None);
    }
}
