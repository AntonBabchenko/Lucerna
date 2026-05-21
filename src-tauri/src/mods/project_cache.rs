//! Process-lifetime in-memory cache for `mods_project` results.
//!
//! `ModProject` metadata (name, icon, description) is near-static and a
//! single app session touches at most a few hundred distinct projects,
//! so the cache has no TTL and no eviction. See
//! `docs/superpowers/specs/2026-05-21-mod-list-caching-design.md`.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{LazyLock, Mutex};

use crate::mods::platform::{ModProject, ModSource};

static CACHE: LazyLock<Mutex<HashMap<(ModSource, String), ModProject>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Return the cached `ModProject` for `(source, project_id)`, or run
/// `fetch` once and cache its successful result. The lock is a
/// `std::sync::Mutex` and is never held across the `await`: a miss
/// releases it, awaits `fetch`, then re-locks to insert. A failed
/// `fetch` caches nothing, so a later call retries.
pub async fn get_or_fetch<F, Fut>(
    source: ModSource,
    project_id: &str,
    fetch: F,
) -> crate::error::Result<ModProject>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = crate::error::Result<ModProject>>,
{
    let key = (source, project_id.to_string());
    {
        let cache = CACHE.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(hit) = cache.get(&key) {
            return Ok(hit.clone());
        }
    }
    let project = fetch().await?;
    {
        let mut cache = CACHE.lock().unwrap_or_else(|p| p.into_inner());
        cache.insert(key, project.clone());
    }
    Ok(project)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::platform::ModSummary;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // CACHE is a process-global static shared by every test in this
    // binary. Each test uses a unique project_id so they never collide.
    fn sample_project(name: &str) -> ModProject {
        ModProject {
            summary: ModSummary {
                source: ModSource::Modrinth,
                project_id: "p".into(),
                slug: None,
                name: name.into(),
                summary: String::new(),
                icon_url: None,
                downloads: 0.0,
                author: String::new(),
                updated_at: None,
            },
            description: String::new(),
            website_url: None,
        }
    }

    #[tokio::test]
    async fn second_call_same_key_returns_cache_without_refetch() {
        let calls = AtomicUsize::new(0);
        let pid = "modlistcache-test-same-key";
        let r1 = get_or_fetch(ModSource::Modrinth, pid, || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(sample_project("first"))
        })
        .await
        .unwrap();
        let r2 = get_or_fetch(ModSource::Modrinth, pid, || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(sample_project("second-must-not-be-seen"))
        })
        .await
        .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(r1.summary.name, "first");
        assert_eq!(r2.summary.name, "first");
    }

    #[tokio::test]
    async fn distinct_keys_each_fetch() {
        let calls = AtomicUsize::new(0);
        let _ = get_or_fetch(ModSource::Modrinth, "modlistcache-test-key-A", || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(sample_project("A"))
        })
        .await
        .unwrap();
        let _ = get_or_fetch(ModSource::Modrinth, "modlistcache-test-key-B", || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(sample_project("B"))
        })
        .await
        .unwrap();
        // Same project_id, different source — distinct key.
        let _ = get_or_fetch(ModSource::Curseforge, "modlistcache-test-key-A", || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(sample_project("A-cf"))
        })
        .await
        .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn fetch_error_is_not_cached() {
        let calls = AtomicUsize::new(0);
        let pid = "modlistcache-test-error";
        let e1 = get_or_fetch(ModSource::Modrinth, pid, || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Err(crate::error::Error::ModsNotFound { platform: "modrinth".into() })
        })
        .await;
        assert!(e1.is_err());
        let r2 = get_or_fetch(ModSource::Modrinth, pid, || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(sample_project("recovered"))
        })
        .await
        .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(r2.summary.name, "recovered");
    }
}
