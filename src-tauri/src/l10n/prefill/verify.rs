//! The machine-output contract.
//!
//! `l10n::validate` (shipped in #331) answers a narrower question: are the
//! argument indices this string references satisfiable? That is the right
//! check for a human typing into the editor, who is trusted to mean what
//! they wrote. It is NOT enough for model output — a translation that
//! silently drops every placeholder passes it.
//!
//! So the machine path adds this: the placeholder multiset must match, the
//! `§` code sequence must match, no control characters may appear, the
//! result must be non-empty, and literals that must never be translated —
//! URLs, file paths, `/commands`, `namespace:id` registry ids — must be
//! reproduced verbatim. A failure here is not an error: the key is left out
//! and the mod's own English shows, which costs nothing because a resource
//! pack merges per key.

use crate::l10n::validate::{self, FormatError};
use serde::Serialize;
use specta::Type;

/// Why a machine-produced translation was refused.
///
/// `Format` carries its `FormatError` in a NAMED field rather than as a
/// newtype (`Format(FormatError)`), and must keep doing so. This enum is
/// internally tagged on `kind` and `FormatError` is internally tagged on
/// `kind` too; serde merges a newtype payload into the OUTER object, so
/// `Format(FormatError)` would emit two `"kind"` keys in one object — the
/// last one wins in JavaScript, and specta's TypeScript for it is a
/// `{ kind: "format" } & FormatError` intersection that types `kind` as
/// `never`, so the UI could never match the variant. `Error::
/// L10nTranslationInvalid { reason: FormatError }` already nests a
/// `FormatError` under a field for exactly this reason. Pinned by
/// `a_format_rejection_serialises_with_exactly_one_discriminant`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RejectReason {
    /// The grammar/index check from `l10n::validate` failed.
    Format {
        error: FormatError,
    },
    Empty,
    PlaceholdersChanged,
    ColourCodesChanged,
    ControlCharacter,
    /// A literal that must never be translated is missing from the result.
    LiteralMissing {
        literal: String,
    },
}

/// Check a model's answer against the English it was given.
///
/// The order of the checks is the order of the reasons a reviewer would want
/// to read: gone, ungrammatical, reshaped, recoloured, corrupted, gutted.
pub fn verify(source: &str, translation: &str) -> Result<(), RejectReason> {
    if translation.trim().is_empty() {
        return Err(RejectReason::Empty);
    }

    validate::validate(source, translation).map_err(|error| RejectReason::Format { error })?;

    if placeholder_multiset(source) != placeholder_multiset(translation) {
        return Err(RejectReason::PlaceholdersChanged);
    }

    if colour_codes(source) != colour_codes(translation) {
        return Err(RejectReason::ColourCodesChanged);
    }

    if introduces_control_char(source, translation) {
        return Err(RejectReason::ControlCharacter);
    }

    for literal in do_not_translate(source) {
        if !translation.contains(literal.as_str()) {
            return Err(RejectReason::LiteralMissing { literal });
        }
    }

    Ok(())
}

/// Every argument this string consumes, as a SORTED multiset of 1-based
/// indices. Implicit `%s` occurrences are numbered in appearance order —
/// the same numbering `l10n::validate::check_indices` uses — so `%s %s` and
/// `%1$s %2$s` are the same multiset and reordering to `%2$s %1$s` is free,
/// while dropping, adding or duplicating an index is not.
///
/// This deliberately does not re-report grammar errors: `validate` has
/// already run and rejected anything malformed, so a specifier that is
/// neither `s` nor `%` is unreachable here and is simply skipped.
fn placeholder_multiset(s: &str) -> Vec<u32> {
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut implicit = 0u32;
    let mut i = 0usize;

    while i < chars.len() {
        if chars[i] != '%' {
            i += 1;
            continue;
        }
        i += 1; // past '%'

        let start = i;
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
        let explicit = if i > start && chars.get(i) == Some(&'$') {
            // Digit-only by construction; `unwrap_or(0)` covers only an
            // absurdly long digit run overflowing u32, and 0 is never a
            // valid index, so such a string was already rejected upstream.
            let n = chars[start..i]
                .iter()
                .collect::<String>()
                .parse::<u32>()
                .unwrap_or(0);
            i += 1; // past '$'
            Some(n)
        } else {
            // Digits not followed by '$' are not an index — rewind so the
            // first digit is read as the specifier position.
            i = start;
            None
        };

        match chars.get(i) {
            Some('s') => {
                i += 1;
                match explicit {
                    Some(n) => out.push(n),
                    None => {
                        implicit += 1;
                        out.push(implicit);
                    }
                }
            }
            // `%%` is a literal percent, not an argument. Anything else is
            // unreachable after `validate`.
            Some(_) => i += 1,
            None => break,
        }
    }

    out.sort_unstable();
    out
}

/// The `§x` codes in appearance order. A SEQUENCE, not a set: `§a…§r` and
/// `§c…§r` use the same codes in different roles and must not compare equal.
/// Codes are case-insensitive in the client, so they fold to lower case here.
fn colour_codes(s: &str) -> Vec<char> {
    let mut out = Vec::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '§' {
            continue;
        }
        match chars.next() {
            Some(code) => out.push(code.to_ascii_lowercase()),
            // A trailing bare `§` is still a difference from no `§` at all;
            // record the marker itself so the two do not compare equal.
            None => out.push('§'),
        }
    }
    out
}

/// True when the translation contains a control character the source did not.
/// A source with a literal newline is rare but legal, and a translation is
/// allowed to keep it; inventing one is what corrupts a `.lang` value.
fn introduces_control_char(source: &str, translation: &str) -> bool {
    translation
        .chars()
        .any(|c| c.is_control() && !source.contains(c))
}

/// Punctuation that belongs to the sentence, not to the literal it follows.
const TRAILING_PUNCTUATION: &[char] = &['.', ',', ';', ':', '!', '?', ')', ']', '}', '"', '\''];

/// Substrings of the source that a translation must reproduce byte for byte.
///
/// A hand scanner rather than a regex: `regex` has no look-around, and the
/// one rule that actually matters here — a `/command` starts a token, so
/// `and/or` is prose — is a word-boundary rule that reads far more clearly
/// spelled out. Each rule is deliberately narrow; a missed literal only
/// means one fewer check, while a false positive rejects a good translation
/// of every tooltip that happens to contain a slash.
fn do_not_translate(source: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for token in source.split_whitespace() {
        if let Some(url) = url_in(token) {
            push_unique(&mut out, url);
            // A URL already contains slashes, colons and dots; letting the
            // other rules re-scan it would only produce fragments of it.
            continue;
        }
        if let Some(command) = command_in(token) {
            push_unique(&mut out, command);
        }
        if let Some(id) = registry_id_in(token) {
            push_unique(&mut out, &id);
        }
        if let Some(path) = path_in(token) {
            push_unique(&mut out, path);
        }
    }
    out
}

fn push_unique(out: &mut Vec<String>, literal: &str) {
    if !literal.is_empty() && !out.iter().any(|existing| existing == literal) {
        out.push(literal.to_string());
    }
}

/// `http://` or `https://` up to the end of the token.
fn url_in(token: &str) -> Option<&str> {
    let at = ["https://", "http://"]
        .iter()
        .filter_map(|scheme| token.find(*scheme))
        .min()?;
    Some(trim_trailing_punctuation(&token[at..]))
}

/// A `/command`: the slash must OPEN the token, which is exactly "preceded
/// by start-of-string or whitespace". That is the whole reason `and/or`,
/// `1/2` and `red/blue` are prose and not commands.
fn command_in(token: &str) -> Option<&str> {
    let rest = token.strip_prefix('/')?;
    if !rest.chars().next()?.is_ascii_alphanumeric() {
        return None;
    }
    Some(trim_trailing_punctuation(token))
}

/// Characters legal in a Minecraft registry id. Lower case only, which is
/// what keeps `Note:` and `Warning:` out of this rule.
fn is_id_char(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '-')
}

/// A `namespace:path` registry id embedded anywhere in the token.
fn registry_id_in(token: &str) -> Option<String> {
    let chars: Vec<char> = token.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if *c != ':' {
            continue;
        }
        let mut start = i;
        while start > 0 && is_id_char(chars[start - 1]) {
            start -= 1;
        }
        let mut end = i + 1;
        while end < chars.len() && is_id_char(chars[end]) {
            end += 1;
        }
        if start < i && end > i + 1 {
            return Some(chars[start..end].iter().collect());
        }
    }
    None
}

/// A path-looking run: a separator plus a file extension AFTER the last
/// separator. Requiring a real extension rather than merely "a dot
/// somewhere" is what keeps `and/or.` — a slash and a dot — out of this
/// rule, which is the same false positive `command_in` guards against.
fn path_in(token: &str) -> Option<&str> {
    let trimmed = trim_trailing_punctuation(token);
    let last_sep = trimmed.rfind(['/', '\\'])?;
    let leaf = trimmed.get(last_sep + 1..)?;
    let dot = leaf.rfind('.')?;
    let ext = leaf.get(dot + 1..)?;
    if ext.is_empty() || ext.len() > 5 || !ext.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    Some(trimmed)
}

fn trim_trailing_punctuation(s: &str) -> &str {
    s.trim_end_matches(TRAILING_PUNCTUATION)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_existing_validator_alone_is_not_enough() {
        // Documents WHY this module exists. If this ever starts returning
        // Err, l10n::validate has grown a multiset check and this module
        // can shrink.
        assert!(crate::l10n::validate::validate("Uses %s of %s", "Тратит ресурсы").is_ok());
        assert!(verify("Uses %s of %s", "Тратит ресурсы").is_err());
    }

    #[test]
    fn accepts_a_faithful_translation() {
        assert!(verify("Copper Ingot", "Медный слиток").is_ok());
        assert!(verify("Uses %s of %s", "Тратит %s из %s").is_ok());
    }

    #[test]
    fn allows_reordering_with_positional_arguments() {
        // %N$s exists to reorder; Russian word order needs it.
        assert!(verify("%1$s gave %2$s", "%2$s получил от %1$s").is_ok());
    }

    #[test]
    fn rejects_a_dropped_added_or_swapped_placeholder() {
        assert!(verify("Uses %s of %s", "Тратит %s").is_err());
        assert!(verify("Uses %s", "Тратит %s из %s").is_err());
        assert!(verify("%1$s of %2$s", "%1$s из %1$s").is_err());
    }

    #[test]
    fn rejects_a_changed_colour_code_sequence() {
        assert!(verify("§aReady§r", "§aГотово§r").is_ok());
        assert!(verify("§aReady§r", "Готово").is_err());
        assert!(verify("§aReady§r", "§cГотово§r").is_err());
    }

    #[test]
    fn rejects_empty_whitespace_and_control_characters() {
        assert!(verify("Copper", "").is_err());
        assert!(verify("Copper", "   ").is_err());
        assert!(verify("Copper", "Медь\u{0}").is_err());
        // A newline the source did not have is a control char too: lang
        // values are single-line and a stray \n corrupts .lang parsing.
        assert!(verify("Copper", "Медь\nЕщё").is_err());
    }

    #[test]
    fn do_not_translate_literals_must_survive_verbatim() {
        assert!(verify(
            "See https://example.com/wiki",
            "См. https://example.com/wiki"
        )
        .is_ok());
        assert!(verify("See https://example.com/wiki", "См. сайт").is_err());
        assert!(verify("Run /gamemode creative", "Введите /gamemode creative").is_ok());
        assert!(verify("Run /gamemode creative", "Введите /режим creative").is_err());
        assert!(verify("Requires minecraft:diamond", "Требует minecraft:diamond").is_ok());
        assert!(verify("Requires minecraft:diamond", "Требует алмаз").is_err());
    }

    #[test]
    fn placeholders_compare_as_a_multiset_not_as_a_set() {
        // `rejects_a_dropped_added_or_swapped_placeholder` above catches a
        // duplicated index only because its source uses two DISTINCT ones —
        // it still fails if the comparison collapses repeats. A source that
        // repeats an index is what pins "how many times", and losing one
        // copy of `%1$s` leaves the sentence a placeholder short.
        assert!(verify("%1$s and %1$s", "%1$s").is_err());
    }

    #[test]
    fn colour_codes_compare_in_order_not_as_a_set() {
        // `rejects_a_changed_colour_code_sequence` above passes whether the
        // codes are compared as a sequence or as a multiset — {a,r} and
        // {c,r} differ either way — so it does not actually pin the
        // "sequence, not set" rule. This does: the same four codes, two of
        // them swapped, which a multiset comparison would wave through.
        assert!(verify("§aReady §rand §cStop§r", "§cГотово §rи §aСтоп§r").is_err());
    }

    #[test]
    fn a_bare_slash_in_prose_is_not_a_command() {
        // "and/or" must not be mistaken for a command literal, or half the
        // tooltips in the game become unverifiable.
        assert!(verify("Copper and/or Zinc", "Медь и/или цинк").is_ok());
    }

    #[test]
    fn a_format_rejection_serialises_with_exactly_one_discriminant() {
        // Guards the NAMED-field shape of `RejectReason::Format` documented
        // on the enum: a newtype payload would be merged into the outer
        // object and emit `"kind"` twice.
        let json = serde_json::to_string(&RejectReason::Format {
            error: FormatError::UnsupportedSpecifier { specifier: 'd' },
        })
        .expect("RejectReason is plain data and cannot fail to serialize");
        assert_eq!(
            json,
            r#"{"kind":"format","error":{"kind":"unsupported_specifier","specifier":"d"}}"#
        );
    }
}
