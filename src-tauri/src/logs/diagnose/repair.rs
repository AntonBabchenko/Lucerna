//! Auto-Repair: turns a diagnoser hit into a typed, confirmable fix.
//! Pure logic only — no I/O, no network. The command layer in
//! `commands.rs` orchestrates instance state + platform calls around
//! these helpers.

use std::collections::HashMap;

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
    DisableBlockingMods,
    /// A client-only mod crashed the dedicated server. Handled by the
    /// server-specific `server_remove_mods` command, not the instance repair
    /// pipeline — `build_repair_plan` returns `Ok(None)` for this variant.
    RemoveClientServerMods,
    /// A known mod bug has a curated third-party fix mod. `build_repair_plan`
    /// resolves the pinned version and emits `RepairPlan::InstallFixMod`.
    InstallFixMod,
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
        "client-extra-mods" => Some(RepairKind::DisableBlockingMods),
        "create-goggle-overlay-crash" => Some(RepairKind::InstallFixMod),
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

/// Suggest a new max-heap (MB) for an OOM: jump straight to the instance's
/// recommended safe maximum (~75% of physical RAM, fallback 8192 when RAM is
/// unknown). Returns `None` when the current heap is already at or above that
/// target — the fix is idempotent and never proposes a decrease.
pub fn suggest_heap_mb(current_mb: u32, total_ram_mb: Option<u64>) -> Option<u32> {
    let target = crate::instances::memory::recommended_max_mb(total_ram_mb);
    if target > current_mb {
        Some(target)
    } else {
        None
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
    DisableBlockingMods {
        mods: Vec<BlockingMod>,
    },
    InstallFixMod {
        /// Human title for the suggested mod.
        title: String,
        source: crate::mods::platform::ModSource,
        /// Project slug — builds the degraded "open project page" link.
        slug: String,
        /// The resolved version label (e.g. "1.0.3"), when `install` is `Some`.
        version_label: Option<String>,
        /// `Some` ⇒ one-click install; `None` ⇒ degraded (no compatible pinned
        /// version / no CF key / distribution disabled) ⇒ UI shows a project link.
        install: Option<VersionRef>,
    },
}

/// One mod the server rejected during the FML channel handshake. The user can
/// disable it (if the server simply lacks the mod) — but a `server side` reject
/// can also mean a version mismatch, which disabling won't fix; the card
/// surfaces both possibilities rather than prescribing disable.
#[derive(Debug, Clone, Serialize, Type)]
pub struct BlockingMod {
    /// The cited mod-id from the reject (e.g. `sophisticatedbackpacks`) — the
    /// clearest label, matching what the server named. Preferred over `name`,
    /// which is filename-derived and unreliable for unenriched mods.
    pub mod_id: String,
    pub sha1: String,
    pub name: String,
    /// Names of *kept* installed mods whose jar declares a mandatory dependency
    /// on this one (read via `local::read_jar_dependency_ids`). Disabling this
    /// would break them; a non-empty list is a warning, not a block.
    pub breaks: Vec<String>,
    /// Source + project id of the matched installed mod, when known. Present ⇒
    /// the card can list versions and offer "Replace version" (the common
    /// version-mismatch fix); both `None` (a manually-added jar) ⇒ guidance +
    /// Disable only. The reject log carries no target version, so the user picks
    /// it from the server's "Server has" column.
    pub source: Option<crate::mods::platform::ModSource>,
    pub project_id: Option<String>,
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
    /// Install the curated, pinned fix mod the user confirmed.
    InstallFixMod {
        source: crate::mods::platform::ModSource,
        project_id: String,
        version_id: String,
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

/// Map the `server side`-cited mod-ids to *installed* mods the user can disable
/// to join, and compute which kept mods each would break.
///
/// An id is matched to an installed mod by `cited_resolve::find_installed_for_cited`
/// (normalized name / project_id / filename token — a jar's internal mod-id is not
/// stored). Already-disabled mods are skipped; matches deduped by `sha1`; unmatched
/// ids dropped. `breaks` lists the *names* of mods that would lose a required
/// dependency if this one is disabled: enabled mods, not themselves in the blocking
/// set, whose **jar-declared** dependency mod-ids (`declared_deps`, keyed by
/// installed `sha1`) include this blocking mod-id. `declared_deps` is read from the
/// jars by the caller (`local::read_jar_dependency_ids`) — the recorded `requires`
/// registry is too sparse to be reliable. An empty map yields empty `breaks` (used
/// by the diagnose-time emptiness check, which only needs the list).
pub fn build_blocking_mods(
    cited_ids: &[String],
    installed: &[InstalledMod],
    declared_deps: &HashMap<String, Vec<String>>,
) -> Vec<BlockingMod> {
    // Resolve each cited (blocking mod-id, installed mod), enabled-only, deduped.
    let mut resolved: Vec<(&str, &InstalledMod)> = Vec::new();
    for id in cited_ids {
        if let Some(m) = crate::mods::cited_resolve::find_installed_for_cited(id, installed) {
            // A mod the user already disabled is no longer blocking the join.
            if !m.enabled {
                continue;
            }
            if resolved.iter().any(|(_, r)| r.sha1 == m.sha1) {
                continue; // dedup by installed identity
            }
            resolved.push((id.as_str(), m));
        }
    }
    let blocking_sha1s: Vec<&str> = resolved.iter().map(|(_, m)| m.sha1.as_str()).collect();

    resolved
        .iter()
        .map(|(blocking_modid, m)| {
            let breaks = installed
                .iter()
                .filter(|other| {
                    other.enabled
                        && !blocking_sha1s.contains(&other.sha1.as_str())
                        && declared_deps.get(&other.sha1).is_some_and(|deps| {
                            deps.iter()
                                .any(|dep| dep.eq_ignore_ascii_case(blocking_modid))
                        })
                })
                .map(|other| other.name.clone())
                .collect();
            BlockingMod {
                mod_id: blocking_modid.to_string(),
                sha1: m.sha1.clone(),
                name: m.name.clone(),
                breaks,
                source: m.source,
                project_id: m.project_id.clone(),
            }
        })
        .collect()
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

    fn inst(
        name: &str,
        sha1: &str,
        project: Option<&str>,
        enabled: bool,
        requires: &[&str],
    ) -> InstalledMod {
        InstalledMod {
            filename: format!("{}.jar", name.to_lowercase().replace([' ', '\''], "")),
            sha1: sha1.into(),
            source: project.map(|_| ModSource::Modrinth),
            project_id: project.map(|p| p.into()),
            version_id: project.map(|_| "verid".into()),
            name: name.into(),
            version_number: Some("1.0".into()),
            installed_at: "2026-01-01T00:00:00Z".into(),
            enabled,
            enrich_attempted: false,
            requires: requires.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn repair_kind_maps_client_extra_mods() {
        assert_eq!(
            repair_kind_for("client-extra-mods"),
            Some(RepairKind::DisableBlockingMods)
        );
    }

    #[test]
    fn build_blocking_maps_cited_ids_to_installed_and_drops_unmatched() {
        let installed = vec![
            inst("Alex's Mobs", "a", Some("alexsmobs"), true, &[]),
            inst("Citadel", "c", Some("citadel"), true, &[]),
            inst("Unrelated", "u", Some("other"), true, &[]),
        ];
        let cited = vec![
            "alexsmobs".to_string(),
            "citadel".to_string(),
            "ghostmod".to_string(), // not installed → dropped
        ];
        let blocking = build_blocking_mods(&cited, &installed, &HashMap::new());
        let names: Vec<&str> = blocking.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, vec!["Alex's Mobs", "Citadel"]);
        // mod_id carries the cited id (the card's display label).
        let ids: Vec<&str> = blocking.iter().map(|b| b.mod_id.as_str()).collect();
        assert_eq!(ids, vec!["alexsmobs", "citadel"]);
        assert!(blocking.iter().all(|b| b.breaks.is_empty()));
        // source + project_id are carried from the matched installed mod so the
        // card can offer "Replace version".
        assert_eq!(blocking[0].source, Some(ModSource::Modrinth));
        assert_eq!(blocking[0].project_id.as_deref(), Some("alexsmobs"));
    }

    #[test]
    fn build_blocking_carries_none_source_for_sourceless_mod() {
        // A manually-added jar (no source/project_id) yields None for both, so
        // the card falls back to guidance instead of offering a version swap.
        let installed = vec![inst("manualmod", "m", None, true, &[])];
        let blocking = build_blocking_mods(&["manualmod".to_string()], &installed, &HashMap::new());
        assert_eq!(blocking.len(), 1);
        assert_eq!(blocking[0].source, None);
        assert_eq!(blocking[0].project_id, None);
    }

    #[test]
    fn build_blocking_breaks_lists_kept_enabled_dependents_only() {
        let installed = vec![
            inst("Citadel", "c", Some("citadel"), true, &[]),
            inst("Alex's Mobs", "a", Some("alexsmobs"), true, &[]),
            inst("Old User", "o", Some("olduser"), false, &[]),
        ];
        // Both Alex's Mobs and Old User declare a jar dep on "citadel"; only the
        // enabled one (Alex's Mobs) is a live break.
        let deps = HashMap::from([
            ("a".to_string(), vec!["citadel".to_string()]),
            ("o".to_string(), vec!["citadel".to_string()]),
        ]);
        let blocking = build_blocking_mods(&["citadel".to_string()], &installed, &deps);
        assert_eq!(blocking.len(), 1);
        assert_eq!(blocking[0].name, "Citadel");
        assert_eq!(blocking[0].breaks, vec!["Alex's Mobs".to_string()]);
    }

    #[test]
    fn build_blocking_excludes_other_blocking_mods_from_breaks() {
        // Alex's Mobs declares a jar dep on Citadel, but since Alex's Mobs is
        // itself in the blocking set it is not counted as a break for Citadel.
        let installed = vec![
            inst("Alex's Mobs", "a", Some("alexsmobs"), true, &[]),
            inst("Citadel", "c", Some("citadel"), true, &[]),
        ];
        let deps = HashMap::from([("a".to_string(), vec!["citadel".to_string()])]);
        let blocking = build_blocking_mods(
            &["alexsmobs".to_string(), "citadel".to_string()],
            &installed,
            &deps,
        );
        let citadel = blocking.iter().find(|b| b.name == "Citadel").unwrap();
        assert!(
            citadel.breaks.is_empty(),
            "alexsmobs is also being disabled, so not a break: {:?}",
            citadel.breaks
        );
    }

    #[test]
    fn build_blocking_empty_when_no_ids_match() {
        let installed = vec![inst("Sodium", "s", Some("sodium"), true, &[])];
        assert!(
            build_blocking_mods(&["alexsmobs".to_string()], &installed, &HashMap::new()).is_empty()
        );
    }

    #[test]
    fn build_blocking_skips_already_disabled_mods() {
        // A blocking mod the user already disabled no longer blocks the join, so
        // it drops out — this is what lets the diagnosis self-suppress afterwards.
        let installed = vec![inst("Alex's Mobs", "a", Some("alexsmobs"), false, &[])];
        assert!(
            build_blocking_mods(&["alexsmobs".to_string()], &installed, &HashMap::new()).is_empty()
        );
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
    fn heap_jumps_straight_to_recommended_max() {
        // recommended_max_mb(16384) == 12288 (75%); from a small heap we jump there.
        assert_eq!(suggest_heap_mb(2048, Some(16384)), Some(12288));
        assert_eq!(suggest_heap_mb(6144, Some(16384)), Some(12288));
    }

    #[test]
    fn heap_idempotent_once_at_or_above_recommended() {
        // Already at the recommended max -> nothing to raise.
        assert_eq!(suggest_heap_mb(12288, Some(16384)), None);
        // Manually set even higher -> still nothing to raise (never decreases).
        assert_eq!(suggest_heap_mb(15000, Some(16384)), None);
    }

    #[test]
    fn heap_unknown_ram_uses_fallback_max() {
        // recommended_max_mb(None) == 8192 (fallback). Raise only if below it.
        assert_eq!(suggest_heap_mb(2048, None), Some(8192));
        assert_eq!(suggest_heap_mb(8192, None), None);
    }

    #[test]
    fn heap_tiny_ram_collapses_to_floor() {
        // recommended_max_mb(Some(512)) == 1024 (slider floor); current already there.
        assert_eq!(suggest_heap_mb(1024, Some(512)), None);
    }

    #[test]
    fn repair_kind_maps_goggle_overlay_to_install_fix_mod() {
        assert_eq!(
            repair_kind_for("create-goggle-overlay-crash"),
            Some(RepairKind::InstallFixMod)
        );
    }

    #[test]
    fn install_fix_mod_plan_serializes_with_kind_tag() {
        let plan = RepairPlan::InstallFixMod {
            title: "X".into(),
            source: crate::mods::platform::ModSource::Curseforge,
            slug: "x".into(),
            version_label: None,
            install: None,
        };
        let j = serde_json::to_string(&plan).unwrap();
        assert!(j.contains(r#""kind":"install_fix_mod""#), "got: {j}");
    }

    #[test]
    fn install_fix_mod_choice_deserializes() {
        let j = r#"{"kind":"install_fix_mod","source":"curseforge","project_id":"123","version_id":"456"}"#;
        let c: RepairChoice = serde_json::from_str(j).unwrap();
        match c {
            RepairChoice::InstallFixMod {
                source,
                project_id,
                version_id,
            } => {
                assert_eq!(source, crate::mods::platform::ModSource::Curseforge);
                assert_eq!(project_id, "123");
                assert_eq!(version_id, "456");
            }
            other => panic!("expected InstallFixMod, got {other:?}"),
        }
    }
}
