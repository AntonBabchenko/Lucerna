//! An instance's `options.txt` as settings sync reads and writes it (spec
//! §C4): only a file the running game wrote (its `version` line is this
//! game's DataVersion), read and edited through the catalog of THAT version,
//! line by line in place. Lucerna never creates the file: pack mods apply
//! their defaults only while it is absent.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use fastnbt::Value;
use serde::{Deserialize, Serialize};

use super::catalog::{entry_for_name, read_value, write_value, Neutral};
use super::keys::Modifier;
use crate::error::{Error, Result};
use crate::options_file::ParsedOptions;
use crate::settings_sync::failure::{self, At};
use crate::settings_sync::fsio;

/// Neutral key → value: what the shared copy keeps and a baseline records.
pub(crate) type Values = BTreeMap<String, Neutral>;

/// Why an instance's options.txt is neither read nor written yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum WaitReason {
    /// No options.txt yet: the game writes it on its first launch.
    FirstLaunch,
    /// The file is not this game's own (no `version` line, or another
    /// version's): the game rewrites it the next time it saves its settings.
    GameSave,
}

pub(crate) enum OptionsRead {
    Waiting(WaitReason),
    File(OptionsFile),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct OptionsFile {
    pub parsed: ParsedOptions,
    pub dv: i32,
    /// Every catalogued value the file holds.
    pub values: Values,
}

/// The instance's file at `path` for a game of DataVersion `dv`. A file
/// that is not UTF-8 is damaged, never "empty".
pub(crate) fn read(at: At, path: &Path, dv: i32) -> Result<OptionsRead> {
    let Some(bytes) = fsio::read_if_present(at, path)? else {
        return Ok(OptionsRead::Waiting(WaitReason::FirstLaunch));
    };
    let text = String::from_utf8(bytes)
        .map_err(|e| failure::damaged(at, path, format!("not UTF-8: {e}")))?;
    Ok(match from_text(&text, dv) {
        Some(file) => OptionsRead::File(file),
        None => OptionsRead::Waiting(WaitReason::GameSave),
    })
}

/// `text` as this game's own file, or `None` when its `version` line is not
/// `dv`: the game would run every DataFix over values written for it and
/// reset the file to its defaults.
pub(crate) fn from_text(text: &str, dv: i32) -> Option<OptionsFile> {
    let parsed = ParsedOptions::parse(text);
    (parsed.version() == Some(dv)).then(|| OptionsFile {
        values: values_of(&parsed, dv),
        parsed,
        dv,
    })
}

/// Every catalogued key `parsed` holds, as the game reads it: the last line
/// of a repeated key wins, and a value its codec cannot read is no value
/// (the game falls back to its default, not to an earlier line).
pub(crate) fn values_of(parsed: &ParsedOptions, dv: i32) -> Values {
    let mut out = Values::new();
    for (name, raw) in parsed.pairs() {
        let Some(entry) = entry_for_name(dv, name) else {
            continue;
        };
        match read_value(&entry, dv, raw) {
            Some(v) => out.insert(entry.neutral.to_string(), v),
            None => out.remove(entry.neutral),
        };
    }
    out
}

/// `file` with every catalogued line set to its shared value, in this
/// game's codec. A value this game cannot take (a key token its table
/// lacks, a graphics level it has no name for) leaves the line as it is,
/// and a key the file has no line for is not added: a mod's binding
/// reaches only an instance where that mod saved it.
pub(crate) fn render(file: &OptionsFile, shared: &Values, has_modifiers: bool) -> OptionsFile {
    let mut parsed = file.parsed.clone();
    let names: std::collections::BTreeSet<&str> = file.parsed.pairs().map(|(k, _)| k).collect();
    for name in names {
        let Some(entry) = entry_for_name(file.dv, name) else {
            continue;
        };
        let Some(value) = shared.get(entry.neutral) else {
            continue;
        };
        if file.values.get(entry.neutral) == Some(value) {
            continue;
        }
        if let Some(text) = write_value(&entry, file.dv, value, has_modifiers) {
            parsed.set_existing(name, text);
        }
    }
    OptionsFile {
        values: values_of(&parsed, file.dv),
        parsed,
        dv: file.dv,
    }
}

/// The projection a baseline stores (NBT, as every kind's), one typed tag
/// per value: a byte flag, a long, a string, a `{float}` or a `{token,
/// modifier}` compound.
pub(crate) fn to_value(values: &Values) -> Value {
    Value::Compound(
        values
            .iter()
            .map(|(k, v)| (k.clone(), neutral_value(v)))
            .collect(),
    )
}

fn neutral_value(v: &Neutral) -> Value {
    match v {
        Neutral::Bool(b) => Value::Byte(i8::from(*b)),
        Neutral::Int(i) => Value::Long(*i),
        Neutral::Text(s) => Value::String(s.clone()),
        Neutral::Float(s) => Value::Compound(HashMap::from([(
            "float".to_string(),
            Value::String(s.clone()),
        )])),
        Neutral::Key { token, modifier } => {
            let mut m = HashMap::from([("token".to_string(), Value::String(token.clone()))]);
            if let Some(md) = modifier {
                m.insert("modifier".to_string(), Value::String(md.as_str().into()));
            }
            Value::Compound(m)
        }
    }
}

pub(crate) fn from_value(value: &Value) -> std::result::Result<Values, String> {
    let Value::Compound(map) = value else {
        return Err("the options are not a compound".into());
    };
    map.iter()
        .map(|(k, v)| {
            Ok((
                k.clone(),
                value_neutral(v).ok_or(format!("{k}: an unknown value"))?,
            ))
        })
        .collect()
}

fn value_neutral(v: &Value) -> Option<Neutral> {
    Some(match v {
        Value::Byte(b) => Neutral::Bool(*b != 0),
        Value::Long(i) => Neutral::Int(*i),
        Value::String(s) => Neutral::Text(s.clone()),
        Value::Compound(m) => match (m.get("float"), m.get("token")) {
            (Some(Value::String(f)), None) if m.len() == 1 => Neutral::Float(f.clone()),
            (None, Some(Value::String(token))) => {
                let modifier = match m.get("modifier") {
                    None => None,
                    Some(Value::String(name)) => Some(Modifier::from_name(name)?),
                    Some(_) => return None,
                };
                Neutral::Key {
                    token: token.clone(),
                    modifier,
                }
            }
            _ => return None,
        },
        _ => return None,
    })
}

/// The shared copy, `options.json`.
#[derive(Serialize, Deserialize)]
struct Stored {
    values: Values,
}

pub(crate) fn store_bytes(values: &Values) -> Result<Vec<u8>> {
    let stored = Stored {
        values: values.clone(),
    };
    let mut bytes = serde_json::to_vec_pretty(&stored)
        .map_err(|e| Error::io("options.json", format!("serialize: {e}")))?;
    bytes.push(b'\n');
    Ok(bytes)
}

pub(crate) fn parse_store(bytes: &[u8]) -> std::result::Result<Values, String> {
    serde_json::from_slice::<Stored>(bytes)
        .map(|s| s.values)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings_sync::failure::{classify, FileReason, SyncFile};
    use crate::settings_sync::Kind;

    /// Until `Kind::Options` exists (Task 6) the file is addressed as a game file.
    fn at() -> At {
        At::game(Kind::Servers)
    }

    fn key(token: &str) -> Neutral {
        Neutral::Key {
            token: token.into(),
            modifier: None,
        }
    }

    fn text(s: &str) -> Neutral {
        Neutral::Text(s.into())
    }

    fn file(src: &str, dv: i32) -> OptionsFile {
        from_text(src, dv).expect("the game's own file")
    }

    fn shared(pairs: &[(&str, Neutral)]) -> Values {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    const V1_16_5: &str = "version:2586\nautoJump:false\ngraphicsMode:1\nao:2\nrenderClouds:true\nmainHand:right\nlang:en_us\nlastServer:play.example.org\nresourcePacks:[]\nkey_key.swapOffhand:key.keyboard.f\nkey_key.examplemod.open:key.keyboard.o\nsoundCategory_master:1.0\nmodelPart_cape:true\n";

    #[test]
    fn only_a_file_this_game_wrote_is_read() {
        let td = tempfile::tempdir().unwrap();
        let p = td.path().join("options.txt");
        assert!(matches!(
            read(at(), &p, 2586).unwrap(),
            OptionsRead::Waiting(WaitReason::FirstLaunch)
        ));
        for (src, why) in [
            ("lang:ru_ru\nfov:0.5\n", "a pack's file without a version"),
            ("version:2975\nlang:ru_ru\n", "another version's file"),
            (
                "version:abc\nlang:ru_ru\n",
                "a version that is not a number",
            ),
        ] {
            std::fs::write(&p, src).unwrap();
            assert!(
                matches!(
                    read(at(), &p, 2586).unwrap(),
                    OptionsRead::Waiting(WaitReason::GameSave)
                ),
                "{why}"
            );
        }
        std::fs::write(&p, V1_16_5).unwrap();
        assert!(matches!(
            read(at(), &p, 2586).unwrap(),
            OptionsRead::File(_)
        ));
    }

    #[test]
    fn a_file_that_is_not_utf8_is_damaged_not_empty() {
        let td = tempfile::tempdir().unwrap();
        let p = td.path().join("options.txt");
        std::fs::write(&p, b"version:2586\nlang:\xff\xfe\n").unwrap();
        let err = read(at(), &p, 2586).err().expect("an error");
        assert_eq!(classify(&err), (Some(SyncFile::Game), FileReason::Damaged));
    }

    #[test]
    fn values_are_read_through_the_catalog_of_the_files_version() {
        let f = file(V1_16_5, 2586);
        assert_eq!(
            f.values,
            shared(&[
                ("autoJump", Neutral::Bool(false)),
                ("graphics", text("fancy")),
                ("ao", Neutral::Bool(true)),
                ("renderClouds", text("true")),
                ("mainHand", text("right")),
                ("lang", text("en_us")),
                ("key_key.swapOffhand", key("key.keyboard.f")),
                ("key_key.examplemod.open", key("key.keyboard.o")),
                ("soundCategory_master", Neutral::Float("1.0".into())),
                ("modelPart_cape", Neutral::Bool(true)),
            ]),
            "lastServer, resourcePacks and version are never synced"
        );
    }

    #[test]
    fn the_last_line_of_a_key_is_the_one_the_game_reads() {
        let f = file("version:2586\nfov:0.1\nfov:0.2\n", 2586);
        assert_eq!(f.values.get("fov"), Some(&Neutral::Float("0.2".into())));
        let f = file("version:2586\nfov:0.1\nfov:wide\n", 2586);
        assert_eq!(f.values.get("fov"), None, "the game reads no 0.1 either");
    }

    #[test]
    fn render_edits_the_catalogued_lines_in_place_and_adds_none() {
        let f = file(V1_16_5, 2586);
        let out = render(
            &f,
            &shared(&[
                ("graphics", text("fabulous")),
                ("ao", Neutral::Bool(false)),
                ("mainHand", text("left")),
                ("key_key.swapOffhand", key("key.keyboard.g")),
                ("key_key.othermod.map", key("key.keyboard.m")),
                ("fov", Neutral::Float("0.5".into())),
            ]),
            false,
        );
        assert_eq!(
            out.parsed.render(),
            V1_16_5
                .replace("graphicsMode:1", "graphicsMode:2")
                .replace("ao:2", "ao:0")
                .replace("mainHand:right", "mainHand:left")
                .replace("swapOffhand:key.keyboard.f", "swapOffhand:key.keyboard.g"),
            "no fov line, no binding of a mod this instance lacks"
        );
        assert_eq!(out.values.get("graphics"), Some(&text("fabulous")));
    }

    #[test]
    fn an_unchanged_file_renders_byte_for_byte() {
        let src = "\u{FEFF}version:2975\r\nfancyGraphics:true\r\nmainHand:right\r\nkey_key.jump:key.keyboard.space\r\n";
        let f = ParsedOptions::parse(src);
        let f = OptionsFile {
            values: values_of(&f, 2975),
            parsed: f,
            dv: 2975,
        };
        assert_eq!(f.values.get("mainHand"), Some(&text("right")));
        let own = f.values.clone();
        assert_eq!(render(&f, &own, false).parsed.render(), src);
    }

    #[test]
    fn strings_take_the_quoting_of_this_games_era() {
        let f = file("version:3105\nmainHand:\"right\"\nlang:en_us\n", 3105);
        let out = render(
            &f,
            &shared(&[("mainHand", text("left")), ("lang", text("ru_ru"))]),
            false,
        );
        assert_eq!(
            out.parsed.render(),
            "version:3105\nmainHand:\"left\"\nlang:ru_ru\n"
        );
    }

    #[test]
    fn a_binding_this_game_cannot_read_keeps_the_instances_own() {
        let glfw = file("version:4903\nkey_key.jump:key.keyboard.space\n", 4903);
        let out = render(
            &glfw,
            &shared(&[("key_key.jump", key("key.keyboard.media.play"))]),
            false,
        );
        assert_eq!(
            out.parsed.render(),
            "version:4903\nkey_key.jump:key.keyboard.space\n"
        );
        let sdl3 = file("version:5023\nkey_key.jump:key.keyboard.space\n", 5023);
        let out = render(
            &sdl3,
            &shared(&[("key_key.jump", key("key.keyboard.f25"))]),
            false,
        );
        assert_eq!(
            out.parsed.render(),
            "version:5023\nkey_key.jump:key.keyboard.space\n"
        );
        let out = render(
            &sdl3,
            &shared(&[("key_key.jump", key("key.keyboard.j"))]),
            false,
        );
        assert_eq!(
            out.parsed.render(),
            "version:5023\nkey_key.jump:key.keyboard.j\n"
        );
    }

    #[test]
    fn a_modifier_reaches_only_a_loader_that_reads_it() {
        let f = file(
            "version:3465\nkey_key.examplemod.open:key.keyboard.o\n",
            3465,
        );
        let shift = shared(&[(
            "key_key.examplemod.open",
            Neutral::Key {
                token: "key.keyboard.o".into(),
                modifier: Some(Modifier::Shift),
            },
        )]);
        assert_eq!(
            render(&f, &shift, true).parsed.render(),
            "version:3465\nkey_key.examplemod.open:key.keyboard.o:SHIFT\n"
        );
        assert_eq!(
            render(&f, &shift, false).parsed.render(),
            "version:3465\nkey_key.examplemod.open:key.keyboard.o\n"
        );
    }

    #[test]
    fn the_projection_and_the_store_round_trip() {
        let v = shared(&[
            ("autoJump", Neutral::Bool(true)),
            ("guiScale", Neutral::Int(3)),
            ("fov", Neutral::Float("0.25".into())),
            ("lang", text("ru_ru")),
            ("key_key.jump", key("key.keyboard.space")),
            (
                "key_key.examplemod.open",
                Neutral::Key {
                    token: "key.keyboard.o".into(),
                    modifier: Some(Modifier::Alt),
                },
            ),
        ]);
        assert_eq!(from_value(&to_value(&v)), Ok(v.clone()));
        assert_ne!(to_value(&v), to_value(&Values::new()));
        let bytes = store_bytes(&v).unwrap();
        assert_eq!(parse_store(&bytes), Ok(v));
        assert!(parse_store(b"{\"values\":{\"fov\":{\"wide\":1}}}").is_err());
        assert!(parse_store(b"not json").is_err());
    }
}
