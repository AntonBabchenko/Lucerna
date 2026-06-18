//! Pattern matchers for a Minecraft *dedicated server* log. Distinct from the
//! client diagnoser in `engine.rs` — server failures (client-only mod on a
//! dedicated server, port in use, EULA) need their own signatures. Pure: no
//! I/O. The command layer reads the log and calls `diagnose_server_log`.

use crate::logs::diagnose::repair::RepairKind;
use crate::logs::diagnose::Diagnosis;
use once_cell::sync::Lazy;
use regex::Regex;

/// First matching server-log diagnosis, if any. Order = specificity.
pub fn diagnose_server_log(log: &str) -> Option<Diagnosis> {
    if log.contains("invalid dist DEDICATED_SERVER") || log.contains("RuntimeDistCleaner") {
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
    None
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

#[cfg(test)]
mod tests {
    use super::*;
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
}
