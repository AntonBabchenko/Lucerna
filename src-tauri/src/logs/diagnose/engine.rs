//! Pattern-matching engine + excerpt extraction. Pure functions —
//! no I/O, no globals, no network.

use super::patterns::{Pattern, PATTERNS};
use super::Diagnosis;
use crate::logs::files::LogSource;
use std::path::Path;

/// Scan `content` for the first matching pattern (in `PATTERNS`
/// declaration order). `source_kind` is a hint to skip patterns
/// whose `source_hint` doesn't apply — never affects correctness.
pub fn match_log(content: &str, source_kind: LogSource) -> Option<Diagnosis> {
    match_log_with(PATTERNS, content, source_kind)
}

/// Same as `match_log` but takes the pattern slice as a parameter.
/// `pub(crate)` so the engine tests can drive it without going through
/// the real PATTERNS array — and so any future caller that wants to
/// scan against a curated subset (e.g. "only Game-relevant patterns
/// after a stop-button kill") can do so without copy-pasting the loop.
pub(crate) fn match_log_with(
    patterns: &[Pattern],
    content: &str,
    source_kind: LogSource,
) -> Option<Diagnosis> {
    for p in patterns {
        if !p.source_hint.matches(source_kind) {
            continue;
        }
        if let Some(idx) = p.matcher.find(content) {
            return Some(Diagnosis {
                pattern_id: p.id.into(),
                title: p.title.into(),
                explanation: p.explanation.into(),
                recommendation: p.recommendation.into(),
                matched_excerpt: extract_excerpt(content, idx, 200),
                repair: super::repair::repair_kind_for(p.id),
            });
        }
    }
    None
}

/// Walk back from `idx` to the previous newline (or up to 80 chars),
/// forward to the next newline (or up to 120 chars). Returns a
/// trimmed slice the user can confirm against the raw log.
pub fn extract_excerpt(content: &str, idx: usize, max_len: usize) -> String {
    // Walk back to the previous '\n' (or up to ~max_len/2.5 chars) so
    // the excerpt is line-bounded when reasonable. Walk forward to the
    // next '\n' (or up to ~max_len*2/3 chars) similarly. The split is
    // asymmetric because the matched substring usually starts the
    // interesting region — more context AFTER than before is the typical
    // useful framing for a JVM stack trace.
    let back_cap = max_len / 3; // ~80 chars when max_len=200
    let fwd_cap = (max_len * 2) / 3; // ~133 chars when max_len=200

    let bytes = content.as_bytes();
    let start = walk_back_to_newline_or_cap(bytes, idx, back_cap);
    let end = walk_forward_to_newline_or_cap(bytes, idx, fwd_cap, content.len());

    // Snap to char boundaries — required because we walked on bytes.
    let start = snap_to_char_boundary(content, start, false);
    let end = snap_to_char_boundary(content, end, true);

    content[start..end].trim().to_string()
}

fn walk_back_to_newline_or_cap(bytes: &[u8], from: usize, cap: usize) -> usize {
    let lower = from.saturating_sub(cap);
    let mut i = from;
    while i > lower {
        if bytes.get(i - 1) == Some(&b'\n') {
            return i;
        }
        i -= 1;
    }
    lower
}

fn walk_forward_to_newline_or_cap(bytes: &[u8], from: usize, cap: usize, total: usize) -> usize {
    let upper = (from + cap).min(total);
    let mut i = from;
    while i < upper {
        if bytes.get(i) == Some(&b'\n') {
            return i;
        }
        i += 1;
    }
    upper
}

fn snap_to_char_boundary(s: &str, mut idx: usize, forward: bool) -> usize {
    while idx > 0 && idx < s.len() && !s.is_char_boundary(idx) {
        if forward {
            idx += 1;
        } else {
            idx -= 1;
        }
    }
    idx
}

/// Map a log file path to its `LogSource` by walking the parent
/// directory. Falls back to `LogSource::Launcher` for paths under
/// our own `<instance>/logs/launch-*.log` captures; uses parent
/// directory name (`crash-reports` vs `logs`) for the MC roots.
pub fn infer_source_from_path(path: &Path) -> LogSource {
    // Walk parent directories looking for the canonical names. Match
    // on the immediate parent first; fall back to grand-parent for
    // the rare case where the file is one level deeper than expected.
    let parent_name = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str());
    match parent_name {
        Some("crash-reports") => LogSource::Crash,
        Some("logs") => {
            // Could be `.minecraft/logs/` (Game) or `<instance>/logs/`
            // (Launcher). Discriminate by checking if "logs/"'s parent
            // is named ".minecraft".
            let grand = path
                .parent()
                .and_then(|p| p.parent())
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str());
            if grand == Some(".minecraft") {
                LogSource::Game
            } else {
                LogSource::Launcher
            }
        }
        _ => LogSource::Launcher,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logs::diagnose::patterns::{Matcher, Pattern, SourceHint};
    use crate::logs::files::LogSource;
    use std::path::PathBuf;

    // --- infer_source_from_path -------------------------------------

    #[test]
    fn infer_source_crash_for_crash_reports_dir() {
        let p = PathBuf::from("C:/x/instances/I/.minecraft/crash-reports/crash-2026-05-23.txt");
        assert_eq!(infer_source_from_path(&p), LogSource::Crash);
    }

    #[test]
    fn infer_source_game_for_dot_minecraft_logs_dir() {
        let p = PathBuf::from("/home/u/instances/I/.minecraft/logs/latest.log");
        assert_eq!(infer_source_from_path(&p), LogSource::Game);
    }

    #[test]
    fn infer_source_launcher_for_our_instance_logs_dir() {
        // Our captures live at <instance>/logs/launch-*.log — i.e.
        // "logs" directly under the instance root, NOT under .minecraft.
        let p = PathBuf::from("C:/x/instances/I/logs/launch-2026-05-23.log");
        assert_eq!(infer_source_from_path(&p), LogSource::Launcher);
    }

    #[test]
    fn infer_source_falls_back_to_launcher_for_unknown_layout() {
        // Defensive — any path that doesn't match the three roots
        // is treated as launcher (the most permissive bucket; pattern
        // hints with Any apply regardless).
        let p = PathBuf::from("C:/some/random.txt");
        assert_eq!(infer_source_from_path(&p), LogSource::Launcher);
    }

    // --- extract_excerpt --------------------------------------------

    #[test]
    fn extract_excerpt_centres_on_match_within_line() {
        let content = "line one\n\
                       at net.example.Foo throws here, this is the matched part of the line, then more\n\
                       line three";
        // Match starts at "matched" — find its byte index.
        let idx = content.find("matched").unwrap();
        let out = extract_excerpt(content, idx, 200);
        assert!(out.contains("matched part of the line"), "got: {out}");
        // Excerpt is bounded by the line, not the whole file.
        assert!(!out.contains("line one"));
        assert!(!out.contains("line three"));
    }

    #[test]
    fn extract_excerpt_clamps_at_line_bounds_no_newlines_in_output() {
        let content = "header\nthe interesting part is right here in this line\nfooter";
        let idx = content.find("interesting").unwrap();
        let out = extract_excerpt(content, idx, 200);
        assert!(
            !out.contains('\n'),
            "excerpt should be single-line, got: {out:?}"
        );
    }

    #[test]
    fn extract_excerpt_handles_match_at_start_of_content() {
        let content = "matched-at-start and then more text";
        let out = extract_excerpt(content, 0, 200);
        assert!(out.starts_with("matched-at-start"), "got: {out}");
    }

    #[test]
    fn extract_excerpt_handles_match_at_end_of_content() {
        let content = "earlier text then matched-at-end";
        let idx = content.find("matched-at-end").unwrap();
        let out = extract_excerpt(content, idx, 200);
        assert!(out.ends_with("matched-at-end"), "got: {out}");
    }

    #[test]
    fn extract_excerpt_caps_long_line_around_match() {
        // 500 chars of pad, then a marker, then 500 more chars.
        let pre = "x".repeat(500);
        let post = "y".repeat(500);
        let content = format!("{pre}MARKER{post}");
        let idx = content.find("MARKER").unwrap();
        let out = extract_excerpt(&content, idx, 200);
        // max_len soft-bounds the excerpt length — exact byte count
        // depends on the walk-back/walk-forward split, but it must
        // not return the full 1006-char line.
        assert!(
            out.len() < 300,
            "excerpt should be capped, got len {}",
            out.len()
        );
        assert!(out.contains("MARKER"), "excerpt must include the match");
    }

    // --- match_log --------------------------------------------------

    // Local PATTERNS for these tests — exercises the engine logic
    // independently of the real knowledge base shipped in Task 3.
    static TEST_HEAP_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"OutOfMemoryError: Java heap").expect("compiles"));

    fn test_patterns() -> Vec<Pattern> {
        vec![
            Pattern {
                id: "test-substring",
                matcher: Matcher::Substring("CRASH_TOKEN"),
                title: "T",
                explanation: "E",
                recommendation: "R",
                source_hint: SourceHint::Any,
            },
            Pattern {
                id: "test-regex",
                matcher: Matcher::Regex(&TEST_HEAP_RE),
                title: "OOM",
                explanation: "out",
                recommendation: "more ram",
                source_hint: SourceHint::Crash,
            },
        ]
    }

    #[test]
    fn match_log_returns_none_when_no_pattern_matches() {
        // The real PATTERNS array is what the production engine
        // consults; for this test we just confirm an unrelated body
        // produces None against the real array.
        let r = match_log("nothing interesting here at all", LogSource::Game);
        assert!(r.is_none());
    }

    #[test]
    fn match_log_against_test_patterns_finds_substring_hit() {
        let r = match_log_with(
            &test_patterns(),
            "preamble CRASH_TOKEN trailer",
            LogSource::Game,
        );
        let d = r.expect("pattern must match");
        assert_eq!(d.pattern_id, "test-substring");
        assert!(d.matched_excerpt.contains("CRASH_TOKEN"));
    }

    #[test]
    fn match_log_against_test_patterns_skips_pattern_when_source_hint_excludes() {
        // test-regex has source_hint = Crash. A Game-source scan
        // must skip it even though the content matches.
        let r = match_log_with(
            &test_patterns(),
            "OutOfMemoryError: Java heap space exhausted",
            LogSource::Game,
        );
        assert!(r.is_none(), "Game-source must skip Crash-only pattern");
    }

    #[test]
    fn match_log_against_test_patterns_first_match_wins() {
        // Both patterns match; substring is declared first → wins.
        let r = match_log_with(
            &test_patterns(),
            "CRASH_TOKEN then OutOfMemoryError: Java heap exhausted",
            LogSource::Crash,
        );
        assert_eq!(r.unwrap().pattern_id, "test-substring");
    }

    use once_cell::sync::Lazy;
    use regex::Regex;

    // --- Per-pattern positive + negative coverage -------------------
    //
    // Each pattern in PATTERNS has one positive test against a
    // real-shaped log excerpt and one negative test against an
    // unrelated trace. Excerpts are simplified-but-realistic — the
    // long form lives in tests/logs_diagnose_integration.rs.

    fn assert_diag(content: &str, src: LogSource, expected_id: &str) {
        let d = match_log(content, src)
            .unwrap_or_else(|| panic!("expected pattern {expected_id} to match, got None"));
        assert_eq!(d.pattern_id, expected_id);
    }

    fn assert_no_diag(content: &str, src: LogSource) {
        if let Some(d) = match_log(content, src) {
            panic!("expected no match, got {}", d.pattern_id);
        }
    }

    // 1. java-version-too-old

    #[test]
    fn pattern_java_version_matches_real_jvm_trace() {
        let content = "java.lang.UnsupportedClassVersionError: net/optifine/Config has been compiled by a more recent version of the Java Runtime (class file version 65.0), this version of the Java Runtime only recognizes class file versions up to 61.0";
        assert_diag(content, LogSource::Game, "java-version-too-old");
    }

    #[test]
    fn pattern_java_version_does_not_match_unrelated_classcast() {
        assert_no_diag(
            "java.lang.ClassCastException: cannot cast Foo to Bar",
            LogSource::Game,
        );
    }

    // 2. mod-resolution-conflict

    #[test]
    fn pattern_mod_conflict_matches_fabric_resolution_exception() {
        let content = "[10:00:01] [main/ERROR]: Failed to start! \
                       net.fabricmc.loader.impl.discovery.ModResolutionException: \
                       Mod resolution encountered an incompatible mod set!";
        assert_diag(content, LogSource::Game, "mod-resolution-conflict");
    }

    #[test]
    fn pattern_mod_conflict_does_not_match_generic_runtime_exception() {
        assert_no_diag(
            "java.lang.RuntimeException: oops something went wrong",
            LogSource::Game,
        );
    }

    // 3. fabric-loader-missing-main

    #[test]
    fn pattern_fabric_missing_main_matches_launcher_stdout() {
        let content = "Error: Could not find or load main class net.fabricmc.loader.impl.launch.knot.KnotClient";
        assert_diag(content, LogSource::Launcher, "fabric-loader-missing-main");
    }

    #[test]
    fn pattern_fabric_missing_main_skipped_on_game_log_source() {
        // source_hint = LauncherStdout — must NOT fire on a game-log.
        let content = "Error: Could not find or load main class net.fabricmc.loader.impl.launch.knot.KnotClient";
        assert_no_diag(content, LogSource::Game);
    }

    // 4. corrupt-mod-jar

    #[test]
    fn pattern_corrupt_jar_matches_zip_exception() {
        let content = "Caused by: java.util.zip.ZipException: zip END header not found";
        assert_diag(content, LogSource::Game, "corrupt-mod-jar");
    }

    #[test]
    fn pattern_corrupt_jar_does_not_match_arbitrary_io_exception() {
        assert_no_diag("java.io.IOException: Connection refused", LogSource::Game);
    }

    // 5. out-of-memory

    #[test]
    fn pattern_oom_matches_heap_space_message() {
        let content = "[Server thread/ERROR]: Encountered an unexpected exception \
                       java.lang.OutOfMemoryError: Java heap space";
        assert_diag(content, LogSource::Game, "out-of-memory");
    }

    #[test]
    fn pattern_oom_does_not_match_stack_overflow() {
        assert_no_diag("java.lang.StackOverflowError", LogSource::Game);
    }

    // 6. port-already-in-use

    #[test]
    fn pattern_port_in_use_matches_bind_exception() {
        let content = "[Server thread/WARN]: java.net.BindException: Address already in use: bind";
        assert_diag(content, LogSource::Game, "port-already-in-use");
    }

    #[test]
    fn pattern_port_in_use_does_not_match_generic_socket_error() {
        assert_no_diag(
            "java.net.SocketException: Connection reset by peer",
            LogSource::Game,
        );
    }

    // 7. disk-full

    #[test]
    fn pattern_disk_full_matches_no_space_left() {
        let content = "java.io.IOException: No space left on device";
        assert_diag(content, LogSource::Game, "disk-full");
    }

    #[test]
    fn pattern_disk_full_does_not_match_read_only_filesystem() {
        assert_no_diag(
            "java.io.IOException: Read-only file system",
            LogSource::Game,
        );
    }

    // 8. server-missing-mods

    #[test]
    fn pattern_server_missing_mods_matches_forge_reject_block() {
        let content = "[12:00:03] [Render thread/ERROR]: Missing or unsupported mods:\n\
                       \tjei (Just Enough Items) 15.2.0.27 [required, missing]";
        assert_diag(content, LogSource::Game, "server-missing-mods");
    }

    #[test]
    fn pattern_server_missing_mods_does_not_match_generic_disconnect() {
        assert_no_diag(
            "[12:00:03] [Render thread/INFO]: Disconnected: Connection closed",
            LogSource::Game,
        );
    }
}
