//! Auto-Repair: turns a diagnoser hit into a typed, confirmable fix.
//! Pure logic only — no I/O, no network. The command layer in
//! `commands.rs` orchestrates instance state + platform calls around
//! these helpers.

use serde::{Deserialize, Serialize};
use specta::Type;

/// Static tag attached to a `Diagnosis` so the UI knows whether to
/// offer a Fix button. Membership in the actionable set is the ONLY
/// input — no instance I/O. Real precondition gating happens later in
/// `build_repair_plan`.
#[derive(Debug, Clone, Copy, Serialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RepairKind {
    RaiseHeap,
    ReinstallLoader,
    RedownloadMod,
    ResolveConflict,
}

/// Map a diagnoser `pattern_id` to its repair kind, or `None` for the
/// advisory-only patterns (`java-version-too-old`, `port-already-in-use`,
/// `disk-full`).
pub fn repair_kind_for(pattern_id: &str) -> Option<RepairKind> {
    match pattern_id {
        "out-of-memory" => Some(RepairKind::RaiseHeap),
        "fabric-loader-missing-main" => Some(RepairKind::ReinstallLoader),
        "corrupt-mod-jar" => Some(RepairKind::RedownloadMod),
        "mod-resolution-conflict" => Some(RepairKind::ResolveConflict),
        _ => None,
    }
}

use once_cell::sync::Lazy;
use regex::Regex;

/// Any `*.jar` path/filename token. Captures the whole token incl. an
/// optional leading path; the caller reduces to the basename.
static JAR_TOKEN_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"[^\s\/]*\.jar").expect("jar-token regex compiles"));

/// Markers that indicate a corrupt-jar failure; we search for a jar
/// token within a window starting at the earliest marker.
const CORRUPT_MARKERS: &[&str] = &["Invalid or corrupt jarfile", "java.util.zip.ZipException"];

/// Best-effort extraction of the corrupt jar's *basename* from a log.
/// Strategy: find the earliest corrupt-marker offset, then return the
/// nearest `*.jar` token within a ±600-char window (preferring a token
/// after the marker, falling back to one before). `None` when no jar
/// token is present near a marker.
pub fn extract_corrupt_jar(log: &str) -> Option<String> {
    let marker = CORRUPT_MARKERS
        .iter()
        .filter_map(|m| log.find(m))
        .min()?;
    let win_start = marker.saturating_sub(600);
    let win_end = (marker + 600).min(log.len());
    // Snap to char boundaries before slicing (log is &str / UTF-8).
    let win_start = floor_char_boundary(log, win_start);
    let win_end = ceil_char_boundary(log, win_end);
    let window = &log[win_start..win_end];

    // Prefer the jar token closest to the marker. Compute marker offset
    // relative to the window, then pick the match with the smallest
    // distance from it.
    let marker_in_win = marker - win_start;
    JAR_TOKEN_RE
        .find_iter(window)
        .min_by_key(|m| {
            let mid = (m.start() + m.end()) / 2;
            mid.abs_diff(marker_in_win)
        })
        .map(|m| basename(m.as_str()).to_string())
        .filter(|s| !s.is_empty() && s != ".jar")
}

fn basename(token: &str) -> &str {
    token.rsplit(['/', '\\']).next().unwrap_or(token)
}

fn floor_char_boundary(s: &str, mut i: usize) -> usize {
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn ceil_char_boundary(s: &str, mut i: usize) -> usize {
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

/// Captures the mod id inside `Mod '<Name>' (<id>)`.
static MOD_NAMED_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"Mod '[^']+' \(([^)]+)\)").expect("mod-named regex compiles"));

/// Extract the distinct mod ids cited in a Fabric `ModResolutionException`,
/// in first-seen order. May return 0, 1, or 2+. The plan builder maps these
/// ids back to *installed* mods and drops any that don't resolve.
pub fn extract_conflict_mods(log: &str) -> Vec<String> {
    let mut seen = Vec::new();
    for caps in MOD_NAMED_RE.captures_iter(log) {
        let id = caps[1].trim().to_string();
        if !id.is_empty() && !seen.contains(&id) {
            seen.push(id);
        }
    }
    seen
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repair_kind_maps_actionable_patterns() {
        assert_eq!(repair_kind_for("out-of-memory"), Some(RepairKind::RaiseHeap));
        assert_eq!(
            repair_kind_for("fabric-loader-missing-main"),
            Some(RepairKind::ReinstallLoader)
        );
        assert_eq!(
            repair_kind_for("corrupt-mod-jar"),
            Some(RepairKind::RedownloadMod)
        );
        assert_eq!(
            repair_kind_for("mod-resolution-conflict"),
            Some(RepairKind::ResolveConflict)
        );
    }

    #[test]
    fn repair_kind_none_for_advisory_patterns() {
        assert_eq!(repair_kind_for("java-version-too-old"), None);
        assert_eq!(repair_kind_for("port-already-in-use"), None);
        assert_eq!(repair_kind_for("disk-full"), None);
        assert_eq!(repair_kind_for("nonexistent"), None);
    }

    #[test]
    fn corrupt_jar_from_invalid_jarfile_line() {
        let log = "Error: Invalid or corrupt jarfile C:\\Users\\x\\mods\\sodium-fabric-0.5.3.jar";
        assert_eq!(
            extract_corrupt_jar(log).as_deref(),
            Some("sodium-fabric-0.5.3.jar")
        );
    }

    #[test]
    fn corrupt_jar_from_zip_exception_window() {
        let log = "[ERROR] Failed to load mod file mods/oldlib-1.2.jar\n\
                   Caused by: java.util.zip.ZipException: zip END header not found";
        assert_eq!(extract_corrupt_jar(log).as_deref(), Some("oldlib-1.2.jar"));
    }

    #[test]
    fn corrupt_jar_none_when_no_jar_token() {
        let log = "Caused by: java.util.zip.ZipException: zip END header not found";
        assert_eq!(extract_corrupt_jar(log), None);
    }

    #[test]
    fn corrupt_jar_basename_only_strips_path() {
        let log = "Invalid or corrupt jarfile /home/u/.local/mods/fabric-api-0.92.jar";
        assert_eq!(
            extract_corrupt_jar(log).as_deref(),
            Some("fabric-api-0.92.jar")
        );
    }

    #[test]
    fn conflict_mods_captures_named_mods() {
        let log = "net.fabricmc.loader.impl.discovery.ModResolutionException: \
                   Mod resolution encountered an incompatible mod set!\n\
                   - Mod 'Sodium' (sodium) 0.5.3 requires version 1.20.1 of fabricloader\n\
                   - Mod 'Old Lib' (oldlib) 1.2 is incompatible";
        assert_eq!(
            extract_conflict_mods(log),
            vec!["sodium".to_string(), "oldlib".to_string()]
        );
    }

    #[test]
    fn conflict_mods_dedupes_repeated_ids() {
        let log = "- Mod 'Sodium' (sodium) 0.5.3 requires X\n\
                   - Mod 'Sodium' (sodium) 0.5.3 also conflicts with Y";
        assert_eq!(extract_conflict_mods(log), vec!["sodium".to_string()]);
    }

    #[test]
    fn conflict_mods_empty_when_no_mod_lines() {
        let log = "net.fabricmc.loader.impl.discovery.ModResolutionException: something";
        assert!(extract_conflict_mods(log).is_empty());
    }
}