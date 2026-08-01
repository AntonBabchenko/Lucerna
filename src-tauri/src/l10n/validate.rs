//! Validation of a translated string against Minecraft's format grammar.
//!
//! The whole engine is one regex, read from `TranslatableContents`:
//!     %(?:(\d+)\$)?([A-Za-z%]|$)
//! and only three tokens are legal: `%s`, `%N$s` and `%%`. Anything else throws
//! `TranslatableFormatException`, which the client catches and degrades to the
//! raw template — visible to the player, invisible to us.

use serde::Serialize;
use specta::Type;

/// Why a translation was rejected. Carries the offending detail so the UI can
/// say what is wrong rather than "invalid".
///
/// `UnsupportedSpecifier` carries its `char` in a NAMED field, not a tuple
/// (`UnsupportedSpecifier(char)`): this enum is internally tagged
/// (`tag = "kind"`), and an internally tagged enum can only merge the tag
/// into a variant that already serializes as a map. A newtype variant
/// wrapping a bare `char` serializes as a JSON string instead, and specta's
/// exporter (`specta-serde`'s `validate_internally_tag_enum_datatype`)
/// rejects exactly that shape with "payload cannot be merged with an
/// internal tag" — verified by reading the vendored `specta-serde-0.0.12`
/// source, not assumed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FormatError {
    /// A `%x` where x is not `s` or `%`. Minecraft supports no other
    /// conversion — notably not `%d`.
    UnsupportedSpecifier { specifier: char },
    /// A `%` at end of string, or followed by nothing usable.
    DanglingPercent,
    /// The translation references argument `index` but the source cannot
    /// supply it: either `index` exceeds `available` (the source's own
    /// arity), or `index` is 0. Minecraft's indices are 1-based — `%N$s`
    /// reads `args[N-1]` — so `%0$s` parses cleanly under the grammar above
    /// but addresses `args[-1]` at substitution time. 0 is therefore never a
    /// valid index, independent of `available`.
    IndexOutOfRange { index: u32, available: u32 },
}

/// Every argument reference in a string: `Some(n)` for an explicit `%n$s`,
/// `None` for an implicit `%s`. `%%` is not an argument and is not reported.
fn scan(s: &str) -> Result<Vec<Option<u32>>, FormatError> {
    let b: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        if b[i] != '%' {
            i += 1;
            continue;
        }
        i += 1; // past '%'
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        let explicit = if i > start && i < b.len() && b[i] == '$' {
            // `b[start..i]` is digit-only by construction (the loop above
            // only advances on `is_ascii_digit()`), so parsing can only fail
            // on overflow (an unreasonably long digit run) — never on
            // malformed input. Falling back to 0 rather than panicking is
            // safe: 0 is never a valid index (see `IndexOutOfRange` above),
            // so an overflowing index is rejected downstream exactly like a
            // literal `%0$s` would be, instead of crashing on a hostile or
            // merely absurd mod string.
            let n: u32 = b[start..i].iter().collect::<String>().parse().unwrap_or(0);
            i += 1; // past '$'
            Some(n)
        } else {
            // Digits not followed by '$' are not an index — rewind so the
            // first digit is read as the specifier letter, which will then be
            // rejected as unsupported.
            i = start;
            None
        };
        let Some(&c) = b.get(i) else {
            return Err(FormatError::DanglingPercent);
        };
        i += 1;
        match c {
            '%' => {
                if explicit.is_some() {
                    return Err(FormatError::UnsupportedSpecifier { specifier: '%' });
                }
            }
            's' => out.push(explicit),
            other => return Err(FormatError::UnsupportedSpecifier { specifier: other }),
        }
    }
    Ok(out)
}

/// How many arguments a string requires: the larger of its implicit count and
/// its highest explicit index. The implicit counter only advances on implicit
/// `%s`, so mixing the two forms is well defined.
fn arity(refs: &[Option<u32>]) -> u32 {
    let implicit = refs.iter().filter(|r| r.is_none()).count() as u32;
    let max_explicit = refs.iter().filter_map(|r| *r).max().unwrap_or(0);
    implicit.max(max_explicit)
}

/// Every reference in `refs` must land in `1..=available` — `0` is never a
/// valid Minecraft argument index (see `FormatError::IndexOutOfRange`).
/// Implicit `%s` occurrences are numbered from 1 in appearance order.
fn check_indices(refs: &[Option<u32>], available: u32) -> Result<(), FormatError> {
    let mut implicit_seen = 0u32;
    for r in refs {
        let index = match r {
            Some(n) => *n,
            None => {
                implicit_seen += 1;
                implicit_seen
            }
        };
        if index == 0 || index > available {
            return Err(FormatError::IndexOutOfRange { index, available });
        }
    }
    Ok(())
}

/// Validate `translation` against the arguments `source` implies.
///
/// Order is deliberately NOT compared: positional `%N$s` exists to reorder
/// arguments and Russian word order needs it. Only the referenced index SET
/// must be satisfiable.
///
/// The source is checked against its OWN arity too, not just parsed: a stray
/// `%0$s` in the mod's own English string parses cleanly under the grammar
/// (see `scan`) but is never a valid index (see `IndexOutOfRange`), and
/// `arity()` alone — which only tracks the maximum explicit index — has no
/// way to notice a zero hiding among otherwise-fine references. Letting that
/// through would be exactly the "silently permit anything" failure this
/// module exists to prevent.
pub fn validate(source: &str, translation: &str) -> Result<(), FormatError> {
    let src = scan(source)?;
    let available = arity(&src);
    check_indices(&src, available)?;

    let tr = scan(translation)?;
    check_indices(&tr, available)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_translation_matching_the_source() {
        assert_eq!(validate("Give %s to %s", "Дать %s кому-то %s"), Ok(()));
    }

    #[test]
    fn accepts_positional_reordering() {
        // The entire point of %N$s. Vanilla itself ships
        // "Missing argument %2$s to function %1$s".
        assert_eq!(validate("%1$s of %2$s", "%2$s из %1$s"), Ok(()));
    }

    #[test]
    fn accepts_a_literal_percent() {
        assert_eq!(validate("%s%% done", "%s%% готово"), Ok(()));
    }

    #[test]
    fn accepts_dropping_an_argument() {
        // A translation may legitimately use fewer arguments than supplied;
        // the game ignores extras.
        assert_eq!(validate("%s of %s", "готово"), Ok(()));
    }

    #[test]
    fn rejects_percent_d() {
        // The single most likely mistake, from humans and LLMs alike.
        assert_eq!(
            validate("%s items", "%d предметов"),
            Err(FormatError::UnsupportedSpecifier { specifier: 'd' })
        );
    }

    #[test]
    fn rejects_other_printf_conversions() {
        assert_eq!(
            validate("%s", "%f"),
            Err(FormatError::UnsupportedSpecifier { specifier: 'f' })
        );
        assert_eq!(
            validate("%s", "%n"),
            Err(FormatError::UnsupportedSpecifier { specifier: 'n' })
        );
    }

    #[test]
    fn rejects_a_trailing_percent() {
        assert_eq!(
            validate("%s", "готово %"),
            Err(FormatError::DanglingPercent)
        );
    }

    #[test]
    fn rejects_an_index_the_source_cannot_supply() {
        assert_eq!(
            validate("%s", "%2$s"),
            Err(FormatError::IndexOutOfRange {
                index: 2,
                available: 1
            })
        );
    }

    #[test]
    fn rejects_more_implicit_placeholders_than_the_source_supplies() {
        assert_eq!(
            validate("%s", "%s и %s"),
            Err(FormatError::IndexOutOfRange {
                index: 2,
                available: 1
            })
        );
    }

    #[test]
    fn counts_source_arity_from_the_max_of_implicit_and_explicit() {
        // Source uses one implicit and an explicit %2$s ⇒ arity 2.
        assert_eq!(validate("%s then %2$s", "%2$s потом %1$s"), Ok(()));
    }

    #[test]
    fn a_percent_free_pair_is_always_valid() {
        assert_eq!(validate("Wrench", "Гаечный ключ"), Ok(()));
    }

    #[test]
    fn double_percent_is_not_counted_as_an_argument() {
        assert_eq!(validate("100%% sure", "100%% точно"), Ok(()));
    }

    #[test]
    fn a_malformed_source_is_an_error_not_a_free_pass() {
        // A mod shipping %d is that mod's bug, but it must not silently
        // permit anything in the translation.
        assert!(validate("%d items", "%s предметов").is_err());
    }

    #[test]
    fn digits_without_a_dollar_are_not_an_index() {
        // "%2s" is not a positional reference — the regex only treats digits
        // as an index when followed by '$'. Here '2' is the specifier letter
        // position, so this is an unsupported specifier, not index 2.
        assert!(matches!(
            validate("%s", "%2s"),
            Err(FormatError::UnsupportedSpecifier { .. })
        ));
    }

    #[test]
    fn rejects_explicit_index_zero_in_the_translation() {
        // Minecraft's indices are 1-based (`%N$s` reads `args[N-1]`); index 0
        // parses cleanly under the grammar regex (group 1 is just the digits
        // "0") but would address `args[-1]` at substitution time. `arity()`
        // only tracks the maximum explicit index, so without an explicit
        // check a stray "%0$s" would satisfy ANY non-negative `available` —
        // exactly the silent-pass this module exists to prevent.
        assert_eq!(
            validate("%s", "%0$s"),
            Err(FormatError::IndexOutOfRange {
                index: 0,
                available: 1
            })
        );
    }

    #[test]
    fn rejects_explicit_index_zero_in_the_source_too() {
        // The source is not exempt: a mod shipping "%0$s" is as broken as one
        // shipping "%d", and must surface as an error rather than silently
        // capping arity at whatever its other, valid references imply.
        assert_eq!(
            validate("%0$s", "%s"),
            Err(FormatError::IndexOutOfRange {
                index: 0,
                available: 0
            })
        );
    }
}
