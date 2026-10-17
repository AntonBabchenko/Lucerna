//! The options catalog (spec §C1): which `options.txt` keys sync, their name
//! and value codec in each release, and the one version-neutral value the
//! shared copy keeps for each. Ranges are DataVersion half-open intervals,
//! the first release that writes the key, from game-written files of every
//! release and Mojang's client jars (2026-10-17; `evidence.rs` checks the
//! table against them). A key outside this catalog is never read into the
//! shared copy and never written.

use serde::{Deserialize, Serialize};

use super::keys::{self, Modifier};

/// Strings are JSON-quoted from 1.19 (verified on game-written files).
pub(crate) const QUOTED_FROM_DV: i32 = 3105;

/// The oldest and the newest release the catalog knows: 1.13 and 26.3. A
/// newer release waits for the catalog to learn it (spec §C3).
pub(crate) const OLDEST_DV: i32 = 1519;
pub(crate) const NEWEST_DV: i32 = 5023;

/// One synced value, the same in every release.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Neutral {
    Bool(bool),
    Int(i64),
    /// As the game wrote it (`0.5`): kept textually, so an unchanged value
    /// is never rewritten in another spelling.
    Float(String),
    Text(String),
    Key {
        token: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        modifier: Option<Modifier>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Codec {
    Bool,
    Int,
    Float,
    /// A string, bare before 1.19 and JSON-quoted from 1.19.
    Str,
    /// A string never quoted (`lang`).
    Bare,
    /// `fancyGraphics`: true / false ↔ the neutral graphics level.
    GraphicsBool,
    /// `graphicsMode`: 0 / 1 / 2 ↔ fast / fancy / fabulous.
    GraphicsInt,
    /// `ao` before 1.19.3: 0 / 1 / 2 ↔ off / on / on.
    AoInt,
    /// `menuBackgroundBlurriness` in 1.20.5–1.20.6: 0–1 ↔ the neutral 0–10.
    Tenths,
    /// `showNowPlayingToast` (1.21.6–1.21.10) ↔ the neutral `musicToast`,
    /// as Mojang's `OptionsMusicToastFix` maps it: false ↔ never, true ↔
    /// pause_and_toast.
    ToastBool,
    Key,
}

/// One key in one range of releases. The names borrow from the caller's
/// key for the prefixed families (a mod's `key_key.<mod>.*` is not static).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Entry<'a> {
    pub neutral: &'a str,
    pub name: &'a str,
    pub from: i32,
    pub until: Option<i32>,
    pub codec: Codec,
}

const fn e(
    neutral: &'static str,
    name: &'static str,
    from: i32,
    until: Option<i32>,
    codec: Codec,
) -> Entry<'static> {
    Entry {
        neutral,
        name,
        from,
        until,
        codec,
    }
}

const ALL: i32 = OLDEST_DV; // 1.13
const V1_14: i32 = 1952;
const V1_15: i32 = 2225;
const V1_16: i32 = 2566;
const V1_16_2: i32 = 2578;
const V1_16_4: i32 = 2584;
const V1_17: i32 = 2724;
const V1_18: i32 = 2860;
const V1_19: i32 = 3105;
const V1_19_3: i32 = 3218;
const V1_19_4: i32 = 3337;
const V1_20_2: i32 = 3578;
const V1_20_3: i32 = 3698;
const V1_20_5: i32 = 3837;
const V1_21: i32 = 3953;
const V1_21_2: i32 = 4080;
const V1_21_6: i32 = 4435;
const V1_21_9: i32 = 4554;
const V1_21_11: i32 = 4671;

use Codec::*;

/// The fixed-name keys. Prefixed families (`soundCategory_*`, `modelPart_*`,
/// `key_key.*`) are matched by [`entry_for_name`].
pub(crate) const FIXED: &[Entry<'static>] = &[
    e("graphics", "fancyGraphics", ALL, Some(V1_16), GraphicsBool),
    e(
        "graphics",
        "graphicsMode",
        V1_16,
        Some(V1_21_11),
        GraphicsInt,
    ),
    e("graphics", "graphicsPreset", V1_21_11, None, Str),
    e("cutoutLeaves", "cutoutLeaves", V1_21_11, None, Bool),
    e("vignette", "vignette", V1_21_11, None, Bool),
    e(
        "improvedTransparency",
        "improvedTransparency",
        V1_21_11,
        None,
        Bool,
    ),
    e("ao", "ao", ALL, Some(V1_19_3), AoInt),
    e("ao", "ao", V1_19_3, None, Bool),
    e("renderClouds", "renderClouds", ALL, None, Str),
    e("mainHand", "mainHand", ALL, None, Str),
    e("attackIndicator", "attackIndicator", ALL, None, Int),
    e("particles", "particles", ALL, None, Int),
    e("chatVisibility", "chatVisibility", ALL, None, Int),
    e("narrator", "narrator", ALL, None, Int),
    e("lang", "lang", ALL, None, Bare),
    e("fov", "fov", ALL, None, Float),
    e("gamma", "gamma", ALL, None, Float),
    e("mouseSensitivity", "mouseSensitivity", ALL, None, Float),
    e("chatOpacity", "chatOpacity", ALL, None, Float),
    e("chatScale", "chatScale", ALL, None, Float),
    e("chatWidth", "chatWidth", ALL, None, Float),
    e("chatHeightFocused", "chatHeightFocused", ALL, None, Float),
    e(
        "chatHeightUnfocused",
        "chatHeightUnfocused",
        ALL,
        None,
        Float,
    ),
    e(
        "mouseWheelSensitivity",
        "mouseWheelSensitivity",
        ALL,
        None,
        Float,
    ),
    e("invertYMouse", "invertYMouse", ALL, None, Bool),
    e("invertXMouse", "invertXMouse", V1_21_9, None, Bool),
    e(
        "discrete_mouse_scroll",
        "discrete_mouse_scroll",
        V1_14,
        None,
        Bool,
    ),
    e(
        "allowCursorChanges",
        "allowCursorChanges",
        V1_21_9,
        None,
        Bool,
    ),
    e("sprintWindow", "sprintWindow", V1_21_9, None, Int),
    e("operatorItemsTab", "operatorItemsTab", V1_19_3, None, Bool),
    e("saveChatDrafts", "saveChatDrafts", V1_21_9, None, Bool),
    e("narratorHotkey", "narratorHotkey", V1_20_2, None, Bool),
    e("hideSplashTexts", "hideSplashTexts", V1_20_3, None, Bool),
    e(
        "rotateWithMinecart",
        "rotateWithMinecart",
        V1_21_2,
        None,
        Bool,
    ),
    e(
        "showAutosaveIndicator",
        "showAutosaveIndicator",
        V1_18,
        None,
        Bool,
    ),
    e("musicFrequency", "musicFrequency", V1_21_6, None, Str),
    e("autoJump", "autoJump", ALL, None, Bool),
    e("bobView", "bobView", ALL, None, Bool),
    e("entityShadows", "entityShadows", ALL, None, Bool),
    e("reducedDebugInfo", "reducedDebugInfo", ALL, None, Bool),
    e("autoSuggestions", "autoSuggestions", ALL, None, Bool),
    e("chatColors", "chatColors", ALL, None, Bool),
    e("chatLinks", "chatLinks", ALL, None, Bool),
    e("chatLinksPrompt", "chatLinksPrompt", ALL, None, Bool),
    e("showSubtitles", "showSubtitles", ALL, None, Bool),
    e("forceUnicodeFont", "forceUnicodeFont", ALL, None, Bool),
    e(
        "advancedItemTooltips",
        "advancedItemTooltips",
        ALL,
        None,
        Bool,
    ),
    e("guiScale", "guiScale", ALL, None, Int),
    e("toggleCrouch", "toggleCrouch", V1_15, None, Bool),
    e("toggleSprint", "toggleSprint", V1_15, None, Bool),
    e("toggleAttack", "toggleAttack", V1_21_9, None, Bool),
    e("toggleUse", "toggleUse", V1_21_9, None, Bool),
    e(
        "textBackgroundOpacity",
        "textBackgroundOpacity",
        V1_14,
        None,
        Float,
    ),
    e(
        "backgroundForChatOnly",
        "backgroundForChatOnly",
        V1_14,
        None,
        Bool,
    ),
    e("chatLineSpacing", "chatLineSpacing", V1_16, None, Float),
    e("chatDelay", "chatDelay", V1_16, None, Float),
    e("fovEffectScale", "fovEffectScale", V1_16_2, None, Float),
    e(
        "screenEffectScale",
        "screenEffectScale",
        V1_16_2,
        None,
        Float,
    ),
    e("hideMatchedNames", "hideMatchedNames", V1_16_4, None, Bool),
    e(
        "darkMojangStudiosBackground",
        "darkMojangStudiosBackground",
        V1_17,
        None,
        Bool,
    ),
    e(
        "hideLightningFlashes",
        "hideLightningFlashes",
        V1_18,
        None,
        Bool,
    ),
    e("directionalAudio", "directionalAudio", V1_19, None, Bool),
    e(
        "onlyShowSecureChat",
        "onlyShowSecureChat",
        V1_19,
        None,
        Bool,
    ),
    e(
        "darknessEffectScale",
        "darknessEffectScale",
        V1_19,
        None,
        Float,
    ),
    e(
        "panoramaScrollSpeed",
        "panoramaScrollSpeed",
        V1_19_3,
        None,
        Float,
    ),
    e("glintSpeed", "glintSpeed", V1_19_4, None, Float),
    e("glintStrength", "glintStrength", V1_19_4, None, Float),
    e(
        "damageTiltStrength",
        "damageTiltStrength",
        V1_19_4,
        None,
        Float,
    ),
    e(
        "notificationDisplayTime",
        "notificationDisplayTime",
        V1_19_4,
        None,
        Float,
    ),
    e("highContrast", "highContrast", V1_19_4, None, Bool),
    e(
        "japaneseGlyphVariants",
        "japaneseGlyphVariants",
        V1_20_5,
        None,
        Bool,
    ),
    e(
        "menuBackgroundBlurriness",
        "menuBackgroundBlurriness",
        V1_20_5,
        Some(V1_21),
        Tenths,
    ),
    e(
        "menuBackgroundBlurriness",
        "menuBackgroundBlurriness",
        V1_21,
        None,
        Int,
    ),
    e(
        "highContrastBlockOutline",
        "highContrastBlockOutline",
        V1_21_2,
        None,
        Bool,
    ),
    e(
        "musicToast",
        "showNowPlayingToast",
        V1_21_6,
        Some(V1_21_11),
        ToastBool,
    ),
    e("musicToast", "musicToast", V1_21_11, None, Str),
    // `swapHands` became `swapOffhand` in 1.16; one neutral key for both.
    e(
        "key_key.swapOffhand",
        "key_key.swapHands",
        ALL,
        Some(V1_16),
        Key,
    ),
    e(
        "key_key.swapOffhand",
        "key_key.swapOffhand",
        V1_16,
        None,
        Key,
    ),
];

pub(super) const SOUND: &[(&str, i32)] = &[
    ("master", ALL),
    ("music", ALL),
    ("record", ALL),
    ("weather", ALL),
    ("block", ALL),
    ("hostile", ALL),
    ("neutral", ALL),
    ("player", ALL),
    ("ambient", ALL),
    ("voice", ALL),
    ("ui", V1_21_6),
];

pub(super) const MODEL_PARTS: &[&str] = &[
    "cape",
    "jacket",
    "left_sleeve",
    "right_sleeve",
    "left_pants_leg",
    "right_pants_leg",
    "hat",
];

pub(super) fn in_range(dv: i32, from: i32, until: Option<i32>) -> bool {
    dv >= from && until.is_none_or(|u| dv < u)
}

/// A prefixed family's entry (`soundCategory_*`, `modelPart_*`, `key_key.*`),
/// whose neutral key IS its name. A name a fixed entry owns in any release
/// is never one of these (a stale `key_key.swapHands` stays the fixed one's).
fn prefixed(dv: i32, name: &str) -> Option<Entry<'_>> {
    if FIXED.iter().any(|e| e.name == name || e.neutral == name) {
        return None;
    }
    let entry = |from, codec| Entry {
        neutral: name,
        name,
        from,
        until: None,
        codec,
    };
    if let Some(category) = name.strip_prefix("soundCategory_") {
        let (_, from) = SOUND.iter().find(|(c, _)| *c == category)?;
        return (dv >= *from).then(|| entry(*from, Float));
    }
    if let Some(part) = name.strip_prefix("modelPart_") {
        return MODEL_PARTS.contains(&part).then(|| entry(ALL, Bool));
    }
    let action = name.strip_prefix("key_key.")?;
    (!action.is_empty()).then(|| entry(ALL, Key))
}

/// The catalog entry for a key as `options.txt` of DataVersion `dv` names it.
pub(crate) fn entry_for_name(dv: i32, name: &str) -> Option<Entry<'_>> {
    if let Some(e) = FIXED
        .iter()
        .find(|e| e.name == name && in_range(dv, e.from, e.until))
    {
        return Some(*e);
    }
    prefixed(dv, name)
}

/// The entry that writes neutral key `neutral` in a game of DataVersion `dv`.
pub(crate) fn entry_for_neutral(dv: i32, neutral: &str) -> Option<Entry<'_>> {
    if FIXED.iter().any(|e| e.neutral == neutral) {
        return FIXED
            .iter()
            .find(|e| e.neutral == neutral && in_range(dv, e.from, e.until))
            .copied();
    }
    prefixed(dv, neutral)
}

/// A string a line can hold: one line, nothing that would end it.
fn one_line(s: &str) -> bool {
    !s.contains(['\n', '\r'])
}

fn text(s: &str) -> Option<Neutral> {
    Some(Neutral::Text(s.to_string()))
}

/// A stored value read through `entry`'s codec; `None` when the value is not
/// one this codec reads (it is then left out of the shared copy).
pub(crate) fn read_value(entry: &Entry, dv: i32, raw: &str) -> Option<Neutral> {
    let t = raw.trim();
    match entry.codec {
        Bool => match t {
            "true" => Some(Neutral::Bool(true)),
            "false" => Some(Neutral::Bool(false)),
            _ => None,
        },
        Int => t.parse::<i64>().ok().map(Neutral::Int),
        Float => t
            .parse::<f64>()
            .ok()
            .filter(|f| f.is_finite())
            .map(|_| Neutral::Float(t.to_string())),
        Str if dv >= QUOTED_FROM_DV => serde_json::from_str::<String>(t)
            .ok()
            .filter(|s| one_line(s))
            .map(Neutral::Text),
        Str | Bare => Some(Neutral::Text(raw.to_string())),
        GraphicsBool => match t {
            "true" => text("fancy"),
            "false" => text("fast"),
            _ => None,
        },
        GraphicsInt => match t {
            "0" => text("fast"),
            "1" => text("fancy"),
            "2" => text("fabulous"),
            _ => None,
        },
        AoInt => match t {
            "0" => Some(Neutral::Bool(false)),
            "1" | "2" => Some(Neutral::Bool(true)),
            _ => None,
        },
        ToastBool => match t {
            "true" => text("pause_and_toast"),
            "false" => text("never"),
            _ => None,
        },
        Tenths => t
            .parse::<f64>()
            .ok()
            .filter(|f| f.is_finite())
            .map(|f| Neutral::Int((f * 10.0).round() as i64)),
        Key => keys::split(t)
            .filter(|(token, _)| keys::readable(keys::family(dv), token))
            .map(|(token, modifier)| Neutral::Key { token, modifier }),
    }
}

/// The stored value of `value` in a game of DataVersion `dv`, or `None` when
/// that game cannot represent it (the instance then keeps its own).
pub(crate) fn write_value(
    entry: &Entry,
    dv: i32,
    value: &Neutral,
    loader_has_modifiers: bool,
) -> Option<String> {
    match (entry.codec, value) {
        (Bool, Neutral::Bool(b)) => Some(b.to_string()),
        (Int, Neutral::Int(i)) => Some(i.to_string()),
        (Float, Neutral::Float(f)) => Some(f.clone()),
        (Str, Neutral::Text(s)) if one_line(s) => Some(if dv >= QUOTED_FROM_DV {
            serde_json::to_string(s).ok()?
        } else {
            s.clone()
        }),
        (Bare, Neutral::Text(s)) if one_line(s) => Some(s.clone()),
        (GraphicsBool, Neutral::Text(s)) => match s.as_str() {
            "fast" => Some("false".into()),
            "fancy" | "fabulous" => Some("true".into()),
            _ => None,
        },
        (GraphicsInt, Neutral::Text(s)) => match s.as_str() {
            "fast" => Some("0".into()),
            "fancy" => Some("1".into()),
            "fabulous" => Some("2".into()),
            _ => None,
        },
        (ToastBool, Neutral::Text(s)) => match s.as_str() {
            "never" => Some("false".into()),
            "pause_and_toast" => Some("true".into()),
            _ => None,
        },
        (AoInt, Neutral::Bool(b)) => Some(if *b { "2" } else { "0" }.into()),
        (Tenths, Neutral::Int(i)) if (0..=10).contains(i) => Some(format!("{}", *i as f64 / 10.0)),
        (Key, Neutral::Key { token, modifier }) => {
            keys::render(keys::family(dv), token, *modifier, loader_has_modifiers)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(dv: i32, name: &str, raw: &str) -> Option<Neutral> {
        read_value(&entry_for_name(dv, name)?, dv, raw)
    }

    fn write(dv: i32, neutral: &str, v: &Neutral) -> Option<String> {
        write_value(&entry_for_neutral(dv, neutral)?, dv, v, false)
    }

    fn text(s: &str) -> Neutral {
        Neutral::Text(s.into())
    }

    #[test]
    fn graphics_travel_through_three_names() {
        assert_eq!(read(1631, "fancyGraphics", "true"), Some(text("fancy")));
        assert_eq!(read(2586, "graphicsMode", "2"), Some(text("fabulous")));
        assert_eq!(
            read(4671, "graphicsPreset", "\"custom\""),
            Some(text("custom"))
        );
        assert_eq!(
            write(1631, "graphics", &text("fabulous")).as_deref(),
            Some("true")
        );
        assert_eq!(
            write(1631, "graphics", &text("fast")).as_deref(),
            Some("false")
        );
        assert_eq!(
            write(3465, "graphics", &text("fabulous")).as_deref(),
            Some("2")
        );
        assert_eq!(
            write(4671, "graphics", &text("fancy")).as_deref(),
            Some("\"fancy\"")
        );
        assert_eq!(
            write(3465, "graphics", &text("custom")),
            None,
            "no custom before 1.21.11"
        );
    }

    #[test]
    fn strings_are_quoted_from_1_19_and_lang_never() {
        assert_eq!(read(2975, "mainHand", "left"), Some(text("left")));
        assert_eq!(read(3105, "mainHand", "\"left\""), Some(text("left")));
        assert_eq!(
            write(2975, "mainHand", &text("left")).as_deref(),
            Some("left")
        );
        assert_eq!(
            write(3105, "mainHand", &text("left")).as_deref(),
            Some("\"left\"")
        );
        assert_eq!(
            write(3105, "lang", &text("ru_ru")).as_deref(),
            Some("ru_ru")
        );
        assert_eq!(read(3105, "renderClouds", "\"true\""), Some(text("true")));
    }

    #[test]
    fn ao_is_an_int_before_1_19_3_and_a_bool_after() {
        assert_eq!(read(3105, "ao", "1"), Some(Neutral::Bool(true)));
        assert_eq!(read(3105, "ao", "0"), Some(Neutral::Bool(false)));
        assert_eq!(read(3218, "ao", "true"), Some(Neutral::Bool(true)));
        assert_eq!(
            write(3105, "ao", &Neutral::Bool(true)).as_deref(),
            Some("2")
        );
        assert_eq!(
            write(3218, "ao", &Neutral::Bool(false)).as_deref(),
            Some("false")
        );
    }

    #[test]
    fn menu_blurriness_changes_scale_in_1_21() {
        assert_eq!(
            read(3837, "menuBackgroundBlurriness", "0.5"),
            Some(Neutral::Int(5))
        );
        assert_eq!(
            read(3953, "menuBackgroundBlurriness", "5"),
            Some(Neutral::Int(5))
        );
        assert_eq!(
            write(3837, "menuBackgroundBlurriness", &Neutral::Int(5)).as_deref(),
            Some("0.5")
        );
        assert_eq!(
            write(4189, "menuBackgroundBlurriness", &Neutral::Int(5)).as_deref(),
            Some("5")
        );
    }

    #[test]
    fn a_key_outside_its_releases_or_the_catalog_is_not_synced() {
        assert_eq!(entry_for_name(3465, "toggleAttack"), None, "1.21.9+");
        assert_eq!(
            entry_for_name(4554, "toggleAttack").map(|e| e.neutral),
            Some("toggleAttack")
        );
        for never in [
            "renderDistance",
            "resourcePacks",
            "lastServer",
            "version",
            "fullscreen",
            "key_key",
        ] {
            assert_eq!(entry_for_name(4189, never), None, "{never}");
        }
        assert_eq!(
            entry_for_name(4189, "soundCategory_ui"),
            None,
            "ui is 1.21.6+"
        );
        assert!(entry_for_name(4435, "soundCategory_ui").is_some());
        assert!(entry_for_name(1519, "modelPart_hat").is_some());
        assert_eq!(entry_for_name(1519, "modelPart_tail"), None);
    }

    #[test]
    fn swap_hands_is_one_binding_under_two_names() {
        let old = entry_for_name(1631, "key_key.swapHands").unwrap();
        let new = entry_for_name(2586, "key_key.swapOffhand").unwrap();
        assert_eq!(old.neutral, new.neutral);
        assert_eq!(
            entry_for_neutral(1631, "key_key.swapOffhand").map(|e| e.name),
            Some("key_key.swapHands")
        );
        assert_eq!(
            entry_for_neutral(2586, "key_key.swapOffhand").map(|e| e.name),
            Some("key_key.swapOffhand")
        );
    }

    #[test]
    fn a_binding_crosses_families_only_with_a_token_the_target_reads() {
        let jump = Neutral::Key {
            token: "key.keyboard.space".into(),
            modifier: None,
        };
        assert_eq!(
            read(4903, "key_key.jump", "key.keyboard.space"),
            Some(jump.clone())
        );
        assert_eq!(
            write(5023, "key_key.jump", &jump).as_deref(),
            Some("key.keyboard.space")
        );
        let media = Neutral::Key {
            token: "key.keyboard.media.play".into(),
            modifier: None,
        };
        assert_eq!(write(4903, "key_key.jump", &media), None);
        assert_eq!(
            read(4903, "key_key.create.rotate", "key.keyboard.r").map(|_| ()),
            Some(()),
            "a mod binding is in the catalog"
        );
    }

    #[test]
    fn values_a_codec_does_not_read_are_left_out() {
        assert_eq!(read(3465, "autoJump", "yes"), None);
        assert_eq!(read(3465, "guiScale", "big"), None);
        assert_eq!(read(3465, "fov", "wide"), None);
        assert_eq!(
            read(3105, "mainHand", "left"),
            None,
            "unquoted in the quoted era"
        );
        assert_eq!(
            read(3465, "fov", "0.25"),
            Some(Neutral::Float("0.25".into()))
        );
    }
}
