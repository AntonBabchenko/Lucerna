//! Disk-backed, TTL'd cache of mod project *summaries* (name, slug, icon,
//! author).
//!
//! Both the installed-mods list and the dependency graph need a `ModSummary`
//! for every platform-identified mod. Fetching them one project at a time is a
//! 429 rate-limit source on large instances. This cache collapses that into a
//! few batched `ModPlatform::summaries` requests and remembers the results
//! across launcher restarts, so a warm open does zero network.
//!
//! Project metadata is global (not per-instance), so the file lives once under
//! the app dir. The cache is platform-agnostic: callers inject the batch
//! fetcher (the command layer wires in `platform_for(source).summaries`), which
//! keeps this module free of the `commands`/Tauri surface and unit-testable.
//!
//! ## Adding a field to `ModSummary` — the migration convention
//!
//! `StoredEntry` persists `ModSummary` verbatim, and [`load`] parses the whole
//! file with `unwrap_or_default()`. So a *required* new field silently discards
//! every existing entry — a full re-fetch storm, and on a build with no
//! CurseForge key those CF summaries can never be re-fetched, permanently
//! degrading their display names to raw numeric ids with no recovery path
//! (the Settings "clear cache" button targets a different directory).
//!
//! Therefore: make the field `Option` + `#[serde(default)]`, and give the one
//! caller that needs it a `require_*` flag that marks a `None` entry stale.
//! Migration then happens per entry, inside a batch that caller already issues,
//! and can never wipe the file. `require_loaders` is the worked example.

use std::collections::HashMap;
use std::future::Future;
use std::path::Path;
use std::sync::Mutex;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::mods::platform::{supplies_project_loaders, ModSource, ModSummary};

/// Serializes the disk read-modify-write across concurrent `get_many` calls
/// (the installed list and the dependency graph fetch around the same time).
/// Held only over the synchronous load/save — never across the network fetch.
static DISK_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredEntry {
    /// RFC 3339 UTC timestamp of when `summary` was fetched.
    fetched_at: String,
    summary: ModSummary,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct CacheFile {
    #[serde(default)]
    entries: Vec<StoredEntry>,
}

fn key(s: &ModSummary) -> (ModSource, String) {
    (s.source, s.project_id.clone())
}

/// Whether a cached entry is still fresh. `ttl_days == 0` means "never expire";
/// an unparseable timestamp is treated as stale (re-fetch).
fn is_fresh(fetched_at: &str, now: DateTime<Utc>, ttl_days: u32) -> bool {
    if ttl_days == 0 {
        return true;
    }
    match DateTime::parse_from_rfc3339(fetched_at) {
        Ok(t) => now.signed_duration_since(t.with_timezone(&Utc)) < Duration::days(ttl_days as i64),
        Err(_) => false,
    }
}

/// Load the cache file into a keyed map. A missing or malformed file is treated
/// as an empty cache (cosmetic data — never an error).
fn load(path: &Path) -> HashMap<(ModSource, String), StoredEntry> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };
    let file: CacheFile = serde_json::from_str(&raw).unwrap_or_default();
    file.entries
        .into_iter()
        .map(|e| (key(&e.summary), e))
        .collect()
}

/// Persist the map atomically (`.tmp` + rename), creating the parent dir.
fn save(path: &Path, map: &HashMap<(ModSource, String), StoredEntry>) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut entries: Vec<StoredEntry> = map.values().cloned().collect();
    // Stable order so the on-disk file diffs cleanly across rewrites.
    entries.sort_by(|a, b| {
        (a.summary.source as u8, &a.summary.project_id)
            .cmp(&(b.summary.source as u8, &b.summary.project_id))
    });
    let json = serde_json::to_string_pretty(&CacheFile { entries })?;
    // Per-process temp name so two launcher instances never race on the rename.
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}

/// Return a `ModSummary` for every requested `(source, id)` that resolves,
/// serving fresh entries from disk and batch-fetching the missing/stale set via
/// `fetch`. Newly fetched summaries are persisted. Ids the platform cannot
/// resolve (404 / unknown) are simply absent from the result — the caller
/// degrades that row. A whole-batch fetch failure degrades to cache-only
/// (logged, never propagated): mod metadata is cosmetic and must not break the
/// list or the graph.
///
/// `require_loaders` additionally treats an entry whose `loaders` is `None` as
/// stale, but only for sources that can actually report them. This is the
/// migration path for the field: entries written before it existed deserialize
/// with `None`, and `None` never suppresses a dependency row, so without this
/// the dependency-graph fix would appear dead until the TTL expired — or
/// forever at `ttl_days == 0`, which is a supported setting. Only the
/// dependency graph passes `true`; every other caller passes `false` and
/// re-fetches nothing.
/// Cache-only lookup: whatever is already on disk for `(source, id)`, nothing
/// else. Ids with no entry are simply absent from the result.
///
/// Takes **no fetch closure** on purpose, rather than reusing [`get_many`] with
/// a closure the caller promises never to invoke. The display-name backfill
/// must be provably offline, and an accessor that *cannot* reach the network is
/// something a reviewer can verify from the signature alone.
///
/// TTL is ignored, also on purpose: a project title does not rot, and a
/// best-effort repair must never trigger the re-fetch that staleness implies.
/// Refreshing entries stays the job of the callers that already do it.
pub fn get_many_cached(path: &Path, source: ModSource, ids: &[String]) -> Vec<ModSummary> {
    if ids.is_empty() {
        return Vec::new();
    }
    let _g = DISK_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let map = load(path);
    ids.iter()
        .filter_map(|id| map.get(&(source, id.clone())).map(|e| e.summary.clone()))
        .collect()
}

pub async fn get_many<F, Fut>(
    path: &Path,
    source: ModSource,
    ids: &[String],
    ttl_days: u32,
    require_loaders: bool,
    fetch: F,
) -> Vec<ModSummary>
where
    F: FnOnce(Vec<String>) -> Fut,
    Fut: Future<Output = crate::error::Result<Vec<ModSummary>>>,
{
    get_many_inner(
        path,
        source,
        ids,
        ttl_days,
        require_loaders,
        Utc::now(),
        fetch,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn get_many_inner<F, Fut>(
    path: &Path,
    source: ModSource,
    ids: &[String],
    ttl_days: u32,
    require_loaders: bool,
    now: DateTime<Utc>,
    fetch: F,
) -> Vec<ModSummary>
where
    F: FnOnce(Vec<String>) -> Fut,
    Fut: Future<Output = crate::error::Result<Vec<ModSummary>>>,
{
    if ids.is_empty() {
        return Vec::new();
    }
    // 1. Load + partition into fresh (already cached) / stale (missing or
    //    expired). Lock scoped so it is released before the network fetch.
    let stale: Vec<String> = {
        let _g = DISK_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let map = load(path);
        // The loaders clause is OR'd with `is_fresh`, never nested inside it —
        // `ttl_days == 0` short-circuits `is_fresh` to always-fresh, and a
        // pre-migration entry must still be re-fetched under that setting.
        let needs_loaders = require_loaders && supplies_project_loaders(source);
        ids.iter()
            .filter(|id| match map.get(&(source, (*id).clone())) {
                Some(e) => {
                    !is_fresh(&e.fetched_at, now, ttl_days)
                        || (needs_loaders && e.summary.loaders.is_none())
                }
                None => true,
            })
            .cloned()
            .collect()
    };

    // 2. Batch-fetch the stale set (no lock held across the await).
    let fetched: Vec<ModSummary> = if stale.is_empty() {
        Vec::new()
    } else {
        match fetch(stale).await {
            Ok(v) => v,
            Err(e) => {
                crate::diag!("[summary_cache] batch fetch failed for {source:?}: {e}");
                Vec::new()
            }
        }
    };

    // 3. Merge the fetched summaries into the file (re-read under lock in case
    //    a concurrent call wrote meanwhile), then build the result.
    let _g = DISK_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut map = load(path);
    let now_str = now.to_rfc3339();
    for s in fetched {
        map.insert(
            key(&s),
            StoredEntry {
                fetched_at: now_str.clone(),
                summary: s,
            },
        );
    }
    if let Err(e) = save(path, &map) {
        crate::diag!("[summary_cache] save failed ({}): {e}", path.display());
    }
    ids.iter()
        .filter_map(|id| map.get(&(source, id.clone())).map(|e| e.summary.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex as StdMutex};
    use tempfile::tempdir;

    fn summary(source: ModSource, id: &str) -> ModSummary {
        ModSummary {
            source,
            project_id: id.into(),
            slug: Some(id.into()),
            name: format!("Name {id}"),
            summary: String::new(),
            icon_url: None,
            downloads: 0.0,
            author: String::new(),
            updated_at: None,
            loaders: Some(Vec::new()),
        }
    }

    /// A fetcher that returns a summary for each requested id and records the
    /// id lists it was called with (so tests can assert "not called" / "called
    /// with only the stale ids").
    fn recording_fetcher(
        source: ModSource,
    ) -> (
        Arc<StdMutex<Vec<Vec<String>>>>,
        impl FnOnce(Vec<String>) -> std::future::Ready<crate::error::Result<Vec<ModSummary>>>,
    ) {
        let calls = Arc::new(StdMutex::new(Vec::new()));
        let rec = calls.clone();
        let f = move |ids: Vec<String>| {
            let out = ids.iter().map(|id| summary(source, id)).collect();
            rec.lock().unwrap().push(ids);
            std::future::ready(Ok(out))
        };
        (calls, f)
    }

    fn seed(path: &Path, source: ModSource, id: &str, fetched_at: DateTime<Utc>) {
        let mut map = load(path);
        map.insert(
            (source, id.to_string()),
            StoredEntry {
                fetched_at: fetched_at.to_rfc3339(),
                summary: summary(source, id),
            },
        );
        save(path, &map).unwrap();
    }

    /// Seed an entry as a pre-migration write would leave it: fresh timestamp,
    /// no `loaders` key.
    fn seed_without_loaders(path: &Path, source: ModSource, id: &str, fetched_at: DateTime<Utc>) {
        let mut map = load(path);
        let mut s = summary(source, id);
        s.loaders = None;
        map.insert(
            (source, id.to_string()),
            StoredEntry {
                fetched_at: fetched_at.to_rfc3339(),
                summary: s,
            },
        );
        save(path, &map).unwrap();
    }

    /// A fresh entry written before `loaders` existed must still be re-fetched
    /// when the caller depends on that field — otherwise the dependency graph
    /// reads it as "unknown", never suppresses, and the fix looks dead on every
    /// warm cache until the TTL expires.
    #[tokio::test]
    async fn refetches_fresh_entry_that_predates_the_loaders_field() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("summaries.json");
        let now = Utc::now();
        seed_without_loaders(&path, ModSource::Modrinth, "jei", now);
        let (calls, f) = recording_fetcher(ModSource::Modrinth);
        let out = get_many_inner(
            &path,
            ModSource::Modrinth,
            &["jei".to_string()],
            7,
            true,
            now,
            f,
        )
        .await;
        assert_eq!(out.len(), 1);
        assert_eq!(
            calls.lock().unwrap().len(),
            1,
            "a None loaders field must count as stale"
        );
    }

    /// …and at `ttl_days == 0`, a supported setting that short-circuits
    /// `is_fresh` to always-fresh. The loaders clause must be OR'd with it, not
    /// nested inside it, or the entry would never migrate.
    #[tokio::test]
    async fn refetches_entry_missing_loaders_even_at_ttl_zero() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("summaries.json");
        let now = Utc::now();
        seed_without_loaders(&path, ModSource::Modrinth, "jei", now);
        let (calls, f) = recording_fetcher(ModSource::Modrinth);
        let out = get_many_inner(
            &path,
            ModSource::Modrinth,
            &["jei".to_string()],
            0,
            true,
            now,
            f,
        )
        .await;
        assert_eq!(out.len(), 1);
        assert_eq!(calls.lock().unwrap().len(), 1);
    }

    /// A source that cannot report loaders must NOT be re-fetched on their
    /// account — it would be permanently stale, i.e. a fresh 429 source on every
    /// single graph resolve.
    #[tokio::test]
    async fn does_not_refetch_a_source_that_cannot_report_loaders() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("summaries.json");
        let now = Utc::now();
        seed_without_loaders(&path, ModSource::Hangar, "plug", now);
        let (calls, f) = recording_fetcher(ModSource::Hangar);
        let out = get_many_inner(
            &path,
            ModSource::Hangar,
            &["plug".to_string()],
            7,
            true,
            now,
            f,
        )
        .await;
        assert_eq!(out.len(), 1);
        assert!(
            calls.lock().unwrap().is_empty(),
            "Hangar has no loader concept — never stale for that reason"
        );
    }

    /// Callers that only display metadata must not pay for the migration.
    #[tokio::test]
    async fn does_not_refetch_missing_loaders_when_not_required() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("summaries.json");
        let now = Utc::now();
        seed_without_loaders(&path, ModSource::Modrinth, "jei", now);
        let (calls, f) = recording_fetcher(ModSource::Modrinth);
        let out = get_many_inner(
            &path,
            ModSource::Modrinth,
            &["jei".to_string()],
            7,
            false,
            now,
            f,
        )
        .await;
        assert_eq!(out.len(), 1);
        assert!(calls.lock().unwrap().is_empty());
    }

    #[test]
    fn is_fresh_honors_ttl_and_zero_means_never_expire() {
        let now = DateTime::parse_from_rfc3339("2026-06-19T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let recent = (now - Duration::days(3)).to_rfc3339();
        let old = (now - Duration::days(10)).to_rfc3339();
        assert!(is_fresh(&recent, now, 7));
        assert!(!is_fresh(&old, now, 7));
        // 0 = never expire: even a very old entry is fresh.
        assert!(is_fresh(&old, now, 0));
        // Unparseable timestamp ⇒ stale.
        assert!(!is_fresh("not-a-date", now, 7));
    }

    #[tokio::test]
    async fn miss_fetches_persists_and_returns() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("mods-cache/summaries.json");
        let now = Utc::now();
        let (calls, f) = recording_fetcher(ModSource::Modrinth);
        let ids = vec!["jei".to_string(), "sodium".to_string()];
        let out = get_many_inner(&path, ModSource::Modrinth, &ids, 7, false, now, f).await;
        assert_eq!(out.len(), 2);
        // Fetched exactly the two missing ids, in one call.
        let recorded = calls.lock().unwrap().clone();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0], ids);
        // Persisted to disk.
        assert!(path.exists());
        assert_eq!(load(&path).len(), 2);
    }

    #[tokio::test]
    async fn fresh_entries_skip_the_fetch() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("summaries.json");
        let now = Utc::now();
        seed(&path, ModSource::Modrinth, "jei", now); // fresh
        let (calls, f) = recording_fetcher(ModSource::Modrinth);
        let out = get_many_inner(
            &path,
            ModSource::Modrinth,
            &["jei".to_string()],
            7,
            false,
            now,
            f,
        )
        .await;
        assert_eq!(out.len(), 1);
        assert!(
            calls.lock().unwrap().is_empty(),
            "no fetch for a fresh entry"
        );
    }

    #[tokio::test]
    async fn only_stale_and_missing_ids_are_fetched() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("summaries.json");
        let now = Utc::now();
        seed(&path, ModSource::Modrinth, "fresh", now);
        seed(
            &path,
            ModSource::Modrinth,
            "stale",
            now - Duration::days(30),
        );
        let (calls, f) = recording_fetcher(ModSource::Modrinth);
        let ids = vec![
            "fresh".to_string(),
            "stale".to_string(),
            "missing".to_string(),
        ];
        let out = get_many_inner(&path, ModSource::Modrinth, &ids, 7, false, now, f).await;
        assert_eq!(out.len(), 3);
        let recorded = calls.lock().unwrap().clone();
        assert_eq!(recorded.len(), 1);
        // Only the stale + missing ids hit the network — never "fresh".
        let mut got = recorded[0].clone();
        got.sort();
        assert_eq!(got, vec!["missing".to_string(), "stale".to_string()]);
    }

    #[tokio::test]
    async fn ttl_zero_never_refetches_cached() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("summaries.json");
        let now = Utc::now();
        seed(
            &path,
            ModSource::Modrinth,
            "ancient",
            now - Duration::days(3650),
        );
        let (calls, f) = recording_fetcher(ModSource::Modrinth);
        let out = get_many_inner(
            &path,
            ModSource::Modrinth,
            &["ancient".to_string()],
            0,
            false,
            now,
            f,
        )
        .await;
        assert_eq!(out.len(), 1);
        assert!(calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn fetch_failure_degrades_to_cache_only() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("summaries.json");
        let now = Utc::now();
        seed(&path, ModSource::Modrinth, "have", now);
        // Fetcher errors for the missing id.
        let f = |_ids: Vec<String>| {
            std::future::ready(Err(crate::error::Error::ModsNotFound {
                platform: "modrinth".into(),
            }))
        };
        let ids = vec!["have".to_string(), "gone".to_string()];
        let out = get_many_inner(&path, ModSource::Modrinth, &ids, 7, false, now, f).await;
        // Cached one survives; the failed one is simply absent (row degrades).
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].project_id, "have");
    }

    #[tokio::test]
    async fn source_scopes_the_key() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("summaries.json");
        let now = Utc::now();
        // Same id under Modrinth is cached fresh; a Curseforge lookup for the
        // same id must still fetch (distinct key).
        seed(&path, ModSource::Modrinth, "42", now);
        let (calls, f) = recording_fetcher(ModSource::Curseforge);
        let out = get_many_inner(
            &path,
            ModSource::Curseforge,
            &["42".to_string()],
            7,
            false,
            now,
            f,
        )
        .await;
        assert_eq!(out.len(), 1);
        assert_eq!(calls.lock().unwrap().len(), 1, "CF id 42 is a distinct key");
    }

    /// The display-name backfill must be provably offline. This accessor takes
    /// no fetch closure at all, so "it cannot reach the network" is visible in
    /// the signature rather than promised in a comment.
    #[test]
    fn get_many_cached_returns_hits_and_skips_misses() {
        let td = tempdir().unwrap();
        let path = td.path().join("mods-cache.json");
        seed(&path, ModSource::Modrinth, "aaa", Utc::now());

        let got = get_many_cached(
            &path,
            ModSource::Modrinth,
            &["aaa".to_string(), "missing".to_string()],
        );
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].project_id, "aaa");
        assert_eq!(got[0].name, "Name aaa");
    }

    /// A stale entry is still returned: a project title does not rot, and a
    /// best-effort backfill must never trigger the re-fetch that TTL implies.
    #[test]
    fn get_many_cached_ignores_ttl() {
        let td = tempdir().unwrap();
        let path = td.path().join("mods-cache.json");
        seed(
            &path,
            ModSource::Modrinth,
            "old",
            Utc::now() - Duration::days(3650),
        );

        let got = get_many_cached(&path, ModSource::Modrinth, &["old".to_string()]);
        assert_eq!(got.len(), 1, "an ancient entry is still the mod's name");
    }

    #[test]
    fn get_many_cached_on_an_absent_file_is_empty_not_an_error() {
        let td = tempdir().unwrap();
        let got = get_many_cached(
            &td.path().join("nope.json"),
            ModSource::Modrinth,
            &["aaa".to_string()],
        );
        assert!(got.is_empty());
    }
}
