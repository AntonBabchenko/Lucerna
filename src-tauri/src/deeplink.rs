//! The single grammar for an inbound import link, shared by the OS protocol
//! handler and the paste-a-URL dialog — one parser, one test suite, no second
//! implementation in TypeScript.
//!
//! A link is an **untrusted** input (any web page can trigger the scheme
//! handler with one navigation), so this module can only ever produce a
//! *description of what to show the user*: there is no representation here for
//! "launch" or "install". See the design spec
//! `2026-07-29-desktop-integration-design.md` §2–§3.
//!
//! Only `lucerna://` is ours. `curseforge://` and `modrinth://` belong to those
//! vendors' own apps and are deliberately never claimed.

use crate::error::Error;
use crate::mods::platform::ModSource;
use serde::Serialize;
use specta::Type;
use url::Url;

/// Max accepted link length. Real links are far shorter; the cap stops a
/// hostile caller from handing us an unbounded string to parse and log.
const MAX_URL_LEN: usize = 2048;

/// Max length of a project/version reference we will put in an API request.
const MAX_REF_LEN: usize = 64;

/// What a link asks Lucerna to *show*. Resolution to a real pack happens in
/// `commands::modpack_resolve_url`; installing still needs the user's click.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ImportTarget {
    pub source: ModSource,
    /// Project slug or platform id, charset-checked.
    pub project: String,
    /// Version id / CurseForge file id / Modrinth version number, when the link
    /// named one.
    pub version: Option<String>,
}

fn invalid(reason: impl Into<String>) -> Error {
    Error::ImportUrlInvalid {
        reason: reason.into(),
    }
}

fn unsupported(platform: impl Into<String>) -> Error {
    Error::ImportUrlUnsupportedSource {
        platform: platform.into(),
    }
}

/// A project / version reference safe to place in an API path or query: ASCII
/// alphanumerics plus `_ . -`, bounded length. One rule rejects path traversal,
/// encoded separators and unbounded junk.
fn checked_ref(raw: &str) -> Result<String, Error> {
    if raw.is_empty() {
        return Err(invalid("link names no project"));
    }
    if raw.len() > MAX_REF_LEN {
        return Err(invalid("project or version reference is too long"));
    }
    if !raw
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-')
    {
        return Err(invalid(
            "project or version reference contains invalid characters",
        ));
    }
    if raw.chars().all(|c| c == '.') {
        return Err(invalid("project or version reference is a path segment"));
    }
    Ok(raw.to_string())
}

/// Source behind a recognised https page host, normalised (ASCII-lowercased,
/// trailing dot stripped). Exact match only — a host that merely *ends with* a
/// known name (`evilmodrinth.com`) is not a match.
fn page_source(host: &str) -> Option<ModSource> {
    match host.trim_end_matches('.').to_ascii_lowercase().as_str() {
        "modrinth.com" | "www.modrinth.com" => Some(ModSource::Modrinth),
        "curseforge.com" | "www.curseforge.com" => Some(ModSource::Curseforge),
        _ => None,
    }
}

/// Parse an inbound import link. Accepts the canonical `lucerna://modpack?…`
/// machine form, the `lucerna://import?url=…` wrapper, and a platform page URL
/// a user can paste.
pub fn parse_import_url(raw: &str) -> Result<ImportTarget, Error> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(invalid("empty link"));
    }
    if raw.len() > MAX_URL_LEN {
        return Err(invalid("link is too long"));
    }
    if raw.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(invalid("link contains whitespace or control characters"));
    }
    let url = Url::parse(raw).map_err(|e| invalid(format!("not a URL ({e})")))?;
    match url.scheme() {
        s if s == crate::cli::URL_SCHEME => parse_scheme_url(&url),
        // https only: a plain-http link would be a silent downgrade, and every
        // platform we accept serves https.
        "https" => parse_page_url(&url),
        other => Err(invalid(format!("unsupported scheme '{other}'"))),
    }
}

/// `lucerna://modpack?source=…&project=…[&version=…]`
/// `lucerna://import?url=<percent-encoded https page URL>`
///
/// The action word lands in the URL's host component for these forms.
fn parse_scheme_url(url: &Url) -> Result<ImportTarget, Error> {
    let action = url.host_str().unwrap_or("").to_ascii_lowercase();
    let q: std::collections::HashMap<String, String> = url.query_pairs().into_owned().collect();
    match action.as_str() {
        "modpack" => {
            let source = match q.get("source").map(|s| s.to_ascii_lowercase()).as_deref() {
                Some("modrinth") => ModSource::Modrinth,
                Some("curseforge") => ModSource::Curseforge,
                Some(other) => return Err(unsupported(other)),
                None => return Err(invalid("link is missing the 'source' parameter")),
            };
            let project = match q.get("project") {
                Some(p) => checked_ref(p)?,
                None => return Err(invalid("link is missing the 'project' parameter")),
            };
            let version = q.get("version").map(|v| checked_ref(v)).transpose()?;
            Ok(ImportTarget {
                source,
                project,
                version,
            })
        }
        "import" => {
            let inner = q
                .get("url")
                .ok_or_else(|| invalid("link is missing the 'url' parameter"))?;
            let inner =
                Url::parse(inner).map_err(|e| invalid(format!("inner link is not a URL ({e})")))?;
            if inner.scheme() != "https" {
                return Err(invalid("inner link must be https"));
            }
            parse_page_url(&inner)
        }
        "" => Err(invalid("link names no action")),
        other => Err(invalid(format!("unknown action '{other}'"))),
    }
}

/// A platform modpack page URL.
fn parse_page_url(url: &Url) -> Result<ImportTarget, Error> {
    let host = url.host_str().unwrap_or("");
    let source = page_source(host).ok_or_else(|| unsupported(host))?;
    let seg: Vec<&str> = url
        .path_segments()
        .map(|s| s.filter(|p| !p.is_empty()).collect())
        .unwrap_or_default();
    match source {
        // /modpack/<slug>[/version/<ref>]  |  /project/<slug>
        ModSource::Modrinth => {
            match seg.first().copied().unwrap_or("") {
                "modpack" | "project" => {}
                _ => {
                    return Err(invalid(
                        "Modrinth link must point at a modpack page (/modpack/<slug>)",
                    ))
                }
            }
            let project = checked_ref(seg.get(1).copied().unwrap_or_default())?;
            let version = match (seg.get(2).copied(), seg.get(3).copied()) {
                (Some("version"), Some(v)) => Some(checked_ref(v)?),
                _ => None,
            };
            Ok(ImportTarget {
                source,
                project,
                version,
            })
        }
        // /minecraft/modpacks/<slug>[/files/<id> | /download/<id>]
        ModSource::Curseforge => {
            if seg.first().copied() != Some("minecraft") || seg.get(1).copied() != Some("modpacks")
            {
                return Err(invalid(
                    "CurseForge link must point at a modpack page (/minecraft/modpacks/<slug>)",
                ));
            }
            let project = checked_ref(seg.get(2).copied().unwrap_or_default())?;
            let version = match (seg.get(3).copied(), seg.get(4).copied()) {
                (Some("files"), Some(v)) | (Some("download"), Some(v)) => Some(checked_ref(v)?),
                _ => None,
            };
            Ok(ImportTarget {
                source,
                project,
                version,
            })
        }
        // `page_source` only yields the two above; kept exhaustive so adding a
        // ModSource variant is a compile error here rather than a silent gap.
        other => Err(unsupported(format!("{other:?}").to_lowercase())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(raw: &str) -> ImportTarget {
        parse_import_url(raw).unwrap_or_else(|e| panic!("{raw} should parse, got {e:?}"))
    }

    #[test]
    fn scheme_modpack_form() {
        assert_eq!(
            ok("lucerna://modpack?source=modrinth&project=cobblemon"),
            ImportTarget {
                source: ModSource::Modrinth,
                project: "cobblemon".into(),
                version: None
            }
        );
    }

    #[test]
    fn scheme_modpack_form_with_version() {
        let t = ok("lucerna://modpack?source=curseforge&project=1234&version=567");
        assert_eq!(t.source, ModSource::Curseforge);
        assert_eq!(t.project, "1234");
        assert_eq!(t.version, Some("567".into()));
    }

    #[test]
    fn scheme_source_is_case_insensitive() {
        assert_eq!(
            ok("lucerna://modpack?source=Modrinth&project=x").source,
            ModSource::Modrinth
        );
    }

    #[test]
    fn scheme_import_wrapper_unwraps_a_page_url() {
        assert_eq!(
            ok("lucerna://import?url=https%3A%2F%2Fmodrinth.com%2Fmodpack%2Fcobblemon"),
            ImportTarget {
                source: ModSource::Modrinth,
                project: "cobblemon".into(),
                version: None
            }
        );
    }

    #[test]
    fn scheme_import_wrapper_rejects_a_non_https_inner_link() {
        for raw in [
            "lucerna://import?url=file%3A%2F%2F%2FC%3A%2FWindows%2Fcalc.exe",
            "lucerna://import?url=javascript%3Aalert(1)",
            "lucerna://import?url=lucerna%3A%2F%2Fmodpack%3Fproject%3Dx",
        ] {
            assert!(parse_import_url(raw).is_err(), "{raw} must be rejected");
        }
    }

    #[test]
    fn scheme_actions_we_do_not_own_are_rejected() {
        for raw in [
            "lucerna://modpack",
            "lucerna://modpack?source=modrinth",
            "lucerna://unknown?source=modrinth&project=x",
            "lucerna://import",
        ] {
            assert!(parse_import_url(raw).is_err(), "{raw} must be rejected");
        }
    }

    #[test]
    fn modrinth_page_urls() {
        assert_eq!(
            ok("https://modrinth.com/modpack/cobblemon").project,
            "cobblemon"
        );
        assert_eq!(
            ok("https://www.modrinth.com/project/cobblemon").project,
            "cobblemon"
        );
        assert_eq!(
            ok("https://modrinth.com/modpack/cobblemon/").project,
            "cobblemon"
        );
        assert_eq!(
            ok("https://modrinth.com/modpack/cobblemon/version/1.6.1").version,
            Some("1.6.1".into())
        );
    }

    #[test]
    fn curseforge_page_urls() {
        let t = ok("https://www.curseforge.com/minecraft/modpacks/all-the-mods-10");
        assert_eq!(t.source, ModSource::Curseforge);
        assert_eq!(t.project, "all-the-mods-10");
        assert_eq!(t.version, None);
        assert_eq!(
            ok("https://www.curseforge.com/minecraft/modpacks/atm10/files/5551234").version,
            Some("5551234".into())
        );
        assert_eq!(
            ok("https://www.curseforge.com/minecraft/modpacks/atm10/download/5551234").version,
            Some("5551234".into())
        );
    }

    #[test]
    fn a_mod_page_is_rejected_with_a_reason_that_names_the_problem() {
        for raw in [
            "https://modrinth.com/mod/sodium",
            "https://www.curseforge.com/minecraft/mc-mods/jei",
        ] {
            match parse_import_url(raw) {
                Err(Error::ImportUrlInvalid { reason }) => {
                    assert!(
                        reason.contains("modpack"),
                        "reason should point at modpack pages, got {reason}"
                    );
                }
                other => panic!("{raw} should be ImportUrlInvalid, got {other:?}"),
            }
        }
    }

    #[test]
    fn dangerous_or_downgraded_schemes_are_rejected() {
        for raw in [
            "javascript:alert(1)",
            "data:text/html,<script>1</script>",
            "file:///C:/Windows/System32/calc.exe",
            "http://modrinth.com/modpack/x",
            "ftp://modrinth.com/modpack/x",
        ] {
            assert!(parse_import_url(raw).is_err(), "{raw} must be rejected");
        }
    }

    #[test]
    fn lookalike_hosts_are_rejected() {
        for raw in [
            "https://evilmodrinth.com/modpack/x",
            "https://modrinth.com.attacker.net/modpack/x",
            "https://xcurseforge.com/minecraft/modpacks/x",
            "https://curseforge.com.evil/minecraft/modpacks/x",
        ] {
            assert!(parse_import_url(raw).is_err(), "{raw} must be rejected");
        }
    }

    #[test]
    fn host_case_and_trailing_dot_are_normalised() {
        assert_eq!(ok("https://MODRINTH.com/modpack/x").project, "x");
        assert_eq!(ok("https://modrinth.com./modpack/x").project, "x");
    }

    #[test]
    fn no_link_can_ask_for_a_launch() {
        // The trust model's load-bearing test: no URL spelling reaches the
        // launch path, because this parser has no way to express one.
        for raw in [
            "lucerna://launch?instance=p",
            "lucerna://launch",
            "lucerna://instance/p",
            "lucerna://start?instance=p&world=w",
        ] {
            assert!(parse_import_url(raw).is_err(), "{raw} must not parse");
        }
    }

    #[test]
    fn extra_query_parameters_cannot_add_meaning() {
        // An unknown parameter is ignored, not honoured — the result is exactly
        // the import target the recognised parameters describe.
        assert_eq!(
            ok("lucerna://modpack?source=modrinth&project=x&launch=1&world=w"),
            ImportTarget {
                source: ModSource::Modrinth,
                project: "x".into(),
                version: None
            }
        );
    }

    #[test]
    fn traversal_and_junk_refs_are_rejected() {
        for raw in [
            "lucerna://modpack?source=modrinth&project=..",
            "lucerna://modpack?source=modrinth&project=.",
            "lucerna://modpack?source=modrinth&project=%2E%2E%2Fetc",
            "lucerna://modpack?source=modrinth&project=a/b",
            "lucerna://modpack?source=modrinth&project=",
            "lucerna://modpack?source=modrinth&project=x&version=../y",
            "https://modrinth.com/modpack/",
        ] {
            assert!(parse_import_url(raw).is_err(), "{raw} must be rejected");
        }
    }

    #[test]
    fn over_length_and_control_characters_are_rejected() {
        assert!(parse_import_url(&format!(
            "https://modrinth.com/modpack/{}",
            "a".repeat(3000)
        ))
        .is_err());
        assert!(parse_import_url(&format!(
            "lucerna://modpack?source=modrinth&project={}",
            "a".repeat(65)
        ))
        .is_err());
        assert!(parse_import_url("https://modrinth.com/modpack/a\nb").is_err());
        assert!(parse_import_url("https://modrinth.com/modpack/a b").is_err());
        assert!(parse_import_url("").is_err());
        assert!(parse_import_url("   ").is_err());
    }

    #[test]
    fn unsupported_sources_are_named_so_the_message_is_actionable() {
        match parse_import_url("lucerna://modpack?source=ftb&project=123") {
            Err(Error::ImportUrlUnsupportedSource { platform }) => assert_eq!(platform, "ftb"),
            other => panic!("expected unsupported-source, got {other:?}"),
        }
        match parse_import_url("https://www.feed-the-beast.com/modpacks/119") {
            Err(Error::ImportUrlUnsupportedSource { platform }) => {
                assert_eq!(platform, "www.feed-the-beast.com")
            }
            other => panic!("expected unsupported-source, got {other:?}"),
        }
    }
}
