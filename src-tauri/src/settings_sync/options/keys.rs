//! Key-binding tokens (spec §C2). Two families, read from Mojang's own client
//! jars (2026-10-17): GLFW names, identical from 1.13 through 26.2, and the
//! SDL3 names of 26.3, which keep every GLFW name except `key.keyboard.f25`
//! and the `scancode.<n>` form and add 127 of their own. A token is written
//! into an instance only if THAT game's table has it: one unknown token
//! aborts the rest of the options loading on 26.2 and older.

use std::collections::HashSet;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

/// The first DataVersion of the SDL3 family (26.3).
pub(crate) const SDL3_FROM_DV: i32 = 5023;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Family {
    Glfw,
    Sdl3,
}

/// A Forge / NeoForge key modifier (`key.keyboard.a:SHIFT`); vanilla has none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum Modifier {
    Shift,
    Control,
    Alt,
}

fn table(json: &str) -> HashSet<String> {
    // The tables are this crate's own files, checked by the tests below.
    serde_json::from_str::<Vec<String>>(json)
        .map(|v| v.into_iter().collect())
        .unwrap_or_default()
}

static GLFW: LazyLock<HashSet<String>> = LazyLock::new(|| table(include_str!("glfw_tokens.json")));
static SDL3: LazyLock<HashSet<String>> = LazyLock::new(|| table(include_str!("sdl3_tokens.json")));

pub(crate) fn family(data_version: i32) -> Family {
    if data_version >= SDL3_FROM_DV {
        Family::Sdl3
    } else {
        Family::Glfw
    }
}

/// Whether `token` is a binding the game of `family` reads.
pub(crate) fn readable(family: Family, token: &str) -> bool {
    let table = match family {
        Family::Glfw => &*GLFW,
        Family::Sdl3 => &*SDL3,
    };
    table.contains(token)
        || (family == Family::Glfw
            && token
                .strip_prefix("scancode.")
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())))
}

impl Modifier {
    fn as_str(self) -> &'static str {
        match self {
            Modifier::Shift => "SHIFT",
            Modifier::Control => "CONTROL",
            Modifier::Alt => "ALT",
        }
    }
}

/// `<token>` or `<token>:<MODIFIER>` as stored; `None` for a value that is
/// neither (an unknown modifier is not guessed at).
pub(crate) fn split(raw: &str) -> Option<(String, Option<Modifier>)> {
    let Some((token, m)) = raw.rsplit_once(':') else {
        return Some((raw.to_string(), None));
    };
    let modifier = match m {
        "SHIFT" => Some(Modifier::Shift),
        "CONTROL" => Some(Modifier::Control),
        "ALT" => Some(Modifier::Alt),
        "NONE" => None,
        _ => return None,
    };
    Some((token.to_string(), modifier))
}

/// The stored value for a binding in a game of `family`, or `None` when that
/// game cannot read the token. The modifier is written only for a loader
/// that has modifiers (Forge, NeoForge).
pub(crate) fn render(
    family: Family,
    token: &str,
    modifier: Option<Modifier>,
    loader_has_modifiers: bool,
) -> Option<String> {
    if !readable(family, token) {
        return None;
    }
    Some(match modifier {
        Some(m) if loader_has_modifiers => format!("{token}:{}", m.as_str()),
        _ => token.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tables_are_the_ones_read_from_the_jars() {
        assert_eq!(GLFW.len(), 129);
        assert_eq!(SDL3.len(), 255);
        assert!(GLFW.contains("key.keyboard.f25") && !SDL3.contains("key.keyboard.f25"));
        assert!(
            SDL3.contains("key.keyboard.keypad.period")
                && !GLFW.contains("key.keyboard.keypad.period")
        );
        assert!(GLFW
            .iter()
            .filter(|t| t.as_str() != "key.keyboard.f25")
            .all(|t| SDL3.contains(t)));
    }

    #[test]
    fn the_family_follows_the_data_version() {
        assert_eq!(family(4903), Family::Glfw); // 26.2
        assert_eq!(family(5023), Family::Sdl3); // 26.3
    }

    #[test]
    fn a_token_is_readable_only_in_a_family_that_has_it() {
        assert!(readable(Family::Glfw, "key.keyboard.space"));
        assert!(readable(Family::Sdl3, "key.keyboard.space"));
        assert!(readable(Family::Glfw, "key.keyboard.f25"));
        assert!(!readable(Family::Sdl3, "key.keyboard.f25"));
        assert!(!readable(Family::Glfw, "key.keyboard.media.play"));
        assert!(
            readable(Family::Glfw, "scancode.42"),
            "GLFW reads scancodes"
        );
        assert!(!readable(Family::Sdl3, "scancode.42"));
        assert!(!readable(Family::Glfw, "scancode.x"));
        assert!(readable(Family::Glfw, "key.mouse.left") && readable(Family::Glfw, "key.mouse.8"));
        assert!(
            !readable(Family::Glfw, "key.mouse.9"),
            "only the table's buttons"
        );
    }

    #[test]
    fn a_modifier_is_split_off_and_kept_only_where_a_loader_reads_it() {
        assert_eq!(
            split("key.keyboard.a"),
            Some(("key.keyboard.a".into(), None))
        );
        assert_eq!(
            split("key.keyboard.a:SHIFT"),
            Some(("key.keyboard.a".into(), Some(Modifier::Shift)))
        );
        assert_eq!(split("key.keyboard.a:HYPER"), None);
        assert_eq!(
            render(
                Family::Glfw,
                "key.keyboard.a",
                Some(Modifier::Control),
                true
            )
            .as_deref(),
            Some("key.keyboard.a:CONTROL")
        );
        assert_eq!(
            render(
                Family::Glfw,
                "key.keyboard.a",
                Some(Modifier::Control),
                false
            )
            .as_deref(),
            Some("key.keyboard.a")
        );
        assert_eq!(render(Family::Sdl3, "key.keyboard.f25", None, false), None);
    }
}
