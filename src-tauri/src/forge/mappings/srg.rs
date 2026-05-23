//! SRG-flavor mapping file parsers. TSRG v1 is the only format used
//! by Forge 1.13-1.16's mcp_config artifacts. v2 + PROGUARD live in
//! Phase 3.

use crate::error::{Error, Result};
use crate::forge::mappings::obf::ObfIndex;

/// Parse TSRG v1.
///
/// Class header (no leading tab):  `<obf-fqn> <srg-fqn>`
/// Field line (tab-indented):       `<obf-name> <srg-name>`
/// Method line (tab-indented):      `<obf-name> <descriptor> <srg-name>`
pub fn load_tsrg_v1(body: &str) -> Result<ObfIndex> {
    let mut idx = ObfIndex::new();
    let mut current_class: Option<String> = None;

    for (lineno, line_raw) in body.lines().enumerate() {
        let line = line_raw.trim_end();
        if line.is_empty() {
            continue;
        }
        let indented = line.starts_with('\t');
        let trimmed = line.trim_start_matches('\t');
        if trimmed.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = trimmed.split_whitespace().collect();

        if !indented {
            if parts.len() != 2 {
                return Err(parse_err(
                    lineno,
                    format!("class header needs 2 tokens, got {parts:?}"),
                ));
            }
            current_class = Some(parts[0].to_string());
            idx.put_class(parts[0], parts[1]);
        } else {
            let obf_class = current_class
                .as_ref()
                .ok_or_else(|| parse_err(lineno, "member line before any class header".into()))?;
            match parts.len() {
                2 => idx.put_field(obf_class, parts[0], parts[1]),
                3 => idx.put_method(obf_class, parts[0], parts[1], parts[2]),
                _ => {
                    return Err(parse_err(
                        lineno,
                        format!("expected 2 (field) or 3 (method) tokens, got {parts:?}"),
                    ))
                }
            }
        }
    }
    Ok(idx)
}

fn parse_err(lineno: usize, msg: String) -> Error {
    Error::ForgeMavenMetadataParseFailed {
        details: format!("TSRG v1 parse error at line {}: {msg}", lineno + 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = "net/minecraft/client/Minecraft net/minecraft/client/Minecraft\n\
\tfield_71445_a activeLanguage\n\
\tfunc_71396_d (Ljava/lang/Throwable;)V displayCrashReport\n\
net/minecraft/util/text/StringTextComponent net/minecraft/util/text/StringTextComponent\n\
\tfield_150264_a text\n";

    #[test]
    fn parses_class_then_field_then_method() {
        let idx = load_tsrg_v1(FIXTURE).expect("parse");
        assert_eq!(
            idx.lookup_class("net/minecraft/client/Minecraft")
                .as_deref(),
            Some("net/minecraft/client/Minecraft")
        );
        assert_eq!(
            idx.lookup_field("net/minecraft/client/Minecraft", "field_71445_a")
                .as_deref(),
            Some("activeLanguage")
        );
        assert_eq!(
            idx.lookup_method(
                "net/minecraft/client/Minecraft",
                "func_71396_d",
                "(Ljava/lang/Throwable;)V"
            )
            .as_deref(),
            Some("displayCrashReport")
        );
    }

    #[test]
    fn member_before_header_errors() {
        let bad = "\tfield_a name\n";
        assert!(load_tsrg_v1(bad).is_err());
    }

    #[test]
    fn ignores_blank_and_comment_lines() {
        let body = "# top\n\nnet/a net/a\n\t# inner\n\tf1 g1\n";
        let idx = load_tsrg_v1(body).expect("parse");
        assert_eq!(idx.lookup_field("net/a", "f1").as_deref(), Some("g1"));
    }
}
