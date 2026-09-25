//! Process-wide TTL cache for `ModPlatform::versions()` results. Repeat
//! compatibility / update / dependency-graph sweeps read from here instead of
//! re-hitting the network. Keyed by (source, project_id, mc, loader);
//! invalidated by TTL only (a project's available versions don't change on
//! local install). Concurrent fetches of one key are joined (`get_or_fetch`).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::time::{Duration, Instant};

use crate::mods::platform::{LoaderKind, ModSource, ModVersion};

const TTL: Duration = Duration::from_secs(5 * 60);

type Key = (ModSource, String, String, LoaderKind);

struct Entry {
    versions: Vec<ModVersion>,
    fetched_at: Instant,
}

fn store() -> &'static Mutex<HashMap<Key, Entry>> {
    static S: OnceLock<Mutex<HashMap<Key, Entry>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

fn key(source: ModSource, project_id: &str, mc: &str, loader: LoaderKind) -> Key {
    (source, project_id.to_string(), mc.to_string(), loader)
}

/// One fetch gate per key: concurrent `get_or_fetch` callers of a key queue on
/// it. Grows with the distinct projects of a session (a few hundred) and is
/// never evicted, like `project_cache`.
fn gates() -> &'static Mutex<HashMap<Key, Arc<tokio::sync::Mutex<()>>>> {
    static G: OnceLock<Mutex<HashMap<Key, Arc<tokio::sync::Mutex<()>>>>> = OnceLock::new();
    G.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Cached lookup. Returns `Some(versions)` when a fresh entry exists.
pub fn get(
    source: ModSource,
    project_id: &str,
    mc: &str,
    loader: LoaderKind,
) -> Option<Vec<ModVersion>> {
    let s = store().lock().expect("version cache mutex poisoned");
    s.get(&key(source, project_id, mc, loader)).and_then(|e| {
        if Instant::now().saturating_duration_since(e.fetched_at) < TTL {
            Some(e.versions.clone())
        } else {
            None
        }
    })
}

/// Record a fresh result.
pub fn put(
    source: ModSource,
    project_id: &str,
    mc: &str,
    loader: LoaderKind,
    versions: Vec<ModVersion>,
) {
    store()
        .lock()
        .expect("version cache mutex poisoned")
        .insert(
            key(source, project_id, mc, loader),
            Entry {
                versions,
                fetched_at: Instant::now(),
            },
        );
}

/// Cached lookup that fetches on a miss. Concurrent callers of one key share
/// one fetch: the second waits for the first and then reads the cache
/// (2026-09-21 spec, D8). A failed fetch caches nothing; a waiter behind a
/// failed leader fetches for itself.
pub async fn get_or_fetch<F, Fut>(
    source: ModSource,
    project_id: &str,
    mc: &str,
    loader: LoaderKind,
    fetch: F,
) -> crate::error::Result<Vec<ModVersion>>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = crate::error::Result<Vec<ModVersion>>>,
{
    if let Some(hit) = get(source, project_id, mc, loader) {
        return Ok(hit);
    }
    let gate = gates()
        .lock()
        .expect("version cache gate mutex poisoned")
        .entry(key(source, project_id, mc, loader))
        .or_default()
        .clone();
    let _held = gate.lock().await;
    // A caller that held the gate before us may have filled the entry. If it
    // failed, nothing was cached and this caller fetches for itself — one
    // waiter at a time, so a failing host is asked in sequence, never at once.
    if let Some(hit) = get(source, project_id, mc, loader) {
        return Ok(hit);
    }
    let v = fetch().await?;
    put(source, project_id, mc, loader, v.clone());
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::platform::ModFile;

    fn mv(id: &str) -> ModVersion {
        ModVersion {
            source: ModSource::Modrinth,
            project_id: id.into(),
            version_id: "v".into(),
            name: id.into(),
            version_number: "1".into(),
            mc_versions: vec!["1.21.1".into()],
            loaders: vec![LoaderKind::NeoForge],
            primary_file: ModFile {
                filename: "f.jar".into(),
                url: "u".into(),
                sha1: Some("a".into()),
                size: 1.0,
                distribution_allowed: true,
                sha256: None,
            },
            deps: vec![],
            published_at: None,
        }
    }

    #[tokio::test(start_paused = true)]
    async fn hit_within_ttl_then_miss_after() {
        put(
            ModSource::Modrinth,
            "balm",
            "1.21.1",
            LoaderKind::NeoForge,
            vec![mv("balm")],
        );
        assert!(get(ModSource::Modrinth, "balm", "1.21.1", LoaderKind::NeoForge).is_some());
        // Different key = miss.
        assert!(get(ModSource::Modrinth, "balm", "1.20.4", LoaderKind::NeoForge).is_none());
        // After TTL = miss.
        tokio::time::advance(Duration::from_secs(5 * 60 + 1)).await;
        assert!(get(ModSource::Modrinth, "balm", "1.21.1", LoaderKind::NeoForge).is_none());
    }

    use std::sync::atomic::{AtomicUsize, Ordering};

    fn offline() -> crate::error::Error {
        crate::error::Error::ModsNetwork {
            url: "u".into(),
            details: "offline".into(),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn concurrent_callers_of_one_key_share_one_fetch() {
        let calls = AtomicUsize::new(0);
        let fetch = || async {
            calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(50)).await;
            Ok(vec![mv("vc-coalesce")])
        };
        let (a, b) = tokio::join!(
            get_or_fetch(
                ModSource::Modrinth,
                "vc-coalesce",
                "1.21.1",
                LoaderKind::NeoForge,
                fetch
            ),
            get_or_fetch(
                ModSource::Modrinth,
                "vc-coalesce",
                "1.21.1",
                LoaderKind::NeoForge,
                fetch
            ),
        );
        assert!(a.is_ok() && b.is_ok());
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "the second caller must join the first fetch"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_fetch_is_not_cached_and_the_next_caller_asks_again() {
        // (pin under the stub)
        let calls = AtomicUsize::new(0);
        let first = get_or_fetch(
            ModSource::Modrinth,
            "vc-fail-once",
            "1.21.1",
            LoaderKind::NeoForge,
            || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Err(offline())
            },
        )
        .await;
        assert!(first.is_err());
        assert!(get(
            ModSource::Modrinth,
            "vc-fail-once",
            "1.21.1",
            LoaderKind::NeoForge
        )
        .is_none());
        let second = get_or_fetch(
            ModSource::Modrinth,
            "vc-fail-once",
            "1.21.1",
            LoaderKind::NeoForge,
            || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(vec![mv("vc-fail-once")])
            },
        )
        .await;
        assert!(second.is_ok());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn when_the_leader_fails_a_waiter_fetches_for_itself() {
        // (pin under the stub)
        let calls = AtomicUsize::new(0);
        let failing = || async {
            calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(50)).await;
            Err(offline())
        };
        let working = || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(vec![mv("vc-leader-fails")])
        };
        let (a, b) = tokio::join!(
            get_or_fetch(
                ModSource::Modrinth,
                "vc-leader-fails",
                "1.21.1",
                LoaderKind::NeoForge,
                failing
            ),
            get_or_fetch(
                ModSource::Modrinth,
                "vc-leader-fails",
                "1.21.1",
                LoaderKind::NeoForge,
                working
            ),
        );
        assert!(a.is_err());
        assert!(b.is_ok(), "a failure must not be handed to the waiter");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}
