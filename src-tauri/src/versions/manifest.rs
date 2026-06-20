//! Fetch and cache the Mojang version manifest.
//!
//! Endpoint: `https://piston-meta.mojang.com/mc/game/version_manifest_v2.json`.
//! Schema: `{ latest: {release, snapshot}, versions: [{id, type, url,
//! time, releaseTime, sha1, complianceLevel}, ...] }`. Mojang lists the
//! freshest entries first.
//!
//! In-memory cache, 5-minute TTL. The cache is process-global — the
//! manifest is a launcher-wide concern, same shape as the audit log.
//!
//! Test override: the env var `LUCERNA_MANIFEST_URL_OVERRIDE`, if
//! set, replaces the URL. Used by `tests/manifest_integration.rs` to
//! point at wiremock without baking a config knob into production.
//! In production runs the env var is never set.

use crate::error::Result;
use crate::network::get_json;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const CACHE_TTL: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VersionType {
    Release,
    Snapshot,
    OldAlpha,
    OldBeta,
}

/// What the UI sees per entry. We strip the cryptographic and
/// compliance fields the launcher core needs but the UI doesn't.
#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct VersionEntry {
    pub id: String,
    pub version_type: VersionType,
    /// ISO-8601 string from Mojang, e.g. "2023-12-07T12:00:00+00:00".
    pub release_date: String,
    /// URL to the per-version JSON. Stored so slice 4 doesn't have to
    /// re-fetch the manifest to know where to install from.
    pub url: String,
}

#[derive(Debug, Deserialize)]
struct RawManifest {
    versions: Vec<RawEntry>,
}

#[derive(Debug, Deserialize)]
struct RawEntry {
    id: String,
    #[serde(rename = "type")]
    version_type: String,
    url: String,
    #[serde(rename = "releaseTime")]
    release_time: String,
}

struct Cached {
    fetched_at: Instant,
    entries: Vec<VersionEntry>,
}

fn cache() -> &'static Mutex<Option<Cached>> {
    static CACHE: OnceLock<Mutex<Option<Cached>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

/// Return the version list. Uses the in-memory cache if it is still
/// fresh; otherwise fetches and refreshes.
pub async fn list_manifest() -> Result<Vec<VersionEntry>> {
    {
        let guard = cache().lock().expect("manifest cache mutex poisoned");
        if let Some(c) = guard.as_ref() {
            if c.fetched_at.elapsed() < CACHE_TTL {
                return Ok(c.entries.clone());
            }
        }
    }

    let url = crate::test_seam::resolve("LUCERNA_MANIFEST_URL_OVERRIDE")
        .unwrap_or_else(|| MANIFEST_URL.to_string());
    let raw: RawManifest = get_json(&url, "versions").await?;
    let entries = parse_and_sort(raw);

    {
        let mut guard = cache().lock().expect("manifest cache mutex poisoned");
        *guard = Some(Cached {
            fetched_at: Instant::now(),
            entries: entries.clone(),
        });
    }

    Ok(entries)
}

fn parse_and_sort(raw: RawManifest) -> Vec<VersionEntry> {
    let mut out: Vec<VersionEntry> = raw
        .versions
        .into_iter()
        .filter_map(|r| {
            let version_type = match r.version_type.as_str() {
                "release" => VersionType::Release,
                "snapshot" => VersionType::Snapshot,
                "old_alpha" => VersionType::OldAlpha,
                "old_beta" => VersionType::OldBeta,
                // Unknown future types are dropped rather than mapped to
                // a catch-all — better to miss a version than to mis-classify it.
                _ => return None,
            };
            Some(VersionEntry {
                id: r.id,
                version_type,
                release_date: r.release_time,
                url: r.url,
            })
        })
        .collect();

    // Sort: release first (newest → oldest), then snapshot (newest → oldest),
    // then old_beta / old_alpha (newest → oldest). Within each type, Mojang
    // already orders newest-first via releaseTime descending; we still sort
    // explicitly so we are not dependent on upstream order.
    out.sort_by(|a, b| {
        let type_order = |t: &VersionType| -> u8 {
            match t {
                VersionType::Release => 0,
                VersionType::Snapshot => 1,
                VersionType::OldBeta => 2,
                VersionType::OldAlpha => 3,
            }
        };
        type_order(&a.version_type)
            .cmp(&type_order(&b.version_type))
            // Sort by ISO-8601 string descending — works because the
            // format is lexicographically orderable.
            .then(b.release_date.cmp(&a.release_date))
            // Total order: ties on (type, date) break by id so the
            // sort is deterministic regardless of upstream order.
            .then(a.id.cmp(&b.id))
    });

    out
}

/// Force-clear the cache. Used by integration tests in external
/// crates that cannot rely on `#[cfg(test)]` items.
#[doc(hidden)]
pub fn clear_cache_for_test() {
    let mut guard = cache().lock().expect("manifest cache mutex poisoned");
    *guard = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(id: &str, ty: &str, date: &str) -> RawEntry {
        RawEntry {
            id: id.into(),
            version_type: ty.into(),
            url: format!("https://example.com/{id}.json"),
            release_time: date.into(),
        }
    }

    #[test]
    fn sort_puts_releases_first_then_snapshots() {
        let raw = RawManifest {
            versions: vec![
                raw("24w08a", "snapshot", "2024-02-21T13:00:00+00:00"),
                raw("1.20.4", "release", "2023-12-07T12:00:00+00:00"),
                raw("1.19.4", "release", "2023-03-14T12:00:00+00:00"),
                raw("1.0", "old_beta", "2011-11-18T00:00:00+00:00"),
            ],
        };
        let sorted = parse_and_sort(raw);
        assert_eq!(sorted.len(), 4);
        assert_eq!(sorted[0].id, "1.20.4");
        assert_eq!(sorted[1].id, "1.19.4");
        assert_eq!(sorted[2].id, "24w08a");
        assert_eq!(sorted[3].id, "1.0");
    }

    #[test]
    fn unknown_version_types_are_dropped() {
        let raw = RawManifest {
            versions: vec![
                raw("1.20.4", "release", "2023-12-07T12:00:00+00:00"),
                raw("future-thing", "unprecedented", "2099-01-01T00:00:00+00:00"),
            ],
        };
        let sorted = parse_and_sort(raw);
        assert_eq!(sorted.len(), 1);
        assert_eq!(sorted[0].id, "1.20.4");
    }

    #[test]
    fn url_is_preserved_for_later_install() {
        let raw = RawManifest {
            versions: vec![raw("1.20.4", "release", "2023-12-07T12:00:00+00:00")],
        };
        let sorted = parse_and_sort(raw);
        assert_eq!(sorted[0].url, "https://example.com/1.20.4.json");
    }
}
