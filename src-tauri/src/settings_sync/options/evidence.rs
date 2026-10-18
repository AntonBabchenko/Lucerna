//! The catalog checked against what the games themselves write.
//! `presence.json` lists, per `options.txt` key, the releases that write it
//! (game-written files of every release 1.13 → 26.2, and the 26.3 client
//! jar). An entry claiming a release that does not write its key, or a
//! release writing a catalogued key no entry covers, is a catalog bug.

use std::collections::BTreeMap;

use serde::Deserialize;

use super::catalog::{
    entry_for_name, entry_for_neutral, in_range, read_value, write_value, Neutral, FIXED,
    MODEL_PARTS, SOUND,
};

#[derive(Deserialize)]
struct Presence {
    releases: BTreeMap<String, i32>,
    keys: BTreeMap<String, Vec<(i32, Option<i32>)>>,
}

fn presence() -> Presence {
    serde_json::from_str(include_str!("presence.json")).expect("presence.json parses")
}

impl Presence {
    fn writes(&self, dv: i32, key: &str) -> bool {
        self.keys
            .get(key)
            .is_some_and(|runs| runs.iter().any(|(from, until)| in_range(dv, *from, *until)))
    }

    fn data_versions(&self) -> Vec<i32> {
        self.releases.values().copied().collect()
    }
}

#[test]
fn the_table_covers_every_release_from_1_13_to_26_3() {
    let p = presence();
    assert_eq!(p.releases.len(), 51);
    assert_eq!(p.releases.get("1.13"), Some(&1519));
    assert_eq!(p.releases.get("1.13.2"), Some(&1631));
    assert_eq!(p.releases.get("26.3"), Some(&5023));
}

#[test]
fn every_entry_names_a_key_its_releases_write() {
    let p = presence();
    for e in FIXED {
        for dv in p.data_versions() {
            if in_range(dv, e.from, e.until) {
                assert!(p.writes(dv, e.name), "{} claimed at {dv}", e.name);
            }
        }
    }
}

#[test]
fn no_release_writes_a_catalogued_key_that_no_entry_covers() {
    let p = presence();
    for e in FIXED {
        for dv in p.data_versions() {
            if p.writes(dv, e.name) {
                assert!(
                    entry_for_name(dv, e.name).is_some(),
                    "{} written at {dv} but not synced",
                    e.name
                );
            }
        }
    }
}

#[test]
fn the_sound_and_skin_families_match_the_releases() {
    let p = presence();
    for dv in p.data_versions() {
        for (category, from) in SOUND {
            let key = format!("soundCategory_{category}");
            assert_eq!(p.writes(dv, &key), dv >= *from, "{key} at {dv}");
        }
        let written = p
            .keys
            .keys()
            .filter(|k| k.starts_with("soundCategory_") && p.writes(dv, k));
        for key in written {
            assert!(entry_for_name(dv, key).is_some(), "{key} at {dv}");
        }
        for part in MODEL_PARTS {
            assert!(p.writes(dv, &format!("modelPart_{part}")), "{part} at {dv}");
        }
    }
}

/// The keys of the settings screens the spec syncs (controls, mouse, chat,
/// accessibility, font, sound, skin), as the 26.2 and 26.3 screens show them
/// (read from the client jars), plus `lang` and the F3+H tooltips.
#[test]
fn every_option_on_a_synced_screen_is_in_the_catalog() {
    const SYNCED: &[&str] = &[
        // controls
        "autoJump",
        "operatorItemsTab",
        "sprintWindow",
        "toggleAttack",
        "toggleCrouch",
        "toggleSprint",
        "toggleUse",
        // mouse
        "allowCursorChanges",
        "discrete_mouse_scroll",
        "invertXMouse",
        "invertYMouse",
        "mouseSensitivity",
        "mouseWheelSensitivity",
        // chat
        "autoSuggestions",
        "chatColors",
        "chatDelay",
        "chatHeightFocused",
        "chatHeightUnfocused",
        "chatLineSpacing",
        "chatLinks",
        "chatLinksPrompt",
        "chatOpacity",
        "chatScale",
        "chatVisibility",
        "chatWidth",
        "hideMatchedNames",
        "narrator",
        "onlyShowSecureChat",
        "reducedDebugInfo",
        "saveChatDrafts",
        "textBackgroundOpacity",
        // accessibility
        "backgroundForChatOnly",
        "bobView",
        "damageTiltStrength",
        "darkMojangStudiosBackground",
        "darknessEffectScale",
        "fovEffectScale",
        "glintSpeed",
        "glintStrength",
        "hideLightningFlashes",
        "hideSplashTexts",
        "highContrast",
        "highContrastBlockOutline",
        "menuBackgroundBlurriness",
        "narratorHotkey",
        "notificationDisplayTime",
        "panoramaScrollSpeed",
        "rotateWithMinecart",
        "screenEffectScale",
        "showSubtitles",
        // font, sound, skin
        "forceUnicodeFont",
        "japaneseGlyphVariants",
        "directionalAudio",
        "musicFrequency",
        "musicToast",
        "mainHand",
        // video, the non-performance ones
        "ao",
        "attackIndicator",
        "cutoutLeaves",
        "entityShadows",
        "fov",
        "gamma",
        "graphicsPreset",
        "guiScale",
        "improvedTransparency",
        "particles",
        "renderClouds",
        "showAutosaveIndicator",
        "vignette",
        // no screen of its own
        "lang",
        "advancedItemTooltips",
    ];
    for dv in [4903, 5023] {
        for key in SYNCED {
            assert!(entry_for_name(dv, key).is_some(), "{key} at {dv}");
        }
    }
}

/// Performance and hardware settings stay per instance (a heavy pack runs at
/// a lower render distance); the online screen and state keys never sync.
#[test]
fn performance_online_and_state_keys_are_left_alone() {
    const NOT_SYNCED: &[&str] = &[
        "biomeBlendRadius",
        "chunkSectionFadeInTime",
        "cloudRange",
        "enableVsync",
        "entityDistanceScaling",
        "fullscreen",
        "inactivityFpsLimit",
        "maxFps",
        "mipmapLevels",
        "prioritizeChunkUpdates",
        "rawMouseInput",
        "renderDistance",
        "simulationDistance",
        "allowServerListing",
        "realmsNotifications",
        "soundDevice",
        "resourcePacks",
        "lastServer",
        "tutorialStep",
        "onboardAccessibility",
        "joinedFirstServer",
    ];
    for key in NOT_SYNCED {
        assert!(entry_for_name(4903, key).is_none(), "{key}");
    }
}

/// 1.21.11 turned `showNowPlayingToast` into `musicToast`; Mojang's own
/// `OptionsMusicToastFix` maps false → "never" and true → "pause_and_toast".
#[test]
fn the_music_toast_crosses_its_rename_as_mojangs_fix_does() {
    let at = |dv, name: &str, raw: &str| read_value(&entry_for_name(dv, name)?, dv, raw);
    let to = |dv, v: &str| {
        write_value(
            &entry_for_neutral(dv, "musicToast")?,
            dv,
            &Neutral::Text(v.into()),
            false,
        )
    };
    assert_eq!(
        at(4554, "showNowPlayingToast", "true"),
        Some(Neutral::Text("pause_and_toast".into()))
    );
    assert_eq!(
        at(4554, "showNowPlayingToast", "false"),
        Some(Neutral::Text("never".into()))
    );
    assert_eq!(to(4554, "never").as_deref(), Some("false"));
    assert_eq!(to(4554, "pause_and_toast").as_deref(), Some("true"));
    assert_eq!(to(4554, "pause"), None, "1.21.10 has no pause-only toast");
    assert_eq!(to(4671, "pause").as_deref(), Some("\"pause\""));
}
