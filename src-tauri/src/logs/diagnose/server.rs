//! Pattern matchers for a Minecraft *dedicated server* log. Distinct from the
//! client diagnoser in `engine.rs` — server failures (client-only mod on a
//! dedicated server, port in use, EULA) need their own signatures. Pure: no
//! I/O. The command layer reads the log and calls `diagnose_server_log`.

use crate::logs::diagnose::repair::RepairKind;
use crate::logs::diagnose::Diagnosis;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;
use specta::Type;

/// Full diagnosis result for a server log. Returned by the `server_diagnose`
/// Tauri command and consumed directly by the UI.
#[derive(Debug, Clone, Serialize, Type)]
pub struct ServerDiagnosis {
    pub status: crate::logs::diagnose::DiagnosisStatus,
    pub diagnosis: Option<Diagnosis>,
    pub client_mods: Vec<ClientModFinding>,
    pub forge_skip_count: Option<u32>,
    pub log_signature: Option<String>,
    /// Which one-click server fix the banner should offer (Phase 1+). `None`
    /// for advisory-only or clean diagnoses.
    pub server_repair: Option<ServerRepairTag>,
    /// The busy port for port-conflict diagnoses (drives "Use port N").
    pub port_in_use: Option<u16>,
    /// The leftover PID for `StopOrphanAndRetry`.
    pub orphan_pid: Option<u32>,
    /// RedownloadServerJar / corrupt-mod: the offending jar basename.
    pub corrupt_jar: Option<String>,
    /// RaiseHeap (the proposed new max) / LowerHeap (the safe max to drop to).
    pub suggested_heap_mb: Option<u32>,
    /// DisableMods / missing-dep: the cited mod ids or filenames the user acts on.
    pub conflict_mods: Vec<String>,
}

/// One-click server fix the diagnosis banner can offer. snake_case on the wire.
/// Phase 1 introduces the first four; later phases extend this enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ServerRepairTag {
    // Phase 1:
    AcceptEula,
    StopOrphanAndRetry,
    ChangePort,
    RemoveClientMods,
    // Phase 2:
    RaiseHeap,
    LowerHeap,
    RedownloadServerJar,
    DisableMods,
    InstallMissingDep,
}

/// True iff the dedicated server finished starting — Minecraft prints
/// `Done (X.XXXs)! For help, type "help"` once the world is up. A mod-loading
/// failure aborts before this line, so anything that reached it loaded fine.
fn server_started(log: &str) -> bool {
    log.contains("! For help, type")
}

/// First matching server-log diagnosis, if any. Order = specificity.
pub fn diagnose_server_log(log: &str) -> Option<Diagnosis> {
    // A client-only class load is logged as a NON-FATAL `RuntimeDistCleaner ...
    // invalid dist DEDICATED_SERVER` warning on many working modpack servers —
    // Forge refuses the class and the mod copes. It is only a crash when the
    // server failed to finish loading; if it reached "Done … For help", the
    // warning was harmless, so don't report a crash. (A real client-mod crash
    // aborts loading and never reaches that line; the crash-report path, which
    // has no success line, is unaffected.)
    if !server_started(log)
        && (log.contains("invalid dist DEDICATED_SERVER") || log.contains("RuntimeDistCleaner"))
    {
        return Some(Diagnosis {
            pattern_id: "server-client-only-mod-crash".into(),
            title: "A client-only mod crashed the server".into(),
            explanation: "A mod meant for the game client was loaded on the dedicated server and tried to load client-only code.".into(),
            recommendation: "Remove the client-only mods from this server, then start it again.".into(),
            matched_excerpt: excerpt(log, "invalid dist DEDICATED_SERVER"),
            repair: Some(RepairKind::RemoveClientServerMods),
        });
    }
    if log.contains("FAILED TO BIND TO PORT") || log.contains("Address already in use") {
        return Some(Diagnosis {
            pattern_id: "server-port-in-use".into(),
            title: "The server port is already in use".into(),
            explanation:
                "Another program (or another copy of this server) is already using the port.".into(),
            recommendation: "Stop the other server, or change the port in Settings.".into(),
            matched_excerpt: excerpt(log, "BIND TO PORT"),
            repair: None,
        });
    }
    if log.contains("agree to the EULA") {
        return Some(Diagnosis {
            pattern_id: "server-eula-not-accepted".into(),
            title: "The Minecraft EULA is not accepted".into(),
            explanation: "The server will not run until the Minecraft EULA is accepted.".into(),
            recommendation: "Recreate the server with the EULA checkbox ticked.".into(),
            matched_excerpt: excerpt(log, "EULA"),
            repair: None,
        });
    }
    // B6: heap larger than free RAM. Very specific; must precede the broader OOM.
    if log.contains("Could not reserve enough space for object heap")
        || log.contains("Invalid maximum heap size")
    {
        return Some(Diagnosis {
            pattern_id: "server-heap-too-big".into(),
            title: "The server's memory setting is larger than the computer has".into(),
            explanation:
                "Java could not start because the maximum heap is bigger than the free RAM on this machine."
                    .into(),
            recommendation: "Lower the server's memory, then start it again.".into(),
            matched_excerpt: excerpt(log, "object heap"),
            repair: None,
        });
    }
    // B4: server ran out of memory.
    if log.contains("OutOfMemoryError: Java heap space") {
        return Some(Diagnosis {
            pattern_id: "server-out-of-memory".into(),
            title: "The server ran out of memory".into(),
            explanation:
                "The memory allotted to the server was not enough — common for heavy modpacks or many loaded chunks."
                    .into(),
            recommendation: "Raise the server's memory, then start it again.".into(),
            matched_excerpt: excerpt(log, "OutOfMemoryError"),
            repair: None,
        });
    }
    // B5: java too old (UnsupportedClassVersionError). High-signal; advisory.
    if crate::logs::diagnose::patterns::detect_java_version_too_old(log) {
        return Some(Diagnosis {
            pattern_id: "server-java-too-old".into(),
            title: "Java is too old for this server".into(),
            explanation:
                "A mod or the loader was built for a newer Java than the one starting this server."
                    .into(),
            recommendation:
                "Use a newer Minecraft version (which installs a newer Java), or update the mod that needs it."
                    .into(),
            matched_excerpt: excerpt(log, "UnsupportedClassVersionError"),
            repair: None,
        });
    }
    // B13: session.lock — the in-log face of an orphaned server still holding the
    // world (Phase 1 Bug A). Maps to StopOrphanAndRetry in the command layer.
    if log.contains("Failed to check session lock") {
        return Some(Diagnosis {
            pattern_id: "server-session-lock".into(),
            title: "The world is still locked by another process".into(),
            explanation:
                "A leftover copy of this server is still holding the world's session lock, so a new copy can't open it."
                    .into(),
            recommendation: "Stop the leftover process, then start the server again.".into(),
            matched_excerpt: excerpt(log, "session lock"),
            repair: None,
        });
    }
    // B12: world corruption — ADVISORY ONLY (data-loss risk). No fix button.
    if log.contains("Failed to load world")
        || log.contains("corrupt chunk")
        || (log.contains("Chunk file at")
            && (log.contains("is missing") || log.contains("corrupt")))
    {
        return Some(Diagnosis {
            pattern_id: "server-world-corrupt".into(),
            title: "The world data looks corrupted".into(),
            explanation:
                "The server could not load the world — a region or chunk file appears damaged."
                    .into(),
            recommendation:
                "Restore this world from a backup. Don't let the launcher overwrite it automatically — that risks losing data."
                    .into(),
            matched_excerpt: excerpt(log, "world"),
            repair: None,
        });
    }
    // B9: Forge mod-loading screen "Missing or unsupported mandatory dependencies".
    if log.contains("Missing or unsupported mandatory dependencies") {
        return Some(Diagnosis {
            pattern_id: "server-missing-dep".into(),
            title: "A mod needs another mod you don't have".into(),
            explanation:
                "One of the server's mods requires a dependency mod that isn't installed (or is the wrong version)."
                    .into(),
            recommendation:
                "Install the missing dependency below, or remove the mod that needs it, then start the server again."
                    .into(),
            matched_excerpt: excerpt(log, "mandatory dependencies"),
            repair: None,
        });
    }
    // B10: Fabric mod requires the Fabric API, which is missing. Declared ABOVE
    // the general ModResolutionException (B8) so it wins the first-match scan.
    if log.contains("requires version") && log.contains("of fabric") && log.contains("missing") {
        return Some(Diagnosis {
            pattern_id: "server-fabric-api-missing".into(),
            title: "The Fabric API mod is missing".into(),
            explanation:
                "A mod on this server needs the Fabric API library mod, which isn't installed."
                    .into(),
            recommendation: "Install the Fabric API, then start the server again.".into(),
            matched_excerpt: excerpt(log, "of fabric"),
            repair: None,
        });
    }
    // B8: Fabric/Quilt mod conflict. extract_conflict_mods reads the cited ids.
    if log.contains("net.fabricmc.loader.impl.discovery.ModResolutionException") {
        return Some(Diagnosis {
            pattern_id: "server-mod-conflict".into(),
            title: "Two mods conflict with each other".into(),
            explanation:
                "The loader stopped because two installed mods can't run together — one requires a version another forbids."
                    .into(),
            recommendation:
                "Disable one of the conflicting mods below, then start the server again.".into(),
            matched_excerpt: excerpt(log, "ModResolutionException"),
            repair: None,
        });
    }
    // B7: corrupt server.jar / mod jar. extract_corrupt_jar names the file.
    if log.contains("Invalid or corrupt jarfile") || log.contains("java.util.zip.ZipException") {
        return Some(Diagnosis {
            pattern_id: "server-corrupt-jar".into(),
            title: "A server file is corrupt".into(),
            explanation:
                "The server tried to read a jar and found it damaged or incomplete — usually an interrupted download."
                    .into(),
            recommendation:
                "Re-download the damaged file (the server jar, or the named mod), then start the server again."
                    .into(),
            matched_excerpt: excerpt(log, "corrupt"),
            repair: None,
        });
    }
    // B11: a mod crashed at mixin init. Medium confidence — dist_crash_tokens
    // hints at the owner mod; the command layer offers a disable checklist.
    if log.contains("Mixin apply failed") || log.contains("mixin.MixinApplyError") {
        return Some(Diagnosis {
            pattern_id: "server-mixin-crash".into(),
            title: "A mod crashed while starting up".into(),
            explanation:
                "A mod failed during its early startup (a mixin apply error). The crash names the mod responsible."
                    .into(),
            recommendation: "Disable the mod named below, then start the server again.".into(),
            matched_excerpt: excerpt(log, "Mixin apply failed"),
            repair: None,
        });
    }
    // B16: wrong-loader mod. Low-confidence and broad; declared LAST so more
    // specific patterns win.
    if log.contains("is not a valid mod file") {
        return Some(Diagnosis {
            pattern_id: "server-wrong-loader-mod".into(),
            title: "A mod is for a different mod loader".into(),
            explanation:
                "A file in the server's mods folder is not a valid mod for this server's loader — usually a mod for a different loader (e.g. a Fabric mod on a Forge server)."
                    .into(),
            recommendation:
                "Remove the mods that don't match this server's loader, then start it again.".into(),
            matched_excerpt: excerpt(log, "is not a valid mod file"),
            repair: None,
        });
    }
    None
}

/// Map a server `pattern_id` to the one-click fix the banner should offer, or
/// `None` for advisory-only patterns (world corruption, java-too-old, wrong
/// loader). The command layer calls this to attach `server_repair`.
pub fn server_repair_for(pattern_id: &str) -> Option<ServerRepairTag> {
    match pattern_id {
        "server-out-of-memory" => Some(ServerRepairTag::RaiseHeap),
        "server-heap-too-big" => Some(ServerRepairTag::LowerHeap),
        "server-corrupt-jar" => Some(ServerRepairTag::RedownloadServerJar),
        "server-mod-conflict" | "server-mixin-crash" => Some(ServerRepairTag::DisableMods),
        "server-missing-dep" | "server-fabric-api-missing" => {
            Some(ServerRepairTag::InstallMissingDep)
        }
        "server-session-lock" => Some(ServerRepairTag::StopOrphanAndRetry),
        "server-client-only-mod-crash" => Some(ServerRepairTag::RemoveClientMods),
        // Phase 1 also maps these via preflight; the log faces map here too:
        "server-eula-not-accepted" => Some(ServerRepairTag::AcceptEula),
        "server-port-in-use" => Some(ServerRepairTag::ChangePort),
        _ => None,
    }
}

/// Cheap status-only classification for the sidebar "needs a fix" badge: derive
/// a `DiagnosisStatus` from a server's latest log WITHOUT reading mod jars or the
/// network, so `server_list` can flag a stopped server that needs a one-click
/// fix. Mirrors `server_diagnose`'s status logic: classify the log, then upgrade
/// Advisory→Actionable only when a real one-click fix is available. The two
/// param-dependent tags (stop-orphan, install-missing-dep) count only when their
/// target actually exists, so we never badge a no-op fix. `handled` (the acted-on
/// log signature) suppresses an already-fixed log; `has_live_orphan` is whether a
/// leftover owned JVM is alive (the caller checks the persisted PID).
pub fn classify_server_status(
    log: &str,
    handled: Option<&str>,
    has_live_orphan: bool,
) -> crate::logs::diagnose::DiagnosisStatus {
    use crate::logs::diagnose::DiagnosisStatus;
    let Some(d) = diagnose_server_log(log) else {
        return DiagnosisStatus::None;
    };
    let signature = crate::logs::diagnose::log_signature(log);
    let status = crate::logs::diagnose::classify_status(&d, 0, None, &signature, handled);
    // None / Handled / already-Actionable are authoritative; only a server
    // Advisory (server fixes aren't in Diagnosis.repair) may upgrade.
    if status != DiagnosisStatus::Advisory {
        return status;
    }
    let has_fix = match server_repair_for(&d.pattern_id) {
        Some(ServerRepairTag::StopOrphanAndRetry) => has_live_orphan,
        Some(ServerRepairTag::InstallMissingDep) => !extract_missing_dep_ids(log).is_empty(),
        Some(_) => true,
        None => false,
    };
    if has_fix {
        DiagnosisStatus::Actionable
    } else {
        DiagnosisStatus::Advisory
    }
}

/// Choose which text to diagnose: prefer the crash report when it yields a
/// diagnosis, otherwise the server log. Pure — the command layer reads the two
/// files and passes their contents. Returns a borrow of the chosen input.
pub fn pick_diagnosable<'a>(server_log: &'a str, crash: Option<&'a str>) -> &'a str {
    match crash {
        Some(c) if diagnose_server_log(c).is_some() => c,
        _ => server_log,
    }
}

static MISSING_DEP_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"Mod ID: '([^']+)'").expect("missing-dep re"));

/// Extract the dependency mod-ids the server says are missing. Forge cites them
/// as `Mod ID: '<id>'` in the mandatory-dependencies screen; the Fabric-API case
/// yields the single id `fabric-api`. Deduped, first-seen order.
pub fn extract_missing_dep_ids(log: &str) -> Vec<String> {
    if log.contains("of fabric") && log.contains("missing") {
        return vec!["fabric-api".to_string()];
    }
    let mut out: Vec<String> = Vec::new();
    for c in MISSING_DEP_RE.captures_iter(log) {
        if let Some(id) = c.get(1).map(|m| m.as_str().to_string()) {
            if !out.contains(&id) {
                out.push(id);
            }
        }
    }
    out
}

static SKIP_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"returned (\d+) files which are client-side-only mods").expect("skip re")
});
/// Count of client-only mods Forge auto-skipped (informational for the UI).
pub fn forge_client_skip_count(log: &str) -> Option<u32> {
    SKIP_RE
        .captures(log)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse().ok())
}

static MIXIN_RE: Lazy<Regex> = Lazy::new(|| {
    // Mixin handler methods are named `handler$<hash>$<owner>$<name>`; the owner
    // token is a strong hint at the offending mod (e.g. "etf").
    Regex::new(r"handler\$[a-z0-9]+\$([a-z0-9_]+)\$").expect("mixin re")
});
/// Best-effort owner tokens from `invalid dist` mixin frames (deduped, ≥2 chars).
pub fn dist_crash_tokens(log: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in MIXIN_RE.captures_iter(log) {
        if let Some(t) = c.get(1).map(|m| m.as_str().to_string()) {
            if t.len() >= 2 && !out.contains(&t) {
                out.push(t);
            }
        }
    }
    out
}
// NOTE: dist_crash_tokens retains the ≥2 filter so that very short tokens still
// pass through for the `leading_initials` exact-match path (e.g. "etf" = 3 chars
// is fine either way). The substring floor is raised to 3 in filename_matches_token.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    High,
    Medium,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ClientModFinding {
    pub filename: String,
    /// i18n reason key: "manifest_client" | "crash".
    pub reason: String,
    pub confidence: Confidence,
}

/// Combine per-jar declared environment + crash owner tokens into findings.
/// High = manifest says client; Medium = matched a crash owner token. A jar
/// matching both is High (reason manifest_client). `Both`/`Server`/`Unknown`
/// jars with no token match are NOT flagged.
pub fn classify_client_only_mods(
    mods: &[(String, crate::mods::local::ModEnvironment)],
    crash_tokens: &[String],
) -> Vec<ClientModFinding> {
    use crate::mods::local::ModEnvironment;
    let mut out: Vec<ClientModFinding> = Vec::new();
    for (filename, env) in mods {
        if *env == ModEnvironment::Client {
            out.push(ClientModFinding {
                filename: filename.clone(),
                reason: "manifest_client".into(),
                confidence: Confidence::High,
            });
        } else if *env != ModEnvironment::Server
            && crash_tokens
                .iter()
                .any(|t| filename_matches_token(filename, t))
        {
            out.push(ClientModFinding {
                filename: filename.clone(),
                reason: "crash".into(),
                confidence: Confidence::Medium,
            });
        }
    }
    out
}

/// Does `filename` plausibly correspond to crash owner `token`?
///
/// Exact `leading_initials` match always wins (covers 3-letter acronyms like
/// "etf"). Substring match requires ≥ 3 normalized chars to avoid false
/// positives from very short 2-char tokens that appear in many jar names.
fn filename_matches_token(filename: &str, token: &str) -> bool {
    let t = normalize(token);
    if leading_initials(filename) == t {
        return true;
    }
    t.len() >= 3 && normalize(filename).contains(&t)
}

fn normalize(s: &str) -> String {
    s.to_ascii_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

/// First letter of each leading alphabetic word, stopping at the first
/// all-digit word. `entity_texture_features_1.20.1-…` → "etf".
fn leading_initials(filename: &str) -> String {
    let mut out = String::new();
    for word in filename.split(|c: char| !c.is_ascii_alphanumeric()) {
        if word.is_empty() {
            continue;
        }
        if word.chars().all(|c| c.is_ascii_digit()) {
            break;
        }
        if let Some(c) = word.chars().next() {
            out.push(c.to_ascii_lowercase());
        }
    }
    out
}

/// One log line containing `needle` (trimmed, capped at 200 chars).
fn excerpt(log: &str, needle: &str) -> String {
    log.lines()
        .find(|l| l.contains(needle))
        .map(|l| {
            let t = l.trim();
            if t.chars().count() > 200 {
                t.chars().take(200).collect()
            } else {
                t.to_string()
            }
        })
        .unwrap_or_default()
}

/// Build a fixable `ServerDiagnosis` from a pre-spawn finding (class A). Copy
/// is plain; the banner localizes by `pattern_id`. `LowDisk` is advisory.
pub fn diagnosis_from_preflight(
    finding: crate::servers_runtime::preflight::PreflightFinding,
) -> ServerDiagnosis {
    use crate::logs::diagnose::{Diagnosis, DiagnosisStatus};
    use crate::servers_runtime::preflight::PreflightFinding;
    let (pattern_id, title, repair, port, orphan, status) = match finding {
        PreflightFinding::EulaNotAccepted => (
            "server-eula-not-accepted",
            "The Minecraft EULA is not accepted",
            Some(ServerRepairTag::AcceptEula),
            None,
            None,
            DiagnosisStatus::Actionable,
        ),
        PreflightFinding::PortInUse(p) => (
            "server-port-in-use",
            "The server port is already in use",
            Some(ServerRepairTag::ChangePort),
            Some(p),
            None,
            DiagnosisStatus::Actionable,
        ),
        PreflightFinding::OrphanRunning(pid) => (
            "server-orphan-running",
            "A leftover copy of this server is still running",
            Some(ServerRepairTag::StopOrphanAndRetry),
            None,
            Some(pid),
            DiagnosisStatus::Actionable,
        ),
        PreflightFinding::LowDisk => (
            "server-disk-low",
            "Low disk space",
            None,
            None,
            None,
            DiagnosisStatus::Advisory,
        ),
    };
    ServerDiagnosis {
        status,
        diagnosis: Some(Diagnosis {
            pattern_id: pattern_id.into(),
            title: title.into(),
            explanation: String::new(),
            recommendation: String::new(),
            matched_excerpt: String::new(),
            repair: None,
        }),
        client_mods: vec![],
        forge_skip_count: None,
        log_signature: None,
        server_repair: repair,
        port_in_use: port,
        orphan_pid: orphan,
        corrupt_jar: None,
        suggested_heap_mb: None,
        conflict_mods: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::local::ModEnvironment;

    #[test]
    fn server_repair_tag_serializes_snake_case() {
        let j = serde_json::to_string(&ServerRepairTag::StopOrphanAndRetry).unwrap();
        assert_eq!(j, "\"stop_orphan_and_retry\"");
    }

    #[test]
    fn server_diagnosis_carries_repair_and_params() {
        let d = ServerDiagnosis {
            status: crate::logs::diagnose::DiagnosisStatus::Actionable,
            diagnosis: None,
            client_mods: vec![],
            forge_skip_count: None,
            log_signature: None,
            server_repair: Some(ServerRepairTag::ChangePort),
            port_in_use: Some(25565),
            orphan_pid: None,
            corrupt_jar: None,
            suggested_heap_mb: None,
            conflict_mods: vec![],
        };
        let j = serde_json::to_string(&d).unwrap();
        assert!(j.contains("\"server_repair\":\"change_port\""), "got: {j}");
        assert!(j.contains("\"port_in_use\":25565"), "got: {j}");
    }

    #[test]
    fn preflight_to_diagnosis_eula_is_actionable() {
        use crate::servers_runtime::preflight::PreflightFinding;
        let d = diagnosis_from_preflight(PreflightFinding::EulaNotAccepted);
        assert_eq!(d.server_repair, Some(ServerRepairTag::AcceptEula));
        assert_eq!(d.status, crate::logs::diagnose::DiagnosisStatus::Actionable);
        assert!(d.diagnosis.is_some());
    }

    #[test]
    fn preflight_to_diagnosis_orphan_carries_pid() {
        use crate::servers_runtime::preflight::PreflightFinding;
        let d = diagnosis_from_preflight(PreflightFinding::OrphanRunning(9999));
        assert_eq!(d.server_repair, Some(ServerRepairTag::StopOrphanAndRetry));
        assert_eq!(d.orphan_pid, Some(9999));
    }

    #[test]
    fn classify_server_status_none_for_clean_or_empty_log() {
        use crate::logs::diagnose::DiagnosisStatus;
        // No diagnosable problem → no badge.
        assert_eq!(
            classify_server_status("", None, false),
            DiagnosisStatus::None
        );
        assert_eq!(
            classify_server_status("[Server] Done (1.2s)! For help, type \"help\"", None, false),
            DiagnosisStatus::None
        );
    }

    #[test]
    fn classify_server_status_upgrades_fixable_and_suppresses_handled() {
        use crate::logs::diagnose::DiagnosisStatus;
        // OOM carries a one-click RaiseHeap fix → Actionable.
        let oom = "java.lang.OutOfMemoryError: Java heap space\n\tat x.y(Z.java:1)";
        assert_eq!(
            classify_server_status(oom, None, false),
            DiagnosisStatus::Actionable
        );
        // Same log, already acted on (matching signature) → Handled (no badge).
        let sig = crate::logs::diagnose::log_signature(oom);
        assert_eq!(
            classify_server_status(oom, Some(&sig), false),
            DiagnosisStatus::Handled
        );
        // Session-lock with no live orphan → Advisory (don't badge a no-op fix).
        let lock = "Failed to check session lock for this world";
        assert_eq!(
            classify_server_status(lock, None, false),
            DiagnosisStatus::Advisory
        );
        // World corruption has no one-click fix → Advisory.
        assert_eq!(
            classify_server_status("Failed to load world", None, false),
            DiagnosisStatus::Advisory
        );
    }

    #[test]
    fn preflight_to_diagnosis_port_carries_port() {
        use crate::servers_runtime::preflight::PreflightFinding;
        let d = diagnosis_from_preflight(PreflightFinding::PortInUse(25565));
        assert_eq!(d.server_repair, Some(ServerRepairTag::ChangePort));
        assert_eq!(d.port_in_use, Some(25565));
    }

    #[test]
    fn leading_initials_etf() {
        assert_eq!(
            super::leading_initials("entity_texture_features_1.20.1-forge-7.1.jar"),
            "etf"
        );
        assert_eq!(super::leading_initials("create-1.20.1-6.0.8.jar"), "c");
    }
    #[test]
    fn classifier_flags_fabric_client_and_crash_token() {
        let mods = vec![
            (
                "entity_texture_features_1.20.1-forge-7.1.jar".to_string(),
                ModEnvironment::Unknown,
            ),
            ("betterf3.jar".to_string(), ModEnvironment::Client),
            ("create-1.20.1-6.0.8.jar".to_string(), ModEnvironment::Both),
        ];
        let tokens = vec!["etf".to_string()];
        let found = classify_client_only_mods(&mods, &tokens);
        let bf3 = found
            .iter()
            .find(|f| f.filename == "betterf3.jar")
            .expect("betterf3 flagged");
        assert_eq!(bf3.confidence, Confidence::High);
        assert_eq!(bf3.reason, "manifest_client");
        let etf = found
            .iter()
            .find(|f| f.filename.contains("entity_texture_features"))
            .expect("etf flagged");
        assert_eq!(etf.confidence, Confidence::Medium);
        assert_eq!(etf.reason, "crash");
        assert!(
            !found.iter().any(|f| f.filename.starts_with("create")),
            "Both must not be flagged"
        );
    }
    #[test]
    fn classifier_high_wins_when_both_signals() {
        // a jar that is BOTH manifest-client AND crash-token → High, reason manifest_client
        let mods = vec![(
            "entity_texture_features-fabric.jar".to_string(),
            ModEnvironment::Client,
        )];
        let found = classify_client_only_mods(&mods, &["etf".to_string()]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].confidence, Confidence::High);
    }

    const ETF_CRASH: &str = "Caused by: java.lang.RuntimeException: Attempted to load class net/minecraft/client/gui/screens/Screen for invalid dist DEDICATED_SERVER\n\tat TRANSFORMER/minecraft@1.20.1/net.minecraft.resources.ResourceLocation.handler$zpl000$etf$illegalPathOverride(ResourceLocation.java:525)\n";
    const PORT: &str = "[Server thread/WARN]: **** FAILED TO BIND TO PORT!\njava.net.BindException: Address already in use: bind\n";
    const EULA: &str = "[main/WARN]: You need to agree to the EULA in order to run the server. Go to eula.txt for more info.\n";

    #[test]
    fn detects_client_only_crash() {
        let d = diagnose_server_log(ETF_CRASH).unwrap();
        assert_eq!(d.pattern_id, "server-client-only-mod-crash");
        assert!(d.repair.is_some());
    }
    #[test]
    fn client_only_warning_suppressed_when_server_started() {
        // Forge logs `invalid dist DEDICATED_SERVER` as a NON-FATAL warning on
        // many working modpack servers (it refuses the class and the mod copes).
        // If the server reached "Done … For help", it loaded fine and must NOT
        // be reported as a client-mod crash.
        let log = "[Server thread/ERROR] [ne.mi.fm.lo.RuntimeDistCleaner/DISTXFORM]: Attempted to load class net/minecraft/client/Foo for invalid dist DEDICATED_SERVER\n\
                   [Server thread/INFO] [minecraft/DedicatedServer/]: Done (138.077s)! For help, type \"help\"\n";
        assert!(
            diagnose_server_log(log).is_none(),
            "a server that reached Done must not be flagged as a client-mod crash"
        );
    }
    #[test]
    fn client_only_crash_still_fires_without_success_line() {
        // The real crash (no "Done … For help") must still be diagnosed.
        let log = "[main/ERROR] [ne.mi.fm.lo.RuntimeDistCleaner/DISTXFORM]: Attempted to load class net/minecraft/client/Foo for invalid dist DEDICATED_SERVER\n\
                   [main/FATAL] [ne.mi.fm.ModLoader/CORE]: Error during pre-loading phase\n";
        assert_eq!(
            diagnose_server_log(log).map(|d| d.pattern_id),
            Some("server-client-only-mod-crash".into())
        );
    }
    #[test]
    fn detects_port_in_use() {
        let d = diagnose_server_log(PORT).unwrap();
        assert_eq!(d.pattern_id, "server-port-in-use");
        assert!(d.repair.is_none());
    }
    #[test]
    fn detects_eula() {
        let d = diagnose_server_log(EULA).unwrap();
        assert_eq!(d.pattern_id, "server-eula-not-accepted");
    }
    #[test]
    fn clean_log_no_match() {
        assert!(diagnose_server_log("[Server thread/INFO]: Done (4.1s)! For help\n").is_none());
    }
    #[test]
    fn parses_forge_client_skip_count() {
        let log = "[main/WARN] [ne.mi.fm.lo.mo.ModDiscoverer/SCAN]: Locator {mods folder locator at C:\\x\\mods} returned 3 files which are client-side-only mods, but we're on a dedicated server. They will be skipped!\n";
        assert_eq!(forge_client_skip_count(log), Some(3));
        assert_eq!(forge_client_skip_count("nothing here"), None);
    }
    #[test]
    fn parses_dist_crash_tokens() {
        let toks = dist_crash_tokens(ETF_CRASH);
        assert!(toks.iter().any(|t| t == "etf"), "tokens: {toks:?}");
    }

    /// A 2-char token must NOT substring-match an unrelated jar (over-match guard).
    #[test]
    fn short_token_does_not_substring_flag_unrelated_jar() {
        // "io" is 2 chars — should not match "biome-1.20.jar" via substring
        let mods = vec![("biome-1.20.jar".to_string(), ModEnvironment::Unknown)];
        let tokens = vec!["io".to_string()];
        let found = classify_client_only_mods(&mods, &tokens);
        assert!(
            found.is_empty(),
            "2-char token 'io' must not substring-flag 'biome-1.20.jar'"
        );
    }

    /// A 3-char acronym token still matches via leading_initials even though it
    /// is short (exact-match path bypasses the substring floor).
    #[test]
    fn etf_acronym_still_matches_via_leading_initials() {
        // leading_initials("entity_texture_features_1.20.1-forge-7.1.jar") == "etf"
        let mods = vec![(
            "entity_texture_features_1.20.1-forge-7.1.jar".to_string(),
            ModEnvironment::Unknown,
        )];
        let tokens = vec!["etf".to_string()];
        let found = classify_client_only_mods(&mods, &tokens);
        assert_eq!(
            found.len(),
            1,
            "etf should still match via leading_initials"
        );
    }

    /// A jar that declares ModEnvironment::Server must NOT be flagged even if a
    /// crash token would substring-match its filename.
    #[test]
    fn server_declared_mod_not_flagged_by_token() {
        let mods = vec![("someservermod.jar".to_string(), ModEnvironment::Server)];
        // "someservermod" substring-matches "someservermod.jar" (≥3 chars)
        let tokens = vec!["someservermod".to_string()];
        let found = classify_client_only_mods(&mods, &tokens);
        assert!(
            found.is_empty(),
            "Server-declared mod must not be flagged by crash-token classifier"
        );
    }

    // --- Task 2: phase-2 fix-param fields + variants -----------------------

    #[test]
    fn server_diagnosis_carries_phase2_fix_params() {
        let d = ServerDiagnosis {
            status: crate::logs::diagnose::DiagnosisStatus::Actionable,
            diagnosis: None,
            client_mods: vec![],
            forge_skip_count: None,
            log_signature: None,
            server_repair: Some(ServerRepairTag::RedownloadServerJar),
            port_in_use: None,
            orphan_pid: None,
            corrupt_jar: Some("sodium-fabric-0.5.3.jar".into()),
            suggested_heap_mb: Some(6144),
            conflict_mods: vec!["sodium".into()],
        };
        let j = serde_json::to_string(&d).unwrap();
        assert!(
            j.contains("\"server_repair\":\"redownload_server_jar\""),
            "got: {j}"
        );
        assert!(
            j.contains("\"corrupt_jar\":\"sodium-fabric-0.5.3.jar\""),
            "got: {j}"
        );
        assert!(j.contains("\"suggested_heap_mb\":6144"), "got: {j}");
        assert!(j.contains("\"conflict_mods\":[\"sodium\"]"), "got: {j}");
    }

    #[test]
    fn server_repair_tag_phase2_variants_snake_case() {
        let pairs = [
            (ServerRepairTag::RaiseHeap, "\"raise_heap\""),
            (ServerRepairTag::LowerHeap, "\"lower_heap\""),
            (
                ServerRepairTag::RedownloadServerJar,
                "\"redownload_server_jar\"",
            ),
            (ServerRepairTag::DisableMods, "\"disable_mods\""),
            (
                ServerRepairTag::InstallMissingDep,
                "\"install_missing_dep\"",
            ),
        ];
        for (tag, expect) in pairs {
            assert_eq!(serde_json::to_string(&tag).unwrap(), expect);
        }
    }

    // --- Task 3: B4 OOM + B6 heap>RAM -------------------------------------

    const SERVER_OOM: &str = "[Server thread/ERROR]: Encountered an unexpected exception\n\
        java.lang.OutOfMemoryError: Java heap space\n\tat java.base/...\n";
    const HEAP_TOO_BIG: &str = "Error occurred during initialization of VM\n\
        Could not reserve enough space for object heap\n";

    #[test]
    fn detects_server_oom() {
        let d = diagnose_server_log(SERVER_OOM).unwrap();
        assert_eq!(d.pattern_id, "server-out-of-memory");
    }
    #[test]
    fn server_oom_negative_on_clean_log() {
        assert!(!matches!(
            diagnose_server_log("[Server thread/INFO]: Done (4.1s)!\n").map(|d| d.pattern_id),
            Some(ref id) if id == "server-out-of-memory"
        ));
    }
    #[test]
    fn detects_heap_too_big() {
        let d = diagnose_server_log(HEAP_TOO_BIG).unwrap();
        assert_eq!(d.pattern_id, "server-heap-too-big");
    }
    #[test]
    fn heap_too_big_negative_on_oom() {
        // A heap-space OOM is NOT a reserve failure — must classify as OOM, not heap-too-big.
        let d = diagnose_server_log(SERVER_OOM).unwrap();
        assert_ne!(d.pattern_id, "server-heap-too-big");
    }

    // --- Task 4: B5 java-too-old + B16 wrong-loader -----------------------

    const JAVA_OLD: &str = "java.lang.UnsupportedClassVersionError: foo/Bar has been compiled by a \
        more recent version of the Java Runtime (class file version 65.0), this version of the Java \
        Runtime only recognizes class file versions up to 61.0\n";
    const WRONG_LOADER: &str = "[main/WARN]: File mods/optifine.jar is not a valid mod file\n";

    #[test]
    fn detects_java_too_old() {
        let d = diagnose_server_log(JAVA_OLD).unwrap();
        assert_eq!(d.pattern_id, "server-java-too-old");
    }
    #[test]
    fn java_too_old_negative_on_oom() {
        assert_ne!(
            diagnose_server_log(JAVA_OLD).unwrap().pattern_id,
            "server-out-of-memory"
        );
    }
    #[test]
    fn detects_wrong_loader_mod() {
        let d = diagnose_server_log(WRONG_LOADER).unwrap();
        assert_eq!(d.pattern_id, "server-wrong-loader-mod");
    }
    #[test]
    fn wrong_loader_negative_on_clean_log() {
        assert!(diagnose_server_log("[Server thread/INFO]: Done (4.1s)!\n").is_none());
    }

    // --- Task 5: B7/B8/B11/B12/B13 ----------------------------------------

    const CORRUPT_JAR: &str =
        "Error: Invalid or corrupt jarfile /srv/mods/sodium-fabric-0.5.3.jar\n";
    const FABRIC_CONFLICT: &str = "net.fabricmc.loader.impl.discovery.ModResolutionException: \
        Mod resolution encountered an incompatible mod set!\n\
        - Mod 'Sodium' (sodium) 0.5.3 requires version 1.20.1 of fabricloader\n\
        - Mod 'Old Lib' (oldlib) 1.2 is incompatible\n";
    const MIXIN_CRASH: &str = "[Server thread/ERROR]: Mixin apply failed \
        somemod.mixins.json:MixinFoo\n\tat TRANSFORMER/minecraft@1.20.1/\
        net.minecraft.server.MinecraftServer.handler$zzz000$etf$onTick(MinecraftServer.java:1)\n";
    const WORLD_CORRUPT: &str = "[Server thread/ERROR]: Failed to load world\n\
        java.lang.RuntimeException: corrupt chunk at -3, 12\n";
    const SESSION_LOCK: &str = "[Server thread/ERROR]: Failed to check session lock, aborting\n";

    #[test]
    fn detects_corrupt_server_jar() {
        let d = diagnose_server_log(CORRUPT_JAR).unwrap();
        assert_eq!(d.pattern_id, "server-corrupt-jar");
    }
    #[test]
    fn corrupt_jar_negative_on_clean_log() {
        assert!(diagnose_server_log("Done (4.1s)!\n").is_none());
    }
    #[test]
    fn detects_mod_conflict() {
        let d = diagnose_server_log(FABRIC_CONFLICT).unwrap();
        assert_eq!(d.pattern_id, "server-mod-conflict");
    }
    #[test]
    fn mod_conflict_negative_on_corrupt_jar() {
        assert_ne!(
            diagnose_server_log(CORRUPT_JAR).unwrap().pattern_id,
            "server-mod-conflict"
        );
    }
    #[test]
    fn detects_mixin_crash() {
        let d = diagnose_server_log(MIXIN_CRASH).unwrap();
        assert_eq!(d.pattern_id, "server-mixin-crash");
    }
    #[test]
    fn mixin_crash_negative_on_clean_log() {
        assert!(diagnose_server_log("[Server thread/INFO]: Done!\n").is_none());
    }
    #[test]
    fn detects_world_corruption() {
        let d = diagnose_server_log(WORLD_CORRUPT).unwrap();
        assert_eq!(d.pattern_id, "server-world-corrupt");
    }
    #[test]
    fn world_corruption_negative_on_session_lock() {
        assert_ne!(
            diagnose_server_log(SESSION_LOCK).unwrap().pattern_id,
            "server-world-corrupt"
        );
    }
    #[test]
    fn world_corruption_negative_on_benign_chunk_migration() {
        // "Chunk file at" without a corruption indicator (e.g. a data-migration
        // notice) must NOT fire the world-corrupt advisory.
        let benign =
            "[Server thread/WARN]: Chunk file at 0, 0 has been saved with a mismatched level\n";
        assert!(diagnose_server_log(benign).is_none());
    }
    #[test]
    fn detects_session_lock() {
        let d = diagnose_server_log(SESSION_LOCK).unwrap();
        assert_eq!(d.pattern_id, "server-session-lock");
    }
    #[test]
    fn session_lock_negative_on_clean_log() {
        assert!(diagnose_server_log("[Server thread/INFO]: Done!\n").is_none());
    }

    // --- Task 6: B9 forge-missing-dep + B10 fabric-api-missing ------------

    const FORGE_MISSING_DEP: &str = "[main/ERROR]: Missing or unsupported mandatory dependencies:\n\
        \tMod ID: 'jei', Requested by: 'somemod', Expected range: '[15,)', Actual version: '[MISSING]'\n";
    const FABRIC_API_MISSING: &str = "net.fabricmc.loader.impl.FormattedException: \
        Mod 'somemod' (somemod) requires version * of fabric, which is missing!\n";

    #[test]
    fn detects_forge_missing_dep() {
        let d = diagnose_server_log(FORGE_MISSING_DEP).unwrap();
        assert_eq!(d.pattern_id, "server-missing-dep");
    }
    #[test]
    fn forge_missing_dep_negative_on_clean_log() {
        assert!(diagnose_server_log("[main/INFO]: All mods loaded\n").is_none());
    }
    #[test]
    fn detects_fabric_api_missing() {
        let d = diagnose_server_log(FABRIC_API_MISSING).unwrap();
        assert_eq!(d.pattern_id, "server-fabric-api-missing");
    }
    #[test]
    fn fabric_api_missing_negative_on_conflict() {
        // A generic ModResolutionException without "requires ... fabric" must not
        // be mistaken for fabric-api-missing.
        assert_ne!(
            diagnose_server_log(
                "net.fabricmc.loader.impl.discovery.ModResolutionException: incompatible\n"
            )
            .map(|d| d.pattern_id),
            Some("server-fabric-api-missing".to_string())
        );
    }

    // --- Task 7: server_repair_for + pick_diagnosable ---------------------

    #[test]
    fn server_repair_for_maps_actionable_patterns() {
        assert_eq!(
            server_repair_for("server-out-of-memory"),
            Some(ServerRepairTag::RaiseHeap)
        );
        assert_eq!(
            server_repair_for("server-heap-too-big"),
            Some(ServerRepairTag::LowerHeap)
        );
        assert_eq!(
            server_repair_for("server-corrupt-jar"),
            Some(ServerRepairTag::RedownloadServerJar)
        );
        assert_eq!(
            server_repair_for("server-mod-conflict"),
            Some(ServerRepairTag::DisableMods)
        );
        assert_eq!(
            server_repair_for("server-mixin-crash"),
            Some(ServerRepairTag::DisableMods)
        );
        assert_eq!(
            server_repair_for("server-missing-dep"),
            Some(ServerRepairTag::InstallMissingDep)
        );
        assert_eq!(
            server_repair_for("server-fabric-api-missing"),
            Some(ServerRepairTag::InstallMissingDep)
        );
        assert_eq!(
            server_repair_for("server-session-lock"),
            Some(ServerRepairTag::StopOrphanAndRetry)
        );
        assert_eq!(
            server_repair_for("server-client-only-mod-crash"),
            Some(ServerRepairTag::RemoveClientMods)
        );
    }
    #[test]
    fn server_repair_for_none_on_advisory_patterns() {
        assert_eq!(server_repair_for("server-world-corrupt"), None);
        assert_eq!(server_repair_for("server-java-too-old"), None);
        assert_eq!(server_repair_for("server-wrong-loader-mod"), None);
        assert_eq!(server_repair_for("nonexistent"), None);
    }

    #[test]
    fn freshest_diagnosable_prefers_crash_when_richer() {
        // When the server log is benign but the crash report carries the signal,
        // pick the crash report's content.
        let server_log = "[Server thread/INFO]: Done (4.1s)!\n";
        let crash = "java.lang.OutOfMemoryError: Java heap space\n";
        let chosen = pick_diagnosable(server_log, Some(crash));
        assert_eq!(
            diagnose_server_log(chosen).map(|d| d.pattern_id),
            Some("server-out-of-memory".into())
        );
    }
    #[test]
    fn freshest_diagnosable_uses_server_log_when_no_crash() {
        let server_log = "Could not reserve enough space for object heap\n";
        assert_eq!(pick_diagnosable(server_log, None), server_log);
    }

    // --- Task 9: extract_missing_dep_ids ----------------------------------

    #[test]
    fn extract_missing_dep_ids_from_forge_screen() {
        let log = "Missing or unsupported mandatory dependencies:\n\
            \tMod ID: 'jei', Requested by: 'somemod', Expected range: '[15,)', Actual version: '[MISSING]'\n\
            \tMod ID: 'curios', Requested by: 'othermod', Expected range: '[5,)', Actual version: '[MISSING]'\n";
        assert_eq!(
            extract_missing_dep_ids(log),
            vec!["jei".to_string(), "curios".to_string()]
        );
    }
    #[test]
    fn extract_missing_dep_ids_fabric_api() {
        let log = "Mod 'somemod' (somemod) requires version * of fabric, which is missing!\n";
        assert_eq!(extract_missing_dep_ids(log), vec!["fabric-api".to_string()]);
    }
    #[test]
    fn extract_missing_dep_ids_empty_on_clean_log() {
        assert!(extract_missing_dep_ids("All mods loaded\n").is_empty());
    }
}
