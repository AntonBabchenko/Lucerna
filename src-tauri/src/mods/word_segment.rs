//! Word-segmentation of a "slammed" Forge mod-id into a spaced search query.
//!
//! A Forge mod-id is lowercase with no separators (`farmersdelight`), but the
//! corresponding Modrinth/CurseForge slug is hyphenated (`farmers-delight`).
//! Both platforms tokenize the slug on hyphens, so a search for the slammed id
//! never surfaces the real project. Splitting the id back into words
//! (`farmers delight`) produces a query that does — at which point the
//! resolver's normalized slug match promotes the real project to the Exact tier.
//!
//! Pure, offline, no network. Backed by a curated project-owned wordlist
//! (`data/segment_wordlist.txt`, CC0 — not third-party data).

use std::collections::HashSet;

use once_cell::sync::Lazy;

const RAW_WORDLIST: &str = include_str!("data/segment_wordlist.txt");

/// Longest run of alphabetic characters we will attempt to segment. mod-ids are
/// short by spec; anything longer is treated as opaque (kept whole) rather than
/// burning DP time on a pathological input.
const MAX_RUN_LEN: usize = 64;

/// The curated dictionary, parsed once. `#`-comments and blank lines dropped.
static WORDLIST: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    RAW_WORDLIST
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect()
});

/// Best-effort segmentation of a mod-id into a space-joined word sequence.
///
/// Returns `Some(spaced)` only when the id splits into **two or more** tokens
/// (i.e. segmentation actually adds signal over the raw id); returns `None` for
/// a single dictionary word, an opaque/uncoverable id, or empty input — in
/// those cases the caller already searches the raw id, so there is nothing to
/// add.
///
/// Each maximal alphabetic run is segmented independently (digits, underscores
/// and other separators act as word boundaries and are dropped from the query).
/// Within a run, dynamic programming picks the full cover that maximizes the
/// sum of squared word lengths, which prefers few long words over many short
/// fragments (`backpacks` over `back packs`). A run with no full dictionary
/// cover is kept whole.
pub fn segment(id: &str) -> Option<String> {
    let lower = id.to_ascii_lowercase();
    let mut tokens: Vec<&str> = Vec::new();

    for run in lower
        .split(|c: char| !c.is_ascii_alphabetic())
        .filter(|run| !run.is_empty())
    {
        match cover_run(run) {
            Some(pieces) => tokens.extend(pieces),
            None => tokens.push(run),
        }
    }

    if tokens.len() >= 2 {
        Some(tokens.join(" "))
    } else {
        None
    }
}

/// Dynamic-programming full cover of one alphabetic `run` into dictionary words,
/// maximizing the sum of squared piece lengths. Returns the pieces, or `None`
/// when no sequence of dictionary words spans the whole run.
fn cover_run(run: &str) -> Option<Vec<&str>> {
    let n = run.len();
    if n == 0 || n > MAX_RUN_LEN {
        return None;
    }
    let dict = &*WORDLIST;

    // best[i] = (max score covering run[0..i], split index of the last piece).
    // best[0] is the empty prefix; a `None` cell is unreachable.
    let mut best: Vec<Option<(u64, usize)>> = vec![None; n + 1];
    best[0] = Some((0, 0));

    for i in 1..=n {
        for j in 0..i {
            let Some((score_j, _)) = best[j] else {
                continue;
            };
            let piece = &run[j..i];
            if !dict.contains(piece) {
                continue;
            }
            let len = (i - j) as u64;
            let candidate = score_j + len * len;
            // On a tie keep the existing (smaller-j) cover → longer leading word.
            let replace = match best[i] {
                Some((existing, _)) => candidate > existing,
                None => true,
            };
            if replace {
                best[i] = Some((candidate, j));
            }
        }
    }

    best[n]?;

    // Reconstruct from the back.
    let mut pieces = Vec::new();
    let mut i = n;
    while i > 0 {
        let (_, j) = best[i].expect("reachable cell on the reconstruction path");
        pieces.push(&run[j..i]);
        i = j;
    }
    pieces.reverse();
    Some(pieces)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_two_word_mod_ids() {
        assert_eq!(
            segment("farmersdelight").as_deref(),
            Some("farmers delight")
        );
        assert_eq!(segment("appleskin").as_deref(), Some("apple skin"));
        assert_eq!(segment("mousetweaks").as_deref(), Some("mouse tweaks"));
        assert_eq!(
            segment("naturescompass").as_deref(),
            Some("natures compass")
        );
    }

    #[test]
    fn prefers_long_words_over_short_fragments() {
        // "backpacks" (single dict word) must win over "back"+"packs".
        assert_eq!(
            segment("sophisticatedbackpacks").as_deref(),
            Some("sophisticated backpacks")
        );
        // The whole-word "blockheads" entry beats "block"+"heads".
        assert_eq!(
            segment("cookingforblockheads").as_deref(),
            Some("cooking for blockheads")
        );
    }

    #[test]
    fn non_alpha_separators_become_boundaries() {
        assert_eq!(
            segment("just_enough_items").as_deref(),
            Some("just enough items")
        );
        // Digits are boundaries too and are dropped from the query.
        assert_eq!(segment("appleskin2").as_deref(), Some("apple skin"));
    }

    #[test]
    fn whole_word_slugs_are_kept_intact() {
        // These slugs have no hyphen and already resolve via the raw query, so
        // segmenting them would only add a needless second search. The whole
        // word in the dictionary suppresses the split.
        assert_eq!(segment("waystones"), None);
        assert_eq!(segment("supplementaries"), None);
    }

    #[test]
    fn single_words_and_opaque_ids_do_not_segment() {
        assert_eq!(segment("jade"), None); // not segmentable
        assert_eq!(segment("create"), None); // single concept, no cover/one word
        assert_eq!(segment("jei"), None); // acronym, no cover
        assert_eq!(segment("xyzzy"), None); // gibberish
        assert_eq!(segment(""), None);
    }

    #[test]
    fn case_is_normalized() {
        assert_eq!(
            segment("FarmersDelight").as_deref(),
            Some("farmers delight")
        );
    }

    /// The 2026-08-12 case. Measured against the live Modrinth API that day:
    /// the slammed query `forgeconfigapiport` returns 0 hits under the
    /// forge + 1.20.6 facets and, unfaceted, only the unrelated
    /// `kilt-forgeconfigapiport-fix`; `forge config api port` returns the real
    /// `forge-config-api-port` as hit #1. The words for it were missing.
    #[test]
    fn segments_the_loader_and_platform_vocabulary() {
        assert_eq!(
            segment("forgeconfigapiport").as_deref(),
            Some("forge config api port")
        );
        assert_eq!(segment("fabricapi").as_deref(), Some("fabric api"));
    }

    /// A multi-loader compound must prefer the longest covering word. If
    /// `neoforge` were absent while `forge` is present, `neoforge` would fail
    /// to cover at all ("neo" is not a word) and be kept whole — correct but
    /// accidental. Pinning it makes the intent explicit.
    #[test]
    fn loader_names_stay_whole_words() {
        assert_eq!(segment("forge"), None);
        assert_eq!(segment("neoforge"), None);
        assert_eq!(segment("fabric"), None);
        assert_eq!(segment("quilt"), None);
    }

    /// The wordlist grew by six entries, three of them short (`api`, `port`,
    /// and the loader names). Short words are the ones that can shatter an id
    /// that used to resolve, so the previously-working set is pinned here.
    /// The DP maximises the sum of squared token lengths, which biases toward
    /// few long words — this test is the proof rather than the hope.
    #[test]
    fn wordlist_growth_does_not_over_segment_previously_working_ids() {
        for id in [
            "jei",
            "jade",
            "create",
            "xyzzy",
            "waystones",
            "supplementaries",
        ] {
            assert_eq!(segment(id), None, "{id} must not start splitting");
        }
        // And the ids that already segmented still segment the same way.
        assert_eq!(
            segment("farmersdelight").as_deref(),
            Some("farmers delight")
        );
    }
}
