//! Matching what the player read on screen against what the lang file holds.
//!
//! The player copies a line out of the game. That line has been through
//! Minecraft's formatter, so it differs from the file value in two systematic
//! ways, both live in a real instance:
//!
//! | file value | what the player sees |
//! |---|---|
//! | `Invalid language: %s` | `Invalid language: ru_ru` |
//! | `§dServux: %s§r` | `Servux: 1.2.3` |
//!
//! A plain substring search finds neither. So both sides are normalised —
//! section codes dropped, whitespace collapsed, lowercased — and placeholders
//! are treated as "anything at all".
//!
//! Deliberately not reusing `l10n::validate`, which also parses `%`
//! placeholders: that counts them to check a translation's arity, and a
//! matcher that shared its parser would inherit rules about *validity* it has
//! no business enforcing. Same grammar, different question.

/// A `§` and the character after it are formatting, never content.
///
/// Minecraft accepts any following char; there is no need to whitelist the
/// legal ones, because a `§` in ordinary text is vanishingly rare and dropping
/// two characters costs a match nobody was going to make.
fn strip_section_codes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '§' {
            chars.next();
        } else {
            out.push(c);
        }
    }
    out
}

/// Lowercased, section-free, single-spaced. Applied to BOTH sides so the
/// comparison is symmetric.
pub fn normalise(s: &str) -> String {
    strip_section_codes(s)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Split a normalised value on its format placeholders.
///
/// `%s`, `%d`, `%1$s` and friends stand for text the player saw and we cannot
/// know, so each becomes a gap the query may fill with anything. `%%` is a
/// literal percent and is NOT a gap.
fn split_on_placeholders(value: &str) -> Vec<String> {
    let bytes: Vec<char> = value.chars().collect();
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != '%' {
            cur.push(bytes[i]);
            i += 1;
            continue;
        }
        if bytes.get(i + 1) == Some(&'%') {
            cur.push('%');
            i += 2;
            continue;
        }
        // `%` [digits `$`] conversion. Anything else is a stray percent and is
        // treated as literal text rather than swallowed.
        let mut j = i + 1;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        if j < bytes.len() && bytes[j] == '$' {
            j += 1;
        }
        if j < bytes.len() && bytes[j].is_ascii_alphabetic() {
            parts.push(std::mem::take(&mut cur));
            i = j + 1;
        } else {
            cur.push('%');
            i += 1;
        }
    }
    parts.push(cur);
    parts
}

/// How well `query` matches `value`. `None` means no match at all.
///
/// The ranking exists because a hundred mods make "save" hit dozens of keys,
/// and an unranked list is barely better than the mod list it replaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rank {
    /// The value contains the query somewhere.
    Contains,
    /// The value starts with the query.
    Prefix,
    /// The whole value is the query.
    Exact,
}

/// Match a raw query against a raw value, normalising both.
pub fn rank(query: &str, value: &str) -> Option<Rank> {
    let q = normalise(query);
    if q.is_empty() {
        return None;
    }
    let v = normalise(value);

    if v == q {
        return Some(Rank::Exact);
    }
    if v.starts_with(&q) {
        return Some(Rank::Prefix);
    }
    if v.contains(&q) {
        return Some(Rank::Contains);
    }

    // Nothing literal matched, so try again across the placeholders: the
    // player's copy has real text where the file has `%s`.
    let parts = split_on_placeholders(&v);
    if parts.len() > 1 && covers_with_gaps(&q, &parts) {
        // Never better than `Contains`: a match that had to skip over unknown
        // substituted text is weaker evidence than a literal one.
        return Some(Rank::Contains);
    }
    None
}

/// Whether `query` contains every literal segment of a placeholder-split
/// value, in order — i.e. the query looks like that value with its gaps filled.
///
/// Leading and trailing empty segments (a value that starts or ends with a
/// placeholder) impose no constraint and are skipped.
fn covers_with_gaps(query: &str, parts: &[String]) -> bool {
    let mut at = 0usize;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        let Some(found) = query[at..].find(part.as_str()) else {
            return false;
        };
        // The first non-empty segment must anchor at the start unless the
        // value began with a placeholder; otherwise "language: ru" would match
        // a value it merely ends inside.
        if i == 0 && found != 0 {
            return false;
        }
        at += found + part.len();
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_section_codes_from_both_sides() {
        assert_eq!(normalise("§dServux: hi§r"), "servux: hi");
    }

    #[test]
    fn a_query_copied_from_the_game_matches_a_value_holding_a_placeholder() {
        // Real pair from a live instance: servux.command.config.invalid_language.
        assert_eq!(
            rank("Invalid language: ru_ru", "Invalid language: %s"),
            Some(Rank::Contains)
        );
    }

    #[test]
    fn a_query_matches_across_section_codes_and_a_placeholder_together() {
        // servux.command.about — the player sees neither the § codes nor the %s.
        assert_eq!(
            rank("Servux: 1.2.3", "§dServux: %s§r"),
            Some(Rank::Contains)
        );
    }

    #[test]
    fn an_exact_hit_outranks_a_prefix_which_outranks_a_substring() {
        assert_eq!(rank("Sticky Basket", "Sticky Basket"), Some(Rank::Exact));
        assert_eq!(rank("Sticky", "Sticky Basket"), Some(Rank::Prefix));
        assert_eq!(rank("Basket", "Sticky Basket"), Some(Rank::Contains));
        assert!(Rank::Exact > Rank::Prefix && Rank::Prefix > Rank::Contains);
    }

    #[test]
    fn unrelated_text_does_not_match_through_a_placeholder() {
        // The gap must not become a wildcard for the whole string.
        assert_eq!(rank("Saved config!", "Invalid language: %s"), None);
    }

    #[test]
    fn a_literal_double_percent_is_not_a_gap() {
        // The property that matters: `%%` must never behave as a wildcard.
        assert_eq!(rank("100 apples", "100%%"), None);
        // It still matches literally. `%%` is deliberately NOT collapsed to a
        // single `%` during normalisation — doing so would turn a value like
        // `%%d` into what looks like a `%d` placeholder — so a query carrying
        // the RENDERED form ranks as a prefix rather than an exact hit. A
        // ranking nuance on a rare input, not a missed match.
        assert_eq!(rank("100%", "100%%"), Some(Rank::Prefix));
    }

    #[test]
    fn an_empty_query_matches_nothing_rather_than_everything() {
        assert_eq!(rank("   ", "anything"), None);
    }

    #[test]
    fn case_and_stray_whitespace_do_not_matter() {
        assert_eq!(
            rank("  sticky   BASKET ", "Sticky Basket"),
            Some(Rank::Exact)
        );
    }
}
