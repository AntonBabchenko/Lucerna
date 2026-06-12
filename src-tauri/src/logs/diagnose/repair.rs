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
    InstallMissingMods,
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
        "server-missing-mods" => Some(RepairKind::InstallMissingMods),
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
    let marker = CORRUPT_MARKERS.iter().filter_map(|m| log.find(m)).min()?;
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

/// Floor for any heap suggestion (MB). Below this, modded MC reliably OOMs.
const HEAP_FLOOR_MB: u32 = 4096;

/// Suggest a new max-heap (MB) for an OOM, or `None` when no safe raise
/// is possible. With known RAM: `clamp(current*2, FLOOR, RAM/2)`, but
/// `None` if `current >= RAM/2` (no headroom). With unknown RAM: a
/// conservative fixed bump to `FLOOR`, only when `current < FLOOR`.
pub fn suggest_heap_mb(current_mb: u32, total_ram_mb: Option<u64>) -> Option<u32> {
    match total_ram_mb {
        Some(ram) => {
            let half = (ram / 2) as u32;
            if current_mb >= half {
                return None; // already at the safe ceiling
            }
            let doubled = current_mb.saturating_mul(2);
            let proposed = doubled.max(HEAP_FLOOR_MB).min(half);
            // min() against `half` can pull below current in pathological
            // cases; guard so we never propose a *decrease*.
            if proposed > current_mb {
                Some(proposed)
            } else {
                None
            }
        }
        None => {
            if current_mb < HEAP_FLOOR_MB {
                Some(HEAP_FLOOR_MB)
            } else {
                None
            }
        }
    }
}

use crate::instances::schema::LoaderKind;
use crate::mods::cited_resolve::ResolvedMod;
use crate::mods::platform::{InstalledMod, VersionRef};

/// The concrete, parameterised fix proposal returned by
/// `build_repair_plan`. Every variant carries everything the executor
/// needs — the executor itself does no resolution.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RepairPlan {
    RaiseHeap {
        from_mb: u32,
        to_mb: u32,
    },
    ReinstallLoader {
        loader: LoaderKind,
    },
    RedownloadMod {
        old_sha1: String,
        filename: String,
        target: VersionRef,
    },
    ResolveConflict {
        candidates: Vec<ConflictCandidate>,
    },
    InstallMissingMods {
        mods: Vec<ResolvedMod>,
    },
}

/// One side of a mod conflict the user can act on.
#[derive(Debug, Clone, Serialize, Type)]
pub struct ConflictCandidate {
    pub sha1: String,
    pub name: String,
    /// compat-check (PR #54) flagged this mod as mismatching the
    /// instance's loader/MC — a *hint*, not a decision.
    pub compat_flagged: bool,
    /// A compatible version to swap to, when one exists. Filled by the
    /// async enrichment step in the command; pure mapping leaves `None`.
    pub swap_target: Option<VersionRef>,
    /// Human label for the swap target (e.g. "1.4.0"); paired with
    /// `swap_target`.
    pub swap_version_label: Option<String>,
}

/// The user's confirmed choice, sent back to `execute_repair`.
#[derive(Debug, Clone, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RepairChoice {
    RaiseHeap {
        to_mb: u32,
    },
    ReinstallLoader,
    /// Re-fetch a mod: uninstall `old_sha1`, install `target`. Covers
    /// both corrupt-redownload (same version) and conflict-swap (new
    /// version).
    Reinstall {
        old_sha1: String,
        target: VersionRef,
    },
    DisableMod {
        sha1: String,
    },
}

/// Map the conflict-cited mod ids to *installed* mods. An id matches an
/// installed mod when it equals the mod's `project_id` (case-insensitive)
/// or, failing that, its `name` (case-insensitive). Unresolvable ids are
/// dropped. `flagged_sha1s` marks candidates that compat-check flagged.
pub fn build_conflict_candidates(
    named: &[String],
    installed: &[InstalledMod],
    flagged_sha1s: &[&str],
) -> Vec<ConflictCandidate> {
    let mut out = Vec::new();
    for id in named {
        let id_lc = id.to_lowercase();
        let hit = installed.iter().find(|m| {
            m.project_id
                .as_deref()
                .map(|p| p.eq_ignore_ascii_case(&id_lc))
                .unwrap_or(false)
                || m.name.eq_ignore_ascii_case(&id_lc)
        });
        if let Some(m) = hit {
            if out.iter().any(|c: &ConflictCandidate| c.sha1 == m.sha1) {
                continue; // dedupe by installed identity
            }
            out.push(ConflictCandidate {
                sha1: m.sha1.clone(),
                name: m.name.clone(),
                compat_flagged: flagged_sha1s.contains(&m.sha1.as_str()),
                swap_target: None,
                swap_version_label: None,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::platform::{InstalledMod, ModSource};

    fn fake_installed(name: &str, sha1: &str, project: Option<&str>) -> InstalledMod {
        InstalledMod {
            filename: format!("{name}.jar"),
            sha1: sha1.into(),
            source: project.map(|_| ModSource::Modrinth),
            project_id: project.map(|p| p.into()),
            version_id: project.map(|_| "verid".into()),
            name: name.into(),
            version_number: Some("1.0".into()),
            installed_at: "2026-01-01T00:00:00Z".into(),
            enabled: true,
            enrich_attempted: false,
            requires: vec![],
        }
    }

    #[test]
    fn conflict_candidates_maps_named_ids_to_installed_by_project_then_name() {
        let installed = vec![
            fake_installed("Sodium", "aaa", Some("sodium")),
            fake_installed("Old Lib", "bbb", Some("oldlib")),
            fake_installed("Unrelated", "ccc", Some("other")),
        ];
        let named = vec!["sodium".to_string(), "oldlib".to_string()];
        let flagged: &[&str] = &["bbb"];
        let cands = build_conflict_candidates(&named, &installed, flagged);
        assert_eq!(cands.len(), 2);
        assert_eq!(cands[0].sha1, "aaa");
        assert!(!cands[0].compat_flagged);
        assert_eq!(cands[1].sha1, "bbb");
        assert!(cands[1].compat_flagged);
        assert!(cands[0].swap_target.is_none());
    }

    #[test]
    fn conflict_candidates_drops_unresolvable_named_ids() {
        let installed = vec![fake_installed("Sodium", "aaa", Some("sodium"))];
        let named = vec!["sodium".to_string(), "ghostmod".to_string()];
        let cands = build_conflict_candidates(&named, &installed, &[]);
        assert_eq!(cands.len(), 1, "ghostmod is not installed → dropped");
        assert_eq!(cands[0].sha1, "aaa");
    }

    #[test]
    fn repair_kind_maps_actionable_patterns() {
        assert_eq!(
            repair_kind_for("out-of-memory"),
            Some(RepairKind::RaiseHeap)
        );
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
    fn repair_kind_maps_server_missing_mods() {
        assert_eq!(
            repair_kind_for("server-missing-mods"),
            Some(RepairKind::InstallMissingMods)
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

    #[test]
    fn install_missing_mods_plan_serializes_with_kind_tag() {
        let plan = RepairPlan::InstallMissingMods { mods: vec![] };
        let j = serde_json::to_string(&plan).unwrap();
        assert!(j.contains(r#""kind":"install_missing_mods""#), "got: {j}");
        assert!(j.contains(r#""mods":[]"#), "got: {j}");
    }

    #[test]
    fn heap_doubles_current_clamped_to_min_4096() {
        assert_eq!(suggest_heap_mb(2048, Some(16384)), Some(4096));
    }

    #[test]
    fn heap_doubles_above_floor_when_current_large() {
        assert_eq!(suggest_heap_mb(4096, Some(16384)), Some(8192));
    }

    #[test]
    fn heap_capped_at_half_ram() {
        assert_eq!(suggest_heap_mb(4096, Some(12288)), Some(6144));
    }

    #[test]
    fn heap_none_when_already_at_or_above_half_ram() {
        assert_eq!(suggest_heap_mb(8192, Some(16384)), None);
        assert_eq!(suggest_heap_mb(9000, Some(16384)), None);
    }

    #[test]
    fn heap_unknown_ram_conservative_bump_to_4096() {
        assert_eq!(suggest_heap_mb(2048, None), Some(4096));
        assert_eq!(suggest_heap_mb(4096, None), None);
        assert_eq!(suggest_heap_mb(6144, None), None);
    }
}
