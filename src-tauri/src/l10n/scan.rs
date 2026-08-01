//! Reading language files out of mod jars. Everything here is pure over bytes
//! so it is unit-testable without an instance on disk, and read-only by
//! construction — an instance's mod jars are hardlinks into the shared store
//! and must never be opened for writing.

use std::collections::BTreeMap;
use std::io::Cursor;

use crate::error::Error;

/// A language file's contents: translation key → string.
/// `BTreeMap` so every derived artefact (pack bytes, cache file) is
/// byte-stable across runs and diffs cleanly.
pub type LangMap = BTreeMap<String, String>;

/// Parse a modern (1.13+) `<code>.json` language file.
/// Returns `None` only when the bytes are not valid JSON object syntax;
/// non-string values inside a valid object are skipped, because some mods
/// ship `"_comment": [...]` alongside real entries.
pub fn parse_lang_json(body: &[u8]) -> Option<LangMap> {
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    let obj = v.as_object()?;
    Some(
        obj.iter()
            .filter_map(|(k, val)| val.as_str().map(|s| (k.clone(), s.to_string())))
            .collect(),
    )
}

/// Parse a legacy (≤1.12.2) `<code>.lang` properties file: `key=value` per
/// line, `#` comments, blank lines ignored. Only the FIRST `=` separates, so a
/// value may itself contain `=`. Never fails — an unparseable line is skipped,
/// mirroring the game's own tolerance.
pub fn parse_lang_properties(body: &[u8]) -> LangMap {
    let text = String::from_utf8_lossy(body);
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    text.lines()
        .filter(|line| {
            let t = line.trim_start();
            !t.is_empty() && !t.starts_with('#')
        })
        .filter_map(|line| {
            let (k, v) = line.split_once('=')?;
            // The key is trimmed but the value is kept VERBATIM — do not
            // "fix" this to `v.trim()`. We are reading somebody else's data,
            // and trailing/leading whitespace in a translation string can be
            // load-bearing (e.g. `"Level: "` before an appended number).
            // Silently trimming it would make our copy diverge from what the
            // game actually displays, which defeats the exact-string
            // comparison this parser exists to support (staleness detection
            // in later tasks). A key with surrounding spaces, by contrast,
            // could never match a lookup, so whitespace there is
            // unambiguously an authoring artefact, not data.
            Some((k.trim().to_string(), v.to_string()))
        })
        .collect()
}

/// One language file discovered inside a jar.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LangEntry {
    /// Resource namespace — the `<ns>` in `assets/<ns>/lang/…`. This, not the
    /// jar, is the unit a resource pack overrides, and one jar may ship
    /// several (Jar-in-Jar).
    pub namespace: String,
    /// Language code, normalised to lowercase (`en_us`). Pre-1.11 jars ship
    /// `en_US`; treating those as a separate language would double-count.
    pub code: String,
    /// True for a `.lang` properties file, false for `.json`.
    pub legacy: bool,
}

/// Enumerate every `assets/<ns>/lang/<code>.{json,lang}` entry in a jar,
/// paired with its VERBATIM zip path.
///
/// The path is returned rather than reconstructed because `LangEntry::code` is
/// lowercased while the path on disk may not be (`en_US.lang`); rebuilding it
/// from the normalised code would miss the lookup on exactly the legacy jars
/// that need it.
///
/// Only an unreadable archive is an error; a jar with no language files yields
/// an empty vec.
pub fn list_lang_entries(jar_bytes: &[u8]) -> Result<Vec<(LangEntry, String)>, Error> {
    let zip = zip::ZipArchive::new(Cursor::new(jar_bytes)).map_err(|e| Error::ModsDecode {
        platform: "local jar".into(),
        details: e.to_string(),
    })?;
    Ok(zip
        .file_names()
        .filter_map(|p| parse_lang_path(p).map(|e| (e, p.to_string())))
        .collect())
}

/// `assets/<ns>/lang/<code>.<ext>` → `LangEntry`, or `None` for any other path.
/// Rejects deeper nesting: exactly one path segment after `lang/`.
fn parse_lang_path(path: &str) -> Option<LangEntry> {
    let rest = path.strip_prefix("assets/")?;
    let mut parts = rest.split('/');
    let namespace = parts.next()?;
    if parts.next()? != "lang" {
        return None;
    }
    let file = parts.next()?;
    if parts.next().is_some() {
        return None; // nested deeper than assets/<ns>/lang/<file>
    }
    let (stem, ext) = file.rsplit_once('.')?;
    let legacy = match ext {
        "json" => false,
        "lang" => true,
        _ => return None,
    };
    if namespace.is_empty() || stem.is_empty() {
        return None;
    }
    // Trust boundary: this is the one place an arbitrary third-party jar's
    // zip-entry string becomes a typed `LangEntry`. Later code (the
    // generated-pack assembler) composes NEW zip-entry paths straight from
    // `namespace`/`code` — e.g. `format!("assets/{namespace}/lang/{code}.json", ..)`
    // — without re-validating them, so a value that survives this function
    // is trusted everywhere downstream. A forward slash can never reach
    // `namespace`/`stem` (we split on it above), but `.`/`..` and a
    // backslash (a directory separator on Windows) would still let a
    // hostile or merely broken jar write outside the `assets/` tree of a
    // pack we later generate, and an embedded control character (including
    // NUL) can corrupt a path silently on some filesystems. Deliberately
    // NOT tightened to Minecraft's full namespace grammar (`[a-z0-9_.-]`):
    // we only count strings for a coverage report, so rejecting an
    // unusual-but-harmless namespace would silently under-report a mod
    // rather than protect anything.
    if is_traversal_unsafe(namespace) || is_traversal_unsafe(stem) {
        return None;
    }
    Some(LangEntry {
        namespace: namespace.to_string(),
        code: stem.to_ascii_lowercase(),
        legacy,
    })
}

/// True when `s` is unsafe to compose into a future zip-entry path: see the
/// trust-boundary comment in `parse_lang_path`.
///
/// `pub(crate)`, not private: `l10n::pack` re-applies this same rule to a
/// `NamespaceStore::namespace` read back off disk (where nothing re-validates
/// it the way this module validates a jar's own paths) before composing it
/// into a NEW archive entry name. One rule, one implementation, reused at
/// both trust boundaries rather than a second copy drifting out of sync.
pub(crate) fn is_traversal_unsafe(s: &str) -> bool {
    s == "." || s == ".." || s.contains('\\') || s.chars().any(|c| c.is_ascii_control())
}

/// Read one entry's raw bytes out of a jar. `None` when absent or unreadable —
/// a missing language file is a normal outcome, not an error.
pub fn read_entry(jar_bytes: &[u8], path: &str) -> Option<Vec<u8>> {
    let mut zip = zip::ZipArchive::new(Cursor::new(jar_bytes)).ok()?;
    crate::mods::local::entry_bytes(&mut zip, path)
}

/// Read and parse one language file, dispatching on its format.
/// `zip_path` must be the verbatim path from `list_lang_entries`.
pub fn read_lang_map(jar_bytes: &[u8], entry: &LangEntry, zip_path: &str) -> Option<LangMap> {
    let body = read_entry(jar_bytes, zip_path)?;
    if entry.legacy {
        Some(parse_lang_properties(&body))
    } else {
        parse_lang_json(&body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_lang_body() {
        let body = br#"{ "item.create.wrench": "Wrench", "gui.create.empty": "" }"#;
        let map = parse_lang_json(body).expect("valid json");
        assert_eq!(
            map.get("item.create.wrench").map(String::as_str),
            Some("Wrench")
        );
        // An empty value is a real entry, not a missing one.
        assert_eq!(map.get("gui.create.empty").map(String::as_str), Some(""));
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn json_ignores_non_string_values() {
        // Some mods ship a "_comment": [..] or numeric junk. Skip, don't fail.
        let body = br#"{ "a": "A", "_note": ["x"], "n": 3 }"#;
        let map = parse_lang_json(body).expect("valid json");
        assert_eq!(map.len(), 1);
        assert!(map.contains_key("a"));
    }

    #[test]
    fn malformed_json_is_none_not_panic() {
        assert!(parse_lang_json(b"{ not json").is_none());
    }

    #[test]
    fn json_top_level_array_is_none() {
        // Valid JSON, but the wrong shape — not an object at all, so there is
        // no key/value pairing to read. Distinct code path from
        // `malformed_json_is_none_not_panic`: this exits via `as_object()`,
        // not via the `from_slice` parse failure.
        assert!(parse_lang_json(b"[1,2]").is_none());
    }

    #[test]
    fn parses_legacy_lang_body() {
        let body = b"# a comment\n\nitem.wrench=Wrench\ntile.x.name=Value with = sign\n";
        let map = parse_lang_properties(body);
        assert_eq!(map.get("item.wrench").map(String::as_str), Some("Wrench"));
        // Only the FIRST '=' separates; the rest belongs to the value.
        assert_eq!(
            map.get("tile.x.name").map(String::as_str),
            Some("Value with = sign")
        );
        // Comments and blank lines contribute nothing.
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn legacy_lang_skips_lines_without_a_separator() {
        let map = parse_lang_properties(b"garbage line\nok=1\n");
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("ok").map(String::as_str), Some("1"));
    }

    #[test]
    fn legacy_lang_key_with_no_value_is_empty_string() {
        // The properties-side mirror of `gui.create.empty` in the JSON test:
        // an empty value is a real entry, not a missing one.
        let map = parse_lang_properties(b"key=\n");
        assert_eq!(map.get("key").map(String::as_str), Some(""));
    }

    #[test]
    fn legacy_lang_value_whitespace_is_preserved_verbatim() {
        // Deliberate asymmetry: the key is trimmed, the value is not. A
        // hand-authored line like `gui.container.crafting = Crafting Table`
        // is a real style in the wild, and the leading space before the
        // value must survive so an exact-string comparison against the
        // game's own rendering (staleness detection, later tasks) isn't
        // fooled by whitespace we introduced ourselves.
        let map = parse_lang_properties(b"gui.container.crafting = Crafting Table\n");
        assert_eq!(
            map.get("gui.container.crafting").map(String::as_str),
            Some(" Crafting Table")
        );
    }

    #[test]
    fn legacy_lang_tolerates_bom_and_crlf() {
        let map = parse_lang_properties("\u{feff}a=1\r\nb=2\r\n".as_bytes());
        assert_eq!(map.get("a").map(String::as_str), Some("1"));
        assert_eq!(map.get("b").map(String::as_str), Some("2"));
    }

    /// Build an in-memory jar containing the given (path, contents) entries.
    fn jar(entries: &[(&str, &str)]) -> Vec<u8> {
        use std::io::Write;
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let opts: zip::write::FileOptions<()> = zip::write::FileOptions::default();
            for (name, body) in entries {
                w.start_file(*name, opts).unwrap();
                w.write_all(body.as_bytes()).unwrap();
            }
            w.finish().unwrap();
        }
        buf.into_inner()
    }

    /// Enumeration helper: drop the verbatim zip paths, keep the entries.
    fn entries_only(v: Vec<(LangEntry, String)>) -> Vec<LangEntry> {
        v.into_iter().map(|(e, _)| e).collect()
    }

    #[test]
    fn finds_lang_entries_across_namespaces_and_locales() {
        let bytes = jar(&[
            ("assets/create/lang/en_us.json", r#"{"a":"A"}"#),
            ("assets/create/lang/ru_ru.json", r#"{"a":"А"}"#),
            ("assets/thermal/lang/en_us.json", r#"{"b":"B"}"#),
            ("META-INF/mods.toml", "modId=\"create\""),
            ("assets/create/textures/item/x.png", "notlang"),
        ]);
        let mut found = entries_only(list_lang_entries(&bytes).expect("readable zip"));
        found.sort();
        assert_eq!(
            found,
            vec![
                LangEntry {
                    namespace: "create".into(),
                    code: "en_us".into(),
                    legacy: false
                },
                LangEntry {
                    namespace: "create".into(),
                    code: "ru_ru".into(),
                    legacy: false
                },
                LangEntry {
                    namespace: "thermal".into(),
                    code: "en_us".into(),
                    legacy: false
                },
            ]
        );
    }

    #[test]
    fn recognises_legacy_lang_extension_and_normalises_uppercase_region() {
        let bytes = jar(&[("assets/tconstruct/lang/en_US.lang", "a=A")]);
        let found = entries_only(list_lang_entries(&bytes).expect("readable zip"));
        assert_eq!(
            found,
            vec![LangEntry {
                namespace: "tconstruct".into(),
                // Codes are normalised to lowercase so `en_US` and `en_us`
                // are one language, not two.
                code: "en_us".into(),
                legacy: true,
            }]
        );
    }

    #[test]
    fn keeps_the_verbatim_zip_path_alongside_the_normalised_entry() {
        // The entry's code is lowercased, but the path on disk may not be —
        // reading back must use the original casing or the lookup misses.
        let bytes = jar(&[("assets/tconstruct/lang/en_US.lang", "a=A")]);
        let found = list_lang_entries(&bytes).expect("readable zip");
        assert_eq!(found[0].1, "assets/tconstruct/lang/en_US.lang");
    }

    #[test]
    fn ignores_nested_paths_that_merely_contain_lang() {
        let bytes = jar(&[
            ("assets/x/lang/sub/en_us.json", "{}"),
            ("assets/x/models/lang/en_us.json", "{}"),
        ]);
        assert!(list_lang_entries(&bytes).expect("readable zip").is_empty());
    }

    #[test]
    fn ignores_unrelated_extensions() {
        let bytes = jar(&[("assets/x/lang/en_us.txt", "a=A")]);
        assert!(list_lang_entries(&bytes).expect("readable zip").is_empty());
    }

    #[test]
    fn unreadable_zip_is_an_error() {
        assert!(list_lang_entries(b"not a zip").is_err());
    }

    #[test]
    fn reads_a_named_entry_body() {
        let bytes = jar(&[("assets/create/lang/en_us.json", r#"{"a":"A"}"#)]);
        let body = read_entry(&bytes, "assets/create/lang/en_us.json").expect("present");
        assert_eq!(parse_lang_json(&body).unwrap().get("a").unwrap(), "A");
    }

    #[test]
    fn reads_an_absent_entry_as_none() {
        let bytes = jar(&[("assets/create/lang/en_us.json", "{}")]);
        assert!(read_entry(&bytes, "assets/create/lang/nope.json").is_none());
    }

    #[test]
    fn read_lang_map_dispatches_on_format() {
        let bytes = jar(&[
            ("assets/a/lang/en_us.json", r#"{"k":"json"}"#),
            ("assets/b/lang/en_us.lang", "k=properties"),
        ]);
        let entries = list_lang_entries(&bytes).expect("readable zip");
        for (entry, path) in &entries {
            let map = read_lang_map(&bytes, entry, path).expect("parsed");
            let expected = if entry.legacy { "properties" } else { "json" };
            assert_eq!(map.get("k").map(String::as_str), Some(expected));
        }
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn rejects_dotdot_namespace_as_traversal() {
        // `assets/../lang/en_us.json` → rest = "../lang/en_us.json", so a
        // naive parse would hand back namespace == ".." — a value later code
        // composes straight into a NEW zip-entry path (the generated-pack
        // assembler). This is the trust boundary; the value must die here.
        let bytes = jar(&[("assets/../lang/en_us.json", "{}")]);
        assert!(list_lang_entries(&bytes).expect("readable zip").is_empty());
    }

    #[test]
    fn rejects_dot_namespace_as_traversal() {
        let bytes = jar(&[("assets/./lang/en_us.json", "{}")]);
        assert!(list_lang_entries(&bytes).expect("readable zip").is_empty());
    }

    #[test]
    fn rejects_namespace_containing_a_backslash() {
        // A forward slash can never reach `namespace` — we split on it — but
        // a backslash is a directory separator on Windows and needs an
        // explicit check.
        let bytes = jar(&[("assets/foo\\bar/lang/en_us.json", "{}")]);
        assert!(list_lang_entries(&bytes).expect("readable zip").is_empty());
    }

    #[test]
    fn rejects_stem_of_dotdot() {
        // "...json".rsplit_once('.') splits at the LAST dot, giving
        // stem = ".." and ext = "json" — this exercises the stem side of the
        // guard, not the namespace side.
        let bytes = jar(&[("assets/x/lang/...json", "{}")]);
        assert!(list_lang_entries(&bytes).expect("readable zip").is_empty());
    }

    #[test]
    fn existing_empty_namespace_and_empty_stem_guards_stay_pinned() {
        // These two guards predate the traversal fix above; pin them so a
        // future refactor of `parse_lang_path` can't silently drop either.
        let empty_namespace = jar(&[("assets//lang/x.json", "{}")]);
        assert!(list_lang_entries(&empty_namespace)
            .expect("readable zip")
            .is_empty());

        let empty_stem = jar(&[("assets/x/lang/.json", "{}")]);
        assert!(list_lang_entries(&empty_stem)
            .expect("readable zip")
            .is_empty());
    }

    #[test]
    fn read_entry_on_unreadable_bytes_is_none() {
        // The doc comment promises "absent OR unreadable"; only the
        // "valid zip, missing entry" half was covered before
        // (`reads_an_absent_entry_as_none`).
        assert!(read_entry(b"not a zip", "assets/create/lang/en_us.json").is_none());
    }

    #[test]
    fn namespace_containing_a_dot_is_still_accepted() {
        // The guard targets bare `.`/`..`, not dots in general — a namespace
        // like `some.mod` is legal and must not be silently dropped from the
        // coverage report.
        let bytes = jar(&[("assets/some.mod/lang/en_us.json", "{}")]);
        let found = entries_only(list_lang_entries(&bytes).expect("readable zip"));
        assert_eq!(
            found,
            vec![LangEntry {
                namespace: "some.mod".into(),
                code: "en_us".into(),
                legacy: false,
            }]
        );
    }
}
