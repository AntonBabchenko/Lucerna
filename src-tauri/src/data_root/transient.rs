//! Entries the RUNNING launcher owns inside — or beside — the data root.
//!
//! A move must not treat them as user data. The bootstrap redirect and the
//! pending-cleanup note live at the OS-default dir (which is the data root
//! whenever no redirect exists). `webview/` is the WebView2 profile of this
//! very process on Windows release builds: its files are held open, so it can
//! be neither copied reliably nor deleted while we run. `logs/lucerna.log` is
//! kept open for append by `diag`, so its length may change mid-move.
//!
//! Every rule matches at its exact depth only — a user file that merely
//! shares a name deeper in the tree is ordinary data.

use std::path::Path;

pub const REDIRECT_FILE: &str = "data-location.json";
/// `redirect::write`'s atomic-write temp (`file.with_extension("tmp")`).
pub const REDIRECT_TMP_FILE: &str = "data-location.tmp";
pub const CLEANUP_NOTE_FILE: &str = "pending-cleanup.json";
/// The note's atomic-write temp (`path.with_extension("tmp")`).
pub const CLEANUP_NOTE_TMP_FILE: &str = "pending-cleanup.tmp";
pub const WEBVIEW_DIR: &str = "webview";
pub const APP_JSON: &str = "app.json";

const LIVE_LOG_DIR: &str = "logs";
const LIVE_LOG_FILE: &str = "lucerna.log";

/// Top-level names skipped by copy, verify, delete and the pre-move scans.
pub const SKIPPED_TOP_LEVEL: [&str; 5] = [
    REDIRECT_FILE,
    REDIRECT_TMP_FILE,
    CLEANUP_NOTE_FILE,
    CLEANUP_NOTE_TMP_FILE,
    WEBVIEW_DIR,
];

/// Top-level names a RESET target (the OS-default dir) may already hold
/// without being "non-empty": the skipped set plus launcher scratch.
pub const SAFE_OVERLAP: [&str; 7] = [
    REDIRECT_FILE,
    REDIRECT_TMP_FILE,
    CLEANUP_NOTE_FILE,
    CLEANUP_NOTE_TMP_FILE,
    WEBVIEW_DIR,
    "logs",
    "updates",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryRule {
    /// Copied, verified by length, deleted.
    Normal,
    /// Not copied, not verified, not deleted, not scanned.
    Skip,
    /// Copied and deleted, but verified for existence only.
    ExistenceOnly,
}

/// The rule for `rel`, a path RELATIVE to the data root.
pub fn classify(rel: &Path) -> EntryRule {
    let mut parts = rel.components();
    let Some(first) = parts.next() else {
        return EntryRule::Normal;
    };
    let first = first.as_os_str();
    match (parts.next(), parts.next()) {
        (None, _) if SKIPPED_TOP_LEVEL.iter().any(|name| first == *name) => EntryRule::Skip,
        (Some(second), None) if first == LIVE_LOG_DIR && second.as_os_str() == LIVE_LOG_FILE => {
            EntryRule::ExistenceOnly
        }
        _ => EntryRule::Normal,
    }
}

/// True when the top-level `name` is in the skipped set.
pub fn is_skipped_top_level(name: &std::ffi::OsStr) -> bool {
    classify(Path::new(name)) == EntryRule::Skip
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_level_transients_are_skipped() {
        for name in SKIPPED_TOP_LEVEL {
            assert_eq!(classify(Path::new(name)), EntryRule::Skip, "{name}");
        }
    }

    #[test]
    fn namesakes_deeper_in_the_tree_are_ordinary_data() {
        assert_eq!(
            classify(&Path::new("instances").join(WEBVIEW_DIR)),
            EntryRule::Normal
        );
        assert_eq!(
            classify(&Path::new("sub").join(REDIRECT_FILE)),
            EntryRule::Normal
        );
    }

    #[test]
    fn only_the_root_launcher_log_is_existence_only() {
        assert_eq!(
            classify(&Path::new("logs").join("lucerna.log")),
            EntryRule::ExistenceOnly
        );
        assert_eq!(
            classify(&Path::new("logs").join("lucerna.prev.log")),
            EntryRule::Normal
        );
        assert_eq!(classify(Path::new("logs")), EntryRule::Normal);
        assert_eq!(
            classify(
                &Path::new("instances")
                    .join("x")
                    .join("logs")
                    .join("lucerna.log")
            ),
            EntryRule::Normal
        );
    }

    #[test]
    fn app_json_and_everything_else_are_normal() {
        assert_eq!(classify(Path::new(APP_JSON)), EntryRule::Normal);
        assert_eq!(classify(Path::new("instances")), EntryRule::Normal);
        assert_eq!(classify(Path::new("")), EntryRule::Normal);
    }

    #[test]
    fn safe_overlap_covers_every_skipped_name() {
        for name in SKIPPED_TOP_LEVEL {
            assert!(SAFE_OVERLAP.contains(&name), "{name}");
        }
    }
}
