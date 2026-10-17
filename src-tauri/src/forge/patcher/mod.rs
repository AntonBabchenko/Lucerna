//! Forge/NeoForge install processors. Every processor an installer's
//! `install_profile.json` names runs the way Forge's own installer runs it: the
//! processor jar's `Main-Class` on Java, with the jar and its declared
//! classpath (spec 2026-10-16 §5.1). The one exception is `DOWNLOAD_MOJMAPS`
//! (`mojmaps`), which downloads, and Lucerna's downloads all go through
//! `network::`.
//!
//! This replaced pure-Rust reimplementations of jarsplitter, binarypatcher and
//! installertools. Their jars carried the same classes in other bytes, so
//! Forge's declared output checksums could never match.

use crate::error::{Error, Result};
use std::path::PathBuf;

pub mod manifest;
pub mod mojmaps;

#[derive(Debug, Clone)]
pub struct ProcessorContext {
    /// The processor jar; its manifest names the main class.
    pub jar: PathBuf,
    /// The processor jar first, then its declared classpath: the order Forge's
    /// installer builds.
    pub classpath: Vec<PathBuf>,
    pub cache_dir: PathBuf,
    /// The Java to run processors on. `None` falls back to `java` on PATH,
    /// which only the test harnesses use.
    pub java_bin: Option<PathBuf>,
}

/// Run one processor. `coord` is its jar's maven coordinate, `args` are already
/// substituted.
pub async fn run_processor(coord: &str, args: Vec<String>, ctx: &ProcessorContext) -> Result<()> {
    if is_download_mojmaps(coord, &args) {
        return mojmaps::run(coord, &args).await;
    }
    let jar = ctx.jar.clone();
    let main_class = tokio::task::spawn_blocking(move || manifest::jar_main_class(&jar))
        .await
        .map_err(|e| patcher_fail(coord, &e))?
        .map_err(|why| patcher_fail(coord, &why))?;
    let java_bin = ctx
        .java_bin
        .clone()
        .unwrap_or_else(|| PathBuf::from("java"));
    crate::process::run_java_processor(&java_bin, &ctx.classpath, &main_class, &args, coord).await
}

fn is_download_mojmaps(coord: &str, args: &[String]) -> bool {
    let Some((group, artifact, _, _)) = parse_maven_coord(coord) else {
        return false;
    };
    matches!(
        (group.as_str(), artifact.as_str()),
        ("net.minecraftforge", "installertools")
            | ("net.neoforged.installertools", "installertools")
    ) && args
        .windows(2)
        .any(|w| w[0] == "--task" && w[1] == "DOWNLOAD_MOJMAPS")
}

pub fn parse_maven_coord(coord: &str) -> Option<(String, String, String, Option<String>)> {
    let parts: Vec<&str> = coord.split(':').collect();
    if parts.len() < 3 || parts.iter().any(|p| p.is_empty()) {
        return None;
    }
    Some((
        parts[0].to_string(),
        parts[1].to_string(),
        parts[2].to_string(),
        parts.get(3).map(|s| s.to_string()),
    ))
}

/// Resolve a maven coordinate `g:a:v[:classifier][@ext]` into the relative
/// path used by `<libraries_dir>`: `g/path/a/v/a-v[-classifier].<ext>`.
/// Moved here from `forge::installer::legacy` because two eras now
/// need it; legacy.rs re-exports for backward compatibility.
///
/// The `@ext` annotation (e.g. `@jar`, `@zip`) is stripped from the version
/// or classifier field before building the path. NeoForge installertools 2.1.2
/// appends `@jar` to every classpath coord — this function handles that case.
pub fn maven_coord_to_relative_path(coord: &str) -> Option<String> {
    let parts: Vec<&str> = coord.split(':').collect();
    if parts.len() < 3 {
        return None;
    }
    let group_path = parts[0].replace('.', "/");
    let artifact = parts[1];
    // Strip @ext from version; default extension is "jar".
    let (version, ext) = strip_at_ext(parts[2]);
    // Strip @ext from classifier too (if present).
    let classifier = parts
        .get(3)
        .map(|c| {
            let (c_base, _) = strip_at_ext(c);
            format!("-{c_base}")
        })
        .unwrap_or_default();
    if group_path.is_empty() || artifact.is_empty() || version.is_empty() {
        return None;
    }
    Some(format!(
        "{group_path}/{artifact}/{version}/{artifact}-{version}{classifier}.{ext}"
    ))
}

/// Strip the `@ext` suffix from a maven coord segment.
/// Returns `(stripped, ext)` where ext defaults to `"jar"`.
fn strip_at_ext(s: &str) -> (&str, &str) {
    if let Some(at) = s.rfind('@') {
        let (base, ext) = s.split_at(at);
        let ext_val = &ext[1..];
        (base, if ext_val.is_empty() { "jar" } else { ext_val })
    } else {
        (s, "jar")
    }
}

pub fn patcher_fail(processor: &str, cause: &dyn std::fmt::Display) -> Error {
    Error::ForgePatcherFailed {
        processor: processor.to_string(),
        details: cause.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_maven_coord_three_segments() {
        let p = parse_maven_coord("net.md-5:SpecialSource:1.8.5").unwrap();
        assert_eq!(
            (p.0.as_str(), p.1.as_str(), p.2.as_str(), p.3),
            ("net.md-5", "SpecialSource", "1.8.5", None)
        );
    }

    #[test]
    fn parse_maven_coord_four_segments_with_classifier() {
        let p = parse_maven_coord("net.minecraftforge:installertools:1.3.0:executable").unwrap();
        assert_eq!(p.3.as_deref(), Some("executable"));
    }

    #[test]
    fn parse_maven_coord_rejects_short_or_empty() {
        assert!(parse_maven_coord("foo:bar").is_none());
        assert!(parse_maven_coord(":a:1").is_none());
        assert!(parse_maven_coord("g::1").is_none());
        assert!(parse_maven_coord("").is_none());
    }

    #[test]
    fn maven_coord_to_relative_path_three_segments() {
        let p = maven_coord_to_relative_path("net.md-5:SpecialSource:1.8.5");
        assert_eq!(
            p.as_deref(),
            Some("net/md-5/SpecialSource/1.8.5/SpecialSource-1.8.5.jar")
        );
    }

    #[test]
    fn maven_coord_to_relative_path_with_classifier() {
        let p = maven_coord_to_relative_path("org.lwjgl:lwjgl:3.3.1:natives-windows");
        assert_eq!(
            p.as_deref(),
            Some("org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1-natives-windows.jar")
        );
    }

    fn ctx(jar: PathBuf) -> ProcessorContext {
        ProcessorContext {
            classpath: vec![jar.clone()],
            jar,
            cache_dir: PathBuf::from("."),
            java_bin: None,
        }
    }

    #[tokio::test]
    async fn download_mojmaps_is_handled_in_rust_not_java() {
        // An unsupported flag makes the Rust task refuse before any network:
        // reaching that refusal proves the routing.
        let args: Vec<String> = ["--task", "DOWNLOAD_MOJMAPS", "--format", "tsrg2"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        for coord in [
            "net.minecraftforge:installertools:1.4.3",
            "net.neoforged.installertools:installertools:2.1.2",
        ] {
            let err = run_processor(coord, args.clone(), &ctx(PathBuf::from("absent.jar")))
                .await
                .unwrap_err();
            assert!(
                matches!(err, Error::ForgeUnsupportedProcessor { .. }),
                "{coord}: {err:?}"
            );
        }
    }

    #[tokio::test]
    async fn any_other_processor_needs_a_main_class() {
        let td = tempfile::tempdir().unwrap();
        let err = run_processor(
            "com.example:tool:1.0",
            vec![],
            &ctx(td.path().join("absent.jar")),
        )
        .await
        .unwrap_err();
        assert!(
            matches!(err, Error::ForgePatcherFailed { ref processor, .. } if processor == "com.example:tool:1.0"),
            "{err:?}"
        );
    }

    #[test]
    fn patcher_fail_wraps_arbitrary_error() {
        let wrapped = patcher_fail("specialsource", &"timeout".to_string());
        match wrapped {
            Error::ForgePatcherFailed { processor, details } => {
                assert_eq!(processor, "specialsource");
                assert!(details.contains("timeout"));
            }
            other => panic!("got {other:?}"),
        }
    }
}
