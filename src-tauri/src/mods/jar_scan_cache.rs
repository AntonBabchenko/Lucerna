//! Disk-backed cache of one jar's parsed descriptors, keyed by the jar's SHA-1.
//!
//! Two detectors read every jar of an instance on every Installed-tab open —
//! `local::scan_instance` for the loader family and
//! `preflight::dependency_preflight_for_root` for providers and dependencies —
//! and on a legacy instance the second one additionally decompresses class
//! entries looking for the `@Mod` annotation. That is affordable once per jar,
//! not once per tab open — and the pre-flight also sits in the launch
//! chokepoint (`+page.svelte`'s `startLaunch`), so every Play press paid it.
//!
//! SHA-1 is the right key, with one caveat the CALLER owns: it must be the
//! digest of the bytes on disk, not `InstalledMod::sha1`. `installed::reconcile`
//! deliberately keeps a record's EXPECTED digest when the file under that name
//! holds different bytes, so the registry value can describe a jar that is no
//! longer there. [`crate::mods::installed::on_disk_sha1`] is the key source;
//! see its doc for why.
//!
//! Every payload field is an `Option`, and that is load-bearing rather than
//! tidy. The two readers want different subsets — the compat scan wants `meta`
//! + `manifest`, the pre-flight wants `manifest` + `jij_provided` and, on a
//! legacy instance, `legacy_deps` — so a record is written partially and
//! completed later. An empty `Vec` cannot say which of "scanned, nothing
//! there" and "never scanned" it means, and on `legacy_deps` those are "this
//! 1.12.2 mod requires nothing" versus "we have not looked": the first is a lie
//! that silences every requirement that era declares.
//!
//! [`SCHEMA_VERSION`] is the second half of the same honesty. The key answers
//! "did the JAR change"; nothing in it answers "did the READER change". These
//! records are the parsers' output, and those parsers have been corrected twice
//! already (#344, #345) — a stale record survives the correction under the same
//! SHA-1 and reports the old, wrong parse.
//!
//! The load/mutate/save cycle mirrors [`crate::l10n::coverage::ScanCache`],
//! including the private `save` behind a disk lock: the temp filename is only
//! `tmp.<pid>`, which is safe across launcher processes but not against two
//! concurrent saves inside this one.
//!
//! Derived data — safe to delete, rebuilt on demand.
//!
//! Deliberately NOT `specta::Type`: these are disk-cache records, not part of
//! any command's return type, so they never cross IPC.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::mods::local::{DeclaredDep, JarMeta, ManifestDeps, ProvidedMod};

/// Bump whenever ANY jar reader's output changes meaning: a new
/// `DescriptorSource`, a new `JarMeta` field a verdict reads, a corrected
/// dependency-kind rule. A jar's SHA-1 cannot notice a parser fix, so this is
/// the only thing standing between a corrected reader and a cache full of the
/// bug it corrected.
///
/// A mismatch discards the ENTIRE file rather than the entries predating it:
/// records carry no per-entry stamp, and the whole file is derived data that
/// rebuilds itself on the next scan.
pub const SCHEMA_VERSION: u32 = 1;

/// Serializes the disk read-modify-write; held only over the synchronous
/// load/save, never across a scan. Mirrors `l10n::coverage::CACHE_DISK_LOCK`.
static CACHE_DISK_LOCK: Mutex<()> = Mutex::new(());

/// What the jar readers produced for one jar, field by field.
///
/// `None` means NOT SCANNED. It never means "scanned, found nothing" — see the
/// module doc for why that distinction is the whole shape of this type.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CachedScan {
    /// `local::read_jar_meta` — loader families, label, descriptor presence.
    #[serde(default)]
    pub meta: Option<JarMeta>,
    /// `local::read_jar_manifest_deps` — providers, declared deps, platform.
    #[serde(default)]
    pub manifest: Option<ManifestDeps>,
    /// `local::read_jar_legacy_deps` — the `@Mod(dependencies = …)` string,
    /// read ONLY for a `DescriptorEra::Legacy` instance. `Some(vec![])` is a
    /// measured "this jar declares none"; `None` is "no legacy-era reader has
    /// ever opened this jar".
    #[serde(default)]
    pub legacy_deps: Option<Vec<DeclaredDep>>,
    /// `local::read_jar_embedded_providers` — the JIJ pass, which recursively
    /// unzips every nested jar. The most expensive of the readers.
    #[serde(default)]
    pub jij_provided: Option<Vec<ProvidedMod>>,
}

impl CachedScan {
    /// Fold a freshly-scanned record into this one: every field `other` has
    /// measured replaces ours, every field it has not measured leaves ours
    /// alone.
    ///
    /// This is why [`ScanCache`] exposes no whole-record `put`. Both records
    /// describe the same bytes — they share a SHA-1 key — so an overwrite is
    /// never wrong about a field it carries, but it is wrong about the fields
    /// it does not: a pre-flight storing its three fields over a compat scan's
    /// `meta` would delete that `meta` and send the next compat scan back to
    /// unzip a jar whose answer was already on disk.
    pub fn merge_from(&mut self, other: CachedScan) {
        if other.meta.is_some() {
            self.meta = other.meta;
        }
        if other.manifest.is_some() {
            self.manifest = other.manifest;
        }
        if other.legacy_deps.is_some() {
            self.legacy_deps = other.legacy_deps;
        }
        if other.jij_provided.is_some() {
            self.jij_provided = other.jij_provided;
        }
    }
}

/// Per-jar parsed descriptors, keyed by the jar's on-disk SHA-1.
#[derive(Debug, Serialize, Deserialize)]
pub struct ScanCache {
    /// Written on every save, checked on every load — see [`SCHEMA_VERSION`].
    /// `#[serde(default)]` so a file predating this field reads as version 0
    /// and is discarded, which is the right answer for one.
    #[serde(default)]
    version: u32,
    #[serde(default)]
    entries: BTreeMap<String, CachedScan>,
}

impl Default for ScanCache {
    fn default() -> Self {
        Self {
            version: SCHEMA_VERSION,
            entries: BTreeMap::new(),
        }
    }
}

impl ScanCache {
    /// A missing, malformed, or out-of-date file yields an EMPTY cache — never
    /// an error and never a partial one. All three are the same fact to every
    /// caller: nothing here can be believed, so read the jars.
    pub fn load(path: &Path) -> Self {
        let Ok(raw) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        let parsed: Self = serde_json::from_str(&raw).unwrap_or_default();
        if parsed.version != SCHEMA_VERSION {
            return Self::default();
        }
        parsed
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Case-folded: a registry storing upper-case hex and a caller lower-casing
    /// it must not occupy two entries for one jar.
    pub fn get(&self, sha1: &str) -> Option<&CachedScan> {
        self.entries.get(&sha1.to_ascii_lowercase())
    }

    /// Fold `entry`'s measured fields into whatever is stored for `sha1`. The
    /// only way in — see [`CachedScan::merge_from`].
    pub fn merge(&mut self, sha1: &str, entry: CachedScan) {
        self.entries
            .entry(sha1.to_ascii_lowercase())
            .or_default()
            .merge_from(entry);
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
    ///
    /// A failed save is LOGGED, not discarded: the scan's own answer is already
    /// correct and unaffected, but "the cache silently stopped persisting" and
    /// "the cache is working" must not look the same in a bug report. Mirrors
    /// `l10n::coverage::ScanCache::update`.
    pub fn update(path: &Path, f: impl FnOnce(&mut Self)) {
        // Deliberate poison-recovery, not an unwrap: a prior panicking holder
        // must not permanently break every future cache read/write.
        let _g = CACHE_DISK_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let mut c = Self::load(path);
        f(&mut c);
        if let Err(e) = c.save(path) {
            crate::diag!("[mods] jar-scan cache save failed ({}): {e}", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::local::{DepSide, DependencyKind, DescriptorSource};
    use crate::mods::version_range::RangeFamily;

    fn legacy_dep(id: &str) -> DeclaredDep {
        DeclaredDep {
            dep_id: id.into(),
            range: String::new(),
            kind: DependencyKind::Required,
            side: DepSide::Both,
            family: RangeFamily::Maven,
            source: DescriptorSource::McmodAnnotation,
        }
    }

    /// THE headline test. A record written while scanning a MODERN instance has
    /// never opened the `@Mod` annotation, and must not read back as "this jar
    /// declares no legacy dependencies" — on a 1.12.2 instance the annotation is
    /// the only place requirements exist, so that emptiness would silence every
    /// one of them.
    #[test]
    fn a_field_nobody_measured_is_none_not_an_empty_measurement() {
        let td = tempfile::TempDir::new().unwrap();
        let path = td.path().join("jar-scans.json");
        ScanCache::update(&path, |c| {
            c.merge(
                "aa",
                CachedScan {
                    manifest: Some(ManifestDeps::default()),
                    jij_provided: Some(Vec::new()),
                    ..CachedScan::default()
                },
            )
        });
        let hit = ScanCache::load(&path).get("aa").cloned().expect("stored");
        assert!(hit.manifest.is_some(), "the reader that ran");
        assert_eq!(
            hit.jij_provided,
            Some(Vec::new()),
            "a JIJ pass that found nothing is a measurement"
        );
        assert!(
            hit.legacy_deps.is_none(),
            "the annotation reader never ran; an empty Vec here would be a lie"
        );
        assert!(hit.meta.is_none(), "and neither did read_jar_meta");
    }

    /// Two readers, one jar, two writes. The second must not delete the first's
    /// field — otherwise the surfaces take turns re-unzipping the same bytes.
    #[test]
    fn merging_a_second_readers_result_keeps_the_first_readers_field() {
        let td = tempfile::TempDir::new().unwrap();
        let path = td.path().join("jar-scans.json");
        ScanCache::update(&path, |c| {
            c.merge(
                "ab",
                CachedScan {
                    meta: Some(JarMeta::default()),
                    manifest: Some(ManifestDeps::default()),
                    ..CachedScan::default()
                },
            )
        });
        ScanCache::update(&path, |c| {
            c.merge(
                "ab",
                CachedScan {
                    legacy_deps: Some(vec![legacy_dep("creativecore")]),
                    jij_provided: Some(Vec::new()),
                    ..CachedScan::default()
                },
            )
        });
        let hit = ScanCache::load(&path).get("ab").cloned().expect("stored");
        assert!(hit.meta.is_some(), "the compat scan's half survives");
        assert_eq!(hit.legacy_deps.unwrap().len(), 1);
    }

    /// A jar's SHA-1 cannot notice that the PARSER changed. Every record is a
    /// parser output, so the file carries the parser's version and a mismatch
    /// is a miss — not a confidently-served stale parse.
    #[test]
    fn a_file_written_under_an_older_schema_is_a_miss_not_a_stale_answer() {
        let td = tempfile::TempDir::new().unwrap();
        let path = td.path().join("jar-scans.json");
        // Every field spelled out, so the fixture is shape-neutral: it
        // deserializes under the pre-fix `Vec`/bare shape too, which is what
        // makes this test genuinely red rather than vacuously green.
        let older = serde_json::json!({
            "version": SCHEMA_VERSION - 1,
            "entries": {
                "aa": {
                    "meta": {
                        "families": [],
                        "loader_label": null,
                        "mc_version": null,
                        "display_name": null,
                        "has_fabric_json": false,
                        "has_quilt_json": false,
                        "has_forge_toml": false,
                        "has_neoforge_toml": false,
                        "has_mcmod_info": false
                    },
                    "manifest": {
                        "provided": [], "deps": [], "sources_present": [], "platform": []
                    },
                    "legacy_deps": [],
                    "jij_provided": []
                }
            }
        });
        std::fs::write(&path, older.to_string()).unwrap();
        assert!(
            ScanCache::load(&path).is_empty(),
            "a record from a previous parser must not be served under the new one"
        );
    }

    /// A record whose payload no longer deserializes must read as a MISS, not
    /// as a record with the broken section silently defaulted away. The
    /// `#[serde(default)]` on every field makes an ABSENT key mean "not
    /// scanned"; a key that is present but malformed is a different fact, and
    /// serde must reject it rather than turn it into that same `None` — because
    /// `None` is what a reader is entitled to fill in and store as measured.
    #[test]
    fn a_corrupt_entry_is_a_miss_never_a_partially_believed_record() {
        let td = tempfile::TempDir::new().unwrap();
        let path = td.path().join("jar-scans.json");
        // `manifest` present but the wrong shape; `meta` perfectly good. Believing
        // the good half of a file we could not fully parse is exactly the lie the
        // whole-file discard exists to prevent.
        let broken = serde_json::json!({
            "version": SCHEMA_VERSION,
            "entries": {
                "aa": {
                    "meta": {
                        "families": [],
                        "loader_label": null,
                        "mc_version": null,
                        "display_name": null,
                        "has_fabric_json": false,
                        "has_quilt_json": false,
                        "has_forge_toml": false,
                        "has_neoforge_toml": false,
                        "has_mcmod_info": false
                    },
                    "manifest": "this is not a ManifestDeps"
                }
            }
        });
        std::fs::write(&path, broken.to_string()).unwrap();
        let loaded = ScanCache::load(&path);
        assert!(
            loaded.is_empty(),
            "a record that would not parse must answer nothing at all"
        );
        assert!(loaded.get("aa").is_none());
    }

    #[test]
    fn a_changed_jar_misses_because_its_sha1_changed() {
        let td = tempfile::TempDir::new().unwrap();
        let path = td.path().join("jar-scans.json");
        ScanCache::update(&path, |c| c.merge("AA", CachedScan::default()));
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

    /// A freshly-saved file must carry the CURRENT version. `Default` is
    /// hand-written for exactly this: a derived one would stamp `version: 0` on
    /// every save, `load` would discard it as foreign, and the cache would be
    /// silently inert with every gate green.
    #[test]
    fn a_saved_cache_carries_the_current_schema_version_and_survives_a_reload() {
        let td = tempfile::TempDir::new().unwrap();
        let path = td.path().join("jar-scans.json");
        ScanCache::update(&path, |c| {
            c.merge(
                "aa",
                CachedScan {
                    manifest: Some(ManifestDeps::default()),
                    ..CachedScan::default()
                },
            )
        });
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("\"version\":1"), "got {raw}");
        assert_eq!(ScanCache::load(&path).len(), 1, "and reloads, not discarded");
    }

    #[test]
    fn a_stored_scan_round_trips_with_its_descriptor_provenance() {
        let td = tempfile::TempDir::new().unwrap();
        let path = td.path().join("jar-scans.json");
        let entry = CachedScan {
            meta: Some(JarMeta::default()),
            manifest: Some(ManifestDeps::default()),
            legacy_deps: Some(vec![legacy_dep("creativecore")]),
            jij_provided: Some(vec![ProvidedMod {
                mod_id: "forgified_fabric_api".into(),
                version: Some("0.92.2".into()),
                source: DescriptorSource::NeoForgeToml,
            }]),
        };
        ScanCache::update(&path, |c| c.merge("ab", entry.clone()));
        assert_eq!(ScanCache::load(&path).get("ab"), Some(&entry));
    }
}
