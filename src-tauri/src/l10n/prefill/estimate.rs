//! What a run will cost, before it starts: cache hits, glossary hits, and the
//! strings that actually reach the model.
//!
//! The estimate exists to be *believed*, which means it has to be arithmetic
//! over the same pipeline the run performs — dedup first, then the cache,
//! then the glossary, and only what is left over reaches a provider. Counting
//! any other way (per key, or cache-before-dedup) produces a headline number
//! the run then contradicts, which is worse than showing nothing.
//!
//! Two numbers, deliberately different:
//!
//! - `missing` counts **keys** — what the coverage report shows the user.
//! - `to_translate` counts **calls** — distinct `(role, English string)`
//!   pairs. In a real modpack the gap is large: "Energy", "Tier" and
//!   "Durability" recur across dozens of mods and are asked once.
//!
//! No monetary figure is produced, only tokens. Provider prices change
//! without notice and a stale price in a launcher is worse than no price.

use serde::Serialize;
use specta::Type;

use crate::instances::schema::AiProvider;
use crate::l10n::prefill::cache::{cache_key, CacheMap, PipelineId};
use crate::l10n::prefill::glossary::Glossary;
use crate::l10n::prefill::plan::{build_batches, PrefillUnit};

/// Characters of source text per token. The usual English rule of thumb; the
/// UI labels the result approximate because that is what it is.
const CHARS_PER_TOKEN: usize = 4;

/// Ask [`build_batches`] for the dedup without the chunking. The estimate
/// needs "which distinct units of work exist", not "how they split across
/// requests", and reusing that function rather than reimplementing its
/// grouping is the whole point: a second copy of the dedup rule is exactly
/// how an estimate starts disagreeing with the run it predicts.
const NO_CHUNKING: usize = usize::MAX;

/// What a pre-fill run would do, before committing to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PrefillEstimate {
    /// Keys with no translation — the number the coverage report shows.
    pub missing: u32,
    /// Distinct units already answered by a previous run's cache.
    pub from_cache: u32,
    /// Distinct units vanilla Minecraft already translates verbatim.
    pub from_glossary: u32,
    /// Distinct units that must actually reach the model. Always ≤ `missing`,
    /// and usually far below it.
    pub to_translate: u32,
    /// Approximate tokens of source text in those calls. Source only: the
    /// prompt scaffolding and the model's own answer are extra, so this is a
    /// floor rather than a bill.
    pub estimated_tokens: u32,
    pub provider: AiProvider,
    /// Whether the run spends the user's provider credit. A property of the
    /// provider, not of the workload — a local model is free however long it
    /// takes.
    pub billable: bool,
}

/// What a run would cost, computed the same way the run will actually work:
/// dedup first, then cache, then glossary, and only the remainder is billable.
/// Estimating any other way produces a number the run then contradicts.
///
/// `units` may repeat a string across namespaces and may contain blank
/// sources; both are resolved here exactly as [`build_batches`] resolves them,
/// so the estimate is correct for any input the caller hands it rather than
/// merely for well-formed input.
#[must_use]
pub fn estimate(
    units: &[PrefillUnit],
    cache: &CacheMap,
    glossary: &Glossary,
    id: &PipelineId,
    provider: AiProvider,
) -> PrefillEstimate {
    let mut missing: usize = 0;
    let mut from_cache: usize = 0;
    let mut from_glossary: usize = 0;
    let mut to_translate: usize = 0;
    let mut billable_chars: usize = 0;

    for batch in build_batches(units, NO_CHUNKING) {
        for unit in &batch.units {
            // Every key waiting on this string is still a missing key, even
            // though they share one answer.
            missing += unit.targets.len();

            if cache.contains_key(&cache_key(&unit.source_en, batch.role, id)) {
                from_cache += 1;
                continue;
            }
            if glossary.exact(&unit.source_en).is_some() {
                from_glossary += 1;
                continue;
            }
            to_translate += 1;
            billable_chars += unit.source_en.chars().count();
        }
    }

    PrefillEstimate {
        missing: clamp_u32(missing),
        from_cache: clamp_u32(from_cache),
        from_glossary: clamp_u32(from_glossary),
        to_translate: clamp_u32(to_translate),
        // Rounded up, so a run with real work in it can never report zero.
        estimated_tokens: clamp_u32(billable_chars.div_ceil(CHARS_PER_TOKEN)),
        provider,
        billable: is_billable(provider),
    }
}

/// Exhaustive on purpose: a new provider variant must be a compile error here
/// rather than silently inheriting a default. Cloud providers spend the user's
/// own API credit; a local model spends only their CPU.
fn is_billable(provider: AiProvider) -> bool {
    match provider {
        AiProvider::Anthropic | AiProvider::Gemini | AiProvider::Groq => true,
        AiProvider::Local => false,
    }
}

/// Counts cross IPC as `u32`. No instance has four billion missing keys, but
/// clamping keeps the degenerate case honest instead of wrapping to a small
/// number that would read as good news.
fn clamp_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::l10n::prefill::role::UiRole;
    use crate::l10n::scan::LangMap;

    fn unit(ns: &str, key: &str, en: &str) -> PrefillUnit {
        PrefillUnit {
            namespace: ns.to_string(),
            key: key.to_string(),
            source_en: en.to_string(),
        }
    }

    fn pipeline() -> PipelineId {
        PipelineId {
            target_lang: "ru_ru".to_string(),
            model: "claude-x".to_string(),
            prompt_version: 1,
            glossary_version: "1.20.1".to_string(),
        }
    }

    /// A cache holding exactly the given `(source, role)` pairs, addressed the
    /// same way the run will address them.
    fn cache_of(entries: &[(&str, UiRole)]) -> CacheMap {
        entries
            .iter()
            .map(|(source, role)| {
                (
                    cache_key(source, *role, &pipeline()),
                    format!("<cached {source}>"),
                )
            })
            .collect()
    }

    /// A vanilla glossary carrying the given English → target pairs.
    fn glossary_of(pairs: &[(&str, &str)]) -> Glossary {
        let en: LangMap = pairs
            .iter()
            .enumerate()
            .map(|(i, (english, _))| (format!("vanilla.k{i}"), (*english).to_string()))
            .collect();
        let target: LangMap = pairs
            .iter()
            .enumerate()
            .map(|(i, (_, translated))| (format!("vanilla.k{i}"), (*translated).to_string()))
            .collect();
        Glossary::from_lang_maps(&en, &target, "1.20.1")
    }

    #[test]
    fn counts_split_into_cache_glossary_and_to_translate() {
        let units = vec![
            unit("create", "item.create.a", "Energy"),
            unit("create", "item.create.b", "Stick"),
            unit("create", "item.create.c", "Cogwheel"),
        ];
        let got = estimate(
            &units,
            &cache_of(&[("Energy", UiRole::Name)]),
            &glossary_of(&[("Stick", "Палка")]),
            &pipeline(),
            AiProvider::Anthropic,
        );

        assert_eq!(got.missing, 3);
        assert_eq!(got.from_cache, 1);
        assert_eq!(got.from_glossary, 1);
        assert_eq!(got.to_translate, 1);
        // Every missing key lands in exactly one bucket. The identity holds
        // here because no source string repeats; when one does, `missing`
        // counts keys while the buckets count calls — pinned separately by
        // `duplicate_strings_are_counted_once_in_to_translate_but_n_times_in_missing`.
        assert_eq!(
            got.from_cache + got.from_glossary + got.to_translate,
            got.missing
        );
    }

    #[test]
    fn a_string_already_in_the_cache_is_never_counted_as_billable() {
        // Deliberately also in the glossary: the run resolves cache hits
        // first, so this must count once, as a cache hit, and never twice.
        let units = vec![unit("create", "item.create.a", "Stick")];
        let got = estimate(
            &units,
            &cache_of(&[("Stick", UiRole::Name)]),
            &glossary_of(&[("Stick", "Палка")]),
            &pipeline(),
            AiProvider::Anthropic,
        );

        assert_eq!(got.missing, 1);
        assert_eq!(got.from_cache, 1);
        assert_eq!(
            got.from_glossary, 0,
            "a cache hit is not also a glossary hit"
        );
        assert_eq!(got.to_translate, 0);
        assert_eq!(got.estimated_tokens, 0, "a cached string costs nothing");
    }

    #[test]
    fn a_glossary_hit_beats_a_model_call_and_is_counted_separately() {
        let units = vec![
            unit("create", "item.create.a", "Stick"),
            unit("create", "item.create.b", "Cogwheel"),
        ];
        let got = estimate(
            &units,
            &CacheMap::new(),
            &glossary_of(&[("Stick", "Палка")]),
            &pipeline(),
            AiProvider::Anthropic,
        );

        assert_eq!(got.missing, 2);
        assert_eq!(got.from_cache, 0);
        assert_eq!(got.from_glossary, 1, "vanilla already translated Stick");
        assert_eq!(
            got.to_translate, 1,
            "only the string vanilla does not know reaches the model"
        );
    }

    #[test]
    fn duplicate_strings_are_counted_once_in_to_translate_but_n_times_in_missing() {
        // This is the number that makes the estimate honest: a 2000-key pack with
        // heavy repetition costs far less than 2000 calls.
        let units = vec![
            unit("create", "item.create.a", "Energy"),
            unit("ae2", "item.ae2.b", "Energy"),
            unit("mekanism", "item.mekanism.c", "Energy"),
            // Same word, prose role — a label and a sentence are different
            // translations, so dedup keeps them apart exactly as the run does.
            unit("create", "create.tooltip.energy", "Energy"),
            unit("create", "item.create.d", "Durability"),
            unit("ae2", "item.ae2.e", "Durability"),
        ];
        let got = estimate(
            &units,
            &cache_of(&[("Durability", UiRole::Name)]),
            &Glossary::empty(),
            &pipeline(),
            AiProvider::Anthropic,
        );

        assert_eq!(got.missing, 6, "six keys are missing a translation");
        assert_eq!(
            got.from_cache, 1,
            "two keys want Durability; the cache answers it once, not twice"
        );
        assert_eq!(
            got.to_translate, 2,
            "Energy as a name and Energy as prose — three Energy keys, one call each role"
        );
        assert_eq!(
            got.from_cache + got.from_glossary + got.to_translate,
            3,
            "six keys reduce to three distinct units of work"
        );
        assert!(
            got.to_translate < got.missing,
            "dedup must make the call count smaller than the key count"
        );
    }

    #[test]
    fn token_estimate_scales_with_characters_and_is_never_zero_for_nonempty_work() {
        let empty_cache = CacheMap::new();

        let short = estimate(
            &[unit("create", "item.create.a", "Hi")],
            &empty_cache,
            &Glossary::empty(),
            &pipeline(),
            AiProvider::Anthropic,
        );
        // Two characters is well under one token's worth, but rounding down
        // would report free work as free.
        assert_eq!(short.estimated_tokens, 1);

        let long = estimate(
            &[unit("create", "item.create.a", &"A".repeat(40))],
            &empty_cache,
            &Glossary::empty(),
            &pipeline(),
            AiProvider::Anthropic,
        );
        assert_eq!(long.estimated_tokens, 10, "40 characters at 4 per token");
        assert!(long.estimated_tokens > short.estimated_tokens);

        // Five keys wanting the same string are one request, so they are also
        // one string's worth of tokens.
        let deduped = estimate(
            &(0..5)
                .map(|i| unit("create", &format!("item.create.k{i}"), "Hi"))
                .collect::<Vec<_>>(),
            &empty_cache,
            &Glossary::empty(),
            &pipeline(),
            AiProvider::Anthropic,
        );
        assert_eq!(deduped.missing, 5);
        assert_eq!(deduped.estimated_tokens, short.estimated_tokens);

        let nothing_billable = estimate(
            &[unit("create", "item.create.a", "Hi")],
            &cache_of(&[("Hi", UiRole::Name)]),
            &Glossary::empty(),
            &pipeline(),
            AiProvider::Anthropic,
        );
        assert_eq!(nothing_billable.estimated_tokens, 0);
    }

    #[test]
    fn the_local_provider_is_not_billable() {
        let units = vec![unit("create", "item.create.a", "Cogwheel")];
        let local = estimate(
            &units,
            &CacheMap::new(),
            &Glossary::empty(),
            &pipeline(),
            AiProvider::Local,
        );

        assert!(!local.billable);
        assert_eq!(local.provider, AiProvider::Local);
        // A local run still has work to do — "not billable" is about money,
        // not about effort, and the dialog still needs the counts.
        assert_eq!(local.to_translate, 1);
        assert!(local.estimated_tokens > 0);

        for provider in [AiProvider::Anthropic, AiProvider::Gemini, AiProvider::Groq] {
            let cloud = estimate(
                &units,
                &CacheMap::new(),
                &Glossary::empty(),
                &pipeline(),
                provider,
            );
            assert!(cloud.billable, "{provider:?} spends the user's own credit");
        }
    }
}
