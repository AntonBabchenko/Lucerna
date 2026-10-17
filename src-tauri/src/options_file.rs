//! A lossless, line-oriented `options.txt` (moved from `l10n::options_txt`
//! so settings sync reads and edits the file the same way the translation
//! pack switch does).
//!
//! Every line the reader does not understand is reproduced byte-for-byte;
//! a file Minecraft wrote round-trips exactly when nothing changes. A key is
//! matched ignoring a leading UTF-8 BOM (U+FEFF) on the first line, which
//! the render keeps.

/// Which line terminator `options.txt` uses. Minecraft's `PrintWriter` picks
/// one separator (`System.lineSeparator()`) for the entire file and uses it
/// consistently — CRLF on Windows, LF on Linux/macOS — never mixed within a
/// single write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineEnding {
    Lf,
    Crlf,
}

impl LineEnding {
    fn as_str(self) -> &'static str {
        match self {
            LineEnding::Lf => "\n",
            LineEnding::Crlf => "\r\n",
        }
    }
}

/// One physical line, preserved losslessly. Splitting on the FIRST `:` only
/// (via `str::split_once`) is what keeps a value containing its own `:`
/// (e.g. `lastServer:127.0.0.1:25565`) intact rather than truncated at the
/// second colon.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Line {
    Pair(String, String),
    /// No colon found (a blank line, or a corrupted one). Reproduced
    /// byte-for-byte on render — this is what makes "preserve every other
    /// line verbatim, including keys we do not understand" hold even for
    /// lines this module cannot interpret at all, not just ones it chooses
    /// not to touch.
    Other(String),
}

impl Line {
    fn render(&self) -> String {
        match self {
            Line::Pair(k, v) => format!("{k}:{v}"),
            Line::Other(s) => s.clone(),
        }
    }
}

/// The key a line names, for matching: a leading BOM is not part of it.
fn key_of(raw: &str) -> &str {
    raw.strip_prefix('\u{FEFF}').unwrap_or(raw)
}

/// A parsed `options.txt`: ordered lines plus enough information to
/// reproduce the original byte stream when nothing changes.
///
/// Line-ending strategy: split on bare `'\n'` and strip a trailing `'\r'`
/// from each resulting segment, then pick ONE ending for the whole document
/// — CRLF if any segment carried a `'\r'`, LF otherwise. For any file with a
/// single consistent terminator (100% of files Minecraft itself writes) this
/// round-trips byte-for-byte: `render(parse(src)) == src` whenever no
/// transform touches a line. A hand-edited file with genuinely MIXED endings
/// is normalised to one style rather than reproduced exactly — a defensible,
/// non-corrupting best effort for a case Minecraft's own writer can never
/// produce, and strictly better than the alternative bug this design avoids:
/// an earlier draft that split on `"\r\n"` as a literal separator would, on
/// a mixed file, silently fold a bare trailing `'\n'` into the last line's
/// VALUE and then append a spurious extra terminator on render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedOptions {
    lines: Vec<Line>,
    ending: LineEnding,
    trailing_newline: bool,
}

impl ParsedOptions {
    pub(crate) fn parse(src: &str) -> Self {
        let trailing_newline = src.ends_with('\n');
        let body = src.strip_suffix('\n').unwrap_or(src);
        let raw_lines: Vec<&str> = if body.is_empty() && !trailing_newline {
            Vec::new()
        } else {
            body.split('\n').collect()
        };

        let ending = if raw_lines.iter().any(|l| l.ends_with('\r')) {
            LineEnding::Crlf
        } else {
            LineEnding::Lf
        };

        let lines = raw_lines
            .into_iter()
            .map(|raw| {
                let raw = raw.strip_suffix('\r').unwrap_or(raw);
                match raw.split_once(':') {
                    Some((k, v)) => Line::Pair(k.to_string(), v.to_string()),
                    None => Line::Other(raw.to_string()),
                }
            })
            .collect();

        Self {
            lines,
            ending,
            trailing_newline,
        }
    }

    pub(crate) fn render(&self) -> String {
        let sep = self.ending.as_str();
        let mut out = self
            .lines
            .iter()
            .map(Line::render)
            .collect::<Vec<_>>()
            .join(sep);
        if self.trailing_newline {
            out.push_str(sep);
        }
        out
    }

    /// Every `key:value` pair, in file order, keys without a BOM.
    pub(crate) fn pairs(&self) -> impl Iterator<Item = (&str, &str)> {
        self.lines.iter().filter_map(|l| match l {
            Line::Pair(k, v) => Some((key_of(k), v.as_str())),
            Line::Other(_) => None,
        })
    }

    /// The value of the first line whose key matches, if any.
    pub(crate) fn get(&self, key: &str) -> Option<&str> {
        self.pairs().find_map(|(k, v)| (k == key).then_some(v))
    }

    pub(crate) fn has_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// The `version:` line as a DataVersion; `None` when it is missing or is
    /// not an integer.
    pub(crate) fn version(&self) -> Option<i32> {
        self.get("version")?.trim().parse().ok()
    }

    /// Overwrite the first matching key's value in place; `false` (and no
    /// change) when no line has that key. Never appends a line.
    pub(crate) fn set_existing(&mut self, key: &str, value: String) -> bool {
        let slot = self.lines.iter_mut().find_map(|l| match l {
            Line::Pair(k, v) if key_of(k) == key => Some(v),
            _ => None,
        });
        match slot {
            Some(v) => {
                *v = value;
                true
            }
            None => false,
        }
    }

    /// Overwrite the first matching key's value in place, or append a new
    /// `key:value` line at the end when no line has that key yet.
    pub(crate) fn set_or_append(&mut self, key: &str, value: String) {
        let existing = self.lines.iter_mut().find_map(|l| match l {
            Line::Pair(k, v) if key_of(k) == key => Some(v),
            _ => None,
        });
        match existing {
            Some(slot) => *slot = value,
            None => self.lines.push(Line::Pair(key.to_string(), value)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WITH_BOM: &str = "\u{FEFF}version:3465\nlang:ru_ru\nfov:0.0\n";

    #[test]
    fn a_bom_on_the_first_line_is_not_part_of_its_key_and_is_kept() {
        let mut doc = ParsedOptions::parse(WITH_BOM);
        assert_eq!(doc.get("version"), Some("3465"));
        assert_eq!(doc.version(), Some(3465));
        assert_eq!(doc.render(), WITH_BOM, "round-trips byte for byte");
        assert!(doc.set_existing("version", "4189".into()));
        assert_eq!(doc.render(), "\u{FEFF}version:4189\nlang:ru_ru\nfov:0.0\n");
    }

    #[test]
    fn set_existing_never_appends() {
        let src = "version:3465\nlang:ru_ru\n";
        let mut doc = ParsedOptions::parse(src);
        assert!(!doc.set_existing("fov", "0.5".into()));
        assert_eq!(doc.render(), src);
        assert!(doc.set_existing("lang", "en_us".into()));
        assert_eq!(doc.render(), "version:3465\nlang:en_us\n");
    }

    #[test]
    fn the_version_line_is_a_data_version_or_nothing() {
        assert_eq!(ParsedOptions::parse("lang:ru_ru\n").version(), None);
        assert_eq!(ParsedOptions::parse("version:abc\n").version(), None);
        assert_eq!(
            ParsedOptions::parse("version:5023\r\n").version(),
            Some(5023)
        );
    }

    #[test]
    fn crlf_and_a_missing_trailing_newline_round_trip() {
        for src in ["version:1\r\nlang:en_us\r\n", "version:1\nlang:en_us", ""] {
            assert_eq!(ParsedOptions::parse(src).render(), src);
        }
    }
}
