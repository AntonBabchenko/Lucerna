//! A one-entry memo over the instance-wide jar scan that instance search needs.
//!
//! `namespace_scan::instance_lang_maps` reads and inflates EVERY enabled jar. On
//! a 300-mod pack that is ~0.5-1 GB off disk and a few thousand zip entries —
//! several seconds. The search command called it once per query, so a person
//! typing with two hesitations longer than the debounce paid for it three
//! times over, concurrently.
//!
//! What makes memoising safe here is that the maps come from JARS ONLY: the
//! user's own overrides are loaded separately, per namespace, on every query.
//! So translating a string — the thing a user does constantly while searching —
//! cannot stale this cache. Only the mod set can, and that is exactly what the
//! fingerprint tracks.
//!
//! One entry rather than a map: a user searches inside one instance in one
//! language at a time, and holding several hundred-megabyte scans alive to
//! serve a switch nobody makes is a poor trade.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::l10n::scan::LangMap;

/// Namespace -> (English, the mod's own target-language file).
pub type LangMaps = BTreeMap<String, (LangMap, LangMap)>;

struct Memo {
    key: String,
    maps: Arc<LangMaps>,
}

static MEMO: Mutex<Option<Memo>> = Mutex::new(None);

/// Identity of a scan: which instance, which language, and which mods were
/// enabled with which contents.
///
/// The sha1s come from `mods::installed::list`, which reconciles them against
/// disk, so a jar swapped for a different build of the same filename changes
/// the fingerprint. Disabled mods are excluded because the scan excludes them —
/// toggling one must therefore invalidate, and it does, by changing the set.
fn fingerprint(inst_root: &Path, lang: &str, enabled: &[(String, String)]) -> String {
    let mut s = format!("{}|{lang}|", inst_root.display());
    for (filename, sha1) in enabled {
        s.push_str(filename);
        s.push(':');
        s.push_str(sha1);
        s.push(';');
    }
    s
}

/// The scan for this instance and language, reusing the previous one when
/// nothing that feeds it has changed.
///
/// `scan` is only called on a miss. It is passed in rather than called here so
/// this module stays free of the async jar walk and can be unit-tested with a
/// counter.
pub async fn lang_maps_cached<F, Fut, E>(
    inst_root: &Path,
    lang: &str,
    enabled: &[(String, String)],
    scan: F,
) -> Result<Arc<LangMaps>, E>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<LangMaps, E>>,
{
    let key = fingerprint(inst_root, lang, enabled);
    {
        // The lock is held only over the comparison, never across the scan:
        // holding it across an await would serialise two different instances'
        // searches behind each other for seconds at a time.
        let guard = MEMO.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(memo) = guard.as_ref() {
            if memo.key == key {
                return Ok(Arc::clone(&memo.maps));
            }
        }
    }

    let maps = Arc::new(scan().await?);
    let mut guard = MEMO.lock().unwrap_or_else(|e| e.into_inner());
    *guard = Some(Memo {
        key,
        maps: Arc::clone(&maps),
    });
    Ok(maps)
}

/// Drop the memo. Tests only — the production path invalidates by fingerprint.
#[cfg(test)]
fn reset() {
    *MEMO.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn maps_with(ns: &str) -> LangMaps {
        let mut m = BTreeMap::new();
        m.insert(ns.to_string(), (LangMap::new(), LangMap::new()));
        m
    }

    async fn call(
        root: &Path,
        lang: &str,
        enabled: &[(String, String)],
        hits: &AtomicU32,
        ns: &str,
    ) -> Arc<LangMaps> {
        lang_maps_cached(root, lang, enabled, || async {
            hits.fetch_add(1, Ordering::SeqCst);
            Ok::<_, ()>(maps_with(ns))
        })
        .await
        .expect("scan")
    }

    #[tokio::test]
    async fn a_repeated_query_does_not_rescan() {
        let _lock = crate::test_env_lock();
        reset();
        let root = Path::new("/inst");
        let mods = vec![("a.jar".to_string(), "sha".to_string())];
        let hits = AtomicU32::new(0);

        call(root, "ru_ru", &mods, &hits, "one").await;
        call(root, "ru_ru", &mods, &hits, "one").await;
        call(root, "ru_ru", &mods, &hits, "one").await;

        assert_eq!(hits.load(Ordering::SeqCst), 1, "three queries, one scan");
    }

    #[tokio::test]
    async fn a_changed_mod_set_invalidates() {
        let _lock = crate::test_env_lock();
        reset();
        let root = Path::new("/inst");
        let hits = AtomicU32::new(0);
        let before = vec![("a.jar".to_string(), "sha".to_string())];
        let after = vec![
            ("a.jar".to_string(), "sha".to_string()),
            ("b.jar".to_string(), "sha2".to_string()),
        ];

        call(root, "ru_ru", &before, &hits, "one").await;
        call(root, "ru_ru", &after, &hits, "two").await;

        assert_eq!(hits.load(Ordering::SeqCst), 2, "a new mod must rescan");
    }

    #[tokio::test]
    async fn a_rebuilt_jar_at_the_same_filename_invalidates() {
        // The case a filename-only fingerprint would miss: same name, new build.
        let _lock = crate::test_env_lock();
        reset();
        let root = Path::new("/inst");
        let hits = AtomicU32::new(0);

        call(
            root,
            "ru_ru",
            &[("a.jar".into(), "sha".into())],
            &hits,
            "one",
        )
        .await;
        call(
            root,
            "ru_ru",
            &[("a.jar".into(), "NEW".into())],
            &hits,
            "one",
        )
        .await;

        assert_eq!(hits.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn another_language_is_a_different_scan() {
        let _lock = crate::test_env_lock();
        reset();
        let root = Path::new("/inst");
        let mods = vec![("a.jar".to_string(), "sha".to_string())];
        let hits = AtomicU32::new(0);

        call(root, "ru_ru", &mods, &hits, "one").await;
        call(root, "de_de", &mods, &hits, "one").await;

        assert_eq!(hits.load(Ordering::SeqCst), 2);
    }
}
