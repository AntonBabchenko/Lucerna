//! Top-level + per-component JRE manifest fetch and parse.
//!
//! Mojang publishes a JRE manifest at a hardcoded URL containing
//! per-platform per-component arrays of component refs. Each ref
//! points (via URL + SHA-1) at a per-component manifest enumerating
//! the actual files. The hardcoded top-level URL is bumped extremely
//! rarely (Mojang has used the same one since 2021).

use crate::error::{Error, Result};
use crate::network::get_json;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const TOP_LEVEL_URL: &str =
    "https://piston-meta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";
const CACHE_TTL: Duration = Duration::from_secs(5 * 60);

/// Top-level "all platforms" manifest. Outer key is Mojang's platform
/// tuple (e.g. `windows-x64`); inner key is component (e.g.
/// `java-runtime-gamma`).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TopLevelManifest(pub HashMap<String, HashMap<String, Vec<ComponentRef>>>);

/// One element of the platform/component array — version + URL to the
/// per-component manifest.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ComponentRef {
    pub manifest: ManifestRef,
    pub version: ComponentVersion,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ManifestRef {
    pub url: String,
    pub sha1: String,
    pub size: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ComponentVersion {
    pub name: String,
    pub released: String,
}

/// Per-component manifest — file tree the actual JRE consists of.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ComponentManifest {
    pub files: HashMap<String, FileEntry>,
}

/// One entry in the JRE file tree. The `type` tag drives variant
/// selection.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum FileEntry {
    File {
        #[serde(default)]
        executable: bool,
        downloads: FileDownloads,
    },
    Directory {},
    /// macOS/Linux only. Windows components don't emit these; if one
    /// arrives we hard-error in `install::ensure_jre`.
    Link {
        target: String,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FileDownloads {
    pub raw: FileDownload,
    /// `lzma` is the same file pre-compressed. We ignore it in v0.1.0;
    /// adding the lzma crate dependency is a separate decision.
    #[serde(default)]
    pub lzma: Option<FileDownload>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FileDownload {
    pub url: String,
    pub sha1: String,
    pub size: u64,
}

// ---------------------------------------------------------------- cache

struct Cached {
    fetched_at: Instant,
    manifest: TopLevelManifest,
}

fn cache() -> &'static Mutex<Option<Cached>> {
    static CACHE: OnceLock<Mutex<Option<Cached>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

/// Fetch the top-level "all platforms" JRE manifest. Uses an in-memory
/// 5-minute cache. Env var `FTLAUNCHER_JRE_TOPLEVEL_URL_OVERRIDE`
/// substitutes the URL — only set by integration tests.
pub async fn fetch_top_level() -> Result<TopLevelManifest> {
    {
        let guard = cache().lock().expect("jre top-level cache mutex poisoned");
        if let Some(c) = guard.as_ref() {
            if c.fetched_at.elapsed() < CACHE_TTL {
                return Ok(c.manifest.clone());
            }
        }
    }

    let url = std::env::var("FTLAUNCHER_JRE_TOPLEVEL_URL_OVERRIDE")
        .ok()
        .unwrap_or_else(|| TOP_LEVEL_URL.to_string());
    let manifest: TopLevelManifest = get_json(&url, "jre").await?;

    {
        let mut guard = cache().lock().expect("jre top-level cache mutex poisoned");
        *guard = Some(Cached {
            fetched_at: Instant::now(),
            manifest: manifest.clone(),
        });
    }

    Ok(manifest)
}

/// Force-clear the cache. Used by integration tests in external crates.
#[doc(hidden)]
pub fn clear_cache_for_test() {
    let mut guard = cache().lock().expect("jre top-level cache mutex poisoned");
    *guard = None;
}

// ---------------------------------------------------------------- pick

/// Map our internal (os, arch) tuple to Mojang's platform key.
/// Returns `UnsupportedPlatform` for anything Mojang doesn't ship.
pub fn mojang_platform_key(os: &str, arch: &str) -> Result<&'static str> {
    match (os, arch) {
        ("windows", "x64") => Ok("windows-x64"),
        ("windows", "x86") => Ok("windows-x86"),
        ("windows", "aarch64") => Ok("windows-arm64"),
        ("linux", "x64") => Ok("linux"),
        ("linux", "x86") => Ok("linux-i386"),
        ("macos", "x64") => Ok("mac-os"),
        ("macos", "aarch64") => Ok("mac-os-arm64"),
        _ => Err(Error::UnsupportedPlatform {
            os: os.into(),
            arch: arch.into(),
        }),
    }
}

/// Look up one component for one platform. Returns the first entry
/// (Mojang publishes a single-element array per slot in practice).
pub fn pick_component(
    top: &TopLevelManifest,
    platform_key: &str,
    component: &str,
) -> Result<ComponentRef> {
    let by_component = top
        .0
        .get(platform_key)
        .ok_or_else(|| Error::UnsupportedPlatform {
            os: platform_key.into(),
            arch: "platform-key".into(),
        })?;
    let arr = by_component
        .get(component)
        .ok_or_else(|| Error::UnknownVersion {
            id: format!("jre/{component}@{platform_key}"),
        })?;
    arr.first().cloned().ok_or_else(|| Error::UnknownVersion {
        id: format!("jre/{component}@{platform_key} (empty)"),
    })
}

/// Fetch + parse the per-component manifest. The SHA-1 in
/// `top.manifest.sha1` is not re-verified at this layer — per-file
/// SHA-1 downstream is the real integrity gate. If a real-world flaky
/// case shows up where the manifest itself is tampered with but
/// schema-valid, add a `network::get_with_sha` that returns raw bytes.
pub async fn fetch_component_manifest(component_ref: &ComponentRef) -> Result<ComponentManifest> {
    let manifest: ComponentManifest = get_json(&component_ref.manifest.url, "jre").await?;
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_top_level_with_known_platforms() {
        let json = r#"{
          "windows-x64": {
            "java-runtime-gamma": [
              {
                "availability": {"group": 1, "progress": 100},
                "manifest": {"url": "https://example/comp.json", "sha1": "aaa", "size": 1234},
                "version": {"name": "21.0.3", "released": "2024-04-16"}
              }
            ],
            "jre-legacy": [
              {
                "availability": {"group": 1, "progress": 100},
                "manifest": {"url": "https://example/legacy.json", "sha1": "bbb", "size": 5678},
                "version": {"name": "8u402", "released": "2024-01-23"}
              }
            ]
          },
          "linux": {}
        }"#;
        let top: TopLevelManifest = serde_json::from_str(json).expect("parse");
        let win = top.0.get("windows-x64").expect("windows-x64 present");
        assert!(win.contains_key("java-runtime-gamma"));
        let gamma = pick_component(&top, "windows-x64", "java-runtime-gamma").expect("pick gamma");
        assert_eq!(gamma.version.name, "21.0.3");
        assert_eq!(gamma.manifest.sha1, "aaa");
    }

    #[test]
    fn platform_key_known_combos() {
        assert_eq!(
            mojang_platform_key("windows", "x64").unwrap(),
            "windows-x64"
        );
        assert_eq!(
            mojang_platform_key("macos", "aarch64").unwrap(),
            "mac-os-arm64"
        );
        assert_eq!(mojang_platform_key("linux", "x64").unwrap(), "linux");
        assert_eq!(mojang_platform_key("linux", "x86").unwrap(), "linux-i386");
    }

    #[test]
    fn platform_key_unknown_combo_errors() {
        let err = mojang_platform_key("freebsd", "x64").unwrap_err();
        match err {
            Error::UnsupportedPlatform { os, arch } => {
                assert_eq!(os, "freebsd");
                assert_eq!(arch, "x64");
            }
            other => panic!("expected UnsupportedPlatform, got {other:?}"),
        }
    }

    #[test]
    fn pick_component_missing_platform_errors() {
        let top = TopLevelManifest(HashMap::new());
        let err = pick_component(&top, "windows-x64", "java-runtime-gamma").unwrap_err();
        assert!(matches!(err, Error::UnsupportedPlatform { .. }));
    }

    #[test]
    fn pick_component_missing_component_errors() {
        let mut by_comp = HashMap::new();
        by_comp.insert("java-runtime-gamma".to_string(), vec![]);
        let mut platforms = HashMap::new();
        platforms.insert("windows-x64".to_string(), by_comp);
        let top = TopLevelManifest(platforms);
        let err = pick_component(&top, "windows-x64", "java-runtime-alpha").unwrap_err();
        assert!(matches!(err, Error::UnknownVersion { .. }));
    }

    #[test]
    fn file_entry_deserialises_all_three_variants() {
        let json = r#"{
          "bin/java.exe": {
            "type": "file",
            "executable": true,
            "downloads": {
              "raw": {"url": "https://x/java.exe", "sha1": "aa", "size": 100},
              "lzma": {"url": "https://x/java.exe.lzma", "sha1": "bb", "size": 60}
            }
          },
          "bin": {"type": "directory"},
          "legal/link": {"type": "link", "target": "../legal"}
        }"#;
        let map: HashMap<String, FileEntry> = serde_json::from_str(json).expect("parse");
        match &map["bin/java.exe"] {
            FileEntry::File {
                executable,
                downloads,
            } => {
                assert!(*executable);
                assert_eq!(downloads.raw.size, 100);
                assert!(downloads.lzma.is_some());
            }
            other => panic!("expected File, got {other:?}"),
        }
        assert!(matches!(map["bin"], FileEntry::Directory {}));
        match &map["legal/link"] {
            FileEntry::Link { target } => assert_eq!(target, "../legal"),
            other => panic!("expected Link, got {other:?}"),
        }
    }
}
