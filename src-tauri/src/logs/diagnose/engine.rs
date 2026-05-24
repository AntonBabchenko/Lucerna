//! Pattern-matching engine + excerpt extraction. Pure functions —
//! no I/O, no globals, no network. Stub bodies are filled in by
//! Task 2.

use super::patterns::{Pattern, PATTERNS};
use super::Diagnosis;
use crate::logs::files::LogSource;
use std::path::Path;

/// Scan `content` for the first matching pattern (in `PATTERNS`
/// declaration order). `source_kind` is a hint to skip patterns
/// whose `source_hint` doesn't apply — never affects correctness.
pub fn match_log(content: &str, source_kind: LogSource) -> Option<Diagnosis> {
    let _ = (content, source_kind);
    None
}

/// Walk back from `idx` to the previous newline (or up to 80 chars),
/// forward to the next newline (or up to 120 chars). Returns a
/// trimmed slice the user can confirm against the raw log.
pub fn extract_excerpt(content: &str, idx: usize, max_len: usize) -> String {
    let _ = (content, idx, max_len);
    String::new()
}

/// Map a log file path to its `LogSource` by walking the parent
/// directory. Falls back to `LogSource::Launcher` for paths under
/// our own `<instance>/logs/launch-*.log` captures; uses parent
/// directory name (`crash-reports` vs `logs`) for the MC roots.
pub fn infer_source_from_path(path: &Path) -> LogSource {
    let _ = path;
    LogSource::Game
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logs::files::LogSource;

    // Sentinel — non-empty test module so the file compiles cleanly.
    #[test]
    fn empty_patterns_array_returns_none() {
        assert!(match_log("anything at all", LogSource::Game).is_none());
        // This passes today because PATTERNS is empty; Task 3 will
        // populate it and this test will be deleted in Task 3 Step 1.
    }
}
