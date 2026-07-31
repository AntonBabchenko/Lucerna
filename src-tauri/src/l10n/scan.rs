//! Reading language files out of mod jars. Everything here is pure over bytes
//! so it is unit-testable without an instance on disk, and read-only by
//! construction — an instance's mod jars are hardlinks into the shared store
//! and must never be opened for writing.

use std::collections::BTreeMap;

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
            Some((k.trim().to_string(), v.to_string()))
        })
        .collect()
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
    fn legacy_lang_tolerates_bom_and_crlf() {
        let map = parse_lang_properties("\u{feff}a=1\r\nb=2\r\n".as_bytes());
        assert_eq!(map.get("a").map(String::as_str), Some("1"));
        assert_eq!(map.get("b").map(String::as_str), Some("2"));
    }
}
