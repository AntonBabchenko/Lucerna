//! The frontend's write path into the launcher log.
//!
//! `log_ui_error` is the only way UI code reaches `lucerna.log`. Everything
//! interesting here is a pure helper on an owned value: the process-wide cap
//! is a thin `Mutex<LineCap>` wrapper at the bottom, mirroring how `diag.rs`
//! keeps `Appender` testable without touching the global `SINK`.

use specta::Type;
use std::sync::{Mutex, OnceLock};

/// Backstop caps. The frontend funnel trims harder before it ever calls in;
/// these exist because a comment telling a future caller not to flood the log
/// is not a guard.
const MAX_MESSAGE_CHARS: usize = 2000;
const MAX_STACK_BYTES: usize = 8 * 1024;
const TRUNCATION_MARK: &str = "…[truncated]";

/// Reports (not lines) allowed per process. One report is a header plus up to
/// ~20 frame lines, so counting lines here would exhaust the budget ~20×
/// sooner than the frontend's own cap expects.
const MAX_REPORTS_PER_PROCESS: usize = 500;

/// Severity of a UI report. A closed enum rather than a `String` so an
/// unrecognised level is a compile-time non-issue instead of a runtime
/// validation question — same shape as `ModsAuthKind` in `error.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum UiErrorLevel {
    Error,
    Warn,
}

impl UiErrorLevel {
    /// The token the Logs viewer's `LEVEL_RE` looks for.
    fn marker(self) -> &'static str {
        match self {
            UiErrorLevel::Error => "ERROR",
            UiErrorLevel::Warn => "WARN",
        }
    }
}

/// One header line. Frames are emitted separately and UNPREFIXED so the
/// viewer's `STACK_FRAME_RE` (anchored at line start) still folds them.
fn format_header(level: UiErrorLevel, signature: &str, tag: &str, message: &str) -> String {
    format!("[ui/{}] {signature} {tag} — {message}", level.marker())
}

/// Flatten a value into something safe to put on one log line: newlines and
/// tabs become single spaces, other C0 controls are dropped, runs of
/// whitespace collapse, and the result is trimmed. Non-ASCII is preserved —
/// the log carries Russian text and box-drawing characters routinely.
fn sanitise_line(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut pending_space = false;
    for ch in input.chars() {
        match ch {
            '\n' | '\r' | '\t' | ' ' => pending_space = true,
            c if c.is_control() => {}
            c => {
                if pending_space && !out.is_empty() {
                    out.push(' ');
                }
                pending_space = false;
                out.push(c);
            }
        }
    }
    out
}

/// Cap the message, marking the cut so a reader never mistakes a truncated
/// message for the whole one. Counts CHARS, not bytes — Cyrillic is routine
/// here and a byte slice would panic mid-character.
fn truncate_message(input: &str) -> String {
    if input.chars().count() <= MAX_MESSAGE_CHARS {
        return input.to_string();
    }
    let kept: String = input.chars().take(MAX_MESSAGE_CHARS).collect();
    format!("{kept}{TRUNCATION_MARK}")
}

/// Cap the stack on a FRAME boundary. Cutting mid-frame produces a line that
/// looks like a real frame but names the wrong location, which is worse than
/// dropping it.
fn truncate_stack(input: &str) -> String {
    if input.len() <= MAX_STACK_BYTES {
        return input.to_string();
    }
    let mut out = String::with_capacity(MAX_STACK_BYTES + TRUNCATION_MARK.len());
    for line in input.lines() {
        if out.len() + line.len() + 1 > MAX_STACK_BYTES {
            break;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(TRUNCATION_MARK);
    out
}

#[derive(Debug, PartialEq, Eq)]
enum Admit {
    Yes,
    /// The cap was just crossed: emit this report's suppression notice and
    /// nothing afterwards.
    AnnounceSuppression,
    No,
}

/// Owned so every test gets a fresh one. The process-wide instance lives
/// behind a `Mutex` in `report_cap()`. This split is not stylistic: CI runs
/// `cargo test` without `--test-threads=1`, so a bare global driven to its
/// limit by one test would silently exhaust it for every later test in the
/// binary — which would then pass because nothing is emitted.
struct LineCap {
    used: usize,
    limit: usize,
    announced: bool,
}

impl LineCap {
    fn new(limit: usize) -> Self {
        Self {
            used: 0,
            limit,
            announced: false,
        }
    }

    fn admit(&mut self) -> Admit {
        if self.used < self.limit {
            self.used += 1;
            return Admit::Yes;
        }
        if self.announced {
            return Admit::No;
        }
        self.announced = true;
        Admit::AnnounceSuppression
    }
}

/// Build every line one report contributes, in order. Split from the command
/// so the format is testable without an `AppHandle` — the crate has no
/// harness for one.
fn emit_lines(
    level: UiErrorLevel,
    signature: &str,
    tag: &str,
    message: &str,
    stack: Option<&str>,
) -> Vec<String> {
    let mut lines = vec![format_header(
        level,
        &sanitise_line(signature),
        &sanitise_line(tag),
        &truncate_message(&sanitise_line(message)),
    )];
    if let Some(stack) = stack {
        for frame in truncate_stack(stack).lines() {
            if !frame.trim().is_empty() {
                lines.push(frame.to_string());
            }
        }
    }
    // Close the block so the viewer's sticky severity does not bleed into the
    // next `diag!` line, which carries no level marker of its own.
    lines.push("[ui/INFO] —".to_string());
    lines
}

fn report_cap() -> &'static Mutex<LineCap> {
    static CAP: OnceLock<Mutex<LineCap>> = OnceLock::new();
    CAP.get_or_init(|| Mutex::new(LineCap::new(MAX_REPORTS_PER_PROCESS)))
}

/// Append one UI error report to `lucerna.log`.
///
/// Infallible by design: `diag!` swallows its own I/O errors by contract, and
/// the caller fires and forgets, so there is nothing a `Result` could usefully
/// carry.
///
/// `async` with the write inside `spawn_blocking`: a sync Tauri command runs
/// on the main thread, and `diag::_write` takes a `Mutex` and flushes. Same
/// shape `structural_no_sync_reconcile.rs` documents for every other blocking
/// command body.
#[tauri::command]
#[specta::specta]
pub async fn log_ui_error(
    level: UiErrorLevel,
    signature: String,
    tag: String,
    message: String,
    stack: Option<String>,
) {
    let _ = tokio::task::spawn_blocking(move || {
        let admit = match report_cap().lock() {
            Ok(mut cap) => cap.admit(),
            // A poisoned lock means a previous panic while logging. Dropping
            // the report is strictly better than panicking inside the error
            // handler.
            Err(_) => return,
        };
        match admit {
            Admit::No => {}
            Admit::AnnounceSuppression => {
                crate::diag!(
                    "[ui/ERROR] suppressed — {MAX_REPORTS_PER_PROCESS} UI reports this process, no further UI errors will be logged"
                );
            }
            Admit::Yes => {
                for line in emit_lines(level, &signature, &tag, &message, stack.as_deref()) {
                    crate::diag!("{line}");
                }
            }
        }
    })
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- header format ----------------------------------------------------

    /// The header MUST match the Logs viewer's level regex
    /// (`src/lib/logs/render.ts`), or a maintainer filtering to `error` hides
    /// every UI error. This is a parity test: if the format drifts, this fails
    /// rather than the drift being found by a user.
    #[test]
    fn header_matches_viewer_level_regex() {
        let re = regex::Regex::new(r"\[[^/\]]+/(INFO|WARN|ERROR|DEBUG|TRACE|FATAL)\]").unwrap();
        let line = format_header(
            UiErrorLevel::Error,
            "3f9a1c04",
            "panel/mods",
            "TypeError: boom",
        );
        assert!(
            re.is_match(&line),
            "header did not match viewer LEVEL_RE: {line}"
        );
        assert_eq!(line, "[ui/ERROR] 3f9a1c04 panel/mods — TypeError: boom");
    }

    #[test]
    fn warn_level_renders_as_warn() {
        let line = format_header(UiErrorLevel::Warn, "0000dead", "boot", "slow");
        assert_eq!(line, "[ui/WARN] 0000dead boot — slow");
    }

    // -- sanitisation -----------------------------------------------------

    #[test]
    fn sanitise_line_folds_newlines_and_strips_controls() {
        assert_eq!(sanitise_line("a\nb"), "a b");
        assert_eq!(sanitise_line("a\r\nb"), "a b");
        assert_eq!(sanitise_line("a\tb"), "a b");
        assert_eq!(sanitise_line("a\u{0}b"), "ab");
        assert_eq!(sanitise_line("a\u{1b}[31mb"), "a[31mb");
    }

    #[test]
    fn sanitise_line_collapses_runs_and_trims() {
        assert_eq!(sanitise_line("a\n\n\nb"), "a b");
        assert_eq!(sanitise_line("  padded  "), "padded");
        assert_eq!(sanitise_line(""), "");
    }

    #[test]
    fn sanitise_line_keeps_non_ascii() {
        assert_eq!(sanitise_line("ошибка — ×10"), "ошибка — ×10");
    }

    // -- truncation -------------------------------------------------------

    #[test]
    fn truncate_message_marks_the_cut() {
        let long = "x".repeat(MAX_MESSAGE_CHARS + 50);
        let out = truncate_message(&long);
        assert!(out.chars().count() <= MAX_MESSAGE_CHARS + TRUNCATION_MARK.chars().count());
        assert!(out.ends_with(TRUNCATION_MARK), "cut was not marked: {out}");
    }

    #[test]
    fn truncate_message_leaves_short_input_alone() {
        assert_eq!(truncate_message("short"), "short");
    }

    /// Truncation must not split a UTF-8 character — a naive byte slice on
    /// Cyrillic text panics.
    #[test]
    fn truncate_message_is_char_safe() {
        let long = "я".repeat(MAX_MESSAGE_CHARS + 50);
        let out = truncate_message(&long);
        assert!(out.starts_with('я'));
    }

    #[test]
    fn truncate_stack_cuts_on_a_frame_boundary() {
        let frame = "    at ModsTab.svelte:412:19\n";
        let many = frame.repeat(2000);
        let out = truncate_stack(&many);
        assert!(out.len() <= MAX_STACK_BYTES + TRUNCATION_MARK.len());
        for line in out.lines() {
            assert!(
                line.is_empty() || line.starts_with("    at ") || line == TRUNCATION_MARK,
                "cut mid-frame: {line:?}"
            );
        }
    }

    #[test]
    fn truncate_stack_leaves_short_input_alone() {
        let s = "    at a.svelte:1:1\n    at b.svelte:2:2";
        assert_eq!(truncate_stack(s), s);
    }

    // -- the report cap ---------------------------------------------------

    #[test]
    fn line_cap_admits_up_to_the_limit() {
        let mut cap = LineCap::new(3);
        assert_eq!(cap.admit(), Admit::Yes);
        assert_eq!(cap.admit(), Admit::Yes);
        assert_eq!(cap.admit(), Admit::Yes);
    }

    /// The whole point: EXACTLY ONE suppression line, not one per subsequent
    /// call.
    #[test]
    fn line_cap_announces_suppression_exactly_once() {
        let mut cap = LineCap::new(2);
        cap.admit();
        cap.admit();
        assert_eq!(cap.admit(), Admit::AnnounceSuppression);
        for _ in 0..100 {
            assert_eq!(cap.admit(), Admit::No);
        }
    }

    #[test]
    fn line_cap_of_zero_announces_on_the_first_call() {
        let mut cap = LineCap::new(0);
        assert_eq!(cap.admit(), Admit::AnnounceSuppression);
        assert_eq!(cap.admit(), Admit::No);
    }

    // -- emitted block ----------------------------------------------------

    #[test]
    fn emit_lines_puts_header_first_then_unprefixed_frames() {
        let lines = emit_lines(
            UiErrorLevel::Error,
            "3f9a1c04",
            "panel/mods",
            "TypeError: boom",
            Some("    at ModsTab.svelte:412:19\n    at Panel.svelte:8:3"),
        );
        assert_eq!(lines[0], "[ui/ERROR] 3f9a1c04 panel/mods — TypeError: boom");
        assert_eq!(lines[1], "    at ModsTab.svelte:412:19");
        assert_eq!(lines[2], "    at Panel.svelte:8:3");
    }

    /// The viewer's severity tagging is STICKY: an unparsed line inherits the
    /// previous line's level. Without a reset the first `[ui/ERROR]` would
    /// paint the rest of lucerna.log red.
    #[test]
    fn emit_lines_ends_with_a_severity_reset() {
        let lines = emit_lines(UiErrorLevel::Error, "sig", "tag", "msg", None);
        assert_eq!(lines.last().unwrap(), "[ui/INFO] —");
    }

    #[test]
    fn emit_lines_without_a_stack_is_header_plus_reset() {
        let lines = emit_lines(UiErrorLevel::Warn, "sig", "tag", "msg", None);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn emit_lines_sanitises_tag_and_message() {
        let lines = emit_lines(UiErrorLevel::Error, "sig", "a\nb", "c\nd", None);
        assert_eq!(lines[0], "[ui/ERROR] sig a b — c d");
    }
}
