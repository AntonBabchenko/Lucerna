//! Log-sharing surface: anonymisation + mclo.gs upload.
//!
//! `anonymise()` is a best-effort regex pipeline that strips identifying or
//! sensitive substrings from a log body before it leaves the launcher. The
//! Share UI warns the user to double-check the body before sharing.
//!
//! `upload_to_mclogs()` POSTs the (optionally anonymised) log body to
//! `api.mclo.gs/1/log` via the `crate::network::request` chokepoint and
//! returns the public paste URL on success.

use crate::error::{Error, Result};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;

// ---------------------------------------------------------------------------
// Anonymisation
// ---------------------------------------------------------------------------

/// Strip identifying / sensitive substrings from a log body before
/// uploading it to a third-party paste service. Best-effort: regex-
/// based, no semantic understanding. The Share UI warns the user to
/// double-check the body before sharing.
pub fn anonymise(input: &str) -> String {
    // All patterns below are static literals validated by the unit tests —
    // `Regex::new` cannot fail at runtime. Per CLAUDE.md `.unwrap()` rule.
    // Home paths come in two shapes and need one rule each.
    //
    // `*_USER_PATH` — username followed by a separator. The username class is
    // permissive (anything but path punctuation) so it keeps working for the two
    // cases that matter in practice: Windows account names containing SPACES
    // (`C:\Users\John Smith\`) and non-ASCII names (Cyrillic on ru-RU installs).
    // The trailing separator is what bounds the match, so a permissive class is
    // safe here.
    //
    // `*_USER_PATH_END` — username that ends at the line, or is closed by
    // punctuation rather than a separator: `HOME=/home/player`,
    // `-Duser.home=/home/player]`. `regex` 1.12 has no look-around, so there is
    // nothing to assert about what follows; the bound has to live in the class,
    // which means WHITESPACE MUST BE EXCLUDED. Without that the greedy run walks
    // straight past the username through the rest of the line
    // (`C:\Users\Player -Djava.io.tmpdir=C:\Users\Player\...` would consume up to
    // the next `:` and strand the second path unscrubbed). Quotes and backticks are
    // excluded for the same reason — logs quote paths (`HOME="/home/player"`,
    // JSON-ish dumps) and swallowing the closing quote corrupts the very text the
    // maintainer is meant to read. The final character is additionally barred from
    // being `,;)]}` so `player,` yields `<user>,` rather than eating the comma.
    //
    // Order matters: the separator rules run first, so a space-containing name is
    // consumed as a whole before the whitespace-free rules ever see it. `<` and `>`
    // are excluded everywhere, so an already-substituted `<user>` never re-matches.
    static WIN_USER_PATH: Lazy<Regex> =
        Lazy::new(|| Regex::new(r#"(?i)([A-Z]:\\Users\\)([^\\/:*?"<>|]+)(\\)"#).unwrap());
    static WIN_USER_PATH_FWD: Lazy<Regex> =
        Lazy::new(|| Regex::new(r#"(?i)([A-Z]:/Users/)([^/:*?"<>|]+)(/)"#).unwrap());
    // The POSIX separator rules exclude whitespace where the Windows ones cannot.
    // A POSIX username genuinely cannot contain a space (`useradd` rejects it, and
    // macOS home dirs use ASCII short names), and without that exclusion `[^/]+`
    // has nothing to stop it: on `HOME=/home/alice OTHERHOME=/home/bob` the run
    // crosses the space and closes on the *second* path's separator, deleting
    // ` OTHERHOME=` from the log. The Windows rules avoid this only by accident —
    // their class excludes `:`, which the next drive letter supplies.
    //
    // The cost is symmetric to the documented Windows one: a POSIX path segment
    // that *does* contain a space (`/Users/John Smith/`) is now only partially
    // scrubbed. That is accepted because such a directory is not an account home —
    // `useradd` rejects spaces and macOS derives the home dir from the space-free
    // short name — whereas `C:\Users\John Smith\` is an ordinary Windows layout,
    // which is why the Windows class keeps permitting spaces. Pinned by
    // `anonymise_posix_space_in_segment_is_partial_known_limitation`.
    static MAC_USER_PATH: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(/Users/)([^/\s<>]+)(/)").unwrap());
    static LINUX_USER_PATH: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(/home/)([^/\s<>]+)(/)").unwrap());
    static WIN_USER_PATH_END: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r#"(?i)([A-Z]:\\Users\\)([^\\/:*?"<>|\s'`]*[^\\/:*?"<>|\s'`,;)\]}])"#).unwrap()
    });
    static WIN_USER_PATH_FWD_END: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r#"(?i)([A-Z]:/Users/)([^/:*?"<>|\s'`]*[^/:*?"<>|\s'`,;)\]}])"#).unwrap()
    });
    static MAC_USER_PATH_END: Lazy<Regex> =
        Lazy::new(|| Regex::new(r#"(/Users/)([^/\s<>"'`]*[^/\s<>"'`,;)\]}])"#).unwrap());
    static LINUX_USER_PATH_END: Lazy<Regex> =
        Lazy::new(|| Regex::new(r#"(/home/)([^/\s<>"'`]*[^/\s<>"'`,;)\]}])"#).unwrap());
    static SETTING_USER_TOKEN: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?i)(Setting user:\s+\S+\s+)([A-Za-z0-9]{30,})").unwrap());
    static ACCESS_TOKEN_FLAG: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(--accessToken\s+)([A-Za-z0-9]{30,})").unwrap());
    static SESSION_PARAM: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?i)(sessionId=)([A-Za-z0-9\-]+)").unwrap());
    static LAN_IP: Lazy<Regex> = Lazy::new(|| {
        Regex::new(concat!(
            r"\b(?:",
            r"192\.168\.\d{1,3}\.\d{1,3}",
            r"|10\.\d{1,3}\.\d{1,3}\.\d{1,3}",
            r"|172\.(?:1[6-9]|2\d|3[0-1])\.\d{1,3}\.\d{1,3}",
            r")\b",
        ))
        .unwrap()
    });

    let mut out = input.to_string();
    out = WIN_USER_PATH.replace_all(&out, r"$1<user>$3").into_owned();
    out = WIN_USER_PATH_FWD
        .replace_all(&out, "$1<user>$3")
        .into_owned();
    out = MAC_USER_PATH.replace_all(&out, "$1<user>$3").into_owned();
    out = LINUX_USER_PATH.replace_all(&out, "$1<user>$3").into_owned();
    out = WIN_USER_PATH_END.replace_all(&out, "$1<user>").into_owned();
    out = WIN_USER_PATH_FWD_END
        .replace_all(&out, "$1<user>")
        .into_owned();
    out = MAC_USER_PATH_END.replace_all(&out, "$1<user>").into_owned();
    out = LINUX_USER_PATH_END
        .replace_all(&out, "$1<user>")
        .into_owned();
    out = SETTING_USER_TOKEN
        .replace_all(&out, "$1<redacted>")
        .into_owned();
    out = ACCESS_TOKEN_FLAG
        .replace_all(&out, "$1<redacted>")
        .into_owned();
    out = SESSION_PARAM.replace_all(&out, "$1<redacted>").into_owned();
    out = LAN_IP.replace_all(&out, "<lan-ip>").into_owned();
    out
}

// ---------------------------------------------------------------------------
// mclo.gs upload
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct McLogsResponse {
    success: bool,
    url: Option<String>,
    error: Option<String>,
}

/// Upload `content` to `api.mclo.gs` and return the public paste URL.
pub async fn upload_to_mclogs(content: &str) -> Result<String> {
    upload_to_mclogs_at("https://api.mclo.gs", content).await
}

/// Test-injectable variant — accepts an arbitrary base URL so wiremock
/// servers can intercept the request.
pub async fn upload_to_mclogs_at(base: &str, content: &str) -> Result<String> {
    let body = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("content", content)
        .finish();
    let endpoint = format!("{base}/1/log");
    let resp = crate::network::request::post(
        &endpoint,
        &[("content-type", "application/x-www-form-urlencoded")],
        body.as_bytes(),
        "logs-share",
    )
    .await
    .map_err(|e| Error::McLogsUpload {
        details: format!("transport: {e}"),
    })?;
    if !(200..300).contains(&resp.status) {
        return Err(Error::McLogsUpload {
            details: format!(
                "HTTP {}: {}",
                resp.status,
                String::from_utf8_lossy(&resp.body)
            ),
        });
    }
    let parsed: McLogsResponse =
        serde_json::from_slice(&resp.body).map_err(|e| Error::McLogsUpload {
            details: format!("decode: {e}"),
        })?;
    if !parsed.success {
        return Err(Error::McLogsUpload {
            details: parsed.error.unwrap_or_else(|| "unknown error".into()),
        });
    }
    parsed.url.ok_or_else(|| Error::McLogsUpload {
        details: "response missing url field".into(),
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anonymise_strips_windows_user_path_basic() {
        let input = r"at file:/C:/Users/Player/AppData/Roaming/Lucerna/something";
        assert!(!anonymise(input).contains("Player"));
        assert!(anonymise(input).contains("<user>"));
    }

    #[test]
    fn anonymise_strips_windows_user_path_backslashes() {
        let input = r"C:\Users\Player\AppData\Roaming";
        let out = anonymise(input);
        assert!(!out.contains("Player"));
        assert!(out.contains(r"C:\Users\<user>\"));
    }

    #[test]
    fn anonymise_strips_macos_user_path() {
        let input = "/Users/player/Library/Application Support/minecraft";
        let out = anonymise(input);
        assert!(!out.contains("/player/"));
        assert!(out.contains("/Users/<user>/"));
    }

    #[test]
    fn anonymise_strips_linux_user_path() {
        let input = "/home/player/.minecraft/logs/latest.log";
        let out = anonymise(input);
        assert!(!out.contains("/player/"));
        assert!(out.contains("/home/<user>/"));
    }

    #[test]
    fn anonymise_keeps_home_without_user_segment() {
        // Nothing that looks like a username follows `/home/`, so there is
        // nothing to scrub and the text must survive untouched.
        assert!(anonymise("reading /home/ directory").contains("/home/ "));
    }

    // --- home paths with no trailing separator (JVM crash dumps, launch args) ---

    #[test]
    fn anonymise_strips_linux_home_at_end_of_line() {
        let out = anonymise("HOME=/home/player");
        assert!(!out.contains("player"));
        assert!(out.ends_with("/home/<user>"));
    }

    #[test]
    fn anonymise_strips_linux_home_before_bracket() {
        let out = anonymise("-Duser.home=/home/player]");
        assert!(!out.contains("player]"));
        assert!(out.contains("/home/<user>]"));
    }

    #[test]
    fn anonymise_strips_macos_home_at_end_of_line() {
        let out = anonymise("user.home = /Users/player");
        assert!(!out.contains("player"));
        assert!(out.ends_with("/Users/<user>"));
    }

    #[test]
    fn anonymise_strips_windows_home_at_end_of_line() {
        let out = anonymise(r"USERPROFILE=C:\Users\Player");
        assert!(!out.contains("Player"));
        assert!(out.ends_with(r"C:\Users\<user>"));
    }

    // --- regression guards: these describe behaviour that ALREADY works and
    // that a careless tightening of the username class would silently break ---

    #[test]
    fn anonymise_strips_windows_user_with_space() {
        // Windows account names routinely contain spaces.
        let out = anonymise(r"C:\Users\John Smith\AppData\Roaming");
        assert!(!out.contains("Smith"));
        assert!(out.contains(r"C:\Users\<user>\AppData"));
    }

    #[test]
    fn anonymise_space_in_name_at_end_of_line_is_partial_known_limitation() {
        // KNOWN LIMITATION, pinned deliberately rather than left to chance.
        // `C:\Users\John Smith` (no trailing separator) is textually
        // indistinguishable from `C:\Users\Player is running out of memory` —
        // nothing in the string says whether the second word is a surname or
        // prose. Consuming past the space would scrub the name but silently eat
        // real log content in the far more common prose case, so the rule stops
        // at the space: the first token is scrubbed, the rest survives.
        // Separator-terminated `C:\Users\John Smith\...` is handled in full by
        // WIN_USER_PATH and is the case that actually occurs in paths.
        let out = anonymise(r"USERPROFILE=C:\Users\John Smith");
        assert!(!out.contains("John"));
        assert!(out.contains(r"C:\Users\<user> Smith"));
    }

    #[test]
    fn anonymise_strips_windows_non_ascii_user() {
        // ru-RU installs have Cyrillic account names, and the negated classes must
        // keep matching them rather than truncating at the first non-ASCII byte.
        let out = anonymise(r"C:\Users\Антон\AppData\Roaming");
        assert!(!out.contains("Антон"));
        assert!(out.contains(r"C:\Users\<user>\AppData"));
    }

    // --- boundaries ---

    #[test]
    fn anonymise_strips_both_paths_on_one_line() {
        // Regression guard for the review finding: a bare path followed by a
        // second, separator-terminated path on the SAME line. A username class
        // that permits whitespace runs past the first name to the next `:` and
        // strands the second path unscrubbed.
        let out = anonymise(
            r"-Duser.home=C:\Users\Player -Djava.io.tmpdir=C:\Users\Player\AppData\Local\Temp",
        );
        assert!(!out.contains("Player"), "username leaked: {out}");
        assert!(out.contains(r"C:\Users\<user> -Djava"));
        assert!(out.contains(r"C:\Users\<user>\AppData"));
    }

    #[test]
    fn anonymise_strips_two_linux_paths_on_one_line() {
        // The POSIX class must stop at the space; otherwise the run closes on the
        // SECOND path's separator and ` OTHERHOME=` is silently deleted.
        let out = anonymise("HOME=/home/alice OTHERHOME=/home/bob");
        assert!(!out.contains("alice") && !out.contains("bob"));
        assert!(out.contains("OTHERHOME="), "log content destroyed: {out}");
        assert_eq!(out, "HOME=/home/<user> OTHERHOME=/home/<user>");
    }

    #[test]
    fn anonymise_strips_two_macos_paths_on_one_line() {
        let out = anonymise("a=/Users/alice b=/Users/bob");
        assert!(!out.contains("alice") && !out.contains("bob"));
        assert_eq!(out, "a=/Users/<user> b=/Users/<user>");
    }

    #[test]
    fn anonymise_keeps_quotes_balanced_around_paths() {
        let out = anonymise("cmd /home/alice \"/home/alice/logs\"");
        assert!(!out.contains("alice"));
        assert_eq!(out.matches('"').count(), 2, "quote destroyed: {out}");
    }

    #[test]
    fn anonymise_keeps_closing_quote_on_bare_path() {
        // A path that ends at the quote goes through the *_END rules, which must
        // not swallow the quote — logs and JSON-ish dumps quote paths routinely.
        let out = anonymise("export HOME=\"/home/alice\"");
        assert!(!out.contains("alice"));
        assert_eq!(out, "export HOME=\"/home/<user>\"");
    }

    #[test]
    fn anonymise_keeps_closing_quote_on_bare_windows_path() {
        let out = anonymise("USERPROFILE='C:\\Users\\Player'");
        assert!(!out.contains("Player"));
        assert_eq!(out, "USERPROFILE='C:\\Users\\<user>'");
    }

    #[test]
    fn anonymise_keeps_json_structure_intact() {
        let out = anonymise("{\"home\":\"/home/alice\"}");
        assert!(!out.contains("alice"));
        assert_eq!(out, "{\"home\":\"/home/<user>\"}");
    }

    #[test]
    fn anonymise_posix_space_in_segment_is_partial_known_limitation() {
        // KNOWN LIMITATION, pinned deliberately — the mirror of the Windows one.
        // Excluding whitespace from the POSIX separator class is what stops a run
        // from crossing into the next path and deleting log content; the cost is
        // that a POSIX segment containing a space scrubs only its first token.
        // Accepted because such a directory is not an account home: `useradd`
        // rejects spaces and macOS derives the home dir from the space-free short
        // name, so this is a hand-made directory rather than a real username.
        let out = anonymise("/Users/John Smith/Library");
        assert!(!out.contains("John"));
        assert!(out.contains("/Users/<user> Smith/Library"));
    }

    #[test]
    fn anonymise_keeps_prose_after_windows_path() {
        // The log is the thing the user wants read — over-matching must not eat it.
        let out = anonymise(r"C:\Users\Player is running out of memory");
        assert!(!out.contains("Player is"));
        assert!(out.contains("is running out of memory"));
    }

    #[test]
    fn anonymise_keeps_following_lines() {
        let out = anonymise("user.home=C:\\Users\\Player\n[12:00:01] Loading world\n");
        assert!(out.contains("Loading world"));
        assert!(out.contains(r"C:\Users\<user>"));
    }

    #[test]
    fn anonymise_strips_macos_non_ascii_user() {
        // An ASCII-only username class would emit `/Users/<user>é/` — worse than
        // doing nothing, because it looks redacted while still leaking.
        let out = anonymise("/Users/José/Desktop");
        assert!(!out.contains("os"), "non-ASCII remainder leaked: {out}");
        assert!(out.contains("/Users/<user>/Desktop"));
    }

    #[test]
    fn anonymise_strips_linux_non_ascii_user_at_end_of_line() {
        let out = anonymise("HOME=/home/José");
        assert!(!out.contains("os"), "non-ASCII remainder leaked: {out}");
        assert!(out.ends_with("/home/<user>"));
    }

    #[test]
    fn anonymise_strips_windows_user_with_parenthetical() {
        // Windows creates `Player (2)` for duplicate profile names.
        let out = anonymise(r"C:\Users\Player (2)\Desktop");
        assert!(!out.contains("Player"));
        assert!(out.contains(r"C:\Users\<user>\Desktop"));
    }

    #[test]
    fn anonymise_keeps_trailing_punctuation() {
        let out = anonymise("path /home/player, and then");
        assert!(!out.contains("player"));
        assert!(out.contains("/home/<user>, and then"));
    }

    #[test]
    fn anonymise_leaves_no_username_in_realistic_log_lines() {
        // Battery over the shapes that actually appear in JVM crash dumps, env
        // blocks and launch-arg lines. Every entry uses the same sentinel name, so
        // any surviving occurrence is a leak regardless of which rule should have
        // caught it. Three review rounds each found a leak the targeted tests
        // missed; this exists so the next new shape is caught by construction.
        const SENTINEL: &str = "zealot";
        let lines = [
            "HOME=/home/zealot",
            "HOME=/home/zealot/",
            "user.home=/home/zealot/.minecraft",
            "-Duser.home=/home/zealot]",
            "-Duser.home=/home/zealot -Dfoo=/home/zealot/x",
            "export HOME=\"/home/zealot\"",
            "path is `/home/zealot` here",
            "{\"home\":\"/home/zealot\"}",
            "a=/home/zealot b=/home/zealot",
            "/home/zealot|/home/zealot",
            "at java.base/java.io.File.<init>(/home/zealot/x)",
            "/Users/zealot",
            "/Users/zealot/Library/Application Support",
            "a=/Users/zealot b=/Users/zealot",
            r"C:\Users\zealot",
            r"C:\Users\zealot\AppData\Roaming",
            r"-Duser.home=C:\Users\zealot -Dtmp=C:\Users\zealot\Temp",
            r"USERPROFILE='C:\Users\zealot'",
            "file:/C:/Users/zealot/AppData",
            "mixed C:/Users/zealot and /home/zealot on one line",
        ];
        for line in lines {
            let out = anonymise(line);
            assert!(
                !out.contains(SENTINEL),
                "username leaked\n  in : {line}\n  out: {out}"
            );
        }
    }

    #[test]
    fn anonymise_preserves_surrounding_log_content() {
        // The counterpart to the leak battery: scrubbing must not eat the log.
        // Each case pins a fragment that has to survive next to the redaction.
        let cases = [
            ("HOME=/home/zealot OTHERHOME=/home/other", "OTHERHOME="),
            ("/home/zealot is out of memory", "is out of memory"),
            ("export HOME=\"/home/zealot\" && run", "\" && run"),
            ("{\"home\":\"/home/zealot\",\"n\":1}", "\",\"n\":1}"),
            (
                "user.home=/home/zealot\n[12:00] Loading world",
                "Loading world",
            ),
            (r"C:\Users\zealot is out of memory", "is out of memory"),
        ];
        for (input, must_survive) in cases {
            let out = anonymise(input);
            assert!(
                out.contains(must_survive),
                "log content destroyed\n  in : {input}\n  out: {out}\n  lost: {must_survive}"
            );
        }
    }

    #[test]
    fn anonymise_is_idempotent() {
        let input = r"C:\Users\Player\AppData and /home/player/.minecraft and /Users/player";
        let once = anonymise(input);
        assert_eq!(anonymise(&once), once);
    }

    #[test]
    fn anonymise_strips_access_token_via_setting_user() {
        let input =
            "[main/INFO]: Setting user: Tester aaaaBBBBccccDDDDeeeeFFFFgggg1111HHHH2222iiii";
        let out = anonymise(input);
        assert!(!out.contains("aaaaBBBBccccDDDDeeeeFFFFgggg1111HHHH2222iiii"));
        assert!(out.contains("<redacted>"));
    }

    #[test]
    fn anonymise_strips_access_token_flag() {
        let input = "--accessToken aaaaBBBBccccDDDDeeeeFFFFgggg1111HHHH2222iiii --uuid foo";
        let out = anonymise(input);
        assert!(!out.contains("aaaaBBBBccccDDDDeeeeFFFFgggg1111HHHH2222iiii"));
    }

    #[test]
    fn anonymise_strips_session_uuid_param() {
        let input = "GET https://session.example/play?sessionId=12345678-abcd-1234-abcd-1234567890ab HTTP/1.1";
        let out = anonymise(input);
        assert!(!out.contains("12345678-abcd-1234-abcd-1234567890ab"));
        assert!(out.contains("sessionId=<redacted>"));
    }

    #[test]
    fn anonymise_strips_lan_ip_192() {
        assert!(anonymise("Connecting to 192.168.1.42:25565").contains("<lan-ip>"));
    }

    #[test]
    fn anonymise_strips_lan_ip_10() {
        assert!(anonymise("from 10.0.5.23 port 25565").contains("<lan-ip>"));
    }

    #[test]
    fn anonymise_strips_lan_ip_172_in_private_range() {
        assert!(anonymise("got 172.16.0.1 packet").contains("<lan-ip>"));
        assert!(anonymise("got 172.31.255.254 packet").contains("<lan-ip>"));
    }

    #[test]
    fn anonymise_keeps_public_ip() {
        let s = anonymise("Connecting to 123.45.67.89:25565");
        assert!(s.contains("123.45.67.89"));
        assert!(!s.contains("<lan-ip>"));
    }

    #[test]
    fn anonymise_keeps_172_15_not_in_private_range() {
        // 172.15.x.x is NOT private (private is 172.16-31).
        assert!(anonymise("from 172.15.0.1 packet").contains("172.15.0.1"));
    }

    #[tokio::test]
    async fn upload_returns_url_on_success() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/1/log"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "id": "abcdef",
                "url": "https://mclo.gs/abcdef",
                "raw": "https://api.mclo.gs/1/raw/abcdef"
            })))
            .mount(&server)
            .await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let url = upload_to_mclogs_at(&server.uri(), "test content")
            .await
            .expect("upload ok");
        assert_eq!(url, "https://mclo.gs/abcdef");
    }

    #[tokio::test]
    async fn upload_returns_error_on_4xx() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/1/log"))
            .respond_with(ResponseTemplate::new(413).set_body_string("Log too large"))
            .mount(&server)
            .await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let err = upload_to_mclogs_at(&server.uri(), "test")
            .await
            .unwrap_err();
        assert!(matches!(err, crate::error::Error::McLogsUpload { .. }));
    }

    #[tokio::test]
    async fn upload_returns_error_on_success_false() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/1/log"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": false,
                "error": "Log too large"
            })))
            .mount(&server)
            .await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let err = upload_to_mclogs_at(&server.uri(), "test")
            .await
            .unwrap_err();
        assert!(matches!(err, crate::error::Error::McLogsUpload { .. }));
    }
}
