//! Cumulative changelog for an update: the pure window logic (which versions,
//! newest→oldest, lie between the installed one and the target) plus the IPC
//! types the `mods_changelog` command returns. No I/O here — platform fetching
//! lives in each `ModPlatform::changelog_range`. Mirrors the `updates.rs`
//! "pure logic + thin command" split.

use serde::Serialize;
use specta::Type;

/// Upper bound on versions in one cumulative window. Bounds the CurseForge
/// per-file changelog fan-out and keeps the modal readable. When the true
/// window is larger, the newest `MAX_CHANGELOG_VERSIONS` are kept and
/// `ChangelogResult::truncated` carries the full count.
pub const MAX_CHANGELOG_VERSIONS: usize = 20;

/// One version's changelog, ready to render. `body_html` is already sanitized
/// (Modrinth markdown → HTML, CurseForge HTML → sanitized); empty when the
/// author published no notes.
#[derive(Debug, Clone, Serialize, Type, PartialEq)]
pub struct ChangelogSection {
    pub version_id: String,
    pub version_number: String,
    pub published_at: Option<String>,
    pub body_html: String,
}

/// The full cumulative changelog for one update. `sections` are newest→oldest.
/// `truncated` is `Some(total)` when the window exceeded the cap.
#[derive(Debug, Clone, Serialize, Type, PartialEq)]
pub struct ChangelogResult {
    pub sections: Vec<ChangelogSection>,
    pub truncated: Option<u32>,
}

/// Compute the cumulative window over a newest-first list of version ids.
///
/// Returns `(start, end, full_len)`: the half-open index range `[start, end)`
/// to display (already capped to [`MAX_CHANGELOG_VERSIONS`]), and `full_len` =
/// the pre-cap window length (so the caller sets `truncated` when
/// `end - start < full_len`).
///
/// - The window covers `(base, target]`: every version strictly newer than the
///   installed one, up to and including the target.
/// - `base_id = None`, absent from the list, or not older than the target →
///   the window is just the target.
/// - `target_id` absent from the list → empty range `(0, 0, 0)`.
pub fn changelog_window(
    ids: &[&str],
    target_id: &str,
    base_id: Option<&str>,
) -> (usize, usize, usize) {
    let Some(t) = ids.iter().position(|&v| v == target_id) else {
        return (0, 0, 0);
    };
    let end_full = match base_id.and_then(|b| ids.iter().position(|&v| v == b)) {
        Some(b) if b > t => b,
        _ => t + 1,
    };
    let full_len = end_full - t;
    let end_capped = t + full_len.min(MAX_CHANGELOG_VERSIONS);
    (t, end_capped, full_len)
}

#[cfg(test)]
mod tests {
    use super::*;

    // newest → oldest, matching how the platforms return version lists.
    const LIST: &[&str] = &["v6", "v5", "v4", "v3", "v2", "v1"];

    #[test]
    fn window_covers_base_exclusive_to_target_inclusive() {
        // installed v3, target v6 → v6, v5, v4 (base v3 excluded).
        let (s, e, full) = changelog_window(LIST, "v6", Some("v3"));
        assert_eq!((s, e, full), (0, 3, 3));
    }

    #[test]
    fn window_is_just_target_when_base_is_none() {
        let (s, e, full) = changelog_window(LIST, "v6", None);
        assert_eq!((s, e, full), (0, 1, 1));
    }

    #[test]
    fn window_is_just_target_when_base_absent_from_list() {
        // base delisted / Unknown update state.
        let (s, e, full) = changelog_window(LIST, "v6", Some("v-gone"));
        assert_eq!((s, e, full), (0, 1, 1));
    }

    #[test]
    fn window_is_just_target_when_base_not_older_than_target() {
        // base newer than target shouldn't happen for updates; degrade to target.
        let (s, e, full) = changelog_window(LIST, "v3", Some("v6"));
        assert_eq!((s, e, full), (3, 4, 1));
    }

    #[test]
    fn empty_when_target_absent() {
        assert_eq!(changelog_window(LIST, "v-nope", Some("v1")), (0, 0, 0));
    }

    #[test]
    fn caps_window_and_reports_full_length() {
        // 25-long list, target newest, base oldest → full 24 (base exclusive),
        // capped to 20.
        let ids: Vec<String> = (0..25).rev().map(|i| format!("v{i}")).collect();
        let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        let (s, e, full) = changelog_window(&refs, "v24", Some("v0"));
        assert_eq!(s, 0);
        assert_eq!(e - s, MAX_CHANGELOG_VERSIONS);
        assert_eq!(full, 24);
    }
}
