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
//! `fabric-loader-0.15.7-1.20.4`). BOTH the loader version
//! (`0.24.0-beta.1`) and the MC version (`1.20.4-pre1`, `24w08a`) may
//! themselves contain `-`, so neither a first-`-` nor a last-`-` split is
//! reliable. When the caller already knows the MC version (every producer
//! composes the id from a known `(loader_ver, mc_ver)` pair) it should use
//! [`parse_synth_id_with_mc`], which strips the known trailing `-<mc>`
//! unambiguously. [`parse_synth_id`] (no MC hint) falls back to a
//! plausibility heuristic that picks the split whose right side looks like a
//! complete MC version.

pub mod fabric;
pub mod forge;
pub mod neoforge;
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
    NeoForge,
}

impl Loader {
    pub fn as_synth_prefix(self) -> &'static str {
        match self {
            Loader::Fabric => "fabric-loader-",
            Loader::Quilt => "quilt-loader-",
            // Forge has no "-loader-" infix; the synth_id is `forge-<fv>-<mc>`.
            // Returned prefix is the part BEFORE <fv>.
            Loader::Forge => "forge-",
            // NeoForge follows the same no-infix shape: `neoforge-<nv>-<mc>`.
            Loader::NeoForge => "neoforge-",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Loader::Fabric => "fabric",
            Loader::Quilt => "quilt",
            Loader::Forge => "forge",
            Loader::NeoForge => "neoforge",
        }
    }

    /// User-facing capitalised label for display in progress messages and UI.
    pub fn display_name(self) -> &'static str {
        match self {
            Loader::Fabric => "Fabric",
            Loader::Quilt => "Quilt",
            Loader::Forge => "Forge",
            Loader::NeoForge => "NeoForge",
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
    // NeoForge: "neoforge-<nv>-<mc>". Same no-infix shape as Forge.
    if let Some(rest) = id.strip_prefix("neoforge-") {
        return parse_forge_rest(Loader::NeoForge, rest);
    }
    None
}

/// Parse a synthetic id when the MC version is already known. This is the
/// unambiguous path: it strips the known trailing `-<mc>` and treats the
/// remaining middle as the loader version, so loader versions that contain
/// `-` (`0.24.0-beta.1`) round-trip correctly. Returns `(loader, loader_ver)`
/// on match, or `None` for vanilla ids, a mismatched MC tail, or a missing
/// loader version. Callers that hold the MC version (every producer does)
/// should prefer this over [`parse_synth_id`].
pub fn parse_synth_id_with_mc(id: &str, mc_ver: &str) -> Option<(Loader, String)> {
    for loader in [
        Loader::Fabric,
        Loader::Quilt,
        Loader::Forge,
        Loader::NeoForge,
    ] {
        let Some(rest) = id.strip_prefix(loader.as_synth_prefix()) else {
            continue;
        };
        // rest == "<loader_ver>-<mc_ver>". Strip the trailing "-<mc_ver>".
        let suffix = format!("-{mc_ver}");
        let loader_ver = rest.strip_suffix(&suffix)?;
        if loader_ver.is_empty() {
            return None;
        }
        return Some((loader, loader_ver.to_string()));
    }
    None
}

/// True when `s` plausibly begins a Minecraft version token: releases
/// and their pre/rc suffixes start `1.` (`1.20.4`, `1.20.4-pre1`,
/// `1.20-rc1`); snapshots start with a 2-digit year followed by `w`
/// (`24w08a`); the classic betas start `b1.`/`a1.`; April-fools and a
/// handful of named builds are digit-led too. We only need to distinguish
/// an MC token from a *loader* version, and loader versions never carry a
/// `1.`/`NNw` shape at their head, so this is a safe discriminator.
fn looks_like_mc_start(s: &str) -> bool {
    if let Some(tail) = s.strip_prefix("1.") {
        return tail.bytes().next().is_some_and(|b| b.is_ascii_digit());
    }
    let b = s.as_bytes();
    // Snapshot `NNwNN...` (e.g. `24w08a`): two digits then `w`.
    if b.len() >= 3 && b[0].is_ascii_digit() && b[1].is_ascii_digit() && b[2] == b'w' {
        return true;
    }
    // Classic `a1.`/`b1.` alpha/beta ids. Byte-compare to stay panic-free on
    // any non-ASCII input (MC ids are ASCII, but a hostile id might not be).
    b.starts_with(b"a1.") || b.starts_with(b"b1.")
}

fn parse_fabric_quilt_rest(loader: Loader, rest: &str) -> Option<(Loader, String, String)> {
    // Both parts may contain `-`. Prefer the split whose right side looks
    // like a complete MC version; scan `-` positions left-to-right so the
    // FIRST plausible MC boundary wins (loader versions are short and never
    // start with an MC-shaped token, so an early `-beta.1` inside the loader
    // version is skipped, while the real `-1.20.4` boundary is taken).
    let bytes = rest.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b != b'-' {
            continue;
        }
        let (loader_ver, mc_ver) = (&rest[..i], &rest[i + 1..]);
        if !loader_ver.is_empty() && looks_like_mc_start(mc_ver) {
            return Some((loader, loader_ver.to_string(), mc_ver.to_string()));
        }
    }
    // No MC-shaped right side found (unrecognised/exotic MC id): fall back to
    // the historical first-`-` split so previously-parseable ids still parse.
    let dash = rest.find('-')?;
    let loader_ver = &rest[..dash];
    let mc_ver = &rest[dash + 1..];
    if loader_ver.is_empty() || mc_ver.is_empty() {
        return None;
    }
    Some((loader, loader_ver.to_string(), mc_ver.to_string()))
}

fn parse_forge_rest(loader: Loader, rest: &str) -> Option<(Loader, String, String)> {
    // Find the first `-` whose right side starts with a digit. Forge and
    // NeoForge versions are dot-and-digit only (e.g. `49.0.49`, `20.4.245`),
    // so the loader-version part must also start with a digit. MC versions
    // always begin with a digit too (e.g. `1.20.4`, `24w08a` — snapshot ids
    // start with a 2-digit year).
    let bytes = rest.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'-' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit() {
            let fv = &rest[..i];
            let mc = &rest[i + 1..];
            // Reject if the loader-version part doesn't start with a digit.
            // This prevents `neoforge-loader-47.0.0-1.20.4` from being
            // misidentified as NeoForge with loader_ver="loader".
            if !fv.is_empty() && !mc.is_empty() && fv.as_bytes()[0].is_ascii_digit() {
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
        Loader::NeoForge => neoforge::list(mc_id).await?,
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
        Loader::NeoForge => neoforge::profile(mc_id, loader_ver, app).await,
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
    fn synth_id_round_trips_loader_version_with_dash() {
        // Regression: a Fabric loader pre-release version contains `-`
        // (`0.24.0-beta.1`). The old first-`-` split truncated it to
        // "0.24.0" and mis-parsed the MC as "beta.1-1.20.4".
        let id = synth_id(Loader::Fabric, "0.24.0-beta.1", "1.20.4");
        assert_eq!(id, "fabric-loader-0.24.0-beta.1-1.20.4");
        let (l, lv, mv) = parse_synth_id(&id).unwrap();
        assert_eq!(l, Loader::Fabric);
        assert_eq!(lv, "0.24.0-beta.1");
        assert_eq!(mv, "1.20.4");
    }

    #[test]
    fn parse_synth_id_with_mc_strips_known_trailing_mc() {
        // Unambiguous path: MC known → loader version with `-` is exact.
        let id = synth_id(Loader::Fabric, "0.24.0-beta.1", "1.20.4-pre1");
        let (l, lv) = parse_synth_id_with_mc(&id, "1.20.4-pre1").unwrap();
        assert_eq!(l, Loader::Fabric);
        assert_eq!(lv, "0.24.0-beta.1");
    }

    #[test]
    fn parse_synth_id_with_mc_forge_and_vanilla() {
        let id = synth_id(Loader::Forge, "49.0.49", "1.20.4");
        assert_eq!(
            parse_synth_id_with_mc(&id, "1.20.4"),
            Some((Loader::Forge, "49.0.49".to_string()))
        );
        // Vanilla id → None.
        assert!(parse_synth_id_with_mc("1.20.4", "1.20.4").is_none());
        // Mismatched MC tail → None (guards against a wrong instance mc).
        assert!(parse_synth_id_with_mc(&id, "1.19.2").is_none());
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
        // NeoForge ids use `neoforge-<nv>-<mc>` (no `-loader-` infix).
        // The infix-style id below is malformed for both old and v0.4.1 parsers.
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

    #[test]
    fn synth_id_neoforge_round_trip() {
        let id = synth_id(Loader::NeoForge, "20.4.245", "1.20.4");
        assert_eq!(id, "neoforge-20.4.245-1.20.4");
        let (l, lv, mv) = parse_synth_id(&id).unwrap();
        assert_eq!(l, Loader::NeoForge);
        assert_eq!(lv, "20.4.245");
        assert_eq!(mv, "1.20.4");
    }

    #[test]
    fn parse_synth_id_neoforge_handles_mc_with_pre1_suffix() {
        let (l, lv, mv) = parse_synth_id("neoforge-21.1.0-1.21.1-pre1").unwrap();
        assert_eq!(l, Loader::NeoForge);
        assert_eq!(lv, "21.1.0");
        assert_eq!(mv, "1.21.1-pre1");
    }

    #[test]
    fn parse_synth_id_rejects_neoforge_without_components() {
        assert!(parse_synth_id("neoforge-").is_none());
        assert!(parse_synth_id("neoforge-20.4.245-").is_none());
        assert!(parse_synth_id("neoforge--1.20.4").is_none());
    }

    #[test]
    fn loader_neoforge_as_str_is_neoforge() {
        assert_eq!(Loader::NeoForge.as_str(), "neoforge");
        assert_eq!(Loader::NeoForge.as_synth_prefix(), "neoforge-");
    }

    #[test]
    fn loader_display_name_capitalises() {
        assert_eq!(Loader::Fabric.display_name(), "Fabric");
        assert_eq!(Loader::Quilt.display_name(), "Quilt");
        assert_eq!(Loader::Forge.display_name(), "Forge");
        assert_eq!(Loader::NeoForge.display_name(), "NeoForge");
    }
}
