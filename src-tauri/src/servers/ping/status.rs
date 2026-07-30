//! Parsing and sanitising the JSON status payload a server returns for a
//! Server List Ping. Pure: a string in, a [`Status`] out.
//!
//! Everything here treats the payload as hostile — it is written by whoever
//! runs the server. Rather than deserialising into typed structs (where one
//! wrong field type fails the whole document), we walk a `serde_json::Value`:
//! a server that reports `"online": "many"` still gets its version and MOTD
//! shown, and the count is simply not claimed.

use serde_json::Value;

/// The MOTD is a one-line hint in the UI, not a document.
pub const MAX_MOTD_CHARS: usize = 200;
/// Version strings are short labels ("1.21.4", "Paper 1.20.1").
pub const MAX_VERSION_CHARS: usize = 64;
/// Servers may report any number; above this we stop believing it.
pub const MAX_REPORTED_PLAYERS: u32 = 1_000_000;
/// Chat components nest arbitrarily; we walk a bounded depth.
pub const MAX_EXTRA_DEPTH: usize = 8;

/// The subset of a status response the UI shows. Every field is optional
/// because every one of them is genuinely optional on the wire.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub players_online: Option<u32>,
    pub players_max: Option<u32>,
    pub version_name: Option<String>,
    pub motd: Option<String>,
}

/// Parse a status payload. `Err(())` means "not a status document at all" — the
/// caller turns that into "no answer".
pub fn parse_status(json: &str) -> Result<Status, ()> {
    let root: Value = serde_json::from_str(json).map_err(|_| ())?;
    let obj = root.as_object().ok_or(())?;

    let players = obj.get("players");
    Ok(Status {
        players_online: players.and_then(|p| p.get("online")).and_then(as_count),
        players_max: players.and_then(|p| p.get("max")).and_then(as_count),
        version_name: obj
            .get("version")
            .and_then(|v| v.get("name"))
            .and_then(Value::as_str)
            .map(|n| sanitise(n, MAX_VERSION_CHARS))
            .filter(|n| !n.is_empty()),
        motd: obj
            .get("description")
            .map(|d| flatten_component(d, 0))
            .map(|m| sanitise(&m, MAX_MOTD_CHARS))
            .filter(|m| !m.is_empty()),
    })
}

/// A player count we are willing to repeat: an integer (or a whole-ish float,
/// which some servers send), clamped into a believable range. Anything else —
/// a string, null, an object — yields `None` rather than a made-up number.
fn as_count(v: &Value) -> Option<u32> {
    let raw = v
        .as_i64()
        .or_else(|| v.as_f64().filter(|f| f.is_finite()).map(|f| f as i64))?;
    Some(raw.clamp(0, i64::from(MAX_REPORTED_PLAYERS)) as u32)
}

/// Flatten a chat component (a plain string, or `{text, extra: [...]}`) into
/// plain text, walking at most [`MAX_EXTRA_DEPTH`] levels of `extra`.
fn flatten_component(v: &Value, depth: usize) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Object(map) => {
            let mut out = String::new();
            if let Some(Value::String(t)) = map.get("text") {
                out.push_str(t);
            }
            if depth < MAX_EXTRA_DEPTH {
                if let Some(Value::Array(items)) = map.get("extra") {
                    for item in items {
                        out.push_str(&flatten_component(item, depth + 1));
                    }
                }
            }
            out
        }
        Value::Array(items) if depth < MAX_EXTRA_DEPTH => items
            .iter()
            .map(|i| flatten_component(i, depth + 1))
            .collect(),
        _ => String::new(),
    }
}

/// Drop `§x` colour codes and control characters, collapse whitespace runs,
/// trim, and truncate to `max_chars` unicode scalars.
fn sanitise(input: &str, max_chars: usize) -> String {
    let mut out = String::new();
    let mut kept = 0usize;
    let mut last_was_space = false;
    let mut chars = input.chars();
    while let Some(c) = chars.next() {
        if kept >= max_chars {
            break;
        }
        if c == '\u{a7}' {
            chars.next(); // the formatting code that follows is not text
            continue;
        }
        let c = if c.is_control() || c == '\u{feff}' {
            ' '
        } else {
            c
        };
        if c.is_whitespace() {
            if last_was_space || out.is_empty() {
                continue;
            }
            last_was_space = true;
        } else {
            last_was_space = false;
        }
        out.push(if c.is_whitespace() { ' ' } else { c });
        kept += 1;
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_player_counts() {
        let s = parse_status(r#"{"players":{"online":3,"max":20}}"#).expect("parses");
        assert_eq!(s.players_online, Some(3));
        assert_eq!(s.players_max, Some(20));
    }

    #[test]
    fn description_as_plain_string_strips_colour_codes() {
        let s = parse_status(r#"{"description":"§aHello §cworld"}"#).expect("parses");
        assert_eq!(s.motd.as_deref(), Some("Hello world"));
    }

    #[test]
    fn description_as_chat_object_flattens_extra() {
        let json = r#"{"description":{"text":"A","extra":[{"text":"B"},{"text":"C"}]}}"#;
        let s = parse_status(json).expect("parses");
        assert_eq!(s.motd.as_deref(), Some("ABC"));
    }

    #[test]
    fn motd_control_chars_dropped_and_length_capped() {
        let json = format!(r#"{{"description":"a\nb{}"}}"#, "x".repeat(400));
        let s = parse_status(&json).expect("parses");
        let motd = s.motd.expect("has motd");
        assert!(!motd.contains('\n'));
        assert!(motd.starts_with("a b"));
        assert!(motd.chars().count() <= MAX_MOTD_CHARS);
    }

    #[test]
    fn absurd_and_non_integer_counts_are_not_trusted() {
        let s = parse_status(r#"{"players":{"online":-5,"max":99999999999}}"#).expect("parses");
        assert_eq!(s.players_online, Some(0));
        assert_eq!(s.players_max, Some(MAX_REPORTED_PLAYERS));

        let s = parse_status(r#"{"players":{"online":"many","max":20}}"#).expect("parses");
        assert_eq!(s.players_online, None);
        assert_eq!(s.players_max, Some(20));
    }

    #[test]
    fn version_name_sanitised_and_capped() {
        let json = format!(r#"{{"version":{{"name":"§b{}"}}}}"#, "v".repeat(200));
        let s = parse_status(&json).expect("parses");
        let v = s.version_name.expect("has version");
        assert!(!v.contains('\u{a7}'));
        assert_eq!(v.chars().count(), MAX_VERSION_CHARS);
    }

    #[test]
    fn empty_object_is_a_valid_but_empty_status() {
        let s = parse_status("{}").expect("parses");
        assert_eq!(s, Status::default());
    }

    #[test]
    fn garbage_is_rejected() {
        assert!(parse_status("not json").is_err());
        assert!(parse_status("[1,2,3]").is_err());
        assert!(parse_status("").is_err());
    }

    #[test]
    fn deeply_nested_extra_stops_at_the_depth_cap() {
        // 40 levels of single-child nesting: only the first MAX_EXTRA_DEPTH
        // contribute, and the walk must not recurse without bound.
        let mut json = String::from(r#"{"text":"x""#);
        for _ in 0..40 {
            json.push_str(r#","extra":[{"text":"x""#);
        }
        for _ in 0..40 {
            json.push_str("}]");
        }
        json.push('}');
        let s = parse_status(&format!(r#"{{"description":{json}}}"#)).expect("parses");
        let motd = s.motd.expect("has motd");
        assert_eq!(motd.chars().count(), MAX_EXTRA_DEPTH + 1);
    }
}
