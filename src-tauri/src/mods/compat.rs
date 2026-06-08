//! Per-mod compatibility classification for a target (MC version, loader).
//!
//! Pure logic — no I/O. The command layer in `commands.rs` is the thin
//! orchestrator that calls the platform and feeds results here.

use crate::error::Result;
use crate::mods::platform::ModVersion;

/// The compatibility status of one installed mod against a target
/// Minecraft version + loader combination.
#[derive(Debug, Clone, serde::Serialize, specta::Type, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ModCompatStatus {
    /// At least one platform version exists for the target (mc, loader).
    /// `available_version` is the version number of the newest match when
    /// present (versions are returned newest-first by the platform layer).
    Compatible { available_version: Option<String> },
    /// The platform responded successfully but returned zero versions for
    /// the target (mc, loader) — the mod has no release for that combination.
    Incompatible,
    /// The platform query failed (network error, missing CurseForge key,
    /// project delisted / 404). A fetch error must NOT be read as
    /// incompatible — the user should be told we simply don't know.
    Unknown,
}

/// One installed mod's compatibility result for a target (mc, loader).
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct ModCompat {
    /// SHA-1 of the installed jar — the primary identity key in the registry.
    pub sha1: String,
    /// Display name from the registry.
    pub name: String,
    pub status: ModCompatStatus,
}

/// Offline (descriptor-only) compatibility result for one installed mod.
/// Layer 1 of the proactive scan: derived purely from the jar's embedded
/// descriptor, no network. Only loader-family mismatch is reported (see the
/// design's decision 1 — MC-version mismatch is left to the live layer to
/// avoid false positives on version-range declarations).
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct ModLocalCompat {
    /// SHA-1 of the installed jar — the registry's primary key.
    pub sha1: String,
    /// The jar's loader family differs from the instance's family.
    pub loader_mismatch: bool,
    /// Display loader name read from the jar ("Forge"/"Fabric"/…), or `None`
    /// when the jar has no recognised descriptor. Used only for the hint text.
    pub detected_loader: Option<String>,
}

/// Classify a platform `.versions(...)` result for a target (mc, loader).
///
/// - Non-empty `Ok` → [`ModCompatStatus::Compatible`] with the version
///   number of the newest entry (`versions[0].version_number`), since the
///   platform returns versions newest-first.
/// - Empty `Ok` → [`ModCompatStatus::Incompatible`] (platform confirmed no
///   release for that combination).
/// - `Err` → [`ModCompatStatus::Unknown`] (fetch failure; must not be
///   read as incompatible).
pub fn classify_compat(versions: Result<Vec<ModVersion>>) -> ModCompatStatus {
    match versions {
        Ok(v) if !v.is_empty() => ModCompatStatus::Compatible {
            available_version: Some(v[0].version_number.clone()),
        },
        Ok(_) => ModCompatStatus::Incompatible,
        Err(_) => ModCompatStatus::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use crate::mods::platform::{LoaderKind, ModFile, ModSource, ModVersion};

    fn make_version(version_number: &str) -> ModVersion {
        ModVersion {
            source: ModSource::Modrinth,
            project_id: "proj".into(),
            version_id: version_number.into(),
            name: "Test Mod".into(),
            version_number: version_number.into(),
            mc_versions: vec!["1.21.1".into()],
            loaders: vec![LoaderKind::Fabric],
            primary_file: ModFile {
                filename: format!("mod-{version_number}.jar"),
                url: "https://example.com/mod.jar".into(),
                sha1: Some("aa".into()),
                size: 1.0,
                distribution_allowed: true,
            },
            deps: vec![],
            published_at: None,
        }
    }

    #[test]
    fn compatible_when_versions_exist() {
        let versions = vec![make_version("1.2.0"), make_version("1.1.0")];
        let result = classify_compat(Ok(versions));
        assert_eq!(
            result,
            ModCompatStatus::Compatible {
                available_version: Some("1.2.0".into()),
            }
        );
    }

    #[test]
    fn compatible_picks_first_version_as_newest() {
        // Platforms return newest-first; .first() must be the newest.
        let versions = vec![make_version("2.0.0"), make_version("1.0.0")];
        match classify_compat(Ok(versions)) {
            ModCompatStatus::Compatible { available_version } => {
                assert_eq!(available_version, Some("2.0.0".into()));
            }
            other => panic!("expected Compatible, got {other:?}"),
        }
    }

    #[test]
    fn incompatible_when_empty() {
        let result = classify_compat(Ok(vec![]));
        assert_eq!(result, ModCompatStatus::Incompatible);
    }

    #[test]
    fn unknown_on_error() {
        let result = classify_compat(Err(Error::Network {
            url: "https://api.modrinth.com".into(),
            details: "connection refused".into(),
        }));
        assert_eq!(result, ModCompatStatus::Unknown);
    }
}
