//! Pack format versions and what a data pack's `pack.mcmeta` declares.
//!
//! Facts only, never a verdict: whether a pack loads, and how the game labels
//! it, depends on the instance's Minecraft version, which can change without a
//! reinstall — `datapacks::verdict` combines the two at listing time.
//!
//! Internal, not IPC: the UI only ever receives the verdict (`PackCompat`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A pack format version as the game compares it: major first, then minor
/// (26.2's `PackFormat.compareTo`). Before data format 82 (1.21.9) there is
/// no minor and every version reads as `N.0`. The derived `Ord` compares the
/// fields in declaration order, which IS that rule — keep `major` first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FormatVersion {
    pub major: u32,
    pub minor: u32,
}

impl FormatVersion {
    #[must_use]
    pub const fn new(major: u32, minor: u32) -> Self {
        Self { major, minor }
    }
}

/// Which half of a client jar's `version.json` `pack_version` to read: the
/// resource-pack format (`l10n::pack_format`) or the data-pack format
/// (`datapacks::compat::game_data_format`). The two diverge from 1.18.2 on,
/// so a reader must never pick one for the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackSide {
    Resource,
    Data,
}

/// One `pack.mcmeta` field as Lucerna read it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fact<T> {
    /// The key is not there.
    Absent,
    Present(T),
    /// The key is there in a shape this build does not read. The game's
    /// Gson/DFU readers accept more than serde (48.5 → 48), so this means
    /// "could not tell" — never "the game rejects it" (§0.5 A16).
    Invalid,
}

/// `min_format`/`max_format` as written: `N` or `[N]` is `Major`, `[N, m]`
/// is `Exact`. Longer lists and non-integers are `Fact::Invalid`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Bound {
    Major(u32),
    Exact(u32, u32),
}

/// Every field of the `pack` section that any era's version check reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackDeclaration {
    /// Only presence matters to the game; JSON `null` counts as absent.
    pub description: Fact<()>,
    /// JSON integers 0..=u32::MAX only; `48.5`, `"48"`, `-1`, `null` are Invalid.
    pub pack_format: Fact<u32>,
    /// `N` ⇒ (N, N); `[a, b]`; `{min_inclusive, max_inclusive}`; `a > b` ⇒ Invalid.
    pub supported_formats: Fact<(u32, u32)>,
    pub min_format: Fact<Bound>,
    pub max_format: Fact<Bound>,
}

/// What a pack's `pack.mcmeta` declares — facts, never a verdict, because
/// the verdict depends on the instance's version. Stored in the datapack
/// registry; internally tagged, with `Fact`/`Bound` externally tagged (A23).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PackMcmeta {
    /// No root `pack.mcmeta` entry: the game skips the pack.
    Missing,
    /// Not a zip, not UTF-8, or not JSON to serde. Gson is more lenient than
    /// serde, so this is "could not tell", never "the game can't read it".
    Unreadable,
    /// JSON, but no `pack` object: the game skips the pack.
    NoPackSection,
    Read(PackDeclaration),
}

impl PackMcmeta {
    /// Transitional: `pack.pack_format` when it is a readable integer, for
    /// the strict `compat_of` until the verdict replaces it. Deleted when
    /// `PackCompat::Mismatch` goes.
    #[must_use]
    pub fn declared_pack_format(&self) -> Option<u32> {
        match self {
            PackMcmeta::Read(PackDeclaration {
                pack_format: Fact::Present(n),
                ..
            }) => Some(*n),
            _ => None,
        }
    }
}

/// One read of a `pack.mcmeta` body: the declaration, and the pack's display
/// name — the plain text of its description, `None` when absent or empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McmetaRead {
    pub mcmeta: PackMcmeta,
    pub name: Option<String>,
}

/// Read a `pack.mcmeta` body. Never an error: bytes serde cannot parse are
/// `PackMcmeta::Unreadable`.
#[must_use]
pub fn read_mcmeta_bytes(body: &[u8]) -> McmetaRead {
    let Ok(root) = serde_json::from_slice::<Value>(body) else {
        return McmetaRead {
            mcmeta: PackMcmeta::Unreadable,
            name: None,
        };
    };
    let name = root
        .get("pack")
        .and_then(|p| p.get("description"))
        .map(plain_text)
        .filter(|s| !s.is_empty());
    McmetaRead {
        mcmeta: declaration_of(&root),
        name,
    }
}

/// The plain text of a Minecraft text component: a string as is; numbers
/// and booleans as text; an array concatenated; an object's `text` (or
/// `fallback` when it has only `translate`), then its `extra`. `§x`
/// formatting codes are stripped, whitespace runs collapse to one space,
/// and the result is trimmed. (serde_json caps nesting at 128, so the
/// recursion is bounded.)
#[must_use]
pub fn plain_text(v: &Value) -> String {
    let mut raw = String::new();
    push_text(v, &mut raw);
    tidy(&raw)
}

fn push_text(v: &Value, out: &mut String) {
    match v {
        Value::String(s) => out.push_str(s),
        Value::Number(n) => out.push_str(&n.to_string()),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Array(xs) => xs.iter().for_each(|x| push_text(x, out)),
        Value::Object(o) => {
            if let Some(text) = o.get("text") {
                push_text(text, out);
            } else if o.contains_key("translate") {
                if let Some(fallback) = o.get("fallback") {
                    push_text(fallback, out);
                }
            }
            if let Some(extra) = o.get("extra") {
                push_text(extra, out);
            }
        }
        Value::Null => {}
    }
}

/// Strip `§x` codes, collapse whitespace runs to one space, trim.
fn tidy(raw: &str) -> String {
    let mut stripped = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c == '§' {
            chars.next();
            continue;
        }
        stripped.push(c);
    }
    stripped.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn declaration_of(root: &Value) -> PackMcmeta {
    let Some(pack) = root.get("pack").and_then(Value::as_object) else {
        return PackMcmeta::NoPackSection;
    };
    PackMcmeta::Read(PackDeclaration {
        description: match pack.get("description") {
            None | Some(Value::Null) => Fact::Absent,
            Some(Value::String(_) | Value::Array(_) | Value::Object(_)) => Fact::Present(()),
            Some(_) => Fact::Invalid,
        },
        pack_format: fact(pack.get("pack_format"), int),
        supported_formats: fact(pack.get("supported_formats"), supported_range),
        min_format: fact(pack.get("min_format"), bound),
        max_format: fact(pack.get("max_format"), bound),
    })
}

fn fact<T>(v: Option<&Value>, read: fn(&Value) -> Option<T>) -> Fact<T> {
    match v {
        None => Fact::Absent,
        Some(v) => read(v).map_or(Fact::Invalid, Fact::Present),
    }
}

/// A JSON integer that fits `u32`. `48.5`, `"48"`, `-1`, `null` ⇒ `None`.
fn int(v: &Value) -> Option<u32> {
    u32::try_from(v.as_u64()?).ok()
}

fn supported_range(v: &Value) -> Option<(u32, u32)> {
    let (lo, hi) = match v {
        Value::Number(_) => {
            let n = int(v)?;
            (n, n)
        }
        Value::Array(xs) => match xs.as_slice() {
            [a, b] => (int(a)?, int(b)?),
            _ => return None,
        },
        Value::Object(o) => (int(o.get("min_inclusive")?)?, int(o.get("max_inclusive")?)?),
        _ => return None,
    };
    (lo <= hi).then_some((lo, hi))
}

fn bound(v: &Value) -> Option<Bound> {
    match v {
        Value::Number(_) => Some(Bound::Major(int(v)?)),
        Value::Array(xs) => match xs.as_slice() {
            [m] => Some(Bound::Major(int(m)?)),
            [m, n] => Some(Bound::Exact(int(m)?, int(n)?)),
            _ => None,
        },
        _ => None,
    }
}

/// The real sample packs' `pack.mcmeta` bodies from the 2026-09-24 design
/// (§1), plus a builder for a minimal datapack zip around any body.
/// `pub mod`, not `pub(crate)`: the structural guards mask only
/// `mod`/`pub mod` test regions (§0.5 A17).
#[cfg(test)]
pub mod samples {
    pub const DAGGER: &str = r##"{"pack":{"pack_format":15,"description":"Fixes daggers not being daggers??? - Datapack"}}"##;
    pub const FDCUT: &str = r##"{"pack":{"pack_format":15,"description":"Adds compatability between Farmer's Delight and many other mods by adding missing cutting board recipes."}}"##;
    pub const COBBLEMARKS: &str =
        r##"{"pack":{"pack_format":34,"description":"Cobblemarks+ by Ernesto_7O!"}}"##;
    pub const MELLOMONS: &str =
        r##"{"pack":{"description":"MelloMatt Mew Remodel, added by BeezyMC","pack_format":34}}"##;
    pub const BETTERCAPS: &str = r##"{"pack":{"pack_format":48,"supported_formats":{"min_inclusive":34,"max_inclusive":48},"description":"§6Three-dimensional Caps!\n§8§oby _Lvnatic and RedRibbon!"}}"##;
    pub const WITCHHUTS: &str = r##"{"pack":{"description":"Repurposed Structures - Yung's Better Witch Huts v5","pack_format":26,"supported_formats":[0,1000000]}}"##;
    pub const GUNS_DP: &str = r##"{"pack":{"description":"Guns in Vanilla Minecraft!","pack_format":101,"min_format":94,"max_format":101}}"##;
    pub const NULLSCAPE: &str = r##"{"pack":{"id":"nullscape","min_format":[107,1],"max_format":[107,1],"description":[{"text":"Nullscape","color":"#9729bc"},{"text":" - End Reborn\nCreated by ","color":"gray"},{"text":"Stardust Labs","color":"#683edd"}]},"overlays":{"entries":[]}}"##;
    pub const TECTONIC: &str = r##"{"pack":{"min_format":121,"max_format":121,"description":["Made with ",{"text":"<3","color":"red"}," by ",{"text":"Apollo","color":"#b891ff"},"\n"]}}"##;
    pub const HOPO: &str =
        r##"{"pack":{"pack_format":7,"description":"§3Explore new ocean ruins to your world"}}"##;

    /// A minimal real datapack zip: `pack.mcmeta` = `body` at the root, plus
    /// one `data/` entry (what `pack_meta::classify` needs for `Datapack`).
    /// In memory only — callers place it.
    pub fn zip_with_mcmeta(body: &str) -> Vec<u8> {
        use std::io::Write;
        let mut zw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zw.start_file("pack.mcmeta", opts).unwrap();
        zw.write_all(body.as_bytes()).unwrap();
        zw.start_file("data/x/function/a.mcfunction", opts).unwrap();
        zw.write_all(b"say hi").unwrap();
        zw.finish().unwrap().into_inner()
    }
}

#[cfg(test)]
mod tests {
    use super::samples::*;
    use super::*;

    #[test]
    fn format_versions_order_by_major_then_minor() {
        assert!(FormatVersion::new(94, 1) > FormatVersion::new(94, 0));
        assert!(FormatVersion::new(95, 0) > FormatVersion::new(94, u32::MAX));
        assert!(FormatVersion::new(48, 0) < FormatVersion::new(94, 1));
    }

    fn decl(pf: Fact<u32>, sf: Fact<(u32, u32)>, min: Fact<Bound>, max: Fact<Bound>) -> PackMcmeta {
        PackMcmeta::Read(PackDeclaration {
            description: Fact::Present(()),
            pack_format: pf,
            supported_formats: sf,
            min_format: min,
            max_format: max,
        })
    }

    #[test]
    fn declaration_reads_the_real_sample_shapes() {
        use Fact::{Absent, Present};
        let cases: [(&str, &str, PackMcmeta); 10] = [
            ("dagger", DAGGER, decl(Present(15), Absent, Absent, Absent)),
            ("fdcut", FDCUT, decl(Present(15), Absent, Absent, Absent)),
            (
                "cobblemarks",
                COBBLEMARKS,
                decl(Present(34), Absent, Absent, Absent),
            ),
            (
                "mellomons",
                MELLOMONS,
                decl(Present(34), Absent, Absent, Absent),
            ),
            (
                "bettercaps",
                BETTERCAPS,
                decl(Present(48), Present((34, 48)), Absent, Absent),
            ),
            (
                "witchhuts",
                WITCHHUTS,
                decl(Present(26), Present((0, 1_000_000)), Absent, Absent),
            ),
            (
                "guns_dp",
                GUNS_DP,
                decl(
                    Present(101),
                    Absent,
                    Present(Bound::Major(94)),
                    Present(Bound::Major(101)),
                ),
            ),
            (
                "nullscape",
                NULLSCAPE,
                decl(
                    Absent,
                    Absent,
                    Present(Bound::Exact(107, 1)),
                    Present(Bound::Exact(107, 1)),
                ),
            ),
            (
                "tectonic",
                TECTONIC,
                decl(
                    Absent,
                    Absent,
                    Present(Bound::Major(121)),
                    Present(Bound::Major(121)),
                ),
            ),
            ("hopo", HOPO, decl(Present(7), Absent, Absent, Absent)),
        ];
        for (name, body, want) in cases {
            assert_eq!(read_mcmeta_bytes(body.as_bytes()).mcmeta, want, "{name}");
        }
    }

    #[test]
    fn rich_text_and_section_signs_become_a_plain_name() {
        let name = |b: &str| read_mcmeta_bytes(b.as_bytes()).name;
        assert_eq!(
            name(NULLSCAPE).as_deref(),
            Some("Nullscape - End Reborn Created by Stardust Labs")
        );
        assert_eq!(name(TECTONIC).as_deref(), Some("Made with <3 by Apollo"));
        assert_eq!(
            name(BETTERCAPS).as_deref(),
            Some("Three-dimensional Caps! by _Lvnatic and RedRibbon!")
        );
        assert_eq!(
            name(HOPO).as_deref(),
            Some("Explore new ocean ruins to your world")
        );
    }

    #[test]
    fn plain_text_follows_text_then_extra_and_translate_fallbacks() {
        let v: Value = serde_json::from_str(
            r#"[{"text":"A","extra":[{"text":"B"}," C"]},{"translate":"k","fallback":" D"},{"translate":"k2"},1,true]"#,
        )
        .unwrap();
        assert_eq!(plain_text(&v), "AB C D1true");
        assert_eq!(plain_text(&Value::Null), "");
    }

    #[test]
    fn json_without_a_pack_object_is_no_pack_section() {
        for body in [r#"{}"#, r#"{"pack":"x"}"#, "[1]"] {
            assert_eq!(
                read_mcmeta_bytes(body.as_bytes()).mcmeta,
                PackMcmeta::NoPackSection,
                "{body}"
            );
        }
    }

    #[test]
    fn bytes_serde_cannot_parse_are_unreadable() {
        let trailing = br#"{"pack":{"pack_format":48,"description":"d",}}"#;
        assert_eq!(read_mcmeta_bytes(trailing).mcmeta, PackMcmeta::Unreadable);
        assert_eq!(
            read_mcmeta_bytes(b"{\"pack\":{\"description\":\"\xff\"}}").mcmeta,
            PackMcmeta::Unreadable
        );
    }

    #[test]
    fn shapes_outside_the_game_s_integers_are_invalid_never_guessed() {
        let field = |body: &str| match read_mcmeta_bytes(body.as_bytes()).mcmeta {
            PackMcmeta::Read(d) => d,
            other => panic!("{body}: {other:?}"),
        };
        for pf in ["48.5", r#""48""#, "-1", "null"] {
            let d = field(&format!(
                r#"{{"pack":{{"description":"d","pack_format":{pf}}}}}"#
            ));
            assert_eq!(d.pack_format, Fact::Invalid, "pack_format {pf}");
        }
        for sf in ["[1]", "[5,3]", r#"{"min_inclusive":1}"#] {
            let d = field(&format!(
                r#"{{"pack":{{"description":"d","supported_formats":{sf}}}}}"#
            ));
            assert_eq!(d.supported_formats, Fact::Invalid, "supported_formats {sf}");
        }
        for b in ["[1,2,3]", r#""90""#, "-1"] {
            let d = field(&format!(
                r#"{{"pack":{{"description":"d","min_format":{b}}}}}"#
            ));
            assert_eq!(d.min_format, Fact::Invalid, "min_format {b}");
        }
        assert_eq!(
            field(r#"{"pack":{"description":5}}"#).description,
            Fact::Invalid
        );
        assert_eq!(
            field(r#"{"pack":{"description":null}}"#).description,
            Fact::Absent
        );
        assert_eq!(
            field(r#"{"pack":{"min_format":[90]}}"#).min_format,
            Fact::Present(Bound::Major(90))
        );
        assert_eq!(
            field(r#"{"pack":{"supported_formats":7}}"#).supported_formats,
            Fact::Present((7, 7))
        );
    }

    #[test]
    fn a_stored_declaration_round_trips_through_serde() {
        let m = read_mcmeta_bytes(GUNS_DP.as_bytes()).mcmeta;
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v["kind"], "read");
        assert_eq!(serde_json::from_value::<PackMcmeta>(v).unwrap(), m);
        for unit in [
            PackMcmeta::Missing,
            PackMcmeta::Unreadable,
            PackMcmeta::NoPackSection,
        ] {
            let v = serde_json::to_value(&unit).unwrap();
            assert!(v.get("kind").is_some(), "{v}");
            assert_eq!(serde_json::from_value::<PackMcmeta>(v).unwrap(), unit);
        }
    }
}
