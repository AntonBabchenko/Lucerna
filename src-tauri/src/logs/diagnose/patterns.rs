//! Diagnoser knowledge base. Adding a pattern: append a `Pattern`
//! entry to `PATTERNS`, add two unit tests in `engine.rs::tests`
//! (one positive against a real-shaped log excerpt, one negative
//! against an unrelated trace). The first matching entry in
//! `PATTERNS` wins — order by specificity (most specific first).

use crate::logs::files::LogSource;
use once_cell::sync::Lazy;
use regex::Regex;

pub struct Pattern {
    pub id: &'static str,
    pub matcher: Matcher,
    pub title: &'static str,
    pub explanation: &'static str,
    pub recommendation: &'static str,
    pub source_hint: SourceHint,
}

pub enum Matcher {
    /// Case-sensitive substring match. Cheapest; use unless a regex
    /// is genuinely needed for shape or capture.
    Substring(&'static str),
    /// Pre-compiled regex via `once_cell::sync::Lazy`.
    Regex(&'static Lazy<Regex>),
}

impl Matcher {
    /// Returns the byte offset of the first match, or `None`.
    pub fn find(&self, haystack: &str) -> Option<usize> {
        match self {
            Matcher::Substring(needle) => haystack.find(needle),
            Matcher::Regex(re) => re.find(haystack).map(|m| m.start()),
        }
    }
}

/// Which log file kinds make sense for this pattern. The engine
/// uses it as a hint to skip clearly-irrelevant patterns — correctness
/// is unaffected if a hint is wrong, only CPU is wasted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceHint {
    Any,
    Crash,
    LauncherStdout,
    GameLog,
}

impl SourceHint {
    pub fn matches(self, src: LogSource) -> bool {
        match self {
            SourceHint::Any => true,
            SourceHint::Crash => matches!(src, LogSource::Crash),
            SourceHint::LauncherStdout => matches!(src, LogSource::Launcher),
            SourceHint::GameLog => matches!(src, LogSource::Game),
        }
    }
}

// --- Compiled regexes (one Lazy per Regex-using pattern) -----------

static JAVA_VERSION_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"UnsupportedClassVersionError.*?class file version (\d+)\.\d+\), this version of the Java Runtime only recognizes class file versions up to (\d+)\.\d+",
    )
    .expect("regex compiles — covered by `all_patterns_regexes_compile`")
});

static CORRUPT_JAR_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"java\.util\.zip\.ZipException|Invalid or corrupt jarfile")
        .expect("regex compiles — covered by `all_patterns_regexes_compile`")
});

// Two FML reject shapes (both confirmed in Phase 0) mean "the client is missing
// mods the server requires": a `client side` channel-version mismatch and a
// datapack-registry sync failure (`Missing required datapack registry: …`, from
// library mods like Moonlight). `server_mods.rs` parses both.
//
// Anchored to `client side` (not a bare `mismatched mod list`): the inverse
// `server side` reject — the client carrying enforced-channel mods the server
// lacks — is the `client-extra-mods` case below, not a missing-mods one. Without
// this anchor that reject would mis-raise an "install missing mods" advisory.
static SERVER_MISSING_MODS_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"rejected their client side version number|Missing required datapack registr")
        .expect("regex compiles — covered by `all_patterns_regexes_compile`")
});

// The inverse reject: a `server side` channel rejection in a client log means
// the client carries enforced-channel mods the server lacks → mods to *disable*,
// not install. `server_mods.rs::parse_blocking_client_mods` extracts them.
static CLIENT_EXTRA_MODS_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"rejected their server side version number")
        .expect("regex compiles — covered by `all_patterns_regexes_compile`")
});

// --- The knowledge base --------------------------------------------

pub const PATTERNS: &[Pattern] = &[
    Pattern {
        id: "java-version-too-old",
        matcher: Matcher::Regex(&JAVA_VERSION_RE),
        title: "Java version is too old for this modpack",
        explanation:
            "One of your mods was built for a newer version of Java than the instance is \
             currently running. Minecraft launched the game, but the mod's classes failed to load.",
        recommendation:
            "Bump the instance's Minecraft version under Manage → Minecraft version — newer \
             MC versions install a newer JRE automatically. If your modpack is locked to this \
             MC version, the mod itself likely needs an update from its author.",
        source_hint: SourceHint::Any,
    },
    Pattern {
        id: "mod-resolution-conflict",
        matcher: Matcher::Substring("net.fabricmc.loader.impl.discovery.ModResolutionException"),
        title: "Two mods conflict with each other",
        explanation:
            "Fabric stopped loading because two of your installed mods can't run together — one \
             requires a version of something the other forbids.",
        recommendation:
            "Open the crash log below to see which mods are named. Disable one of the two in the \
             Installed tab and try again. If you imported a modpack, see if the pack author lists \
             a known-bad combination.",
        source_hint: SourceHint::Any,
    },
    Pattern {
        id: "fabric-loader-missing-main",
        matcher: Matcher::Substring("Could not find or load main class net.fabricmc.loader"),
        title: "Fabric loader wasn't installed for this instance",
        explanation:
            "The launcher asked Java to start Fabric, but the Fabric loader files aren't on the \
             classpath. This usually means the instance was created vanilla and Fabric wasn't \
             chosen, or the loader install failed.",
        recommendation:
            "Open Manage for this instance, set the loader to Fabric, pick a loader version, and \
             let the launcher reinstall before launching.",
        source_hint: SourceHint::LauncherStdout,
    },
    Pattern {
        id: "corrupt-mod-jar",
        matcher: Matcher::Regex(&CORRUPT_JAR_RE),
        title: "A mod file is corrupt",
        explanation:
            "Minecraft tried to read a mod jar and found it was damaged or incomplete. The most \
             common cause is a download that was interrupted partway.",
        recommendation:
            "Open the crash log below to find the jar's filename. In the Installed tab, uninstall \
             that mod and reinstall it. If you imported a modpack, re-importing the pack will \
             re-fetch every jar with SHA-1 verification.",
        source_hint: SourceHint::Any,
    },
    Pattern {
        id: "server-missing-mods",
        // Anchors confirmed in the Phase 0 spike against real modern-Forge
        // (47.4.10) client logs: FML rejects a join either with a channel
        // "mismatched mod list" or a "Missing required datapack registry" sync
        // failure. server_mods.rs parses the mod-ids from whichever appears.
        matcher: Matcher::Regex(&SERVER_MISSING_MODS_RE),
        title: "The server needs mods you don't have",
        explanation:
            "The server rejected the connection because your client is missing mods it \
             requires, or has them at the wrong version. Minecraft listed them before \
             disconnecting.",
        recommendation:
            "Open this log and use \"Install missing mods\" to add them to this instance, \
             then reconnect. Mods the launcher can't identify automatically are listed so \
             you can find them in the Add-ons browser.",
        source_hint: SourceHint::GameLog,
    },
    Pattern {
        id: "client-extra-mods",
        // The inverse of `server-missing-mods`: a `server side` channel reject
        // means the client carries enforced-channel mods the server lacks, so the
        // client refused to join. Declared AFTER `server-missing-mods` — the
        // engine matches first-in-declaration-order, so a log containing both
        // directions surfaces the (more common) install case first.
        matcher: Matcher::Regex(&CLIENT_EXTRA_MODS_RE),
        title: "Your mods are blocking this server",
        explanation:
            "Your client has mods the server doesn't, and they enforce a connection \
             channel, so your client refused to join. They must be disabled to connect — \
             you can re-enable them later for single-player or other servers.",
        recommendation:
            "Open this log and disable the listed mods, then reconnect. Disabling is \
             reversible.",
        source_hint: SourceHint::GameLog,
    },
    Pattern {
        id: "out-of-memory",
        matcher: Matcher::Substring("OutOfMemoryError: Java heap space"),
        title: "Minecraft ran out of memory",
        explanation:
            "The instance's allotted memory (heap) wasn't enough — typical for heavy modpacks or \
             when many chunks are loaded.",
        recommendation:
            "Open Manage for this instance, raise the Max heap value (try 4096 MB for a vanilla-ish \
             modpack, 6144 MB or more for heavy packs), then try again. Don't exceed half of your \
             system RAM.",
        source_hint: SourceHint::Any,
    },
    Pattern {
        id: "port-already-in-use",
        matcher: Matcher::Substring("BindException: Address already in use"),
        title: "Network port is already taken",
        explanation:
            "Minecraft tried to open a network port — usually for the integrated LAN server — and \
             another program is already using it.",
        recommendation:
            "Quit any other running Minecraft windows (including from other launchers), then try \
             again. If the problem persists, restart the computer to clear stuck connections.",
        source_hint: SourceHint::Any,
    },
    Pattern {
        id: "disk-full",
        matcher: Matcher::Substring("No space left on device"),
        title: "Hard drive is full",
        explanation:
            "Minecraft tried to write a file (chunks, screenshots, mod cache, logs) and the disk \
             refused because it ran out of space.",
        recommendation:
            "Free up space on the drive that holds the launcher's data — typically %APPDATA% on \
             Windows. The launcher's Storage settings show how much space the mod cache takes if \
             you want to start there.",
        source_hint: SourceHint::Any,
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn all_patterns_have_unique_ids() {
        let ids: HashSet<&str> = PATTERNS.iter().map(|p| p.id).collect();
        assert_eq!(ids.len(), PATTERNS.len(), "duplicate pattern id detected");
    }

    #[test]
    fn all_patterns_regexes_compile() {
        // Force-evaluate every Lazy<Regex> referenced from PATTERNS.
        // A regex with a typo would panic here rather than the first
        // time a user crashes in production.
        for p in PATTERNS {
            if let Matcher::Regex(re) = &p.matcher {
                let _ = Lazy::force(re);
            }
        }
    }

    #[test]
    fn all_patterns_have_non_empty_copy() {
        for p in PATTERNS {
            assert!(!p.title.is_empty(), "{} has empty title", p.id);
            assert!(!p.explanation.is_empty(), "{} has empty explanation", p.id);
            assert!(
                !p.recommendation.is_empty(),
                "{} has empty recommendation",
                p.id
            );
        }
    }

    #[test]
    fn all_pattern_titles_under_60_chars() {
        for p in PATTERNS {
            assert!(
                p.title.chars().count() <= 60,
                "{} title is {} chars, max 60: {:?}",
                p.id,
                p.title.chars().count(),
                p.title
            );
        }
    }

    #[test]
    fn v1_ships_exactly_nine_patterns() {
        // 7 → 8 for `server-missing-mods`; 8 → 9 for `client-extra-mods`
        // (the inverse "your mods block this server" diagnosis).
        assert_eq!(PATTERNS.len(), 9);
    }
}
