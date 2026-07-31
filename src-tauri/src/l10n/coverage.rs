//! Per-namespace translation coverage for one instance.
//!
//! Counted per KEY, never per file: a mod that ships a `ru_ru.json` covering
//! half its strings is half-translated, and the player sees English for the
//! rest (the game loads en_us into the shared map first, so missing keys fall
//! back rather than disappearing).

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
/// language file (if any) and a count of user overrides.
///
/// Only keys present in `en` count: a target file carrying keys the mod has
/// since dropped must not inflate the result.
pub fn namespace_coverage(
    namespace: &str,
    en: &LangMap,
    target: Option<&LangMap>,
    overridden: u32,
) -> NamespaceCoverage {
    let from_mod = match target {
        Some(t) => en.keys().filter(|k| t.contains_key(*k)).count() as u32,
        None => 0,
    };
    NamespaceCoverage {
        namespace: namespace.to_string(),
        total_keys: en.len() as u32,
        from_mod,
        overridden,
    }
}

/// As `namespace_coverage`, but derives the override count from the actual key
/// list so a key the mod already translates is never counted twice.
pub fn namespace_coverage_with_overrides(
    namespace: &str,
    en: &LangMap,
    target: Option<&LangMap>,
    override_keys: &[String],
) -> NamespaceCoverage {
    let mut c = namespace_coverage(namespace, en, target, 0);
    c.overridden = override_keys
        .iter()
        .filter(|k| en.contains_key(*k) && !target.is_some_and(|t| t.contains_key(*k)))
        .count() as u32;
    c
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::l10n::scan::LangMap;

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
        let c = namespace_coverage("create", &en, Some(&target), 0);
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
        let c = namespace_coverage("ae2", &en, None, 0);
        assert_eq!(c.total_keys, 2);
        assert_eq!(c.covered(), 0);
        assert_eq!(c.percent(), 0);
    }

    #[test]
    fn keys_present_only_in_the_target_do_not_inflate_coverage() {
        // A stale ru_ru carrying keys the mod dropped must not push us over.
        let en = map(&[("a", "A")]);
        let target = map(&[("a", "А"), ("gone", "Удалено")]);
        let c = namespace_coverage("x", &en, Some(&target), 0);
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
        let c =
            namespace_coverage_with_overrides("x", &en, Some(&target), &["a".into(), "b".into()]);
        assert_eq!(c.total_keys, 3);
        assert_eq!(c.covered(), 2);
        assert_eq!(c.percent(), 66);
    }

    #[test]
    fn an_override_for_a_key_the_mod_dropped_does_not_count() {
        // Orphan override: not in en_us, so not part of the denominator and
        // not part of the numerator either.
        let en = map(&[("a", "A")]);
        let c = namespace_coverage_with_overrides("x", &en, None, &["gone".into()]);
        assert_eq!(c.total_keys, 1);
        assert_eq!(c.covered(), 0);
    }

    #[test]
    fn empty_english_is_defined_as_fully_covered() {
        // Avoids a divide-by-zero and reads correctly in the UI: a namespace
        // with nothing to translate is not "0% translated".
        let c = namespace_coverage("empty", &map(&[]), None, 0);
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
        assert_eq!(namespace_coverage("x", &en, Some(&target), 0).percent(), 66);
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
        assert_eq!(namespace_coverage("x", &en, Some(&target), 0).percent(), 99);
    }
}
