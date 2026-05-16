//! Forge meta layer — maven-metadata.xml + promotions_slim.json
//! parsing, on-disk installer cache. See spec section "Component:
//! Meta layer (forge::meta)".

use crate::error::{Error, Result};
use quick_xml::de::from_str as xml_from_str;
use serde::Deserialize;

// ---- maven-metadata.xml parsing ----------------------------------

#[derive(Debug, Deserialize)]
struct MavenMetadata {
    versioning: MavenVersioning,
}

#[derive(Debug, Deserialize)]
struct MavenVersioning {
    versions: MavenVersionsList,
}

#[derive(Debug, Deserialize)]
struct MavenVersionsList {
    #[serde(rename = "version", default)]
    versions: Vec<String>,
}

/// One entry from `maven-metadata.xml`. `mc` and `fv` are the canonical
/// MC + Forge-version pair shown to the user; `raw` is the original
/// version string used by maven (which sometimes duplicates the MC
/// suffix, see `parse_maven_metadata`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MavenEntry {
    pub mc: String,
    pub fv: String,
    /// Raw maven-version string, e.g. `"1.7.10-10.13.4.1614-1.7.10"`.
    /// Used as both the path segment and the artifact suffix when
    /// building installer URLs.
    pub raw: String,
}

/// Parse Forge's `maven-metadata.xml` into `MavenEntry` records.
/// Entries that don't match a `<mc>-<forge>` shape are silently
/// dropped (defensive — very old Forge releases had irregular entries).
///
/// Quirk: for some MC ranges (notably 1.7.10 and parts of 1.9), Forge's
/// maven stores versions as `<mc>-<fv>-<mc>` — the MC id is duplicated
/// at the end. We detect this by re-splitting the left part and
/// confirming the trailing suffix matches the leading MC prefix
/// (exactly, or via a `1.9` ↔ `1.9.0` shape-prefix overlap). The
/// canonical (mc, fv) is then taken from the inner split, while `raw`
/// preserves the full original string so the installer URL builder
/// can reconstruct the actual maven path.
pub fn parse_maven_metadata(xml: &str) -> Result<Vec<MavenEntry>> {
    let parsed: MavenMetadata = xml_from_str(xml).map_err(|e| {
        Error::ForgeMavenMetadataParseFailed {
            details: format!("{e}"),
        }
    })?;
    let mut out = Vec::with_capacity(parsed.versioning.versions.versions.len());
    for entry in parsed.versioning.versions.versions {
        if let Some(parsed) = parse_maven_entry(&entry) {
            out.push(parsed);
        }
    }
    Ok(out)
}

fn parse_maven_entry(entry: &str) -> Option<MavenEntry> {
    let (left, right) = entry.rsplit_once('-')?;
    if left.is_empty() || right.is_empty() {
        return None;
    }

    // Try the duplicate-MC-suffix quirk first. Algorithm:
    //   - Re-split `left` by its last `-` into `(mc_prefix, fv_inner)`.
    //   - If `right` "matches" `mc_prefix` (equal, or one is a dotted
    //     extension of the other — handles `1.9` ↔ `1.9.0`), treat this
    //     entry as the legacy `<mc>-<fv>-<mc>` shape.
    if let Some((mc_prefix, fv_inner)) = left.rsplit_once('-') {
        if !mc_prefix.is_empty()
            && !fv_inner.is_empty()
            && mc_suffix_matches(right, mc_prefix)
        {
            return Some(MavenEntry {
                mc: mc_prefix.to_string(),
                fv: fv_inner.to_string(),
                raw: entry.to_string(),
            });
        }
    }

    // Normal case: `<mc>-<fv>`.
    Some(MavenEntry {
        mc: left.to_string(),
        fv: right.to_string(),
        raw: entry.to_string(),
    })
}

/// True iff `suffix` is plausibly the same MC id as `prefix`. Equality
/// covers `1.7.10 == 1.7.10`; one-side dotted-extension covers
/// `1.9.0` vs `1.9` (the `1.9` line ships some entries with the
/// `.0` patch in the trailing suffix).
fn mc_suffix_matches(suffix: &str, prefix: &str) -> bool {
    if suffix == prefix {
        return true;
    }
    suffix.starts_with(&format!("{prefix}.")) || prefix.starts_with(&format!("{suffix}."))
}

// ---- promotions_slim.json parsing --------------------------------

#[derive(Debug, Deserialize)]
struct PromotionsRaw {
    promos: std::collections::HashMap<String, String>,
}

/// Parsed promotions table: efficient lookup of "what's recommended/latest
/// for this MC version".
#[derive(Debug, Default)]
pub struct Promotions {
    /// Keys are MC versions (e.g. "1.20.4"). Values are the recommended
    /// Forge version. `recommended` wins over `latest`; if only `latest`
    /// exists, that's used as the recommendation.
    by_mc: std::collections::HashMap<String, String>,
}

impl Promotions {
    pub fn recommended_for(&self, mc: &str) -> Option<&str> {
        self.by_mc.get(mc).map(|s| s.as_str())
    }
}

/// Parse promotions_slim.json. Returns `ForgePromotionsUnavailable` if
/// the JSON is malformed — callers can downgrade this to a non-fatal
/// "stability info unavailable" UI hint.
pub fn parse_promotions(body: &str) -> Result<Promotions> {
    let raw: PromotionsRaw = serde_json::from_str(body).map_err(|_| {
        Error::ForgePromotionsUnavailable {
            flavor: "forge".to_string(),
        }
    })?;
    let mut by_mc: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    // First pass: pick up "<mc>-latest" entries.
    for (key, value) in &raw.promos {
        if let Some(mc) = key.strip_suffix("-latest") {
            by_mc.insert(mc.to_string(), value.clone());
        }
    }
    // Second pass: "<mc>-recommended" overrides "<mc>-latest".
    for (key, value) in &raw.promos {
        if let Some(mc) = key.strip_suffix("-recommended") {
            by_mc.insert(mc.to_string(), value.clone());
        }
    }
    Ok(Promotions { by_mc })
}

// ---- version sort + LoaderVersion build --------------------------

/// Parse Forge version string `MAJOR.MINOR.PATCH.BUILD` into a 4-tuple
/// for lex compare. Missing trailing segments default to 0; non-numeric
/// segments default to 0 (defensive — string-sort fallback would order
/// pre-release weirdly).
pub(crate) fn version_parts(v: &str) -> (u32, u32, u32, u32) {
    let mut it = v.split('.');
    let parse = |s: Option<&str>| s.and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    (
        parse(it.next()),
        parse(it.next()),
        parse(it.next()),
        parse(it.next()),
    )
}

/// Filter `entries` to `mc_id`, sort descending by version, tag stable
/// from promotions, and return both the IPC-shaped `LoaderVersion`
/// list and an `fv → raw maven version` index. The raw index lets the
/// installer URL builder reconstruct paths for MC ranges that use the
/// `<mc>-<fv>-<mc>` quirk (1.7.10, parts of 1.9).
pub(crate) fn build_loader_versions(
    entries: &[MavenEntry],
    mc_id: &str,
    promos: &Promotions,
) -> (
    Vec<crate::versions::loaders::LoaderVersion>,
    std::collections::HashMap<String, String>,
) {
    let recommended = promos.recommended_for(mc_id);
    let mut filtered: Vec<&MavenEntry> =
        entries.iter().filter(|e| e.mc == mc_id).collect();
    filtered.sort_by(|a, b| version_parts(&b.fv).cmp(&version_parts(&a.fv)));

    let mut raw_by_fv =
        std::collections::HashMap::<String, String>::with_capacity(filtered.len());
    let loader_versions = filtered
        .into_iter()
        .map(|e| {
            raw_by_fv.insert(e.fv.clone(), e.raw.clone());
            crate::versions::loaders::LoaderVersion {
                version: e.fv.clone(),
                stable: Some(e.fv.as_str()) == recommended,
                build: 0,
            }
        })
        .collect();
    (loader_versions, raw_by_fv)
}

// ---- public API: list_versions -----------------------------------

use crate::forge::flavor::ForgeFlavor;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const LIST_CACHE_TTL: Duration = Duration::from_secs(5 * 60);

struct CachedList {
    fetched_at: Instant,
    entries: Vec<crate::versions::loaders::LoaderVersion>,
    /// `fv → raw` index from `build_loader_versions`. Used by
    /// `fetch_installer_bytes` to recover the actual maven path for
    /// MC ranges that use the `<mc>-<fv>-<mc>` quirk.
    raw_by_fv: std::collections::HashMap<String, String>,
}

fn list_cache() -> &'static Mutex<std::collections::HashMap<(ForgeFlavor, String), CachedList>> {
    static CACHE: OnceLock<
        Mutex<std::collections::HashMap<(ForgeFlavor, String), CachedList>>,
    > = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

/// `meta_url_override` for tests — points at a wiremock URL. `None` in production.
fn meta_url_for(flavor: ForgeFlavor) -> String {
    let env_key = match flavor {
        ForgeFlavor::Forge => "FTLAUNCHER_FORGE_META_OVERRIDE",
        ForgeFlavor::NeoForge => "FTLAUNCHER_NEOFORGE_META_OVERRIDE",
    };
    if let Ok(base) = std::env::var(env_key) {
        // Treat the override as a base — append the maven-metadata.xml path.
        format!("{}/maven-metadata.xml", base.trim_end_matches('/'))
    } else {
        flavor.maven_metadata_url().to_string()
    }
}

fn promotions_url_for(flavor: ForgeFlavor) -> Option<String> {
    let env_key = match flavor {
        ForgeFlavor::Forge => "FTLAUNCHER_FORGE_PROMOTIONS_OVERRIDE",
        ForgeFlavor::NeoForge => "FTLAUNCHER_NEOFORGE_PROMOTIONS_OVERRIDE",
    };
    if let Ok(base) = std::env::var(env_key) {
        Some(format!(
            "{}/promotions_slim.json",
            base.trim_end_matches('/')
        ))
    } else {
        flavor.promotions_url().map(|s| s.to_string())
    }
}

/// Return Forge/NeoForge versions compatible with `mc_id`, sorted
/// descending, with recommended versions tagged `stable: true`.
/// 5-minute in-memory cache per `(flavor, mc_id)`. Empty list →
/// `Error::LoaderUnavailable`.
pub async fn list_versions(
    flavor: ForgeFlavor,
    mc_id: &str,
) -> Result<Vec<crate::versions::loaders::LoaderVersion>> {
    {
        let guard = list_cache().lock().expect("forge meta cache mutex poisoned");
        if let Some(c) = guard.get(&(flavor, mc_id.to_string())) {
            if c.fetched_at.elapsed() < LIST_CACHE_TTL {
                return Ok(c.entries.clone());
            }
        }
    }

    // Fetch maven-metadata.xml.
    let meta_url = meta_url_for(flavor);
    let xml = crate::network::get_text(&meta_url, "forge/meta")
        .await
        .map_err(|e| Error::network(meta_url.clone(), format!("{e:?}")))?;
    let pairs = parse_maven_metadata(&xml)?;

    // Fetch promotions (optional). Failure → empty promotions table
    // (versions still listed, just none marked recommended).
    let promos = match promotions_url_for(flavor) {
        Some(url) => match crate::network::get_text(&url, "forge/promotions").await {
            Ok(body) => parse_promotions(&body).unwrap_or_default(),
            Err(_) => Promotions::default(),
        },
        None => Promotions::default(),
    };

    let (entries, raw_by_fv) = build_loader_versions(&pairs, mc_id, &promos);

    if entries.is_empty() {
        return Err(Error::LoaderUnavailable {
            loader: flavor.as_str().to_string(),
            mc_version: mc_id.to_string(),
        });
    }

    {
        let mut guard = list_cache().lock().expect("forge meta cache mutex poisoned");
        guard.insert(
            (flavor, mc_id.to_string()),
            CachedList {
                fetched_at: Instant::now(),
                entries: entries.clone(),
                raw_by_fv,
            },
        );
    }
    Ok(entries)
}

/// Look up the raw maven-version string for a `(flavor, mc, fv)` triple
/// from the cache. Returns `None` if `list_versions` hasn't been called
/// for this MC yet (callers should fall back to the canonical
/// `<mc>-<fv>` form).
pub(crate) fn cached_raw_version(flavor: ForgeFlavor, mc: &str, fv: &str) -> Option<String> {
    let guard = list_cache().lock().expect("forge meta cache mutex poisoned");
    guard
        .get(&(flavor, mc.to_string()))
        .and_then(|c| c.raw_by_fv.get(fv).cloned())
}

// ---- public API: fetch_installer_bytes ---------------------------

use std::path::PathBuf;

fn installer_cache_path(app_data_dir: &std::path::Path, mc: &str, fv: &str) -> PathBuf {
    app_data_dir
        .join("forge")
        .join("installers")
        .join(format!("{mc}-{fv}.jar"))
}

/// Fetch (or load from disk cache) the installer JAR bytes for
/// `(flavor, mc, fv)`. On a fresh download, writes to
/// `<app_data>/forge/installers/<mc>-<fv>.jar`; subsequent calls
/// return cached bytes.
pub async fn fetch_installer_bytes(
    flavor: ForgeFlavor,
    mc: &str,
    fv: &str,
    app: &tauri::AppHandle,
) -> Result<Vec<u8>> {
    let app_data = crate::paths::app_dir(app)
        .map_err(|e| Error::io("<app_data>", e))?;
    let path = installer_cache_path(&app_data, mc, fv);

    if path.exists() {
        return tokio::fs::read(&path)
            .await
            .map_err(|e| Error::io(path.display().to_string(), e));
    }

    // For MC ranges that use the `<mc>-<fv>-<mc>` maven path quirk
    // (1.7.10, parts of 1.9), `list_versions` will have cached the raw
    // form. Pass it through; for everything else `None` falls back to
    // the canonical `<mc>-<fv>` form.
    let raw = cached_raw_version(flavor, mc, fv);
    let url = if let Ok(base) = std::env::var(match flavor {
        ForgeFlavor::Forge => "FTLAUNCHER_FORGE_INSTALLER_OVERRIDE",
        ForgeFlavor::NeoForge => "FTLAUNCHER_NEOFORGE_INSTALLER_OVERRIDE",
    }) {
        format!(
            "{}/forge-{mc}-{fv}-installer.jar",
            base.trim_end_matches('/')
        )
    } else {
        flavor.installer_url(mc, fv, raw.as_deref())
    };

    let bytes = crate::network::get_bytes(&url, "forge/installer")
        .await
        .map_err(|e| Error::network(url.clone(), format!("{e:?}")))?;

    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| Error::io(parent.display().to_string(), e))?;
    }
    tokio::fs::write(&path, &bytes)
        .await
        .map_err(|e| Error::io(path.display().to_string(), e))?;

    Ok(bytes)
}

#[doc(hidden)]
pub fn clear_cache_for_test() {
    let mut guard = list_cache().lock().expect("forge meta cache mutex poisoned");
    guard.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fixture mirrors what Forge's real maven-metadata.xml looks like —
    // includes both the normal `<mc>-<fv>` shape and the legacy
    // `<mc>-<fv>-<mc>` quirk (1.7.10) plus the 1.9 patch-shape variant
    // (`1.9-...-1.9.0`).
    const FIXTURE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<metadata>
  <groupId>net.minecraftforge</groupId>
  <artifactId>forge</artifactId>
  <versioning>
    <versions>
      <version>1.6.4-9.11.1.965</version>
      <version>1.7.10-10.13.4.1614-1.7.10</version>
      <version>1.9-12.16.1.1938-1.9.0</version>
      <version>1.12.2-14.23.5.2860</version>
      <version>1.16.5-36.2.42</version>
      <version>1.20.4-49.0.49</version>
      <version>1.20.4-pre1-49.0.0</version>
    </versions>
  </versioning>
</metadata>"#;

    fn entry(mc: &str, fv: &str, raw: &str) -> MavenEntry {
        MavenEntry {
            mc: mc.into(),
            fv: fv.into(),
            raw: raw.into(),
        }
    }

    #[test]
    fn parses_realistic_maven_metadata() {
        let parsed = parse_maven_metadata(FIXTURE).expect("parse");
        assert_eq!(parsed.len(), 7);
        assert_eq!(parsed[0], entry("1.6.4", "9.11.1.965", "1.6.4-9.11.1.965"));
        // Quirk: trailing `-1.7.10` mirrors the leading MC; canonical (mc, fv)
        // comes from the inner split, raw preserves the full string.
        assert_eq!(
            parsed[1],
            entry("1.7.10", "10.13.4.1614", "1.7.10-10.13.4.1614-1.7.10")
        );
        // Quirk: trailing `-1.9.0` is a dotted extension of leading `1.9`.
        assert_eq!(
            parsed[2],
            entry("1.9", "12.16.1.1938", "1.9-12.16.1.1938-1.9.0")
        );
        assert_eq!(
            parsed[3],
            entry("1.12.2", "14.23.5.2860", "1.12.2-14.23.5.2860")
        );
        assert_eq!(parsed[4], entry("1.16.5", "36.2.42", "1.16.5-36.2.42"));
        assert_eq!(parsed[5], entry("1.20.4", "49.0.49", "1.20.4-49.0.49"));
        // MC version with `-pre1` keeps the suffix; the trailing `49.0.0` is
        // not a dotted overlap with `1.20.4`, so we fall back to the normal
        // `<mc>-<fv>` shape.
        assert_eq!(
            parsed[6],
            entry("1.20.4-pre1", "49.0.0", "1.20.4-pre1-49.0.0")
        );
    }

    #[test]
    fn malformed_xml_returns_typed_error() {
        let err = parse_maven_metadata("<metadata>incomplete").unwrap_err();
        match err {
            Error::ForgeMavenMetadataParseFailed { details } => {
                assert!(!details.is_empty(), "details should not be empty");
            }
            other => panic!("expected ForgeMavenMetadataParseFailed, got {other:?}"),
        }
    }

    #[test]
    fn empty_versions_list_returns_empty_vec() {
        let empty = r#"<?xml version="1.0"?>
<metadata><versioning><versions></versions></versioning></metadata>"#;
        let parsed = parse_maven_metadata(empty).expect("parse");
        assert!(parsed.is_empty());
    }

    #[test]
    fn entries_without_dash_are_dropped() {
        let weird = r#"<?xml version="1.0"?>
<metadata><versioning><versions>
  <version>nodash</version>
  <version>1.20.4-49.0.49</version>
</versions></versioning></metadata>"#;
        let parsed = parse_maven_metadata(weird).expect("parse");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].mc, "1.20.4");
        assert_eq!(parsed[0].fv, "49.0.49");
    }

    #[test]
    fn mc_suffix_matches_handles_exact_and_dotted_overlap() {
        assert!(mc_suffix_matches("1.7.10", "1.7.10"));
        assert!(mc_suffix_matches("1.9.0", "1.9")); // suffix extends prefix
        assert!(mc_suffix_matches("1.9", "1.9.0")); // and the reverse
        assert!(!mc_suffix_matches("49.0.0", "1.20.4"));
        assert!(!mc_suffix_matches("pre1", "1.20.4"));
    }

    #[test]
    fn parses_promotions_extracts_recommended_and_latest() {
        let json = r#"{
          "homepage": "https://files.minecraftforge.net/",
          "promos": {
            "1.20.4-recommended": "49.0.49",
            "1.20.4-latest": "49.0.49",
            "1.12.2-recommended": "14.23.5.2860",
            "1.12.2-latest": "14.23.5.2860",
            "1.7.10-recommended": "10.13.4.1614",
            "1.7.10-latest": "10.13.4.1614"
          }
        }"#;
        let p = parse_promotions(json).expect("parse");
        // Recommended wins over latest when both exist.
        assert_eq!(p.recommended_for("1.20.4"), Some("49.0.49"));
        assert_eq!(p.recommended_for("1.12.2"), Some("14.23.5.2860"));
        assert_eq!(p.recommended_for("1.7.10"), Some("10.13.4.1614"));
        // Unknown MC version → None.
        assert_eq!(p.recommended_for("99.99.99"), None);
    }

    #[test]
    fn recommended_falls_back_to_latest_when_recommended_missing() {
        let json = r#"{
          "promos": {
            "1.21-latest": "50.0.0"
          }
        }"#;
        let p = parse_promotions(json).expect("parse");
        assert_eq!(p.recommended_for("1.21"), Some("50.0.0"));
    }

    #[test]
    fn promotions_unknown_mc_returns_none() {
        let json = r#"{"promos":{}}"#;
        let p = parse_promotions(json).expect("parse");
        assert_eq!(p.recommended_for("1.20.4"), None);
    }

    #[test]
    fn malformed_promotions_json_returns_typed_error() {
        let err = parse_promotions("{not json").unwrap_err();
        match err {
            Error::ForgePromotionsUnavailable { flavor } => {
                assert_eq!(flavor, "forge");
            }
            other => panic!("expected ForgePromotionsUnavailable, got {other:?}"),
        }
    }

    #[test]
    fn version_parts_extracts_4_tuple() {
        assert_eq!(version_parts("47.2.0"), (47, 2, 0, 0));
        assert_eq!(version_parts("14.23.5.2860"), (14, 23, 5, 2860));
        assert_eq!(version_parts("10.13.4.1614"), (10, 13, 4, 1614));
        assert_eq!(version_parts("0"), (0, 0, 0, 0));
        // Defensive: non-numeric segment → 0 (rather than panic).
        assert_eq!(version_parts("47.x.0"), (47, 0, 0, 0));
    }

    #[test]
    fn build_loader_versions_sorts_descending_and_tags_stable() {
        let entries = vec![
            entry("1.20.4", "49.0.49", "1.20.4-49.0.49"),
            entry("1.20.4", "49.0.0", "1.20.4-49.0.0"),
            entry("1.20.4", "49.0.30", "1.20.4-49.0.30"),
            entry("1.20.4", "48.0.1", "1.20.4-48.0.1"),
            // filtered out — wrong MC
            entry("1.20.4-pre1", "49.0.0", "1.20.4-pre1-49.0.0"),
        ];
        let json = r#"{"promos":{"1.20.4-recommended":"49.0.30","1.20.4-latest":"49.0.49"}}"#;
        let promos = parse_promotions(json).expect("parse");

        let (out, raw) = build_loader_versions(&entries, "1.20.4", &promos);

        // Filtered to only "1.20.4" (4 entries).
        assert_eq!(out.len(), 4);
        // Sorted descending by version tuple.
        assert_eq!(out[0].version, "49.0.49");
        assert_eq!(out[1].version, "49.0.30");
        assert_eq!(out[2].version, "49.0.0");
        assert_eq!(out[3].version, "48.0.1");
        // Only "49.0.30" is recommended → stable=true.
        assert_eq!(out[0].stable, false);  // 49.0.49 is latest, but recommended is 49.0.30
        assert_eq!(out[1].stable, true);   // 49.0.30 = recommended
        assert_eq!(out[2].stable, false);
        assert_eq!(out[3].stable, false);
        // `build` field — unused for Forge (we don't have a build number);
        // populated as 0 so the struct is constructable.
        assert!(out.iter().all(|lv| lv.build == 0));
        // raw index covers all 4 retained entries, MC-pre1 excluded.
        assert_eq!(raw.len(), 4);
        assert_eq!(raw.get("49.0.49").map(String::as_str), Some("1.20.4-49.0.49"));
    }

    #[test]
    fn build_loader_versions_with_no_promotions_marks_none_stable() {
        let entries = vec![
            entry("1.7.10", "10.13.4.1614", "1.7.10-10.13.4.1614-1.7.10"),
            entry("1.7.10", "10.13.2.1291", "1.7.10-10.13.2.1291-1.7.10"),
        ];
        let promos = Promotions::default();
        let (out, raw) = build_loader_versions(&entries, "1.7.10", &promos);
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|lv| !lv.stable));
        // Raw index preserves the legacy-quirk maven path for URL building.
        assert_eq!(
            raw.get("10.13.4.1614").map(String::as_str),
            Some("1.7.10-10.13.4.1614-1.7.10")
        );
    }

    #[test]
    fn build_loader_versions_for_quirk_mc_via_realistic_metadata() {
        // Regression: prior to the MavenEntry/raw rework, filtering for
        // mc_id="1.7.10" against entries parsed from the duplicate-suffix
        // form returned an empty list (the canonical mc came back as
        // "1.7.10-10.13.4.1614"). This test pins the parse + build
        // pipeline against that.
        let parsed = parse_maven_metadata(FIXTURE).expect("parse");
        let (out, raw) = build_loader_versions(&parsed, "1.7.10", &Promotions::default());
        assert_eq!(out.len(), 1, "expected exactly one 1.7.10 entry");
        assert_eq!(out[0].version, "10.13.4.1614");
        assert_eq!(
            raw.get("10.13.4.1614").map(String::as_str),
            Some("1.7.10-10.13.4.1614-1.7.10")
        );
    }
}
