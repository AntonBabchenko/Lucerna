//! The prompt and its answer.
//!
//! Instructions here are a cheap prior, not the guarantee — integrity is
//! enforced afterwards by `prefill::verify`. Bump [`PROMPT_VERSION`] on ANY
//! textual change: it is part of the cache key, so a reworded prompt must be
//! a cache miss rather than a pack that silently mixes two phrasings.
//!
//! Parsing is lenient about WRAPPING and strict about SHAPE. Lenient because
//! JSON mode is not available everywhere — Anthropic's OpenAI-compatibility
//! layer documents `response_format` as ignored — so fenced or prefaced JSON
//! is normal, not an error. Strict because a positional array cannot be
//! realigned to ids and would mistranslate silently.

use crate::l10n::prefill::role::UiRole;
use std::collections::BTreeMap;

/// Part of the cache key. Bump on every change to the text below.
pub const PROMPT_VERSION: u32 = 1;

pub fn system_prompt() -> &'static str {
    "You are a professional game-localization translator working on Minecraft \
mod text. Translate only what you are asked to translate.\n\
RULES:\n\
1. Reproduce every format placeholder exactly as it appears (%s, %1$s) and \
every section-sign colour code (§a, §l) in the same order and the same \
number of times.\n\
2. Never translate: mod names, brand names, registry identifiers such as \
minecraft:diamond, URLs, file paths, and command literals beginning with '/'. \
Copy them through character for character.\n\
3. Keep the register of the original: an item name is a noun phrase, not a \
sentence. Do not add explanations, quotes, or trailing punctuation the \
original does not have.\n\
4. Answer with a single JSON object and nothing else: \
{\"translations\": {\"<id>\": \"<translation>\"}}. Use exactly the ids you \
were given. If you cannot translate one, omit that id entirely — omitting is \
correct and costs nothing."
}

/// The per-batch user message. `units` is `(id, source_en)`. `glossary` is a
/// short list of approved term pairs; it is injected rather than described
/// because a model follows an example far more reliably than an instruction.
pub fn build_user_prompt(
    target_lang: &str,
    role: UiRole,
    units: &[(String, String)],
    glossary: &[(&str, &str)],
) -> String {
    let mut s = format!(
        "Translate the following Minecraft mod strings from English into the \
locale {target_lang}.\n"
    );
    s.push_str(match role {
        UiRole::Name => {
            "These are short names shown on items, blocks or tabs. Translate \
them as noun phrases.\n"
        }
        UiRole::Prose => {
            "These are sentences shown in tooltips or guidebooks. Translate \
them as natural prose.\n"
        }
        UiRole::Other => "These are interface strings.\n",
    });
    if !glossary.is_empty() {
        s.push_str("\nUse these established term translations exactly:\n");
        for (en, tr) in glossary {
            s.push_str(&format!("- {en} = {tr}\n"));
        }
    }
    s.push_str("\nStrings:\n");
    let obj: serde_json::Map<String, serde_json::Value> = units
        .iter()
        .map(|(id, value)| (id.clone(), serde_json::Value::String(value.clone())))
        .collect();
    s.push_str(
        &serde_json::to_string_pretty(&serde_json::Value::Object(obj))
            .unwrap_or_else(|_| "{}".to_string()),
    );
    s
}

/// Pull the translations object out of a model answer.
pub fn parse_response(raw: &str) -> Result<BTreeMap<String, String>, String> {
    let slice = extract_json_object(raw).ok_or_else(|| "no JSON object in response".to_string())?;
    let parsed: serde_json::Value =
        serde_json::from_str(slice).map_err(|e| format!("invalid JSON: {e}"))?;
    let translations = parsed
        .get("translations")
        .ok_or_else(|| "response has no `translations` field".to_string())?;
    let map = translations
        .as_object()
        .ok_or_else(|| "`translations` must be an object keyed by the batch ids".to_string())?;
    Ok(map
        .iter()
        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
        .collect())
}

/// The substring from the first `{` to its matching `}`, ignoring braces
/// inside JSON strings. Enough to survive fences and chatty preambles.
fn extract_json_object(raw: &str) -> Option<&str> {
    let start = raw.find('{')?;
    let bytes = raw.as_bytes();
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for i in start..bytes.len() {
        let c = bytes[i];
        if in_string {
            if escaped {
                escaped = false;
            } else if c == b'\\' {
                escaped = true;
            } else if c == b'"' {
                in_string = false;
            }
            continue;
        }
        match c {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                // Cannot underflow: the scan starts at the first `{`, so the
                // very first byte matched raises depth to 1, and reaching 0
                // returns immediately.
                depth -= 1;
                if depth == 0 {
                    return raw.get(start..=i);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::l10n::prefill::role::UiRole;

    fn units() -> Vec<(String, String)> {
        vec![("s0".to_string(), "Andesite Alloy".to_string())]
    }

    #[test]
    fn parses_a_plain_json_object() {
        let out = parse_response(r#"{"translations":{"s0":"Альфа","s1":"Бета"}}"#)
            .expect("well-formed response parses");
        assert_eq!(out.get("s0").map(String::as_str), Some("Альфа"));
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn parses_through_a_markdown_fence_and_a_preamble() {
        // Models wrap JSON in fences even when told not to, and Anthropic's
        // OpenAI-compat layer IGNORES response_format, so JSON mode cannot be
        // relied on. Rejecting a fence would burn a retry on a fine answer.
        let out = parse_response("Sure!\n```json\n{\"translations\":{\"s0\":\"Альфа\"}}\n```\n")
            .expect("fenced response parses");
        assert_eq!(out.get("s0").map(String::as_str), Some("Альфа"));
    }

    #[test]
    fn survives_braces_inside_string_values() {
        let out = parse_response(r#"{"translations":{"s0":"Значение {0} готово"}}"#)
            .expect("braces inside a string must not end the object early");
        assert_eq!(
            out.get("s0").map(String::as_str),
            Some("Значение {0} готово")
        );
    }

    #[test]
    fn an_unbalanced_brace_inside_a_string_does_not_end_the_object() {
        // `survives_braces_inside_string_values` above does NOT pin the
        // string-awareness of the scanner: `{0}` is itself balanced, so a
        // counter that ignores string context still lands on the right byte.
        // Verified by mutation — dropping the `in_string` tracking leaves all
        // of the other tests green. This one is the assertion that fails,
        // because a lone `{` is only survivable if the scanner knows it is
        // inside a JSON string value.
        let out = parse_response(r#"{"translations":{"s0":"Открывающая { скобка"}}"#)
            .expect("an unbalanced brace inside a string value must not end the object");
        assert_eq!(
            out.get("s0").map(String::as_str),
            Some("Открывающая { скобка")
        );
    }

    #[test]
    fn rejects_a_positional_array() {
        // A positional array cannot be aligned back to ids with confidence —
        // that is a silent mistranslation at scale, so it is a hard error.
        assert!(parse_response(r#"{"translations":["Альфа","Бета"]}"#).is_err());
    }

    #[test]
    fn rejects_a_response_with_no_json_at_all() {
        assert!(parse_response("I cannot help with that.").is_err());
    }

    #[test]
    fn the_prompt_carries_ids_sources_glossary_and_the_locale() {
        let p = build_user_prompt("ru_ru", UiRole::Name, &units(), &[("Copper", "Медь")]);
        assert!(p.contains("ru_ru"));
        assert!(p.contains("s0"));
        assert!(p.contains("Andesite Alloy"));
        assert!(p.contains("Медь"));
    }

    #[test]
    fn the_system_prompt_names_the_do_not_translate_classes() {
        let s = system_prompt().to_lowercase();
        assert!(s.contains("url"));
        assert!(s.contains("placeholder"));
        assert!(s.contains("omit"));
    }
}
