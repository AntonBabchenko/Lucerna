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
pub mod forge;
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
    Forge,
}

impl Loader {
    pub fn as_synth_prefix(self) -> &'static str {
        match self {
            Loader::Fabric => "fabric-loader-",
            Loader::Quilt => "quilt-loader-",
            // Forge has no "-loader-" infix; the synth_id is `forge-<fv>-<mc>`.
            // Returned prefix is the part BEFORE <fv>.
            Loader::Forge => "forge-",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Loader::Fabric => "fabric",
            Loader::Quilt => "quilt",
            Loader::Forge => "forge",
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
    // Fabric / Quilt: "<loader>-loader-<lv>-<mc>". Loader version is
    // SemVer-shaped (no `-`); MC may contain `-`.
    if let Some(rest) = id.strip_prefix("fabric-loader-") {
        return parse_fabric_quilt_rest(Loader::Fabric, rest);
    }
    if let Some(rest) = id.strip_prefix("quilt-loader-") {
        return parse_fabric_quilt_rest(Loader::Quilt, rest);
    }
    // Forge: "forge-<fv>-<mc>". Forge version never contains `-`;
    // MC may. After the prefix, split on the FIRST `-` whose right
    // side starts with a digit (MC versions always start with a digit).
    if let Some(rest) = id.strip_prefix("forge-") {
        return parse_forge_rest(Loader::Forge, rest);
    }
    None
}

fn parse_fabric_quilt_rest(loader: Loader, rest: &str) -> Option<(Loader, String, String)> {
    let dash = rest.find('-')?;
    let loader_ver = &rest[..dash];
    let mc_ver = &rest[dash + 1..];
    if loader_ver.is_empty() || mc_ver.is_empty() {
        return None;
    }
    Some((loader, loader_ver.to_string(), mc_ver.to_string()))
}

fn parse_forge_rest(loader: Loader, rest: &str) -> Option<(Loader, String, String)> {
    // Find the first `-` whose right side starts with a digit. Forge
    // versions are dot-and-digit only; once we hit `-<digit>`, the right
    // side is the MC id (which always begins with a digit).
    let bytes = rest.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'-'
            && i + 1 < bytes.len()
            && bytes[i + 1].is_ascii_digit()
        {
            let fv = &rest[..i];
            let mc = &rest[i + 1..];
            if !fv.is_empty() && !mc.is_empty() {
                return Some((loader, fv.to_string(), mc.to_string()));
            }
        }
    }
    None
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
        Loader::Forge => forge::list(mc_id).await?,
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
    app: &tauri::AppHandle,
) -> Result<VersionDetails> {
    match loader {
        Loader::Fabric => fabric::profile(mc_id, loader_ver).await,
        Loader::Quilt => quilt::profile(mc_id, loader_ver).await,
        Loader::Forge => forge::profile(mc_id, loader_ver, app).await,
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
        // Unknown loader prefix — neoforge ships in v0.4.1.
        assert!(parse_synth_id("neoforge-loader-47.0.0-1.20.4").is_none());
    }

    #[test]
    fn synth_id_forge_round_trip() {
        let id = synth_id(Loader::Forge, "49.0.49", "1.20.4");
        assert_eq!(id, "forge-49.0.49-1.20.4");
        let (l, lv, mv) = parse_synth_id(&id).unwrap();
        assert_eq!(l, Loader::Forge);
        assert_eq!(lv, "49.0.49");
        assert_eq!(mv, "1.20.4");
    }

    #[test]
    fn parse_synth_id_forge_with_legacy_4segment_version() {
        let (l, lv, mv) = parse_synth_id("forge-14.23.5.2860-1.12.2").unwrap();
        assert_eq!(l, Loader::Forge);
        assert_eq!(lv, "14.23.5.2860");
        assert_eq!(mv, "1.12.2");
    }

    #[test]
    fn parse_synth_id_forge_handles_mc_with_pre1_suffix() {
        // Forge versions never contain `-`; MC may. Split on the FIRST `-`
        // whose right side starts with a digit. That distinguishes
        // "forge-<fv>" from the trailing "<mc>" part.
        let (l, lv, mv) = parse_synth_id("forge-49.0.0-1.20.4-pre1").unwrap();
        assert_eq!(l, Loader::Forge);
        assert_eq!(lv, "49.0.0");
        assert_eq!(mv, "1.20.4-pre1");
    }

    #[test]
    fn parse_synth_id_rejects_forge_without_components() {
        assert!(parse_synth_id("forge-").is_none());
        assert!(parse_synth_id("forge-49.0.49-").is_none());
        assert!(parse_synth_id("forge--1.20.4").is_none());
    }
}
