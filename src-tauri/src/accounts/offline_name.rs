//! Offline account name rule. Minecraft offline play (the integrated
//! server that runs a singleplayer world) only accepts ASCII player names
//! `[A-Za-z0-9_]`, 3–16 chars — the classic Mojang username rule. A name
//! outside it (e.g. Cyrillic) is faithfully passed to Minecraft by the
//! launcher but rejected by the game when entering a world, so the world
//! won't load. We refuse such names at the boundary instead of creating an
//! account that can never play. We never modify the Minecraft client.
//!
//! This rule is mirrored on the frontend in `src/lib/accounts/offline-name.ts`
//! — keep the two in sync.

use serde::{Deserialize, Serialize};
use specta::Type;

/// Min / max offline name length (inclusive). Mojang's classic bounds.
pub const MIN_LEN: usize = 3;
pub const MAX_LEN: usize = 16;

/// Why an offline name was rejected. Serializes snake_case (`too_short`,
/// `too_long`, `invalid_chars`) and is exported to `bindings.ts` via specta,
/// so the frontend reuses the exact same reason values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum OfflineNameRejection {
    TooShort,
    TooLong,
    InvalidChars,
}

/// Validate an offline account name. The caller is responsible for trimming
/// surrounding whitespace first (a leading/trailing space is itself an
/// invalid char and would otherwise report `InvalidChars`). Length is checked
/// before charset so an over-long name reports `TooLong` rather than a charset
/// quibble. Counts Unicode scalar values for length (matches the frontend's
/// code-point count).
pub fn validate(name: &str) -> Result<(), OfflineNameRejection> {
    let len = name.chars().count();
    if len < MIN_LEN {
        return Err(OfflineNameRejection::TooShort);
    }
    if len > MAX_LEN {
        return Err(OfflineNameRejection::TooLong);
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(OfflineNameRejection::InvalidChars);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_plain_latin_name() {
        assert!(validate("Steve").is_ok());
        assert!(validate("Alex_99").is_ok());
        assert!(validate("____").is_ok());
        assert!(validate("123").is_ok());
    }

    #[test]
    fn accepts_length_boundaries_3_and_16() {
        assert!(validate("abc").is_ok()); // exactly MIN_LEN
        assert!(validate("abcdefghijklmnop").is_ok()); // exactly MAX_LEN (16)
    }

    #[test]
    fn rejects_too_short_including_empty() {
        assert_eq!(validate(""), Err(OfflineNameRejection::TooShort));
        assert_eq!(validate("ab"), Err(OfflineNameRejection::TooShort));
    }

    #[test]
    fn rejects_too_long() {
        // 17 ASCII chars.
        assert_eq!(
            validate("abcdefghijklmnopq"),
            Err(OfflineNameRejection::TooLong)
        );
    }

    #[test]
    fn rejects_cyrillic_as_invalid_chars() {
        // 5 code points — passes length, fails charset.
        assert_eq!(validate("Игрок"), Err(OfflineNameRejection::InvalidChars));
    }

    #[test]
    fn rejects_space_hyphen_and_dot() {
        assert_eq!(validate("a b c"), Err(OfflineNameRejection::InvalidChars));
        assert_eq!(validate("ab-cd"), Err(OfflineNameRejection::InvalidChars));
        assert_eq!(validate("ab.cd"), Err(OfflineNameRejection::InvalidChars));
    }

    #[test]
    fn long_cyrillic_reports_too_long_not_charset() {
        // 17 Cyrillic code points: length is checked first.
        assert_eq!(
            validate("абвгдеёжзийклмноп"),
            Err(OfflineNameRejection::TooLong)
        );
    }

    #[test]
    fn rejection_serializes_snake_case() {
        let json = serde_json::to_string(&OfflineNameRejection::InvalidChars).unwrap();
        assert_eq!(json, r#""invalid_chars""#);
    }
}
