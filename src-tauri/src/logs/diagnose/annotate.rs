//! Per-line log annotation for inline hover hints. Pure functions — no
//! I/O. Scans `patterns::PATTERNS` (banner patterns double as inline
//! hints) followed by `inline_patterns::INLINE_PATTERNS`, filtered by
//! `Side`. First match wins within a line; table order = priority.

use super::inline_patterns::INLINE_PATTERNS;
use super::patterns::{Pattern, Side, PATTERNS};
use serde::{Deserialize, Serialize};
use specta::Type;

/// Hard cap on annotations per call. Spammy logs (e.g. a GL error every
/// frame) would otherwise produce hundreds of thousands of identical
/// hits; the badge is an affordance, not an audit trail, so silent
/// truncation is by design.
pub const MAX_ANNOTATIONS: usize = 1000;

/// Which surface is asking. Command input — maps onto `Side` scoping.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnnotateSide {
    Client,
    Server,
}

impl AnnotateSide {
    fn admits(self, side: Side) -> bool {
        match self {
            AnnotateSide::Client => side.matches_client(),
            AnnotateSide::Server => side.matches_server(),
        }
    }
}

/// One annotated line: 0-based index into the text split by '\n'.
#[derive(Debug, Clone, Serialize, Type)]
pub struct LineAnnotation {
    pub line: u32,
    pub pattern_id: String,
}

/// English fallback copy for one matched pattern — mirrors how
/// `Diagnosis` ships its strings so the UI never renders an empty card
/// when an i18n key lags behind the Rust catalog.
#[derive(Debug, Clone, Serialize, Type)]
pub struct AnnotatedPatternCopy {
    pub id: String,
    pub title: String,
    pub explanation: String,
    pub recommendation: String,
}

/// The wire result: per-line hits plus copy for each distinct pattern
/// that matched (deduplicated — spam-heavy logs stay cheap).
#[derive(Debug, Clone, Serialize, Type)]
pub struct AnnotateResult {
    pub annotations: Vec<LineAnnotation>,
    pub patterns: Vec<AnnotatedPatternCopy>,
}

/// Scan `text` line-by-line. `u32` line indices match a `text.split('\n')`
/// on the consumer side (Rust `lines()` and a trailing-newline JS split
/// agree on every index that can carry an annotation).
pub fn annotate_lines(text: &str, side: AnnotateSide) -> AnnotateResult {
    let eligible: Vec<&Pattern> = PATTERNS
        .iter()
        .chain(INLINE_PATTERNS.iter())
        .filter(|p| side.admits(p.side))
        .collect();

    let mut annotations: Vec<LineAnnotation> = Vec::new();
    let mut matched: Vec<&Pattern> = Vec::new();

    for (i, line) in text.lines().enumerate() {
        if annotations.len() >= MAX_ANNOTATIONS {
            break;
        }
        if line.is_empty() {
            continue;
        }
        for p in &eligible {
            if p.matcher.find(line).is_some() {
                annotations.push(LineAnnotation {
                    line: i as u32,
                    pattern_id: p.id.into(),
                });
                if !matched.iter().any(|m| m.id == p.id) {
                    matched.push(p);
                }
                break; // first match wins within a line
            }
        }
    }

    AnnotateResult {
        annotations,
        patterns: matched
            .into_iter()
            .map(|p| AnnotatedPatternCopy {
                id: p.id.into(),
                title: p.title.into(),
                explanation: p.explanation.into(),
                recommendation: p.recommendation.into(),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids_for(text: &str, side: AnnotateSide) -> Vec<(u32, String)> {
        annotate_lines(text, side)
            .annotations
            .into_iter()
            .map(|a| (a.line, a.pattern_id))
            .collect()
    }

    // --- Engine behavior ------------------------------------------

    #[test]
    fn annotates_correct_line_indices() {
        let text = "line zero\n\
                    java.lang.OutOfMemoryError: Metaspace\n\
                    line two\n\
                    GC overhead limit exceeded";
        let got = ids_for(text, AnnotateSide::Client);
        assert_eq!(
            got,
            vec![
                (1, "oom-metaspace".to_string()),
                (3, "oom-gc-overhead".to_string())
            ]
        );
    }

    #[test]
    fn first_match_wins_within_a_line_banner_table_first() {
        // A line matching a PATTERNS entry AND an inline entry must resolve
        // to the banner-table pattern (declaration precedence).
        let text = "Error rendering overlay 'create:goggle_info' while The game crashed whilst rendering overlay";
        let got = ids_for(text, AnnotateSide::Client);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].1, "create-goggle-overlay-crash");
    }

    #[test]
    fn client_side_filter_excludes_server_patterns() {
        let got = ids_for(
            "You need to agree to the EULA in order to run the server",
            AnnotateSide::Client,
        );
        assert!(
            got.is_empty(),
            "server-only pattern fired on client: {got:?}"
        );
    }

    #[test]
    fn server_side_filter_excludes_client_patterns() {
        // `out-of-memory` (banner table) is Client; the server side must
        // resolve the same line to `server-out-of-memory` instead — this is
        // the regression test for "the annotator MUST side-filter the banner
        // table too" (see the NOTE above PATTERNS).
        let got = ids_for(
            "java.lang.OutOfMemoryError: Java heap space",
            AnnotateSide::Server,
        );
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].1, "server-out-of-memory");
    }

    #[test]
    fn client_oom_resolves_to_banner_pattern() {
        let got = ids_for(
            "java.lang.OutOfMemoryError: Java heap space",
            AnnotateSide::Client,
        );
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].1, "out-of-memory");
    }

    #[test]
    fn caps_annotations_on_spammy_logs() {
        let text = "OpenGL error 1281\n".repeat(5000);
        let r = annotate_lines(&text, AnnotateSide::Client);
        assert_eq!(r.annotations.len(), MAX_ANNOTATIONS);
        // Copy is deduplicated: one pattern entry despite 1000 hits.
        assert_eq!(r.patterns.len(), 1);
        assert_eq!(r.patterns[0].id, "opengl-error-spam");
    }

    #[test]
    fn returns_fallback_copy_for_each_distinct_pattern() {
        let text = "GC overhead limit exceeded\n\
                    java.net.UnknownHostException: play.example.com\n\
                    GC overhead limit exceeded";
        let r = annotate_lines(text, AnnotateSide::Client);
        assert_eq!(r.annotations.len(), 3);
        let mut ids: Vec<&str> = r.patterns.iter().map(|p| p.id.as_str()).collect();
        ids.sort_unstable();
        assert_eq!(ids, vec!["dns-unknown-host", "oom-gc-overhead"]);
        assert!(r.patterns.iter().all(|p| !p.title.is_empty()));
    }

    #[test]
    fn no_annotations_on_clean_log() {
        let text = "[12:00:00] [main/INFO]: Loading Minecraft 1.21\n\
                    [12:00:05] [main/INFO]: Done (4.2s)!";
        assert!(ids_for(text, AnnotateSide::Client).is_empty());
        assert!(ids_for(text, AnnotateSide::Server).is_empty());
    }

    // --- Catalog fixtures: one positive per inline pattern (plus two
    // banner-table side cases), and negatives for the generic matchers.
    // Table-driven so the list stays greppable; the (line, side,
    // expected-id) triple is the contract.

    const CATALOG_FIXTURES: &[(&str, AnnotateSide, &str)] = &[
        (
            "net.minecraftforge.fml.loading.moddiscovery.DuplicateModsFoundException: Found duplicate mods",
            AnnotateSide::Client,
            "forge-duplicate-mods",
        ),
        (
            "Missing or unsupported mandatory dependencies:",
            AnnotateSide::Client,
            "forge-missing-dependencies",
        ),
        (
            "net.fabricmc.loader.impl.FormattedException: Mod resolution failed ... Unmet dependency listing:",
            AnnotateSide::Client,
            "fabric-unmet-dependency",
        ),
        (
            "org.spongepowered.asm.mixin.transformer.throwables.MixinTransformerError: An unexpected critical error was encountered",
            AnnotateSide::Client,
            "mixin-apply-failed",
        ),
        (
            "Mixin apply failed somemod.mixins.json:MinecraftMixin",
            AnnotateSide::Client,
            "mixin-apply-failed",
        ),
        (
            "net.fabricmc.loader.impl.metadata.ParseMetadataException: Missing required field 'id'",
            AnnotateSide::Client,
            "broken-mod-metadata",
        ),
        (
            "---- Minecraft Crash Report ---- The game crashed whilst rendering overlay",
            AnnotateSide::Client,
            "overlay-render-crash",
        ),
        (
            "[main/ERROR]: Errors in currently selected datapacks prevented the world from loading",
            AnnotateSide::Client,
            "datapack-load-failed",
        ),
        (
            "Caused by: java.lang.NoSuchMethodError: 'void net.minecraft.client.Minecraft.m_91152_()'",
            AnnotateSide::Client,
            "mod-linkage-error",
        ),
        (
            "Caused by: java.lang.NoClassDefFoundError: net/fabricmc/fabric/api/item/v1/FabricItemSettings",
            AnnotateSide::Client,
            "mod-linkage-error",
        ),
        (
            "java.lang.OutOfMemoryError: Metaspace",
            AnnotateSide::Client,
            "oom-metaspace",
        ),
        (
            "java.lang.OutOfMemoryError: GC overhead limit exceeded",
            AnnotateSide::Client,
            "oom-gc-overhead",
        ),
        (
            "Error occurred during initialization of VM: Could not reserve enough space for 1048576KB object heap",
            AnnotateSide::Client,
            "heap-reserve-failed",
        ),
        (
            "Invalid maximum heap size: -Xmx8G0",
            AnnotateSide::Client,
            "heap-reserve-failed",
        ),
        (
            "Unrecognized VM option 'UseConcMarkSweepGC'",
            AnnotateSide::Client,
            "bad-jvm-arg",
        ),
        (
            "Caused by: java.lang.UnsatisfiedLinkError: Failed to locate library: lwjgl.dll",
            AnnotateSide::Client,
            "natives-link-error",
        ),
        (
            "# A fatal error has been detected by the Java Runtime Environment:",
            AnnotateSide::Client,
            "jvm-native-crash",
        ),
        (
            "#  EXCEPTION_ACCESS_VIOLATION (0xc0000005) at pc=0x00007ffd, pid=1234",
            AnnotateSide::Client,
            "jvm-native-crash",
        ),
        (
            "[LWJGL] GLFW error 65542: WGL: The driver does not appear to support OpenGL",
            AnnotateSide::Client,
            "glfw-no-opengl",
        ),
        (
            "Caused by: org.lwjgl.LWJGLException: Pixel format not accelerated",
            AnnotateSide::Client,
            "pixel-format-not-accelerated",
        ),
        (
            "[Render thread/ERROR]: OpenGL error 1281 (Invalid value)",
            AnnotateSide::Client,
            "opengl-error-spam",
        ),
        (
            "[Render thread/INFO]: Disconnected: Invalid session (Try restarting your game and the launcher)",
            AnnotateSide::Client,
            "invalid-session",
        ),
        (
            "[Server thread/INFO]: com.mojang.authlib: Failed to verify username Steve!",
            AnnotateSide::Server,
            "invalid-session",
        ),
        (
            "[Render thread/INFO]: Disconnected: You are not white-listed on this server!",
            AnnotateSide::Client,
            "not-whitelisted",
        ),
        (
            "java.net.UnknownHostException: mc.example.org",
            AnnotateSide::Client,
            "dns-unknown-host",
        ),
        (
            "javax.net.ssl.SSLHandshakeException: PKIX path building failed",
            AnnotateSide::Client,
            "ssl-handshake-failed",
        ),
        (
            "java.net.ConnectException: Connection timed out: no further information",
            AnnotateSide::Client,
            "connection-timed-out",
        ),
        (
            "io.netty.handler.timeout.ReadTimeoutException",
            AnnotateSide::Client,
            "connection-timed-out",
        ),
        (
            "java.net.ConnectException: Connection refused: no further information",
            AnnotateSide::Client,
            "connection-refused",
        ),
        (
            "[Server thread/WARN]: Failed to check session lock for world ./world, aborting",
            AnnotateSide::Client,
            "session-lock",
        ),
        (
            "The save is being accessed from another location, aborting",
            AnnotateSide::Client,
            "session-lock",
        ),
        (
            "[Server thread/ERROR]: Failed to read chunk [12, -4]",
            AnnotateSide::Client,
            "chunk-corruption",
        ),
        (
            "[Server thread/WARN]: Chunk file at [4,9] is in the wrong location; relocating.",
            AnnotateSide::Client,
            "chunk-corruption",
        ),
        (
            "[Server thread/ERROR]: Exception reading C:\\worlds\\demo\\level.dat",
            AnnotateSide::Client,
            "level-dat-corrupt",
        ),
        (
            "Caused by: java.nio.file.AccessDeniedException: C:\\instances\\pack\\mods\\a.jar",
            AnnotateSide::Client,
            "access-denied",
        ),
        (
            "[Iris] Failed to compile the shader pack: Sildur's Vibrant",
            AnnotateSide::Client,
            "shader-compile-failed",
        ),
        (
            "[main/INFO]: You need to agree to the EULA in order to run the server.",
            AnnotateSide::Server,
            "server-eula-not-accepted",
        ),
        (
            "[Server thread/WARN]: **** FAILED TO BIND TO PORT!",
            AnnotateSide::Server,
            "server-port-in-use",
        ),
        (
            "java.lang.OutOfMemoryError: Java heap space",
            AnnotateSide::Server,
            "server-out-of-memory",
        ),
        (
            "[Server thread/WARN]: Can't keep up! Is the server overloaded? Running 2500ms or 50 ticks behind",
            AnnotateSide::Server,
            "server-cant-keep-up",
        ),
        (
            "[Server thread/WARN]: Can't keep up! Is the server overloaded? Running 2500ms or 50 ticks behind",
            AnnotateSide::Client,
            "server-cant-keep-up",
        ),
        (
            "[Server Watchdog/FATAL]: A single server tick took 60.00 seconds (should be max 0.05)",
            AnnotateSide::Server,
            "server-watchdog-stall",
        ),
        (
            "java.io.IOException: No space left on device",
            AnnotateSide::Server,
            "disk-full",
        ),
    ];

    #[test]
    fn catalog_positive_fixtures_resolve_to_expected_pattern() {
        for (line, side, expected) in CATALOG_FIXTURES {
            let got = ids_for(line, *side);
            assert_eq!(
                got.len(),
                1,
                "fixture {expected:?} matched {} times: {line:?}",
                got.len()
            );
            assert_eq!(&got[0].1, expected, "line: {line:?}");
        }
    }

    #[test]
    fn every_inline_pattern_has_a_positive_fixture() {
        use crate::logs::diagnose::inline_patterns::INLINE_PATTERNS;
        for p in INLINE_PATTERNS {
            assert!(
                CATALOG_FIXTURES.iter().any(|(_, _, id)| *id == p.id),
                "inline pattern {} has no positive fixture",
                p.id
            );
        }
    }

    // Negative fixtures: near-miss lines that must NOT annotate. One per
    // generic/loose matcher — the specific substrings are self-evidently
    // negative-safe.
    const NEGATIVE_FIXTURES: &[(&str, AnnotateSide)] = &[
        // linkage: an ordinary exception is not a linkage error
        (
            "java.lang.IllegalStateException: ticking entity",
            AnnotateSide::Client,
        ),
        // gl-error regex requires a digit / banner marker
        (
            "[Render thread/INFO]: OpenGL error handling enabled",
            AnnotateSide::Client,
        ),
        // "Connection reset" is neither timeout nor refused
        (
            "java.net.SocketException: Connection reset",
            AnnotateSide::Client,
        ),
        // shader regex must not fire on successful load lines
        (
            "[Iris] Shader pack loaded successfully",
            AnnotateSide::Client,
        ),
        // watchdog phrasing must not fire on the settings echo
        ("[main/INFO]: max-tick-time=60000", AnnotateSide::Server),
        // chunk saving info lines must not read as corruption
        (
            "[Server thread/INFO]: Saving chunks for level 'world'",
            AnnotateSide::Server,
        ),
        // stack frames mentioning session classes must not fire session-lock
        (
            "\tat net.minecraft.server.MinecraftServer.run(MinecraftServer.java:666)",
            AnnotateSide::Server,
        ),
    ];

    #[test]
    fn negative_fixtures_do_not_annotate() {
        for (line, side) in NEGATIVE_FIXTURES {
            let got = ids_for(line, *side);
            assert!(
                got.is_empty(),
                "unexpected match {got:?} for line: {line:?}"
            );
        }
    }
}
