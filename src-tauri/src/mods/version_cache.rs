//! Process-wide TTL cache for `ModPlatform::versions()` results. Repeat
//! compatibility / update / dependency-graph sweeps read from here instead of
//! re-hitting the network. Keyed by (source, project_id, mc, loader);
//! invalidated by TTL only (a project's available versions don't change on
//! local install).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
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
}
