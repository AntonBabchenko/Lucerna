//! Session cache of platform version objects by id — what the dependency graph
//! reads for each installed mod (the dependencies and loaders its installed
//! version declares).
//!
//! A version object fetched by its id is effectively immutable: a platform
//! publishes a new version rather than rewrite a released one's dependencies.
//! So the graph asks the platform only for the ids this cache misses. A toggle
//! burst after one good load then costs no network, and an offline reload after
//! an earlier good load in the same session is complete.
//!
//! In memory only, for the process lifetime; no disk writes. Bounded: past
//! [`MAX_ENTRIES`] it starts over. Only answers are kept — a failed lookup
//! caches nothing, so the next build asks again.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, MutexGuard, PoisonError};

use crate::mods::platform::{ModSource, ModVersion};

/// Far more versions than any one instance holds, and small in memory.
const MAX_ENTRIES: usize = 4096;

type Entries = HashMap<(ModSource, String), ModVersion>;

/// Version objects keyed by the source asked and the id they answer to.
#[derive(Default)]
pub struct VersionByIdCache {
    entries: Mutex<Entries>,
}

/// The cache the launcher uses for the whole session.
pub static SESSION: LazyLock<VersionByIdCache> = LazyLock::new(VersionByIdCache::default);

impl VersionByIdCache {
    /// The cached versions among `ids`, and the ids it does not hold (both in
    /// the given order).
    pub fn lookup(&self, source: ModSource, ids: &[String]) -> (Vec<ModVersion>, Vec<String>) {
        let entries = self.entries();
        let mut hits = Vec::new();
        let mut misses = Vec::new();
        for id in ids {
            match entries.get(&(source, id.clone())) {
                Some(v) => hits.push(v.clone()),
                None => misses.push(id.clone()),
            }
        }
        (hits, misses)
    }

    /// Keep `versions`, each under the id it answers to. Past the bound the
    /// cache starts over with this batch, so it never grows without limit and
    /// the newest answers survive.
    pub fn remember(&self, source: ModSource, versions: &[ModVersion]) {
        let mut entries = self.entries();
        if entries.len() + versions.len() > MAX_ENTRIES {
            entries.clear();
        }
        for v in versions {
            entries.insert((source, v.version_id.clone()), v.clone());
        }
    }

    /// The map, even when a panic elsewhere poisoned the lock: each entry is a
    /// whole version inserted in one step, so whatever it holds is still true.
    fn entries(&self) -> MutexGuard<'_, Entries> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::platform::{LoaderKind, ModFile};

    fn version(id: &str) -> ModVersion {
        ModVersion {
            source: ModSource::Modrinth,
            project_id: format!("p-{id}"),
            version_id: id.into(),
            name: id.into(),
            version_number: "1.0".into(),
            mc_versions: vec!["1.21.1".into()],
            loaders: vec![LoaderKind::NeoForge],
            primary_file: ModFile {
                filename: format!("{id}.jar"),
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

    fn ids(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn answers_what_it_holds_and_names_what_it_misses_per_source() {
        let cache = VersionByIdCache::default();
        cache.remember(ModSource::Modrinth, &[version("a"), version("b")]);

        let (hits, misses) = cache.lookup(ModSource::Modrinth, &ids(&["a", "c", "b"]));
        let hit_ids: Vec<&str> = hits.iter().map(|v| v.version_id.as_str()).collect();
        assert_eq!(hit_ids, ["a", "b"]);
        assert_eq!(misses, ["c"]);

        // The same id asked of the other platform is another version.
        let (hits, misses) = cache.lookup(ModSource::Curseforge, &ids(&["a"]));
        assert!(hits.is_empty());
        assert_eq!(misses, ["a"]);
    }

    #[test]
    fn past_its_bound_it_starts_over_keeping_the_new_batch() {
        let cache = VersionByIdCache::default();
        let full: Vec<ModVersion> = (0..MAX_ENTRIES)
            .map(|i| version(&format!("v{i}")))
            .collect();
        cache.remember(ModSource::Modrinth, &full);
        assert_eq!(
            cache.len(),
            MAX_ENTRIES,
            "up to the bound everything is kept"
        );

        cache.remember(ModSource::Modrinth, &[version("next")]);
        assert_eq!(cache.len(), 1, "one more starts it over");
        let (hits, misses) = cache.lookup(ModSource::Modrinth, &ids(&["next", "v0"]));
        assert_eq!(hits.len(), 1, "the new batch survives");
        assert_eq!(misses, ["v0"]);
    }
}
