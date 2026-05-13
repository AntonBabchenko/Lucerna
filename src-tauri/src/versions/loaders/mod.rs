//! Fabric + Quilt loader integration.
//!
//! Two parallel concrete implementations (no `Loader` trait — only two
//! consumers ever). The public surface is:
//! - `Loader` enum + `synth_id`/`parse_synth_id` (id convention).
//! - `LoaderVersion` (shape returned to the UI dropdown).
//! - `list_loaders(loader, mc) → Vec<LoaderVersion>` — 5-min cached.
//! - `fetch_profile(loader, mc, ver) → VersionDetails` — single-shot,
//!   cached on disk via `versions::install::ensure_version_json`.
//!
//! ## Synthetic id convention
//!
//! `fabric-loader-<loader_ver>-<mc_ver>` (e.g.
//! `fabric-loader-0.15.7-1.20.4`). The MC version may itself contain `-`
//! (`1.20.4-pre1`, `24w08a`); the parser splits on the first `-` after
//! the `<loader>-loader-` prefix because loader versions are SemVer-like
//! and contain no `-`.

pub mod fabric;
pub mod quilt;

use crate::error::{Error, Result};
use crate::versions::version_json::VersionDetails;
use serde::Serialize;
use specta::Type;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Loader {
    Fabric,
    Quilt,
}

impl Loader {
    pub fn as_synth_prefix(self) -> &'static str {
        match self {
            Loader::Fabric => "fabric-loader-",
            Loader::Quilt => "quilt-loader-",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Loader::Fabric => "fabric",
            Loader::Quilt => "quilt",
        }
    }
}

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct LoaderVersion {
    pub version: String,
    pub stable: bool,
    pub build: u32,
}

/// Compose the synthetic id for a (loader, loader_ver, mc_ver) tuple.
pub fn synth_id(loader: Loader, loader_ver: &str, mc_ver: &str) -> String {
    format!("{}{}-{}", loader.as_synth_prefix(), loader_ver, mc_ver)
}

/// Parse a synthetic id into its components. Returns `None` for vanilla
/// ids and malformed inputs.
pub fn parse_synth_id(id: &str) -> Option<(Loader, String, String)> {
    let (loader, rest) = if let Some(r) = id.strip_prefix("fabric-loader-") {
        (Loader::Fabric, r)
    } else if let Some(r) = id.strip_prefix("quilt-loader-") {
        (Loader::Quilt, r)
    } else {
        return None;
    };
    // `rest = "<loader_ver>-<mc_ver>"`. Loader versions are SemVer
    // (`0.15.7`) and never contain `-`; MC ids may contain `-`.
    let dash = rest.find('-')?;
    let loader_ver = &rest[..dash];
    let mc_ver = &rest[dash + 1..];
    if loader_ver.is_empty() || mc_ver.is_empty() {
        return None;
    }
    Some((loader, loader_ver.to_string(), mc_ver.to_string()))
}

// ---- Loader-version list cache (5-min TTL) ----------------------------------

const LIST_CACHE_TTL: Duration = Duration::from_secs(5 * 60);

struct CachedList {
    fetched_at: Instant,
    entries: Vec<LoaderVersion>,
}

fn list_cache() -> &'static Mutex<std::collections::HashMap<(Loader, String), CachedList>> {
    static CACHE: OnceLock<Mutex<std::collections::HashMap<(Loader, String), CachedList>>> =
        OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

/// Return the list of loader versions compatible with `mc_id`. Cached
/// 5 minutes per `(loader, mc_id)` key. Empty list → propagate as
/// `Error::LoaderUnavailable`.
pub async fn list_loaders(loader: Loader, mc_id: &str) -> Result<Vec<LoaderVersion>> {
    {
        let guard = list_cache().lock().expect("loader cache mutex poisoned");
        if let Some(c) = guard.get(&(loader, mc_id.to_string())) {
            if c.fetched_at.elapsed() < LIST_CACHE_TTL {
                return Ok(c.entries.clone());
            }
        }
    }

    let entries = match loader {
        Loader::Fabric => fabric::list(mc_id).await?,
        Loader::Quilt => quilt::list(mc_id).await?,
    };

    if entries.is_empty() {
        return Err(Error::LoaderUnavailable {
            loader: loader.as_str().to_string(),
            mc_version: mc_id.to_string(),
        });
    }

    {
        let mut guard = list_cache().lock().expect("loader cache mutex poisoned");
        guard.insert(
            (loader, mc_id.to_string()),
            CachedList {
                fetched_at: Instant::now(),
                entries: entries.clone(),
            },
        );
    }
    Ok(entries)
}

/// Fetch the loader profile JSON (Mojang-format `VersionDetails` with
/// `inherits_from = Some(mc_id)`) for a specific loader version.
pub async fn fetch_profile(
    loader: Loader,
    mc_id: &str,
    loader_ver: &str,
) -> Result<VersionDetails> {
    match loader {
        Loader::Fabric => fabric::profile(mc_id, loader_ver).await,
        Loader::Quilt => quilt::profile(mc_id, loader_ver).await,
    }
}

#[doc(hidden)]
pub fn clear_cache_for_test() {
    let mut guard = list_cache().lock().expect("loader cache mutex poisoned");
    guard.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synth_id_fabric_round_trip() {
        let id = synth_id(Loader::Fabric, "0.15.7", "1.20.4");
        assert_eq!(id, "fabric-loader-0.15.7-1.20.4");
        let (l, lv, mv) = parse_synth_id(&id).unwrap();
        assert_eq!(l, Loader::Fabric);
        assert_eq!(lv, "0.15.7");
        assert_eq!(mv, "1.20.4");
    }

    #[test]
    fn synth_id_quilt_round_trip() {
        let id = synth_id(Loader::Quilt, "0.23.1", "1.20.4");
        assert_eq!(id, "quilt-loader-0.23.1-1.20.4");
        let (l, lv, mv) = parse_synth_id(&id).unwrap();
        assert_eq!(l, Loader::Quilt);
        assert_eq!(lv, "0.23.1");
        assert_eq!(mv, "1.20.4");
    }

    #[test]
    fn parse_synth_id_handles_mc_with_dashes() {
        let (l, lv, mv) = parse_synth_id("fabric-loader-0.15.7-1.20.4-pre1").unwrap();
        assert_eq!(l, Loader::Fabric);
        assert_eq!(lv, "0.15.7");
        assert_eq!(mv, "1.20.4-pre1");
    }

    #[test]
    fn parse_synth_id_handles_snapshot_mc() {
        let (l, lv, mv) = parse_synth_id("quilt-loader-0.23.1-24w08a").unwrap();
        assert_eq!(l, Loader::Quilt);
        assert_eq!(lv, "0.23.1");
        assert_eq!(mv, "24w08a");
    }

    #[test]
    fn parse_synth_id_rejects_vanilla() {
        assert!(parse_synth_id("1.20.4").is_none());
        assert!(parse_synth_id("24w08a").is_none());
    }

    #[test]
    fn parse_synth_id_rejects_malformed() {
        assert!(parse_synth_id("fabric-loader-").is_none());
        assert!(parse_synth_id("fabric-loader-0.15.7-").is_none());
        assert!(parse_synth_id("fabric-loader--1.20.4").is_none());
        assert!(parse_synth_id("").is_none());
        assert!(parse_synth_id("forge-loader-47.0.0-1.20.4").is_none());
    }
}
