//! Inline-only diagnoser knowledge base: patterns that annotate single log
//! lines with a hover hint but do NOT participate in the whole-file banner
//! diagnosis (`patterns::PATTERNS` keeps that role). Adding a pattern:
//! append a `Pattern` entry, add a (positive, negative) fixture pair to
//! `annotate.rs::tests::CATALOG_FIXTURES`, add i18n copy under
//! `logs.diagnosis.patterns.<camelCaseId>` in en.json + ru.json, and extend
//! `DIAGNOSIS_COPY` in src/lib/logs/diagnosis-copy.ts.
//!
//! Order = specificity (most specific first); the annotator scans
//! `patterns::PATTERNS` before this table, so banner patterns win a line.
//!
//! Dropped as too ambiguous (do not re-add without new evidence):
//! OptiFine-conflict shapes, pack.mcmeta format mismatch, a dedicated
//! missing-Fabric-API matcher (generic `fabric-unmet-dependency` covers it).

use super::patterns::{Matcher, Pattern, Side, SourceHint};
use once_cell::sync::Lazy;
use regex::Regex;

// --- Compiled regexes ----------------------------------------------

static DUPLICATE_MODS_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"DuplicateModsFoundException|Found duplicate mods")
        .expect("regex compiles — covered by `inline_regexes_compile`")
});

static MIXIN_FAILED_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"Mixin apply failed|MixinTransformerError|Critical injection failure|InvalidMixinException")
        .expect("regex compiles — covered by `inline_regexes_compile`")
});

static MOD_METADATA_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"ParseMetadataException|Missing mods\.toml file")
        .expect("regex compiles — covered by `inline_regexes_compile`")
});

static LINKAGE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"java\.lang\.NoSuchMethodError|java\.lang\.NoSuchFieldError|java\.lang\.NoClassDefFoundError")
        .expect("regex compiles — covered by `inline_regexes_compile`")
});

static HEAP_RESERVE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"Could not reserve enough space for.*object heap|Invalid maximum heap size")
        .expect("regex compiles — covered by `inline_regexes_compile`")
});

static BAD_JVM_ARG_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"Unrecognized VM option|Unrecognized option: -")
        .expect("regex compiles — covered by `inline_regexes_compile`")
});

static NATIVE_CRASH_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"EXCEPTION_ACCESS_VIOLATION|A fatal error has been detected by the Java Runtime Environment")
        .expect("regex compiles — covered by `inline_regexes_compile`")
});

// Both halves usually co-occur on one line; the OR is defense against GLFW phrasing drift across versions.
static GLFW_NO_OPENGL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"GLFW error 65542|WGL: The driver does not appear to support OpenGL")
        .expect("regex compiles — covered by `inline_regexes_compile`")
});

static GL_ERROR_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"########## GL ERROR ##########|OpenGL error \d+")
        .expect("regex compiles — covered by `inline_regexes_compile`")
});

static INVALID_SESSION_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"Invalid session|Failed to verify username")
        .expect("regex compiles — covered by `inline_regexes_compile`")
});

static NOT_WHITELISTED_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"not white-?listed").expect("regex compiles — covered by `inline_regexes_compile`")
});

static TIMEOUT_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"Connection timed out|ReadTimeoutException")
        .expect("regex compiles — covered by `inline_regexes_compile`")
});

static SESSION_LOCK_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"Failed to check session lock|The save is being accessed from another location")
        .expect("regex compiles — covered by `inline_regexes_compile`")
});

static CHUNK_CORRUPTION_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"Failed to read chunk|corrupt chunk|Chunk file at .* is (missing|in the wrong location)",
    )
    .expect("regex compiles — covered by `inline_regexes_compile`")
});

static LEVEL_DAT_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"Exception reading .*level\.dat")
        .expect("regex compiles — covered by `inline_regexes_compile`")
});

static SHADER_FAILED_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)failed to (compile|load) (the )?shader")
        .expect("regex compiles — covered by `inline_regexes_compile`")
});

static SERVER_PORT_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"FAILED TO BIND TO PORT|Address already in use")
        .expect("regex compiles — covered by `inline_regexes_compile`")
});

// --- The inline-only knowledge base --------------------------------

pub const INLINE_PATTERNS: &[Pattern] = &[
    // ---- Mods & loaders --------------------------------------------
    Pattern {
        id: "forge-duplicate-mods",
        matcher: Matcher::Regex(&DUPLICATE_MODS_RE),
        title: "Two copies of the same mod are installed",
        explanation: "The mod loader found the same mod twice in the mods folder — usually an old \
             version left behind next to a newer one. It refuses to start until \
             one copy is removed.",
        recommendation: "Open the Installed tab (or the mods folder) and delete the older \
             duplicate jar, then launch again.",
        source_hint: SourceHint::Any,
        side: Side::Any,
    },
    Pattern {
        id: "forge-missing-dependencies",
        matcher: Matcher::Substring("Missing or unsupported mandatory dependencies"),
        title: "A mod is missing a required dependency",
        explanation: "One of your mods needs another mod (or a newer version of it) that \
             isn't installed. The lines after this one name the mod and the \
             version it wants.",
        recommendation: "Install the dependency named below at the required version, or \
             remove the mod that demands it.",
        source_hint: SourceHint::Any,
        side: Side::Any,
    },
    Pattern {
        id: "fabric-unmet-dependency",
        matcher: Matcher::Substring("Unmet dependency listing"),
        title: "A mod's dependency is missing or the wrong version",
        explanation: "Fabric stopped loading because a mod requires something that isn't \
             installed or is the wrong version. The lines below name exactly \
             which mod wants what.",
        recommendation: "Install the mod named below at the version it asks for (most \
             Fabric mods also need Fabric API), or remove the mod that \
             requires it.",
        source_hint: SourceHint::Any,
        side: Side::Any,
    },
    Pattern {
        id: "mixin-apply-failed",
        matcher: Matcher::Regex(&MIXIN_FAILED_RE),
        title: "A mod failed to hook into the game",
        explanation: "A mod's mixin (its way of patching game code) failed to apply. \
             This almost always means the mod was built for a different \
             Minecraft version, or two mods are patching the same code and \
             collide.",
        recommendation: "Check the mod named in this line. Install the build matching your \
             exact Minecraft version and loader; if it's current, try removing \
             it to identify the conflicting pair.",
        source_hint: SourceHint::Any,
        side: Side::Any,
    },
    Pattern {
        id: "broken-mod-metadata",
        matcher: Matcher::Regex(&MOD_METADATA_RE),
        title: "A mod file has broken metadata",
        explanation: "The loader couldn't read a mod's descriptor (fabric.mod.json / \
             mods.toml). The jar is malformed — usually a corrupted download or \
             a file that isn't actually a mod for this loader.",
        recommendation: "Re-download the mod from its official page for your loader and \
             Minecraft version, replacing the broken file.",
        source_hint: SourceHint::Any,
        side: Side::Any,
    },
    Pattern {
        id: "overlay-render-crash",
        matcher: Matcher::Substring("The game crashed whilst rendering overlay"),
        title: "Crash while drawing the loading overlay",
        explanation: "The game died during the mod-loading screen. The 'Caused by' lines \
             below name the mod or resource that failed to initialize.",
        recommendation: "Find the mod named in the stack below and update or remove it. If \
             a resource pack is named, disable that pack.",
        source_hint: SourceHint::Any,
        side: Side::Client,
    },
    Pattern {
        id: "datapack-load-failed",
        matcher: Matcher::Substring(
            "Errors in currently selected datapacks prevented the world from loading",
        ),
        title: "Datapacks blocked the world from loading",
        explanation: "A datapack in this world is broken or was made for a different \
             Minecraft version, so the world refused to load.",
        recommendation: "Remove or update the offending datapack in the world's datapacks \
             folder. Choosing 'safe mode' when Minecraft offers it disables \
             them temporarily.",
        source_hint: SourceHint::Any,
        side: Side::Any,
    },
    Pattern {
        id: "mod-linkage-error",
        matcher: Matcher::Regex(&LINKAGE_RE),
        title: "A mod was built for a different game version",
        explanation: "Java couldn't find a method, field or class another mod expected — \
             the classic sign of a mod compiled against a different Minecraft \
             or mod version than what's installed. NoClassDefFoundError can \
             also be an echo of an earlier failure in the same mod — check the \
             first error above it.",
        recommendation: "Look at the mod named in this line (or the 'Caused by' below) and \
             install the build that matches your exact Minecraft version and \
             the versions of the mods it interacts with.",
        source_hint: SourceHint::Any,
        side: Side::Any,
    },
    // ---- JVM & memory ----------------------------------------------
    Pattern {
        id: "oom-metaspace",
        matcher: Matcher::Substring("OutOfMemoryError: Metaspace"),
        title: "Java ran out of metaspace",
        explanation: "The class-metadata area filled up — this happens with very large \
             mod counts, independent of the normal heap setting.",
        recommendation: "Add -XX:MaxMetaspaceSize=1G to the instance's custom JVM \
             arguments, or reduce the number of installed mods.",
        source_hint: SourceHint::Any,
        side: Side::Any,
    },
    Pattern {
        id: "oom-gc-overhead",
        matcher: Matcher::Substring("GC overhead limit exceeded"),
        title: "Memory is so full Java gave up",
        explanation: "Java spent almost all its time garbage-collecting because the heap \
             is nearly exhausted — effectively an out-of-memory condition.",
        recommendation: "Raise the instance's maximum memory (heap). For heavy modpacks try \
             6144 MB or more, but stay under half of your system RAM.",
        source_hint: SourceHint::Any,
        side: Side::Any,
    },
    Pattern {
        id: "heap-reserve-failed",
        matcher: Matcher::Regex(&HEAP_RESERVE_RE),
        title: "The memory setting is larger than available RAM",
        explanation: "Java could not even start because the configured maximum heap is \
             bigger than the memory the system can give it (or the value is \
             malformed).",
        recommendation: "Lower the maximum memory setting, close memory-hungry programs, \
             and make sure a 64-bit Java is being used.",
        source_hint: SourceHint::Any,
        side: Side::Any,
    },
    Pattern {
        id: "bad-jvm-arg",
        matcher: Matcher::Regex(&BAD_JVM_ARG_RE),
        title: "A custom Java argument is invalid",
        explanation: "Java refused to start because one of the custom JVM arguments \
             isn't recognized — often a typo or a flag from a different Java \
             version.",
        recommendation: "Open the instance's Java settings and fix or remove the argument \
             named in this line.",
        source_hint: SourceHint::Any,
        side: Side::Any,
    },
    Pattern {
        id: "natives-link-error",
        matcher: Matcher::Substring("java.lang.UnsatisfiedLinkError"),
        title: "A native library failed to load",
        explanation: "Java couldn't load a platform-native library (typically LWJGL). \
             The files may be missing, corrupted, or blocked by antivirus.",
        recommendation: "Run instance repair to re-download the libraries. If it recurs, \
             add the launcher's data folder to your antivirus exclusions.",
        source_hint: SourceHint::Any,
        side: Side::Any,
    },
    Pattern {
        id: "jvm-native-crash",
        matcher: Matcher::Regex(&NATIVE_CRASH_RE),
        title: "Java itself crashed (native crash)",
        explanation: "The crash happened below Java, in native code — on clients this is \
             most often a graphics-driver fault; overclocking and faulty RAM \
             are the other usual suspects.",
        recommendation: "Update your GPU driver first. If it keeps happening, remove \
             recently added mods that use native code, and check the hs_err \
             file mentioned nearby for the failing module.",
        source_hint: SourceHint::Any,
        side: Side::Any,
    },
    // ---- Graphics (client) -----------------------------------------
    Pattern {
        id: "glfw-no-opengl",
        matcher: Matcher::Regex(&GLFW_NO_OPENGL_RE),
        title: "The graphics driver doesn't provide OpenGL",
        explanation: "Minecraft couldn't create an OpenGL window. The GPU driver is \
             missing, too old, or the game ran on the wrong GPU (common on \
             laptops with two graphics chips).",
        recommendation: "Install the latest driver from NVIDIA/AMD/Intel (not just Windows \
             Update). On dual-GPU laptops, force the discrete GPU for Java in \
             Windows graphics settings or via the launcher's GPU option.",
        source_hint: SourceHint::Any,
        side: Side::Client,
    },
    Pattern {
        id: "pixel-format-not-accelerated",
        matcher: Matcher::Substring("Pixel format not accelerated"),
        title: "No hardware graphics acceleration available",
        explanation: "The system offered only software rendering — the classic sign of a \
             missing or outdated GPU driver (frequent after Windows reinstalls \
             and on remote desktops).",
        recommendation: "Install the proper GPU driver from the vendor's site and restart. \
             Remote-desktop sessions often lack acceleration entirely — try \
             locally.",
        source_hint: SourceHint::Any,
        side: Side::Client,
    },
    Pattern {
        id: "opengl-error-spam",
        matcher: Matcher::Regex(&GL_ERROR_RE),
        title: "OpenGL is reporting rendering errors",
        explanation: "The renderer hit invalid GL state. In practice this comes from a \
             shader pack, a resource pack, or a rendering mod that doesn't \
             match the current game/driver.",
        recommendation: "Disable the most recently added shader or resource pack. If it \
             persists, update the GPU driver and rendering mods (Sodium/Iris/\
             OptiFine) to builds for this exact Minecraft version.",
        source_hint: SourceHint::Any,
        side: Side::Client,
    },
    // ---- Network & account -----------------------------------------
    Pattern {
        id: "invalid-session",
        matcher: Matcher::Regex(&INVALID_SESSION_RE),
        title: "The Minecraft session is invalid",
        explanation: "The session token has expired or the account isn't properly signed \
             in, so the server rejected the join. Occasionally the Minecraft \
             session service itself is down — then waiting is the only fix.",
        recommendation: "Sign out and back in (Accounts), then restart the game. On a \
             server log this means the joining player needs to do that.",
        source_hint: SourceHint::Any,
        side: Side::Any,
    },
    Pattern {
        id: "not-whitelisted",
        matcher: Matcher::Regex(&NOT_WHITELISTED_RE),
        title: "Not on the server's whitelist",
        explanation: "The server runs a whitelist and this account isn't on it — nothing \
             is broken on your side.",
        recommendation: "Ask the server owner to whitelist your exact username \
             (whitelist add <name>).",
        source_hint: SourceHint::Any,
        side: Side::Any,
    },
    Pattern {
        id: "dns-unknown-host",
        matcher: Matcher::Substring("UnknownHostException"),
        title: "The server address couldn't be resolved",
        explanation: "DNS lookup failed — the address has a typo, the domain is gone, or \
             this machine has no working internet/DNS. The address is usually \
             the server you tried to join, but mods checking for updates can \
             log this too.",
        recommendation: "Double-check the server address for typos, then verify your \
             internet connection. If both are fine, try a different DNS \
             (e.g. 1.1.1.1).",
        source_hint: SourceHint::Any,
        side: Side::Any,
    },
    Pattern {
        id: "ssl-handshake-failed",
        matcher: Matcher::Substring("SSLHandshakeException"),
        title: "Secure connection failed",
        explanation: "An HTTPS connection (login, mod download, API) couldn't be \
             established. The most common cause is a wrong system clock; \
             intercepting antivirus/proxy software is next.",
        recommendation: "Check that the system date and time are correct, then try again. \
             If it persists, temporarily disable HTTPS-scanning in your \
             antivirus or proxy to confirm the culprit.",
        source_hint: SourceHint::Any,
        side: Side::Any,
    },
    Pattern {
        id: "connection-timed-out",
        matcher: Matcher::Regex(&TIMEOUT_RE),
        title: "The connection timed out",
        explanation: "The other side never answered — the server is down, the address/\
             port is wrong, or a firewall is dropping the traffic. (If it \
             appears without you joining a server, a mod's background download \
             timed out — harmless.)",
        recommendation: "Verify the server is actually running and the address and port are \
             right, then check firewalls on both ends (including the router's \
             port forwarding for self-hosted servers).",
        source_hint: SourceHint::Any,
        side: Side::Any,
    },
    Pattern {
        id: "connection-refused",
        matcher: Matcher::Substring("Connection refused"),
        title: "The connection was refused",
        explanation: "The target machine answered but nothing is listening on that port \
             — the server isn't running there, or it listens on a different \
             port. (Mods' background update checks can also log this — \
             harmless if you weren't joining a server.)",
        recommendation: "Make sure the server is running and the port matches its \
             configuration. For self-hosted servers, confirm the port in \
             server.properties.",
        source_hint: SourceHint::Any,
        side: Side::Any,
    },
    // ---- Worlds & data ---------------------------------------------
    Pattern {
        id: "session-lock",
        matcher: Matcher::Regex(&SESSION_LOCK_RE),
        title: "The world is opened by another process",
        explanation: "This world's session lock is held by another running copy of the \
             game or server, so it can't be opened twice.",
        recommendation: "Close the other Minecraft/server process that has this world open \
             (check the system tray and Task Manager), then try again.",
        source_hint: SourceHint::Any,
        side: Side::Any,
    },
    Pattern {
        id: "chunk-corruption",
        matcher: Matcher::Regex(&CHUNK_CORRUPTION_RE),
        title: "World chunk data is damaged",
        explanation: "The game failed to read part of the world — a region or chunk file \
             is corrupted or missing, usually after a crash or forced shutdown \
             while saving.",
        recommendation: "Restore this world from a backup (Worlds → Backups). Keep playing \
             on a corrupted world only if you accept losing the damaged area.",
        source_hint: SourceHint::Any,
        side: Side::Any,
    },
    Pattern {
        id: "level-dat-corrupt",
        matcher: Matcher::Regex(&LEVEL_DAT_RE),
        title: "The world's level.dat is damaged",
        explanation: "The world's main metadata file couldn't be read — typically after \
             an interrupted save.",
        recommendation: "Restore the world from a backup; Minecraft also keeps \
             level.dat_old next to level.dat, which can be renamed over it to \
             recover most worlds.",
        source_hint: SourceHint::Any,
        side: Side::Any,
    },
    Pattern {
        id: "access-denied",
        matcher: Matcher::Substring("java.nio.file.AccessDeniedException"),
        title: "A file couldn't be accessed",
        explanation: "The operating system blocked a file operation — the file is locked \
             by another program (often antivirus mid-scan) or the folder's \
             permissions are wrong.",
        recommendation: "Close other programs using the folder and add the launcher's data \
             directory to your antivirus exclusions. Avoid OneDrive-synced \
             folders for game data.",
        source_hint: SourceHint::Any,
        side: Side::Any,
    },
    // ---- Shaders (client) ------------------------------------------
    Pattern {
        id: "shader-compile-failed",
        matcher: Matcher::Regex(&SHADER_FAILED_RE),
        title: "The shader pack failed to load",
        explanation: "The shader pack couldn't be compiled for this GPU/driver — it's \
             incompatible with the current Iris/OptiFine version or the pack \
             itself is broken. If no shader pack is installed, the rendering \
             mod or GPU driver itself is at fault.",
        recommendation: "Try a different version of the shader pack (or a different pack), \
             and make sure Iris/OptiFine matches your Minecraft version.",
        source_hint: SourceHint::Any,
        side: Side::Client,
    },
    // ---- Server-only -----------------------------------------------
    Pattern {
        id: "server-eula-not-accepted",
        matcher: Matcher::Substring("agree to the EULA"),
        title: "The Minecraft EULA is not accepted",
        explanation: "A Minecraft server refuses to start until the EULA is accepted in \
             eula.txt.",
        recommendation: "Accept the EULA in the server's settings (or set eula=true in \
             eula.txt) and start it again.",
        source_hint: SourceHint::Any,
        side: Side::Server,
    },
    Pattern {
        id: "server-port-in-use",
        matcher: Matcher::Regex(&SERVER_PORT_RE),
        title: "The server port is already in use",
        explanation: "Another program — usually another copy of this server — is already \
             listening on the configured port.",
        recommendation: "Stop the other process, or change this server's port in its \
             settings and start again.",
        source_hint: SourceHint::Any,
        side: Side::Server,
    },
    Pattern {
        id: "server-out-of-memory",
        matcher: Matcher::Substring("OutOfMemoryError: Java heap space"),
        title: "The server ran out of memory",
        explanation: "The memory allotted to the server wasn't enough — common for heavy \
             modpacks, many players, or large loaded areas.",
        recommendation: "Raise the server's memory in its settings, keeping it under half \
             of the machine's RAM, then start it again.",
        source_hint: SourceHint::Any,
        side: Side::Server,
    },
    // Side::Any despite the section: the integrated singleplayer server
    // prints this exact line into the client's own log.
    Pattern {
        id: "server-cant-keep-up",
        matcher: Matcher::Substring("Can't keep up! Is the server overloaded?"),
        title: "The server is lagging behind",
        explanation: "Game ticks are taking longer than 50 ms, so game time runs slower \
             than real time. In singleplayer this comes from the built-in \
             server inside the game. Not a crash — a performance warning.",
        recommendation: "Reduce the load: fewer or lighter mods, smaller view/simulation \
             distance, or more memory. Occasional single occurrences are \
             harmless.",
        source_hint: SourceHint::Any,
        side: Side::Any,
    },
    Pattern {
        id: "server-watchdog-stall",
        matcher: Matcher::Substring("A single server tick took"),
        title: "A server tick stalled — watchdog triggered",
        explanation: "One tick took far longer than the configured limit, so the \
             watchdog killed the server to break the hang. Usually one mod or \
             one chunk operation is stuck.",
        recommendation: "Check the stack printed below this line for the mod that hung. As \
             a stopgap, max-tick-time in server.properties can be raised, but \
             finding the stuck mod is the real fix.",
        source_hint: SourceHint::Any,
        side: Side::Server,
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logs::diagnose::patterns::PATTERNS;
    use std::collections::HashSet;

    #[test]
    fn inline_ids_unique_and_disjoint_from_banner_table() {
        let mut ids: HashSet<&str> = PATTERNS.iter().map(|p| p.id).collect();
        for p in INLINE_PATTERNS {
            assert!(ids.insert(p.id), "duplicate pattern id: {}", p.id);
        }
    }

    #[test]
    fn inline_regexes_compile() {
        for p in INLINE_PATTERNS {
            if let Matcher::Regex(re) = &p.matcher {
                let _ = Lazy::force(re);
            }
        }
    }

    #[test]
    fn inline_patterns_have_non_empty_copy() {
        for p in INLINE_PATTERNS {
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
    fn inline_titles_under_60_chars() {
        for p in INLINE_PATTERNS {
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
    fn ships_expected_inline_pattern_count() {
        // Bump deliberately when the catalog grows; a surprise change here
        // means an accidental deletion or duplication.
        assert_eq!(INLINE_PATTERNS.len(), 33);
    }
}
