//! Per-namespace translation coverage for one instance.
//!
//! Counted per KEY, never per file: a mod that ships a `ru_ru.json` covering
//! half its strings is half-translated, and the player sees English for the
//! rest (the game loads en_us into the shared map first, so missing keys fall
//! back rather than disappearing).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::l10n::scan::LangMap;

/// Translation coverage for one resource namespace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NamespaceCoverage {
    pub namespace: String,
    /// Keys in the namespace's `en_us` file — the denominator.
    pub total_keys: u32,
    /// Keys the mod itself translates into the target language.
    pub from_mod: u32,
    /// Keys the user overrides that the mod does NOT already translate.
    /// Disjoint from `from_mod` by construction so `covered()` is a plain sum.
    pub overridden: u32,
}

impl NamespaceCoverage {
    pub fn covered(&self) -> u32 {
        self.from_mod + self.overridden
    }

    /// Whole-percent coverage, truncated. A namespace with no English keys is
    /// 100% — there is nothing to translate, and reporting 0% would read as a
    /// problem.
    pub fn percent(&self) -> u32 {
        if self.total_keys == 0 {
            return 100;
        }
        (self.covered() as u64 * 100 / self.total_keys as u64) as u32
    }
}

/// Coverage for one namespace given its English file, the mod's own target
/// language file (if any), and the keys the user overrides.
///
/// Only keys present in `en` count, toward either `from_mod` or `overridden`:
/// a target file carrying keys the mod has since dropped must not inflate the
/// result, and neither may an override for a key the mod no longer ships.
/// `override_keys` may contain duplicates or overlap with what the mod
/// already translates — both are resolved internally (via a set) so
/// `covered()` is a correct plain sum for ANY input, not just well-formed
/// ones. Deliberately not `&BTreeSet<String>`: that would move the dedup
/// obligation onto every caller, and a function that is correct regardless of
/// what it is handed is worth the small allocation over one that merely
/// documents an obligation nobody is forced to honour.
pub fn namespace_coverage(
    namespace: &str,
    en: &LangMap,
    target: Option<&LangMap>,
    override_keys: &[String],
) -> NamespaceCoverage {
    let from_mod = match target {
        Some(t) => en.keys().filter(|k| t.contains_key(*k)).count() as u32,
        None => 0,
    };
    let overridden = override_keys
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|k| en.contains_key(*k) && !target.is_some_and(|t| t.contains_key(*k)))
        .count() as u32;
    NamespaceCoverage {
        namespace: namespace.to_string(),
        total_keys: en.len() as u32,
        from_mod,
        overridden,
    }
}

/// Instance-wide percentage, weighted by key count. Averaging the per-namespace
/// percentages would let a tiny fully-translated mod mask a huge untranslated
/// one — the opposite of what the player experiences.
pub fn instance_percent(all: &[NamespaceCoverage]) -> u32 {
    let total: u64 = all.iter().map(|c| c.total_keys as u64).sum();
    if total == 0 {
        return 100;
    }
    let covered: u64 = all.iter().map(|c| c.covered() as u64).sum();
    (covered * 100 / total) as u32
}

/// Coverage for one instance: per-namespace counts plus the languages the
/// installed mods actually ship, so the UI can offer a real target list rather
/// than a hardcoded one.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct InstanceCoverage {
    pub lang: String,
    pub percent: u32,
    pub namespaces: Vec<NamespaceCoverage>,
    /// Every language code found in any installed mod, sorted. Always contains
    /// `lang` even when nothing ships it, so the picker can show the current
    /// selection.
    pub available_codes: Vec<String>,
    /// Whether the override editor's Apply action is available right now, and
    /// why not when it isn't — so the UI can disable the button with an
    /// explanation instead of letting the user press it and hit an error.
    /// Resolved from the SAME client jar `commands::l10n_apply` itself reads
    /// (`crate::l10n::pack_format::from_client_jar_path`), so the report and
    /// the button can never disagree about whether Apply will work. Cheap to
    /// compute on every scan: only the jar's central directory and its tiny
    /// `version.json` entry are read, never the ~tens-of-megabytes jar body.
    pub apply_gate: crate::l10n::pack_format::ApplyGate,
}

/// Map a launcher UI locale (`ru`, `en`, or a fuller tag) to a Minecraft
/// language code. Minecraft codes are always `<lang>_<region>` lowercase; for
/// the common case the region repeats the language.
pub fn default_target_code(ui_locale: &str) -> String {
    let lower = ui_locale.to_ascii_lowercase().replace('-', "_");
    if lower.is_empty() {
        return "en_us".to_string();
    }
    if lower.contains('_') {
        return lower;
    }
    match lower.as_str() {
        "en" => "en_us".to_string(),
        "ru" => "ru_ru".to_string(),
        other => format!("{other}_{other}"),
    }
}

/// Scan every enabled mod jar of an instance and aggregate coverage.
///
/// Reads jars only. An instance's mod jars are HARDLINKS into a shared content
/// store — opening one for writing would corrupt that mod for every instance
/// sharing it.
///
/// `versions_dir` + `mc_version` locate the instance's PRISTINE vanilla
/// client jar (`versions/<mc_version>/<mc_version>.jar` — the same jar for
/// every loader, see `l10n::pack_format`'s module doc), read here purely to
/// fill `InstanceCoverage::apply_gate`. `mc_version` must be the instance's
/// raw version, never the effective/synth id — a synth-named jar would not
/// exist at that path.
pub async fn scan_instance(
    inst_root: &Path,
    cache_path: &Path,
    store_dir: &Path,
    versions_dir: &Path,
    mc_version: &str,
    lang: &str,
) -> Result<InstanceCoverage, crate::error::Error> {
    let installed = crate::mods::installed::list(inst_root).await?;
    let mods_dir = crate::mods::installed::mods_dir(inst_root);

    let cached = ScanCache::load(cache_path);

    let mut per_ns: BTreeMap<String, NamespaceCoverage> = BTreeMap::new();
    let mut available: BTreeSet<String> = BTreeSet::new();
    let mut fresh: Vec<(String, CachedJarScan)> = Vec::new();

    for m in installed.iter().filter(|m| m.enabled) {
        if let Some(hit) = cached.get(lang, &m.sha1) {
            available.extend(hit.codes.iter().cloned());
            for c in &hit.namespaces {
                merge_into(&mut per_ns, c.clone());
            }
            continue;
        }
        let Ok(bytes) = tokio::fs::read(mods_dir.join(&m.filename)).await else {
            continue; // a jar that vanished between list() and here
        };
        // A present-but-unreadable jar (truncated download, unusual archive)
        // is no worse than the already-handled "missing" case above: both
        // mean this mod's coverage is unknowable right now. Skip it rather
        // than aborting the whole instance's report over one bad jar.
        let (cov, codes) = match scan_one_jar(&bytes, lang, store_dir) {
            Ok(v) => v,
            Err(e) => {
                crate::diag!("[l10n] scan failed for {}: {e}", m.filename);
                continue;
            }
        };
        available.extend(codes.iter().cloned());
        for c in &cov {
            merge_into(&mut per_ns, c.clone());
        }
        fresh.push((
            m.sha1.clone(),
            CachedJarScan {
                codes,
                namespaces: cov,
            },
        ));
    }

    if !fresh.is_empty() {
        ScanCache::update(cache_path, |c| {
            for (sha, entry) in fresh {
                c.put(lang, &sha, entry);
            }
        });
    }

    let namespaces: Vec<NamespaceCoverage> = per_ns.into_values().collect();
    available.insert(lang.to_string());

    let client_jar = crate::datapacks::compat::client_jar_path(versions_dir, mc_version);
    let fmt = crate::l10n::pack_format::from_client_jar_path(&client_jar);
    let apply_gate = crate::l10n::pack_format::apply_gate(fmt);

    Ok(InstanceCoverage {
        lang: lang.to_string(),
        percent: instance_percent(&namespaces),
        namespaces,
        available_codes: available.into_iter().collect(),
        apply_gate,
    })
}

/// Fold one namespace's counts into the aggregate.
///
/// Two jars may both supply the same namespace (Jar-in-Jar). This SUMS their
/// counts rather than deduplicating shared keys, so a key both jars translate
/// is double-counted — a known, accepted over-count, not a union. Fixing it
/// properly would mean caching each jar's actual key SET instead of just
/// counts: a 150-mod instance's cache would then hold on the order of
/// 300,000 strings, which is precisely the memory/disk blowup the cache
/// exists to avoid. The case this would fix is rare in practice — modern
/// Jar-in-Jar nests dependencies as opaque `.jar` entries this scanner never
/// opens, so overlapping namespaces are mostly a pre-1.13 concern — so the
/// double-count is accepted rather than paid for.
fn merge_into(acc: &mut BTreeMap<String, NamespaceCoverage>, c: NamespaceCoverage) {
    acc.entry(c.namespace.clone())
        .and_modify(|e| {
            e.total_keys += c.total_keys;
            e.from_mod += c.from_mod;
            e.overridden += c.overridden;
        })
        .or_insert(c);
}

/// Coverage for one jar, plus every language code it ships.
/// `store_dir` is unused in this PR (no overrides exist yet) and is threaded
/// through so the next PR adds the override count without changing signatures.
fn scan_one_jar(
    jar_bytes: &[u8],
    lang: &str,
    _store_dir: &Path,
) -> Result<(Vec<NamespaceCoverage>, Vec<String>), crate::error::Error> {
    let entries = crate::l10n::scan::list_lang_entries(jar_bytes)?;
    let codes: Vec<String> = entries.iter().map(|(e, _)| e.code.clone()).collect();

    let mut out = Vec::new();
    let namespaces: BTreeSet<String> = entries.iter().map(|(e, _)| e.namespace.clone()).collect();

    for ns in namespaces {
        let en = entries
            .iter()
            .find(|(e, _)| e.namespace == ns && e.code == "en_us")
            .and_then(|(e, p)| crate::l10n::scan::read_lang_map(jar_bytes, e, p));
        let Some(en) = en else {
            continue; // no English source ⇒ nothing to measure against
        };
        let target = entries
            .iter()
            .find(|(e, _)| e.namespace == ns && e.code == lang)
            .and_then(|(e, p)| crate::l10n::scan::read_lang_map(jar_bytes, e, p));
        out.push(namespace_coverage(&ns, &en, target.as_ref(), &[]));
    }
    Ok((out, codes))
}

/// Serializes the disk read-modify-write; held only over the synchronous
/// load/save, never across a scan. Mirrors `mods::summary_cache::DISK_LOCK`.
static CACHE_DISK_LOCK: Mutex<()> = Mutex::new(());

/// One jar's cached scan result. Both halves are needed: the coverage rows
/// feed the report, and the language codes feed the UI's target-language
/// picker. An earlier version cached only the rows, which made
/// `available_codes` collapse to the requested language as soon as the cache
/// warmed — the picker lost its options on the second render.
///
/// Deliberately NOT `specta::Type`: this is a disk-cache record, not part of
/// any command's return type, so it never crosses IPC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedJarScan {
    #[serde(default)]
    pub codes: Vec<String>,
    pub namespaces: Vec<NamespaceCoverage>,
}

/// Per-jar coverage, keyed by `(target language, jar SHA-1)`.
///
/// SHA-1 is the right invalidation key: it is already computed and stored in
/// `installed-mods.json`, and it changes exactly when the jar's contents do.
/// Derived data — safe to delete, rebuilt on demand.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ScanCache {
    #[serde(default)]
    entries: BTreeMap<String, CachedJarScan>,
}

fn cache_key(lang: &str, sha1: &str) -> String {
    format!("{lang}/{}", sha1.to_ascii_lowercase())
}

impl ScanCache {
    /// A missing or malformed file yields an empty cache — never an error.
    pub fn load(path: &Path) -> Self {
        let Ok(raw) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        serde_json::from_str(&raw).unwrap_or_default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, lang: &str, sha1: &str) -> Option<&CachedJarScan> {
        self.entries.get(&cache_key(lang, sha1))
    }

    pub fn put(&mut self, lang: &str, sha1: &str, entry: CachedJarScan) {
        self.entries.insert(cache_key(lang, sha1), entry);
    }

    /// Atomic write (per-process temp + rename), creating the parent dir.
    /// Private: the temp filename is only `tmp.<pid>`, which is safe across
    /// separate launcher processes (distinct pids) but NOT against two
    /// concurrent calls within this same process (same pid, same tmp path —
    /// the second `rename` loses the race). `update()` is the only caller,
    /// and it holds `CACHE_DISK_LOCK` for the full cycle, so that intra-process
    /// race can never happen. Making this a free-standing `pub fn` would let a
    /// future caller (e.g. two Overview cards racing a save) reintroduce
    /// exactly the collision `mods/installed.rs` hit before it added a
    /// sequence number to its temp name — so instead of defending this
    /// entry point, it is removed.
    fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, path)
    }

    /// Load, mutate, save under the disk lock. The ONLY sanctioned write path:
    /// `save()` is private specifically so a caller cannot skip this lock.
    /// The lock is what makes `save()`'s `tmp.<pid>` temp name safe — it
    /// guarantees at most one save-in-flight per process, so two concurrent
    /// writers (e.g. two Overview cards finishing a scan at once) can never
    /// collide on that path the way `mods/installed.rs`'s unlocked `write()`
    /// once did.
    pub fn update<F: FnOnce(&mut Self)>(path: &Path, f: F) {
        // Deliberate poison-recovery, not an unwrap: a prior panicking holder
        // must not permanently break every future cache read/write.
        let _g = CACHE_DISK_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let mut cache = Self::load(path);
        f(&mut cache);
        if let Err(e) = cache.save(path) {
            crate::diag!("[l10n] scan cache save failed ({}): {e}", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::l10n::scan::LangMap;
    use tempfile::tempdir;

    fn cov(ns: &str, total: u32, from_mod: u32) -> Vec<NamespaceCoverage> {
        vec![NamespaceCoverage {
            namespace: ns.into(),
            total_keys: total,
            from_mod,
            overridden: 0,
        }]
    }

    /// Build a `CachedJarScan` for the `ScanCache` tests below.
    fn entry(ns: &str, total: u32, from_mod: u32, codes: &[&str]) -> CachedJarScan {
        CachedJarScan {
            codes: codes.iter().map(|s| s.to_string()).collect(),
            namespaces: cov(ns, total, from_mod),
        }
    }

    #[test]
    fn cache_round_trips_and_is_keyed_by_sha_and_language() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("l10n/scan-cache.json");
        let c = entry("create", 10, 5, &["ru_ru", "en_us"]);

        let mut cache = ScanCache::load(&path);
        cache.put("ru_ru", "abc123", c.clone());
        cache.save(&path).unwrap();

        let reloaded = ScanCache::load(&path);
        assert_eq!(reloaded.get("ru_ru", "abc123"), Some(&c));
        // A different target language is a different entry, not a hit.
        assert_eq!(reloaded.get("de_de", "abc123"), None);
        // A different jar is a miss.
        assert_eq!(reloaded.get("ru_ru", "other"), None);
    }

    #[test]
    fn sha_lookup_is_case_insensitive() {
        // Registries and hashers disagree on hex casing; the same jar must not
        // occupy two cache entries.
        let dir = tempdir().unwrap();
        let path = dir.path().join("scan-cache.json");
        let mut cache = ScanCache::load(&path);
        cache.put("ru_ru", "ABC123", entry("create", 1, 1, &[]));
        assert!(cache.get("ru_ru", "abc123").is_some());
    }

    #[test]
    fn missing_cache_file_loads_empty() {
        let dir = tempdir().unwrap();
        assert_eq!(ScanCache::load(&dir.path().join("nope.json")).len(), 0);
    }

    #[test]
    fn malformed_cache_file_loads_empty_rather_than_failing() {
        // Derived data: a corrupt file must degrade to a rescan, never surface
        // as an error to the user.
        let dir = tempdir().unwrap();
        let path = dir.path().join("scan-cache.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{ not json").unwrap();
        assert_eq!(ScanCache::load(&path).len(), 0);
    }

    #[test]
    fn cache_entry_missing_codes_field_still_loads() {
        // `#[serde(default)]` on `CachedJarScan::codes`: an on-disk cache
        // written by the pre-fix shape of this file (namespaces only, no
        // codes) must still load rather than being discarded as malformed.
        // Belt-and-braces alongside `malformed_cache_file_loads_empty...`,
        // which already covers the "can't parse at all" case — this covers
        // "parses, but an old shape".
        let dir = tempdir().unwrap();
        let path = dir.path().join("scan-cache.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let legacy = serde_json::json!({
            "entries": {
                "ru_ru/sha1": {
                    "namespaces": [
                        { "namespace": "create", "totalKeys": 10, "fromMod": 5, "overridden": 0 }
                    ]
                }
            }
        });
        std::fs::write(&path, legacy.to_string()).unwrap();

        let cache = ScanCache::load(&path);
        let hit = cache.get("ru_ru", "sha1").expect("still loads");
        assert!(hit.codes.is_empty());
        assert_eq!(hit.namespaces.len(), 1);
    }

    #[test]
    fn save_creates_the_parent_directory() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("deep/nested/scan-cache.json");
        let mut cache = ScanCache::default();
        cache.put("ru_ru", "x", entry("y", 1, 1, &[]));
        cache.save(&path).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn put_replaces_a_prior_entry_for_the_same_jar_and_language() {
        let mut cache = ScanCache::default();
        cache.put("ru_ru", "sha", entry("create", 10, 1, &[]));
        cache.put("ru_ru", "sha", entry("create", 10, 9, &[]));
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.get("ru_ru", "sha").unwrap().namespaces[0].from_mod, 9);
    }

    #[test]
    fn update_persists_through_the_disk_lock() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("scan-cache.json");
        ScanCache::update(&path, |c| c.put("ru_ru", "sha", entry("create", 4, 2, &[])));
        assert_eq!(
            ScanCache::load(&path)
                .get("ru_ru", "sha")
                .unwrap()
                .namespaces[0]
                .total_keys,
            4
        );
    }

    #[test]
    fn cached_scan_round_trip_preserves_language_codes() {
        // Regression for the bug this task fixes: an earlier `ScanCache`
        // stored only `Vec<NamespaceCoverage>` per jar, so a cache hit had no
        // codes to contribute to `available_codes` — the target-language
        // picker collapsed to just the requested language on the second
        // render of an instance, once every mod had been scanned once.
        let dir = tempdir().unwrap();
        let path = dir.path().join("scan-cache.json");
        let e = entry("create", 10, 5, &["en_us", "ru_ru", "de_de"]);

        ScanCache::update(&path, |c| c.put("ru_ru", "sha1", e.clone()));

        let reloaded = ScanCache::load(&path);
        let hit = reloaded.get("ru_ru", "sha1").expect("cache hit");
        assert_eq!(hit.codes, vec!["en_us", "ru_ru", "de_de"]);
        assert_eq!(hit.namespaces, e.namespaces);
    }

    #[test]
    fn a_saved_cache_is_byte_stable_across_rewrites() {
        // Same content in, same bytes out — so an unchanged cache does not
        // churn on disk and diffs cleanly if a human ever looks at it.
        let dir = tempdir().unwrap();
        let a = dir.path().join("a.json");
        let b = dir.path().join("b.json");
        let mut first = ScanCache::default();
        first.put("ru_ru", "s2", entry("z", 1, 1, &[]));
        first.put("ru_ru", "s1", entry("y", 2, 2, &[]));
        first.save(&a).unwrap();
        let mut second = ScanCache::default();
        second.put("ru_ru", "s1", entry("y", 2, 2, &[]));
        second.put("ru_ru", "s2", entry("z", 1, 1, &[]));
        second.save(&b).unwrap();
        assert_eq!(
            std::fs::read_to_string(&a).unwrap(),
            std::fs::read_to_string(&b).unwrap()
        );
    }

    fn map(pairs: &[(&str, &str)]) -> LangMap {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn counts_translated_keys_against_english() {
        let en = map(&[("a", "A"), ("b", "B"), ("c", "C"), ("d", "D")]);
        let target = map(&[("a", "А"), ("b", "Б")]);
        let c = namespace_coverage("create", &en, Some(&target), &[]);
        assert_eq!(c.namespace, "create");
        assert_eq!(c.total_keys, 4);
        assert_eq!(c.from_mod, 2);
        assert_eq!(c.overridden, 0);
        assert_eq!(c.covered(), 2);
        assert_eq!(c.percent(), 50);
    }

    #[test]
    fn no_target_file_is_zero_not_missing() {
        let en = map(&[("a", "A"), ("b", "B")]);
        let c = namespace_coverage("ae2", &en, None, &[]);
        assert_eq!(c.total_keys, 2);
        assert_eq!(c.covered(), 0);
        assert_eq!(c.percent(), 0);
    }

    #[test]
    fn keys_present_only_in_the_target_do_not_inflate_coverage() {
        // A stale ru_ru carrying keys the mod dropped must not push us over.
        let en = map(&[("a", "A")]);
        let target = map(&[("a", "А"), ("gone", "Удалено")]);
        let c = namespace_coverage("x", &en, Some(&target), &[]);
        assert_eq!(c.total_keys, 1);
        assert_eq!(c.covered(), 1);
        assert_eq!(c.percent(), 100);
    }

    #[test]
    fn overrides_count_toward_coverage_without_double_counting() {
        // 'a' is translated by the mod AND overridden by the user: still one
        // covered key, not two.
        let en = map(&[("a", "A"), ("b", "B"), ("c", "C")]);
        let target = map(&[("a", "А")]);
        let c = namespace_coverage("x", &en, Some(&target), &["a".into(), "b".into()]);
        assert_eq!(c.total_keys, 3);
        assert_eq!(c.covered(), 2);
        assert_eq!(c.percent(), 66);
    }

    #[test]
    fn an_override_for_a_key_the_mod_dropped_does_not_count() {
        // Orphan override: not in en_us, so not part of the denominator and
        // not part of the numerator either.
        let en = map(&[("a", "A")]);
        let c = namespace_coverage("x", &en, None, &["gone".into()]);
        assert_eq!(c.total_keys, 1);
        assert_eq!(c.covered(), 0);
    }

    #[test]
    fn duplicate_override_keys_do_not_inflate_the_count() {
        // The exact case a review caught empirically: a naive `.count()` over
        // the raw slice would give overridden=2, covered=2, percent=200 — a
        // value the type cannot legitimately hold. Dedup must happen inside
        // the function, since nothing forces a caller to pass a set.
        let en = map(&[("a", "A")]);
        let c = namespace_coverage("x", &en, None, &["a".into(), "a".into()]);
        assert_eq!(c.overridden, 1);
        assert_eq!(c.percent(), 100);
    }

    #[test]
    fn empty_target_map_behaves_like_no_target_file() {
        // An empty-but-present translation file must read the same as an
        // absent one, not as partial progress.
        let en = map(&[("a", "A")]);
        let empty_target = map(&[]);
        let c = namespace_coverage("x", &en, Some(&empty_target), &[]);
        assert_eq!(c.total_keys, 1);
        assert_eq!(c.covered(), 0);
        assert_eq!(c.percent(), 0);
    }

    #[test]
    fn empty_english_is_defined_as_fully_covered() {
        // Avoids a divide-by-zero and reads correctly in the UI: a namespace
        // with nothing to translate is not "0% translated".
        let c = namespace_coverage("empty", &map(&[]), None, &[]);
        assert_eq!(c.total_keys, 0);
        assert_eq!(c.percent(), 100);
    }

    #[test]
    fn instance_percent_weights_by_key_count_not_by_namespace() {
        // A 1000-key namespace at 0% and a 10-key namespace at 100% is ~1%,
        // not 50%. Averaging percentages would lie about what the player sees.
        let a = NamespaceCoverage {
            namespace: "big".into(),
            total_keys: 1000,
            from_mod: 0,
            overridden: 0,
        };
        let b = NamespaceCoverage {
            namespace: "small".into(),
            total_keys: 10,
            from_mod: 10,
            overridden: 0,
        };
        assert_eq!(instance_percent(&[a, b]), 0);
    }

    #[test]
    fn instance_percent_of_nothing_is_full() {
        assert_eq!(instance_percent(&[]), 100);
    }

    #[test]
    fn percent_truncates_rather_than_rounds() {
        // 2/3 must read 66, not 67 — a rounded-up 100% on an incomplete
        // namespace would be a lie the user can see through.
        let en = map(&[("a", "A"), ("b", "B"), ("c", "C")]);
        let target = map(&[("a", "А"), ("b", "Б")]);
        assert_eq!(
            namespace_coverage("x", &en, Some(&target), &[]).percent(),
            66
        );
    }

    #[test]
    fn a_nearly_complete_namespace_does_not_round_up_to_a_hundred() {
        // 999/1000 must read 99, never 100 — "100%" must mean actually done.
        let en: LangMap = (0..1000)
            .map(|i| (format!("k{i}"), "v".to_string()))
            .collect();
        let target: LangMap = (0..999)
            .map(|i| (format!("k{i}"), "т".to_string()))
            .collect();
        assert_eq!(
            namespace_coverage("x", &en, Some(&target), &[]).percent(),
            99
        );
    }

    #[test]
    fn default_target_language_maps_ui_locale_to_a_minecraft_code() {
        assert_eq!(default_target_code("ru"), "ru_ru");
        assert_eq!(default_target_code("en"), "en_us");
        // Unknown UI locale: mirror the code (`fr` → `fr_fr`) rather than
        // silently falling back to English, which would hide the mismatch.
        assert_eq!(default_target_code("fr"), "fr_fr");
        // Already a full code: pass through, lowercased and underscored.
        assert_eq!(default_target_code("pt_BR"), "pt_br");
        assert_eq!(default_target_code("zh-Hans"), "zh_hans");
        assert_eq!(default_target_code(""), "en_us");
    }

    // ---------------------------------------------------------------------
    // merge_into
    // ---------------------------------------------------------------------

    #[test]
    fn merge_into_sums_counts_for_the_same_namespace() {
        let mut acc: BTreeMap<String, NamespaceCoverage> = BTreeMap::new();
        merge_into(
            &mut acc,
            NamespaceCoverage {
                namespace: "create".into(),
                total_keys: 10,
                from_mod: 4,
                overridden: 1,
            },
        );
        merge_into(
            &mut acc,
            NamespaceCoverage {
                namespace: "create".into(),
                total_keys: 5,
                from_mod: 2,
                overridden: 0,
            },
        );
        assert_eq!(acc.len(), 1);
        let c = &acc["create"];
        assert_eq!(c.total_keys, 15);
        assert_eq!(c.from_mod, 6);
        assert_eq!(c.overridden, 1);
    }

    #[test]
    fn merge_into_keeps_different_namespaces_separate() {
        let mut acc: BTreeMap<String, NamespaceCoverage> = BTreeMap::new();
        merge_into(
            &mut acc,
            NamespaceCoverage {
                namespace: "create".into(),
                total_keys: 10,
                from_mod: 4,
                overridden: 0,
            },
        );
        merge_into(
            &mut acc,
            NamespaceCoverage {
                namespace: "thermal".into(),
                total_keys: 5,
                from_mod: 5,
                overridden: 0,
            },
        );
        assert_eq!(acc.len(), 2);
        assert_eq!(acc["create"].total_keys, 10);
        assert_eq!(acc["thermal"].total_keys, 5);
    }

    // ---------------------------------------------------------------------
    // scan_one_jar
    // ---------------------------------------------------------------------

    /// Build an in-memory jar containing the given (path, contents) entries.
    /// Local copy of the identical helper in `scan.rs`'s test module — not
    /// worth making `pub` for a six-line helper only tests use.
    fn jar(entries: &[(&str, &str)]) -> Vec<u8> {
        use std::io::Write;
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let opts: zip::write::FileOptions<()> = zip::write::FileOptions::default();
            for (name, body) in entries {
                w.start_file(*name, opts).unwrap();
                w.write_all(body.as_bytes()).unwrap();
            }
            w.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn scan_one_jar_counts_partial_target_coverage() {
        let bytes = jar(&[
            (
                "assets/create/lang/en_us.json",
                r#"{"a":"A","b":"B","c":"C"}"#,
            ),
            ("assets/create/lang/ru_ru.json", r#"{"a":"А"}"#),
        ]);
        let store_dir = Path::new("unused");
        let (cov, codes) = scan_one_jar(&bytes, "ru_ru", store_dir).expect("readable jar");
        assert_eq!(cov.len(), 1);
        assert_eq!(cov[0].namespace, "create");
        assert_eq!(cov[0].total_keys, 3);
        assert_eq!(cov[0].from_mod, 1);
        let mut sorted = codes;
        sorted.sort();
        assert_eq!(sorted, vec!["en_us".to_string(), "ru_ru".to_string()]);
    }

    #[test]
    fn scan_one_jar_skips_a_namespace_with_no_english_source() {
        // No en_us file at all ⇒ nothing to measure against, so the
        // namespace is dropped from the report entirely rather than showing
        // up with a zero (or worse, undefined) denominator.
        let bytes = jar(&[("assets/create/lang/ru_ru.json", r#"{"a":"А"}"#)]);
        let store_dir = Path::new("unused");
        let (cov, codes) = scan_one_jar(&bytes, "ru_ru", store_dir).expect("readable jar");
        assert!(cov.is_empty());
        // The code list is independent of whether English is present.
        assert_eq!(codes, vec!["ru_ru".to_string()]);
    }

    #[test]
    fn scan_one_jar_reports_every_code_including_ones_not_requested() {
        // The code list feeds the UI's target-language picker, so it must
        // include languages the caller did not ask to scan for — not just
        // the requested `lang`.
        let bytes = jar(&[
            ("assets/create/lang/en_us.json", r#"{"a":"A"}"#),
            ("assets/create/lang/ru_ru.json", r#"{"a":"А"}"#),
            ("assets/create/lang/de_de.json", r#"{"a":"D"}"#),
        ]);
        let store_dir = Path::new("unused");
        let (_, codes) = scan_one_jar(&bytes, "ru_ru", store_dir).expect("readable jar");
        let mut sorted = codes;
        sorted.sort();
        assert_eq!(
            sorted,
            vec![
                "de_de".to_string(),
                "en_us".to_string(),
                "ru_ru".to_string()
            ]
        );
    }

    // ---------------------------------------------------------------------
    // scan_instance
    // ---------------------------------------------------------------------

    #[tokio::test]
    async fn scan_instance_cache_hit_still_returns_the_jars_language_codes() {
        // The Finding-1 regression, exercised through the real cache-hit
        // branch of `scan_instance` (not just `ScanCache` in isolation): the
        // SECOND scan of an unchanged instance — the normal steady state —
        // must not lose `available_codes`. Before the fix, this second call
        // would have returned only `["ru_ru"]`.
        let td = tempdir().unwrap();
        let inst_root = td.path().join("instance");
        let mods_dir = crate::mods::installed::mods_dir(&inst_root);
        tokio::fs::create_dir_all(&mods_dir).await.unwrap();

        let bytes = jar(&[
            ("assets/create/lang/en_us.json", r#"{"a":"A","b":"B"}"#),
            ("assets/create/lang/ru_ru.json", r#"{"a":"А"}"#),
            ("assets/create/lang/de_de.json", r#"{"a":"D"}"#),
        ]);
        tokio::fs::write(mods_dir.join("create.jar"), &bytes)
            .await
            .unwrap();

        let cache_path = td.path().join("l10n/scan-cache.json");
        let store_dir = td.path().join("l10n");
        // No client jar on disk at this path — irrelevant to this test, which
        // is only about `available_codes` surviving a cache hit.
        let versions_dir = td.path().join("versions");

        let first = scan_instance(
            &inst_root,
            &cache_path,
            &store_dir,
            &versions_dir,
            "1.20.1",
            "ru_ru",
        )
        .await
        .unwrap();
        assert_eq!(
            first.available_codes,
            vec![
                "de_de".to_string(),
                "en_us".to_string(),
                "ru_ru".to_string()
            ]
        );

        // Nothing on disk changed, so this hits the cache-hit branch.
        let second = scan_instance(
            &inst_root,
            &cache_path,
            &store_dir,
            &versions_dir,
            "1.20.1",
            "ru_ru",
        )
        .await
        .unwrap();
        assert_eq!(second.available_codes, first.available_codes);
        assert_eq!(second.namespaces, first.namespaces);
    }

    #[tokio::test]
    async fn scan_instance_skips_a_corrupt_jar_and_still_reports_the_rest() {
        // Finding 2: a present-but-unreadable jar must not abort the whole
        // instance's report, the same way an already-vanished jar is
        // deliberately skipped a few lines above it in `scan_instance`.
        let td = tempdir().unwrap();
        let inst_root = td.path().join("instance");
        let mods_dir = crate::mods::installed::mods_dir(&inst_root);
        tokio::fs::create_dir_all(&mods_dir).await.unwrap();

        let good = jar(&[
            ("assets/create/lang/en_us.json", r#"{"a":"A"}"#),
            ("assets/create/lang/ru_ru.json", r#"{"a":"А"}"#),
        ]);
        tokio::fs::write(mods_dir.join("create.jar"), &good)
            .await
            .unwrap();
        tokio::fs::write(mods_dir.join("broken.jar"), b"not a zip at all")
            .await
            .unwrap();

        let cache_path = td.path().join("l10n/scan-cache.json");
        let store_dir = td.path().join("l10n");

        let versions_dir = td.path().join("versions");
        let out = scan_instance(
            &inst_root,
            &cache_path,
            &store_dir,
            &versions_dir,
            "1.20.1",
            "ru_ru",
        )
        .await
        .expect("one corrupt jar must not fail the whole scan");
        assert_eq!(out.namespaces.len(), 1);
        assert_eq!(out.namespaces[0].namespace, "create");
        assert_eq!(out.namespaces[0].covered(), 1);
    }

    // ---------------------------------------------------------------------
    // InstanceCoverage::apply_gate
    // ---------------------------------------------------------------------

    #[tokio::test]
    async fn scan_instance_reports_unknown_format_when_the_client_jar_is_absent() {
        // No instance has ever been launched ⇒ no versions/<mc>/<mc>.jar on
        // disk yet — the common case this reason exists to explain.
        let td = tempdir().unwrap();
        let inst_root = td.path().join("instance");
        let mods_dir = crate::mods::installed::mods_dir(&inst_root);
        tokio::fs::create_dir_all(&mods_dir).await.unwrap();
        let cache_path = td.path().join("l10n/scan-cache.json");
        let store_dir = td.path().join("l10n");
        let versions_dir = td.path().join("versions"); // never created

        let out = scan_instance(
            &inst_root,
            &cache_path,
            &store_dir,
            &versions_dir,
            "1.20.1",
            "ru_ru",
        )
        .await
        .unwrap();
        assert_eq!(
            out.apply_gate,
            crate::l10n::pack_format::ApplyGate::UnknownFormat
        );
    }

    #[tokio::test]
    async fn scan_instance_reports_ready_when_the_client_jar_declares_a_modern_format() {
        let td = tempdir().unwrap();
        let inst_root = td.path().join("instance");
        let mods_dir = crate::mods::installed::mods_dir(&inst_root);
        tokio::fs::create_dir_all(&mods_dir).await.unwrap();
        let cache_path = td.path().join("l10n/scan-cache.json");
        let store_dir = td.path().join("l10n");
        let versions_dir = td.path().join("versions");
        let client_jar_dir = versions_dir.join("1.20.1");
        tokio::fs::create_dir_all(&client_jar_dir).await.unwrap();
        let client_jar = jar(&[(
            "version.json",
            r#"{"id":"1.20.1","pack_version":{"resource":15,"data":15}}"#,
        )]);
        tokio::fs::write(client_jar_dir.join("1.20.1.jar"), &client_jar)
            .await
            .unwrap();

        let out = scan_instance(
            &inst_root,
            &cache_path,
            &store_dir,
            &versions_dir,
            "1.20.1",
            "ru_ru",
        )
        .await
        .unwrap();
        assert_eq!(out.apply_gate, crate::l10n::pack_format::ApplyGate::Ready);
    }

    #[tokio::test]
    async fn scan_instance_reports_too_old_below_resource_format_4() {
        let td = tempdir().unwrap();
        let inst_root = td.path().join("instance");
        let mods_dir = crate::mods::installed::mods_dir(&inst_root);
        tokio::fs::create_dir_all(&mods_dir).await.unwrap();
        let cache_path = td.path().join("l10n/scan-cache.json");
        let store_dir = td.path().join("l10n");
        let versions_dir = td.path().join("versions");
        let client_jar_dir = versions_dir.join("1.12.2");
        tokio::fs::create_dir_all(&client_jar_dir).await.unwrap();
        let client_jar = jar(&[("version.json", r#"{"id":"1.12.2","pack_version":1}"#)]);
        tokio::fs::write(client_jar_dir.join("1.12.2.jar"), &client_jar)
            .await
            .unwrap();

        let out = scan_instance(
            &inst_root,
            &cache_path,
            &store_dir,
            &versions_dir,
            "1.12.2",
            "ru_ru",
        )
        .await
        .unwrap();
        assert_eq!(out.apply_gate, crate::l10n::pack_format::ApplyGate::TooOld);
    }
}
