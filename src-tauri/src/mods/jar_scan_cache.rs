//! Disk-backed cache of one jar's parsed descriptor, keyed by the jar's SHA-1.
//!
//! Two detectors read every jar of an instance on every Installed-tab open —
//! `local::scan_instance` for the loader family and
//! `preflight::dependency_preflight_for_root` for providers and dependencies —
//! and on a legacy instance the second one additionally decompresses class
//! entries looking for the `@Mod` annotation. That is affordable once per jar,
//! not once per tab open.
//!
//! SHA-1 is the right key: it is already computed and stored in
//! `installed-mods.json`, and it changes exactly when the jar's bytes do, so no
//! TTL and no invalidation logic are needed. Derived data — safe to delete.
//!
//! The load/mutate/save cycle mirrors [`crate::l10n::coverage::ScanCache`],
//! including the private `save` behind a disk lock: the temp filename is only
//! `tmp.<pid>`, which is safe across launcher processes but not against two
//! concurrent saves inside this one.
//!
//! Deliberately NOT `specta::Type`: these are disk-cache records, not part of
//! any command's return type, so they never cross IPC.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::mods::local::{DeclaredDep, JarMeta, ManifestDeps, ProvidedMod};

/// Serializes the disk read-modify-write; held only over the synchronous
/// load/save, never across a scan. Mirrors `l10n::coverage::CACHE_DISK_LOCK`.
static CACHE_DISK_LOCK: Mutex<()> = Mutex::new(());

/// Everything both jar readers produce for one jar.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CachedScan {
    pub meta: JarMeta,
    pub manifest: ManifestDeps,
    /// Legacy `@Mod` requirements, empty on a modern jar. Stored separately so a
    /// modern-era read never has to reason about them — the era belongs to the
    /// instance, not to the jar, and the same jar can be read from both.
    #[serde(default)]
    pub legacy_deps: Vec<DeclaredDep>,
    /// `read_jar_embedded_providers` — the JIJ pass, which recursively unzips
    /// every nested jar. The third full read of the same bytes today, and the
    /// most expensive of the three.
    #[serde(default)]
    pub jij_provided: Vec<ProvidedMod>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ScanCache {
    #[serde(default)]
    entries: BTreeMap<String, CachedScan>,
}

impl ScanCache {
    /// A missing or malformed file yields an empty cache — never an error.
    pub fn load(path: &Path) -> Self {
        let Ok(raw) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        serde_json::from_str(&raw).unwrap_or_default()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, sha1: &str) -> Option<&CachedScan> {
        self.entries.get(&sha1.to_ascii_lowercase())
    }

    pub fn put(&mut self, sha1: &str, entry: CachedScan) {
        self.entries.insert(sha1.to_ascii_lowercase(), entry);
    }

    /// Atomic write (per-process temp + rename), creating the parent dir.
    /// Private for the same reason `l10n::coverage`'s is: the temp filename is
    /// only `tmp.<pid>`, so two concurrent saves in this process would race the
    /// rename. [`Self::update`] is the only caller and holds the disk lock for
    /// the whole cycle.
    fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string(self)?;
        let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, path)
    }

    /// Load, mutate, save under the disk lock — the only sanctioned write path.
    pub fn update(path: &Path, f: impl FnOnce(&mut Self)) {
        let _g = CACHE_DISK_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let mut c = Self::load(path);
        f(&mut c);
        let _ = c.save(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_changed_jar_misses_because_its_sha1_changed() {
        let td = tempfile::TempDir::new().unwrap();
        let path = td.path().join("jar-scans.json");
        ScanCache::update(&path, |c| c.put("AA", CachedScan::default()));
        // Keys are case-folded, so a registry that stores upper-case hex and a
        // caller that lower-cases it still hit the same entry.
        assert!(ScanCache::load(&path).get("aa").is_some());
        assert!(ScanCache::load(&path).get("bb").is_none());
    }

    #[test]
    fn a_missing_or_corrupt_file_is_an_empty_cache_not_an_error() {
        let td = tempfile::TempDir::new().unwrap();
        assert!(ScanCache::load(&td.path().join("nope.json")).is_empty());
        let bad = td.path().join("bad.json");
        std::fs::write(&bad, b"{ not json").unwrap();
        assert!(ScanCache::load(&bad).is_empty());
    }

    #[test]
    fn a_stored_scan_round_trips_with_its_descriptor_provenance() {
        use crate::mods::local::{DepSide, DependencyKind, DescriptorSource};
        use crate::mods::version_range::RangeFamily;

        let td = tempfile::TempDir::new().unwrap();
        let path = td.path().join("jar-scans.json");
        let entry = CachedScan {
            meta: JarMeta::default(),
            manifest: ManifestDeps::default(),
            legacy_deps: vec![DeclaredDep {
                dep_id: "creativecore".into(),
                range: String::new(),
                kind: DependencyKind::Required,
                side: DepSide::Both,
                family: RangeFamily::Maven,
                source: DescriptorSource::McmodAnnotation,
            }],
            jij_provided: vec![ProvidedMod {
                mod_id: "forgified_fabric_api".into(),
                version: Some("0.92.2".into()),
            }],
        };
        ScanCache::update(&path, |c| c.put("ab", entry.clone()));
        assert_eq!(ScanCache::load(&path).get("ab"), Some(&entry));
    }
}
