//! Forge processor subsystem. Each install_profile's `processors[]`
//! lists `(jar, args)` pairs; this module dispatches each `jar`
//! (a maven coord) to the appropriate implementation.
//!
//! Unknown coords return `Error::ForgeUnsupportedProcessor`. Phase 2
//! supports four processors:
//!   - net.minecraftforge:installertools (sub-command via --task) — pure-Rust
//!   - net.minecraftforge:jarsplitter — pure-Rust
//!   - net.md-5:SpecialSource — Java subprocess (byte-fidelity required)
//!   - net.minecraftforge:binarypatcher — pure-Rust

use crate::error::{Error, Result};
use std::path::PathBuf;

pub mod installertools;
pub mod jarsplitter;
pub mod specialsource;
pub mod binarypatcher;

#[derive(Debug, Clone)]
pub struct ProcessorContext {
    pub classpath: Vec<PathBuf>,
    pub cache_dir: PathBuf,
    /// Path to the `java` (or `javaw`) executable to use for processors
    /// that require a Java subprocess (e.g. SpecialSource). If `None`,
    /// `specialsource::run` falls back to `"java"` on PATH.
    /// Populated by `transitional::install` after `ensure_jre` succeeds.
    pub java_bin: Option<PathBuf>,
}

pub async fn run_processor(
    coord: &str,
    args: Vec<String>,
    ctx: &ProcessorContext,
) -> Result<()> {
    let parsed = parse_maven_coord(coord).ok_or_else(|| Error::ForgePatcherFailed {
        processor: coord.to_string(),
        details: format!("could not parse maven coordinate: {coord}"),
    })?;
    let (group, artifact, _version, classifier) = parsed;

    match (group.as_str(), artifact.as_str(), classifier.as_deref()) {
        ("net.md-5", "SpecialSource", _) => specialsource::run(args, ctx).await,
        ("net.minecraftforge", "installertools", c) => installertools::run(c, args, ctx).await,
        ("net.minecraftforge", "jarsplitter", _) => jarsplitter::run(args, ctx).await,
        ("net.minecraftforge", "binarypatcher", _) => binarypatcher::run(args, ctx).await,
        _ => Err(Error::ForgeUnsupportedProcessor {
            coord: coord.to_string(),
        }),
    }
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

/// Resolve a maven coordinate `g:a:v[:classifier]` into the relative
/// path used by `<libraries_dir>`: `g/path/a/v/a-v[-classifier].jar`.
/// Moved here from `forge::installer::legacy` because two eras now
/// need it; legacy.rs re-exports for backward compatibility.
pub fn maven_coord_to_relative_path(coord: &str) -> Option<String> {
    let parts: Vec<&str> = coord.split(':').collect();
    if parts.len() < 3 {
        return None;
    }
    let group_path = parts[0].replace('.', "/");
    let artifact = parts[1];
    let version = parts[2];
    let classifier = parts.get(3).map(|c| format!("-{c}")).unwrap_or_default();
    if group_path.is_empty() || artifact.is_empty() || version.is_empty() {
        return None;
    }
    Some(format!(
        "{group_path}/{artifact}/{version}/{artifact}-{version}{classifier}.jar"
    ))
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
        assert_eq!((p.0.as_str(), p.1.as_str(), p.2.as_str(), p.3), ("net.md-5", "SpecialSource", "1.8.5", None));
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
        assert_eq!(p.as_deref(), Some("net/md-5/SpecialSource/1.8.5/SpecialSource-1.8.5.jar"));
    }

    #[test]
    fn maven_coord_to_relative_path_with_classifier() {
        let p = maven_coord_to_relative_path("org.lwjgl:lwjgl:3.3.1:natives-windows");
        assert_eq!(p.as_deref(), Some("org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1-natives-windows.jar"));
    }

    #[tokio::test]
    async fn unknown_coord_returns_unsupported_processor() {
        let ctx = ProcessorContext { classpath: vec![], cache_dir: PathBuf::from("."), java_bin: None };
        let err = run_processor("com.example:unknown:1.0", vec![], &ctx).await.unwrap_err();
        assert!(matches!(err, Error::ForgeUnsupportedProcessor { .. }));
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
