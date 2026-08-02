//! Vanilla Minecraft's own translations, used as a free termbase.
//!
//! Mojang ships a professionally translated `en_us` → `<target>` corpus with
//! every copy of the game, and for any instance the user can actually play it
//! is already on disk: the English side inside the client jar, the target side
//! in the asset-objects store. Reading it costs one zip-entry read and one
//! file read — no network, no tokens, no provider.
//!
//! It buys two different things:
//!
//! - **Translation memory.** A mod string that is *character-for-character* a
//!   vanilla string ("Stick", "Copper Ingot", "Press %1$s to open") is already
//!   translated by Mojang. [`Glossary::exact`] answers those without a model
//!   call at all, which is both free and better than anything a model would
//!   have produced.
//! - **A termbase.** For everything else, [`Glossary::terms_for`] injects the
//!   handful of vanilla terms that actually occur in the batch into the
//!   prompt. Models follow an example far more reliably than an instruction,
//!   and it is what stops one term becoming three phrases across a pack.
//!
//! # Pre-1.8 yields an empty glossary, deliberately
//!
//! `virtual` (1.6–1.7.2) and `map_to_resources` (1.7.3–1.7.9) asset indexes
//! spell their language files `minecraft/lang/ru_RU.lang`, not
//! `minecraft/lang/ru_ru.json`, and those client jars likewise ship
//! `en_US.lang`. This module reads only the modern spelling, so those versions
//! get [`Glossary::empty`] — stated plainly rather than papered over.
//!
//! That costs nothing in practice: `pack_format::supports_apply`
//! (`l10n/pack_format.rs:128`) refuses every resource format below 4
//! (Minecraft 1.13), so an instance old enough to use the legacy spelling
//! cannot apply a translation pack at all and never reaches this module in
//! earnest. Should one arrive anyway, the miss is logged through
//! `crate::diag!` rather than swallowed, so the quality loss is visible in
//! `lucerna.log` instead of silently degrading every prompt.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Deserialize;

use crate::l10n::scan::{self, LangMap};

/// [`Glossary::version`] when no vanilla glossary was available. A real
/// value, not a placeholder: `prefill::cache`'s `PipelineId` carries it, so
/// strings translated without a termbase must not share a cache identity with
/// strings translated with one.
const NO_GLOSSARY: &str = "none";

/// Longest English string, in whitespace-separated words, that may become a
/// *term*. A whole sentence in the term list matches nothing else and teaches
/// nothing — it is translation memory, which [`Glossary::exact`] already
/// serves.
const MAX_TERM_WORDS: usize = 3;

/// One approved term pair.
struct Term {
    /// The English side, lowercased once at construction — the needle
    /// [`Glossary::terms_for`] matches with.
    needle: String,
    /// The English side as vanilla writes it, for the prompt.
    english: String,
    /// The target-language side, verbatim.
    translated: String,
}

/// Vanilla's translations for one target language: a whole-string memory plus
/// a termbase, both derived from the same corpus.
pub struct Glossary {
    /// Lowercased English string → vanilla's translation of it.
    memory: BTreeMap<String, String>,
    /// Terms, longest needle first — the order [`Glossary::terms_for`]'s
    /// longest-match rule depends on.
    terms: Vec<Term>,
    /// The Minecraft version the corpus came from, or [`NO_GLOSSARY`].
    version: String,
}

impl Glossary {
    /// No corpus: every lookup misses and no term is ever injected. The
    /// answer for every failure path in [`load_for_instance`] — a run without
    /// a glossary is a run that costs more and reads slightly worse, never a
    /// failed run.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            memory: BTreeMap::new(),
            terms: Vec::new(),
            version: NO_GLOSSARY.to_string(),
        }
    }

    /// Pair the two maps **by key** — the only sound way to align them, since
    /// a translation is defined as "what Mojang wrote for this key".
    ///
    /// A pair is dropped when either side is blank: an English key with no
    /// target-language counterpart (`target` is a partial translation for
    /// every language but a few) would otherwise pair English with nothing,
    /// and a blank target string would teach the model to erase text.
    #[must_use]
    pub fn from_lang_maps(en: &LangMap, target: &LangMap, version: &str) -> Self {
        let mut memory: BTreeMap<String, String> = BTreeMap::new();
        let mut terms: Vec<Term> = Vec::new();
        let mut seen: BTreeSet<String> = BTreeSet::new();

        for (key, english) in en {
            let Some(translated) = target.get(key) else {
                continue;
            };
            if english.trim().is_empty() || translated.trim().is_empty() {
                continue;
            }

            let needle = english.to_lowercase();
            // First writer wins for both maps. Two keys CAN carry the same
            // English string with different translations (grammatical
            // gender, a block vs. the biome named after it); `en` is a
            // BTreeMap, so "first" is its key order — arbitrary but stable,
            // which is what the cache identity needs.
            memory
                .entry(needle.clone())
                .or_insert_with(|| translated.clone());

            if english.split_whitespace().count() > MAX_TERM_WORDS {
                continue;
            }
            if seen.insert(needle.clone()) {
                terms.push(Term {
                    needle,
                    english: english.clone(),
                    translated: translated.clone(),
                });
            }
        }

        // Longest first, so `terms_for` can consume "Block of Copper" before
        // "Copper" ever gets a chance to match inside it. Ties broken by the
        // needle itself purely for determinism.
        terms.sort_by(|a, b| {
            b.needle
                .len()
                .cmp(&a.needle.len())
                .then_with(|| a.needle.cmp(&b.needle))
        });

        Self {
            memory,
            terms,
            version: version.to_string(),
        }
    }

    /// The Minecraft version this corpus came from, or `"none"`.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Vanilla's translation of `source` as a WHOLE string, or `None`.
    ///
    /// Case-insensitive because mods routinely differ from vanilla only in
    /// capitalisation, and vanilla's own casing is the one worth copying.
    #[must_use]
    pub fn exact(&self, source: &str) -> Option<String> {
        self.memory.get(&source.to_lowercase()).cloned()
    }

    /// At most `max` term pairs, in longest-first order, restricted to terms
    /// that actually OCCUR in `sources`.
    ///
    /// Restricting to occurrences is the whole point: a prompt carrying the
    /// entire vanilla termbase would be enormous, would cost more than the
    /// translation it guides, and would bury the handful of terms that matter.
    ///
    /// A matched span is blanked out of the working copy before the shorter
    /// terms are tried, so `Block of Copper` is never also taught as `Copper`
    /// — while a `Copper` occurring somewhere else in the same string still
    /// counts, because only the covered span was consumed.
    #[must_use]
    pub fn terms_for(&self, sources: &[String], max: usize) -> Vec<(&str, &str)> {
        if max == 0 {
            return Vec::new();
        }
        let mut haystacks: Vec<String> = sources.iter().map(|s| s.to_lowercase()).collect();
        let mut out: Vec<(&str, &str)> = Vec::new();

        for term in &self.terms {
            let mut hit = false;
            for hay in &mut haystacks {
                while let Some(at) = hay.find(&term.needle) {
                    hit = true;
                    // Same byte length in, same byte length out, so every
                    // other match position in this haystack stays valid. The
                    // replacement is spaces, which no term can match: a term
                    // is non-blank by construction above.
                    hay.replace_range(at..at + term.needle.len(), &" ".repeat(term.needle.len()));
                }
            }
            if hit {
                out.push((term.english.as_str(), term.translated.as_str()));
                if out.len() == max {
                    break;
                }
            }
        }
        out
    }
}

/// Vanilla's corpus for `target_code`, read off disk for one instance.
///
/// `version_json` is `<versions_dir>/<mc_version>/<mc_version>.json` built
/// from the **raw** `mc_version` — the same value
/// `datapacks::compat::client_jar_path` takes; there is no path helper for it
/// in the repo, so the caller builds it.
///
/// Never fails. Every reason the corpus cannot be read — jar absent (the
/// instance was never launched), asset index not yet downloaded, target
/// language not shipped by vanilla, a pre-1.8 index using the legacy spelling
/// (see the module docs) — yields [`Glossary::empty`] and one `crate::diag!`
/// line. The run then costs more and reads slightly worse; it does not stop.
#[must_use]
pub fn load_for_instance(
    client_jar: &Path,
    assets_dir: &Path,
    version_json: &Path,
    target_code: &str,
    mc_version: &str,
) -> Glossary {
    let en = match read_vanilla_english(client_jar) {
        Ok(map) => map,
        Err(why) => {
            crate::diag!(
                "[l10n] glossary: no vanilla English for {mc_version} ({why}) — \
                 prefill will run without a termbase"
            );
            return Glossary::empty();
        }
    };
    let target = match read_vanilla_target(assets_dir, version_json, target_code) {
        Ok(map) => map,
        Err(why) => {
            crate::diag!(
                "[l10n] glossary: no vanilla {target_code} for {mc_version} ({why}) — \
                 prefill will run without a termbase"
            );
            return Glossary::empty();
        }
    };
    Glossary::from_lang_maps(&en, &target, mc_version)
}

/// `assets/minecraft/lang/en_us.json` out of the client jar.
///
/// Reads that one entry rather than the whole archive, mirroring
/// `pack_format::from_client_jar_path`: a real Mojang client jar is tens of
/// megabytes and the entry is a few hundred kilobytes.
fn read_vanilla_english(client_jar: &Path) -> Result<LangMap, String> {
    const ENTRY: &str = "assets/minecraft/lang/en_us.json";

    let file = std::fs::File::open(client_jar)
        .map_err(|e| format!("client jar {}: {e}", client_jar.display()))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("client jar not a zip: {e}"))?;
    let mut entry = archive
        .by_name(ENTRY)
        .map_err(|e| format!("client jar has no {ENTRY}: {e}"))?;
    let mut body = Vec::new();
    std::io::Read::read_to_end(&mut entry, &mut body)
        .map_err(|e| format!("reading {ENTRY}: {e}"))?;
    scan::parse_lang_json(&body).ok_or_else(|| format!("{ENTRY} is not a lang object"))
}

/// Just enough of a version JSON to find the asset index.
///
/// A narrow local shape rather than `versions::version_json::VersionDetails`:
/// that type requires `mainClass` and `libraries`, neither of which this
/// module cares about, so a version JSON that is odd in some unrelated way
/// would cost us the glossary for no reason. The `assetIndex` value itself is
/// the repo's own `AssetIndexRef`, so the two cannot drift on its shape.
#[derive(Deserialize)]
struct AssetIndexOnly {
    #[serde(rename = "assetIndex")]
    asset_index: Option<crate::versions::version_json::AssetIndexRef>,
}

/// `minecraft/lang/<code>.json` out of the asset-objects store: version JSON →
/// asset index id → the index → the object's hash → `<assets>/objects/<2
/// hex>/<hash>`.
fn read_vanilla_target(
    assets_dir: &Path,
    version_json: &Path,
    target_code: &str,
) -> Result<LangMap, String> {
    let raw = std::fs::read(version_json)
        .map_err(|e| format!("version json {}: {e}", version_json.display()))?;
    let parsed: AssetIndexOnly =
        serde_json::from_slice(&raw).map_err(|e| format!("version json unreadable: {e}"))?;
    let index_id = parsed
        .asset_index
        .ok_or_else(|| "version json carries no assetIndex".to_string())?
        .id;

    let index_path = assets_dir.join("indexes").join(format!("{index_id}.json"));
    let index_raw = std::fs::read(&index_path)
        .map_err(|e| format!("asset index {}: {e}", index_path.display()))?;
    let index: crate::versions::assets::AssetIndex =
        serde_json::from_slice(&index_raw).map_err(|e| format!("asset index unreadable: {e}"))?;

    let logical = format!("minecraft/lang/{}.json", target_code.to_ascii_lowercase());
    let object = index
        .objects
        .get(&logical)
        .ok_or_else(|| format!("asset index {index_id} has no {logical}"))?;

    let shard = object
        .hash
        .get(0..2)
        .ok_or_else(|| format!("asset object hash {:?} is too short", object.hash))?;
    let object_path = assets_dir.join("objects").join(shard).join(&object.hash);
    let body = std::fs::read(&object_path)
        .map_err(|e| format!("asset object {}: {e}", object_path.display()))?;
    scan::parse_lang_json(&body).ok_or_else(|| format!("{logical} is not a lang object"))
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::path::PathBuf;

    use sha1::{Digest, Sha1};

    use super::*;

    fn map(pairs: &[(&str, &str)]) -> LangMap {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    /// Three pairs: one standalone term, one term that is a substring of the
    /// third. That overlap is the whole point — it is what the longest-first
    /// rule has to get right.
    fn glossary() -> Glossary {
        let en = map(&[
            ("item.minecraft.stick", "Stick"),
            ("item.minecraft.copper_ingot", "Copper"),
            ("block.minecraft.copper_block", "Block of Copper"),
        ]);
        let tr = map(&[
            ("item.minecraft.stick", "Палка"),
            ("item.minecraft.copper_ingot", "Медь"),
            ("block.minecraft.copper_block", "Медный блок"),
        ]);
        Glossary::from_lang_maps(&en, &tr, "1.20.1")
    }

    #[test]
    fn an_exact_whole_string_hit_needs_no_model_call() {
        assert_eq!(glossary().exact("Stick"), Some("Палка".to_string()));
        assert_eq!(
            glossary().exact("stick"),
            Some("Палка".to_string()),
            "case-insensitive"
        );
        assert_eq!(glossary().exact("Stick of Truth"), None);
    }

    #[test]
    fn terms_are_extracted_only_for_strings_that_contain_them() {
        let g = glossary();
        assert!(
            g.terms_for(&["Andesite Alloy".to_string()], 8).is_empty(),
            "no glossary term occurs in this source"
        );
        assert_eq!(
            g.terms_for(&["Reinforced Stick Machine".to_string()], 8),
            vec![("Stick", "Палка")]
        );
        assert_eq!(
            g.terms_for(&["a stick".to_string()], 8),
            vec![("Stick", "Палка")],
            "matching is case-insensitive, like `exact`"
        );

        // A whole sentence is translation MEMORY, never a term: injected into
        // a prompt it matches nothing and teaches nothing, so only short
        // strings are promoted to terms.
        let sentence = Glossary::from_lang_maps(
            &map(&[("k", "Place the block on the ground")]),
            &map(&[("k", "Поставьте блок на землю")]),
            "1.20.1",
        );
        assert_eq!(
            sentence.exact("Place the block on the ground"),
            Some("Поставьте блок на землю".to_string())
        );
        assert!(sentence
            .terms_for(&["Place the block on the ground".to_string()], 8)
            .is_empty());
    }

    #[test]
    fn longer_terms_win_over_their_own_substrings() {
        // "Block of Copper" must not be taught as "Copper".
        let g = glossary();
        assert_eq!(
            g.terms_for(&["Block of Copper Frame".to_string()], 8),
            vec![("Block of Copper", "Медный блок")],
            "the shorter term is covered by the longer one and must not be taught twice"
        );
        // …but an occurrence OUTSIDE the longer term still counts.
        assert_eq!(
            g.terms_for(&["Block of Copper and Raw Copper".to_string()], 8),
            vec![("Block of Copper", "Медный блок"), ("Copper", "Медь")]
        );
    }

    #[test]
    fn term_injection_is_capped_so_the_prompt_cannot_balloon() {
        let g = glossary();
        let sources = vec!["Stick, Copper, Block of Copper".to_string()];
        assert_eq!(
            g.terms_for(&sources, 8).len(),
            3,
            "all three terms occur, so the cap below is a real cap"
        );
        assert_eq!(g.terms_for(&sources, 2).len(), 2);
        assert!(g.terms_for(&sources, 0).is_empty());
    }

    #[test]
    fn an_empty_glossary_is_valid_and_simply_never_hits() {
        let g = Glossary::empty();
        assert_eq!(g.exact("Stick"), None);
        assert!(g.terms_for(&["Stick".to_string()], 8).is_empty());
        assert_eq!(g.version(), "none");
    }

    /// Build an in-memory jar containing the given (path, contents) entries.
    /// Local copy of the identical helper in `namespace_scan.rs`'s and
    /// `scan.rs`'s test modules — not worth making `pub` for six lines only
    /// tests use.
    fn jar(entries: &[(&str, &str)]) -> Vec<u8> {
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

    struct Fixture {
        // Kept alive for the test's duration; dropping it removes the tree.
        _td: tempfile::TempDir,
        client_jar: PathBuf,
        assets: PathBuf,
        version_json: PathBuf,
    }

    const VANILLA_EN: &str = r#"{
        "item.minecraft.stick": "Stick",
        "block.minecraft.copper_block": "Block of Copper",
        "item.minecraft.bundle": "Bundle"
    }"#;

    const VANILLA_RU: &str = r#"{
        "item.minecraft.stick": "Палка",
        "block.minecraft.copper_block": "Медный блок"
    }"#;

    /// A real on-disk instance layout: the client jar carrying vanilla
    /// English, a `versions/<mc>/<mc>.json` naming an asset index, that index
    /// naming the Russian lang object, and the object itself at its sharded
    /// hash path. `with_index = false` omits only the index file — the one
    /// piece a never-launched instance is missing.
    fn seed(with_index: bool) -> Fixture {
        let td = tempfile::tempdir().unwrap();

        let version_dir = td.path().join("versions").join("1.20.1");
        std::fs::create_dir_all(&version_dir).unwrap();
        let client_jar = version_dir.join("1.20.1.jar");
        std::fs::write(
            &client_jar,
            jar(&[("assets/minecraft/lang/en_us.json", VANILLA_EN)]),
        )
        .unwrap();
        let version_json = version_dir.join("1.20.1.json");
        std::fs::write(
            &version_json,
            r#"{"id":"1.20.1","assetIndex":{"id":"5","url":"https://example.invalid/5.json","sha1":"0000000000000000000000000000000000000000","size":1,"totalSize":2}}"#,
        )
        .unwrap();

        let assets = td.path().join("assets");
        let hash = hex::encode(Sha1::digest(VANILLA_RU.as_bytes()));
        let shard = assets.join("objects").join(&hash[0..2]);
        std::fs::create_dir_all(&shard).unwrap();
        std::fs::write(shard.join(&hash), VANILLA_RU).unwrap();

        if with_index {
            let indexes = assets.join("indexes");
            std::fs::create_dir_all(&indexes).unwrap();
            std::fs::write(
                indexes.join("5.json"),
                format!(
                    r#"{{"objects":{{"minecraft/lang/ru_ru.json":{{"hash":"{hash}","size":{}}}}}}}"#,
                    VANILLA_RU.len()
                ),
            )
            .unwrap();
        }

        Fixture {
            _td: td,
            client_jar,
            assets,
            version_json,
        }
    }

    #[test]
    fn load_for_instance_reads_the_jar_and_the_asset_object() {
        // The end-to-end path, against real files in a tempdir.
        let f = seed(true);
        let g = load_for_instance(&f.client_jar, &f.assets, &f.version_json, "ru_ru", "1.20.1");

        assert_eq!(g.version(), "1.20.1");
        assert_eq!(g.exact("Stick"), Some("Палка".to_string()));
        assert_eq!(
            g.terms_for(&["Block of Copper Frame".to_string()], 8),
            vec![("Block of Copper", "Медный блок")]
        );
        assert_eq!(
            g.exact("Bundle"),
            None,
            "vanilla ships no Russian for this key, so it must not pair at all"
        );
    }

    #[test]
    fn a_missing_asset_index_degrades_to_an_empty_glossary_not_an_error() {
        // Quality loss, never a failed run.
        let f = seed(false);
        let g = load_for_instance(&f.client_jar, &f.assets, &f.version_json, "ru_ru", "1.20.1");

        assert_eq!(g.version(), "none");
        assert_eq!(g.exact("Stick"), None);
        assert!(g.terms_for(&["Block of Copper".to_string()], 8).is_empty());

        // The control that stops this test being vacuous: an empty result is
        // evidence about the MISSING INDEX only if the same fixture with the
        // index present is non-empty. Without it, a fixture broken any other
        // way — wrong jar entry name, wrong object shard — would satisfy the
        // assertions above for entirely the wrong reason.
        let ok = seed(true);
        assert_eq!(
            load_for_instance(
                &ok.client_jar,
                &ok.assets,
                &ok.version_json,
                "ru_ru",
                "1.20.1"
            )
            .exact("Stick"),
            Some("Палка".to_string()),
            "this fixture differs from the one above ONLY in the asset index"
        );
    }
}
