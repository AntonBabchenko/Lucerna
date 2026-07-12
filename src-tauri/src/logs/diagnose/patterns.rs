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
    pub side: Side,
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

/// Which launcher surface an error is meaningful on. The banner engine
/// ignores this (its table is client-scoped by construction); the inline
/// annotator filters on it so client-worded copy never shows on the
/// server console and vice versa.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Client,
    Server,
    Any,
}

impl Side {
    pub fn matches_client(self) -> bool {
        matches!(self, Side::Client | Side::Any)
    }
    pub fn matches_server(self) -> bool {
        matches!(self, Side::Server | Side::Any)
    }
}

// --- Compiled regexes (one Lazy per Regex-using pattern) -----------

static JAVA_VERSION_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"UnsupportedClassVersionError.*?class file version (\d+)\.\d+\), this version of the Java Runtime only recognizes class file versions up to (\d+)\.\d+",
    )
    .expect("regex compiles — covered by `all_patterns_regexes_compile`")
});

/// True iff the log shows a Java-too-old failure (`UnsupportedClassVersionError`
/// with a class-file version newer than the running JRE recognizes). Exposed for
/// the server diagnoser so it can reuse the client regex without leaking it.
pub fn detect_java_version_too_old(log: &str) -> bool {
    JAVA_VERSION_RE.is_match(log)
}

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
    Regex::new(
        r"rejected their client side version number|Missing required datapack registr|Missing registry data for impl connection",
    )
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
        side: Side::Client,
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
        side: Side::Client,
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
        side: Side::Client,
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
        side: Side::Client,
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
        side: Side::Client,
    },
    Pattern {
        id: "client-extra-mods",
        // A `server side` channel reject is ambiguous: the server may not have the
        // mod at all (disable to join), OR it has a different version than the
        // client (a version mismatch — disabling won't help; you need the server's
        // version, which FML shows on the in-game disconnect screen but never
        // logs). The log can't distinguish them, so the copy presents both cases
        // rather than prescribing disable. Declared AFTER `server-missing-mods` —
        // the engine matches first-in-declaration-order.
        matcher: Matcher::Regex(&CLIENT_EXTRA_MODS_RE),
        title: "The server rejected some of your mods",
        explanation:
            "The server refused the connection over these mods. Either the server doesn't \
             have them, or your version differs from the server's — Minecraft can't tell \
             the launcher which, but its disconnect screen shows any version it needs.",
        recommendation:
            "Open this log. If the server doesn't have a mod, disable it (reversible) and \
             reconnect. If it's a version difference, install the version the server needs \
             instead of disabling.",
        source_hint: SourceHint::GameLog,
        side: Side::Client,
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
        side: Side::Client,
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
        side: Side::Client,
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
        side: Side::Client,
    },
    Pattern {
        id: "create-goggle-overlay-crash",
        // A specific Create-addon mixin (tfmg / Big Cannons) casts the player's
        // hit-result to BlockHitResult while rendering the goggles overlay; when
        // the player looks at an ENTITY it is an EntityHitResult and the cast
        // throws every render frame. Forge catches it, so the game does not
        // crash — but the log fills with this error and the overlay flickers.
        matcher: Matcher::Substring("Error rendering overlay 'create:goggle_info'"),
        title: "Create goggles are spamming errors",
        explanation:
            "While wearing Create goggles and looking at a mob or other entity, a Create \
             add-on (The Factory Must Grow / Big Cannons) throws an error every frame. The \
             game keeps running, but the log fills up and the goggle overlay flickers.",
        recommendation:
            "Lucerna can install a small community fix mod that resolves this. It's a \
             third-party mod, not an official patch — review it before installing.",
        source_hint: SourceHint::GameLog,
        side: Side::Client,
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
    fn ships_exactly_ten_patterns() {
        // 9 → 10 for `create-goggle-overlay-crash` (first fix-mod-backed pattern).
        assert_eq!(PATTERNS.len(), 10);
    }

    #[test]
    fn detect_java_version_too_old_true_on_unsupported_class_version() {
        let log = "Exception in thread \"main\" java.lang.UnsupportedClassVersionError: \
                   foo/Bar has been compiled by a more recent version of the Java Runtime \
                   (class file version 65.0), this version of the Java Runtime only \
                   recognizes class file versions up to 61.0";
        assert!(super::detect_java_version_too_old(log));
    }

    #[test]
    fn detect_java_version_too_old_false_on_unrelated_log() {
        assert!(!super::detect_java_version_too_old(
            "[Server thread/INFO]: Done (4.1s)! For help, type \"help\"\n"
        ));
    }

    #[test]
    fn banner_patterns_are_all_client_side() {
        // Deliberate property: every banner pattern's copy is client-worded.
        // Server surfaces get their own inline entries instead.
        for p in PATTERNS {
            assert_eq!(p.side, Side::Client, "{} must be Side::Client", p.id);
        }
    }

    #[test]
    fn side_matching_helpers() {
        assert!(Side::Any.matches_client() && Side::Any.matches_server());
        assert!(Side::Client.matches_client() && !Side::Client.matches_server());
        assert!(Side::Server.matches_server() && !Side::Server.matches_client());
    }
}
