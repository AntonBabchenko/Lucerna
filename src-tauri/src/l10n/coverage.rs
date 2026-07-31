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

/// Serializes the disk read-modify-write; held only over the synchronous
/// load/save, never across a scan. Mirrors `mods::summary_cache::DISK_LOCK`.
static CACHE_DISK_LOCK: Mutex<()> = Mutex::new(());

/// Per-jar coverage, keyed by `(target language, jar SHA-1)`.
///
/// SHA-1 is the right invalidation key: it is already computed and stored in
/// `installed-mods.json`, and it changes exactly when the jar's contents do.
/// Derived data — safe to delete, rebuilt on demand.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ScanCache {
    #[serde(default)]
    entries: BTreeMap<String, Vec<NamespaceCoverage>>,
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

    pub fn get(&self, lang: &str, sha1: &str) -> Option<&[NamespaceCoverage]> {
        self.entries.get(&cache_key(lang, sha1)).map(Vec::as_slice)
    }

    pub fn put(&mut self, lang: &str, sha1: &str, cov: Vec<NamespaceCoverage>) {
        self.entries.insert(cache_key(lang, sha1), cov);
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

    #[test]
    fn cache_round_trips_and_is_keyed_by_sha_and_language() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("l10n/scan-cache.json");
        let c = cov("create", 10, 5);

        let mut cache = ScanCache::load(&path);
        cache.put("ru_ru", "abc123", c.clone());
        cache.save(&path).unwrap();

        let reloaded = ScanCache::load(&path);
        assert_eq!(reloaded.get("ru_ru", "abc123"), Some(c.as_slice()));
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
        cache.put("ru_ru", "ABC123", cov("create", 1, 1));
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
    fn save_creates_the_parent_directory() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("deep/nested/scan-cache.json");
        let mut cache = ScanCache::default();
        cache.put("ru_ru", "x", cov("y", 1, 1));
        cache.save(&path).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn put_replaces_a_prior_entry_for_the_same_jar_and_language() {
        let mut cache = ScanCache::default();
        cache.put("ru_ru", "sha", cov("create", 10, 1));
        cache.put("ru_ru", "sha", cov("create", 10, 9));
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.get("ru_ru", "sha").unwrap()[0].from_mod, 9);
    }

    #[test]
    fn update_persists_through_the_disk_lock() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("scan-cache.json");
        ScanCache::update(&path, |c| c.put("ru_ru", "sha", cov("create", 4, 2)));
        assert_eq!(
            ScanCache::load(&path).get("ru_ru", "sha").unwrap()[0].total_keys,
            4
        );
    }

    #[test]
    fn a_saved_cache_is_byte_stable_across_rewrites() {
        // Same content in, same bytes out — so an unchanged cache does not
        // churn on disk and diffs cleanly if a human ever looks at it.
        let dir = tempdir().unwrap();
        let a = dir.path().join("a.json");
        let b = dir.path().join("b.json");
        let mut first = ScanCache::default();
        first.put("ru_ru", "s2", cov("z", 1, 1));
        first.put("ru_ru", "s1", cov("y", 2, 2));
        first.save(&a).unwrap();
        let mut second = ScanCache::default();
        second.put("ru_ru", "s1", cov("y", 2, 2));
        second.put("ru_ru", "s2", cov("z", 1, 1));
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
}
