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
/// descriptor, no network. Carries two independent verdicts: `loader_mismatch`
/// (a SUSPECT — the live layer may clear it for a platform mod) and
/// `platform_mismatch` (AUTHORITATIVE — read off the jar's own declared
/// `minecraft`/loader ranges, see `mc_compat::platform_verdict`; nothing ever
/// overrides it).
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct ModLocalCompat {
    /// SHA-1 of the installed jar — the registry's primary key.
    pub sha1: String,
    /// The jar's loader family differs from the instance's family.
    pub loader_mismatch: bool,
    /// Display loader name read from the jar ("Forge"/"Fabric"/…), or `None`
    /// when the jar has no recognised descriptor. Used only for the hint text.
    pub detected_loader: Option<String>,
    /// True iff this mod has a platform identity and is not pack-bundled — i.e.
    /// it can be authoritatively re-checked against the platform. The frontend
    /// auto-runs a live check on platform suspects; manual jars (false) rely on
    /// the offline `loader_mismatch` verdict.
    pub live_checkable: bool,
    /// The jar declares a Minecraft or loader range this instance does not
    /// provide. Unlike `loader_mismatch` this is AUTHORITATIVE — it is read
    /// off the jar that will actually be launched, so no live confirmation
    /// applies and a platform "compatible" answer must never clear it.
    pub platform_mismatch: bool,
    /// Which axis fired, for the row hint. `None` unless `platform_mismatch`.
    pub platform_axis: Option<crate::mods::mc_compat::PlatformAxis>,
    /// The range the jar declared, for the row hint.
    pub platform_declared: Option<String>,
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

// =========================================================================
// One classifier behind the chip and the migration plan (2026-09-20 spec)
// =========================================================================

/// What the platform says about ONE installed file for the instance's
/// platform. «Asked and failed» and «never asked» are different facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveAvailability {
    /// This exact file is one of the builds listed for (mc, probe loader).
    FileListed,
    /// Builds are listed, this file is not among them.
    OtherBuildsOnly,
    /// The platform answered with no build at all.
    NoBuilds,
    /// Asked, and the query failed (offline, no CurseForge key, 404).
    Unreachable,
    /// Never asked: no identity, pack-owned, unreadable jar, Vanilla instance.
    NotAsked,
}

/// The platform's raw answer, as the command layer holds it.
pub enum ProbeAnswer<'a> {
    NotAsked,
    Failed,
    Found(&'a [ModVersion]),
}

/// The file on disk, as far as it can be matched against a platform listing.
pub struct InstalledFile<'a> {
    /// `installed::on_disk_sha1` — lowercase hex, `None` = could not tell.
    pub on_disk_sha1: Option<&'a str>,
    /// The registry's EXPECTED digest for this record.
    pub registry_sha1: &'a str,
    pub registry_version_id: Option<&'a str>,
}

/// Why a mod can or cannot be asked about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityKind {
    Project,
    PackOwned,
    NoModPage,
}

/// Why a mod could not be judged either way. Crosses IPC — each variant is a
/// distinct real state with its own copy; a missing CurseForge key must never
/// read as a claim about the mod.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum UnjudgedReason {
    Unreadable,
    PackOwned,
    NoModPage,
    NoLoader,
    PlatformUnavailable,
    FileNotListed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModPlatformClass {
    /// Nothing on the instance opens the jar (offline-authoritative).
    Rejected,
    /// The jar declares a range the instance does not provide (offline-authoritative).
    Violated,
    /// The mod's page lists no build for this platform and the jar makes no
    /// bounded statement of its own. Probable, never proven.
    NoPlatformBuild,
    Fits,
    Unjudged(UnjudgedReason),
}

pub struct ClassifyFacts<'a> {
    pub readable: bool,
    pub rejected: bool,
    pub verdict: &'a crate::mods::mc_compat::PlatformVerdict,
    pub mc_fit_bounded: bool,
    pub identity: IdentityKind,
    pub availability: LiveAvailability,
}

pub fn live_availability(_file: &InstalledFile<'_>, _answer: ProbeAnswer<'_>) -> LiveAvailability {
    LiveAvailability::NotAsked // stub — red round
}

pub fn classify(_facts: &ClassifyFacts<'_>) -> ModPlatformClass {
    ModPlatformClass::Fits // stub — red round
}

/// The chip's projection of a class.
pub fn compat_status(_class: ModPlatformClass, _newest: Option<String>) -> ModCompatStatus {
    ModCompatStatus::Unknown // stub — red round
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
                sha256: None,
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

    // ── D4: live_availability ───────────────────────────────────────────────
    const DISK: &str = "459b5f4c7297b2f7649d43137f3e5a069b69b707";

    fn listed(version_id: &str, sha1: Option<&str>) -> ModVersion {
        let mut v = make_version(version_id);
        v.version_id = version_id.into();
        v.primary_file.sha1 = sha1.map(str::to_string);
        v
    }

    fn file<'a>(on_disk: Option<&'a str>, version_id: Option<&'a str>) -> InstalledFile<'a> {
        InstalledFile {
            on_disk_sha1: on_disk,
            registry_sha1: DISK,
            registry_version_id: version_id,
        }
    }

    #[test]
    fn a_listed_primary_file_is_matched_by_sha_ignoring_case() {
        let vs = vec![listed(
            "v1",
            Some("459B5F4C7297B2F7649D43137F3E5A069B69B707"),
        )];
        assert_eq!(
            live_availability(&file(Some(DISK), None), ProbeAnswer::Found(&vs)),
            LiveAvailability::FileListed
        );
    }

    #[test]
    fn a_non_primary_or_hashless_file_is_matched_by_version_id() {
        // Route (b): CurseForge may publish no sha1; a multi-file version's
        // primary file may not be ours. The record must still describe the disk.
        let vs = vec![listed("v1", None)];
        assert_eq!(
            live_availability(&file(Some(DISK), Some("v1")), ProbeAnswer::Found(&vs)),
            LiveAvailability::FileListed
        );
    }

    #[test]
    fn a_stale_record_never_vouches_for_a_replaced_file() {
        let vs = vec![listed("v1", None)];
        let replaced = InstalledFile {
            on_disk_sha1: Some("ffff"),
            registry_sha1: DISK,
            registry_version_id: Some("v1"),
        };
        assert_eq!(
            live_availability(&replaced, ProbeAnswer::Found(&vs)),
            LiveAvailability::OtherBuildsOnly
        );
        // No on-disk sha at all: could not tell, so nothing is confirmed.
        assert_eq!(
            live_availability(&file(None, Some("v1")), ProbeAnswer::Found(&vs)),
            LiveAvailability::OtherBuildsOnly
        );
    }

    #[test]
    fn empty_failed_and_unasked_answers_stay_distinct() {
        let other = vec![listed("v9", Some("aaaa"))];
        let f = file(Some(DISK), Some("v1"));
        assert_eq!(
            live_availability(&f, ProbeAnswer::Found(&other)),
            LiveAvailability::OtherBuildsOnly
        );
        assert_eq!(
            live_availability(&f, ProbeAnswer::Found(&[])),
            LiveAvailability::NoBuilds
        );
        assert_eq!(
            live_availability(&f, ProbeAnswer::Failed),
            LiveAvailability::Unreachable
        );
        assert_eq!(
            live_availability(&f, ProbeAnswer::NotAsked),
            LiveAvailability::NotAsked
        ); // (pin under the stub)
    }

    // ── classify ────────────────────────────────────────────────────────────
    use crate::mods::mc_compat::{PlatformAxis, PlatformVerdict};

    fn violated() -> PlatformVerdict {
        PlatformVerdict::Violated {
            axis: PlatformAxis::Minecraft,
            declared: "[1.20,1.21)".into(),
            actual: "1.21.1".into(),
            source: crate::mods::local::DescriptorSource::NeoForgeToml,
            family: crate::mods::version_range::RangeFamily::Maven,
        }
    }

    fn facts(verdict: &PlatformVerdict, availability: LiveAvailability) -> ClassifyFacts<'_> {
        ClassifyFacts {
            readable: true,
            rejected: false,
            verdict,
            mc_fit_bounded: false,
            identity: IdentityKind::Project,
            availability,
        }
    }

    #[test]
    fn offline_proof_outranks_everything_live() {
        let v = violated();
        let fits = PlatformVerdict::Fits;
        assert_eq!(
            classify(&facts(&v, LiveAvailability::FileListed)),
            ModPlatformClass::Violated
        );
        assert_eq!(
            classify(&ClassifyFacts {
                rejected: true,
                ..facts(&fits, LiveAvailability::FileListed)
            }),
            ModPlatformClass::Rejected
        );
        assert_eq!(
            classify(&ClassifyFacts {
                readable: false,
                ..facts(&fits, LiveAvailability::NoBuilds)
            }),
            ModPlatformClass::Unjudged(UnjudgedReason::Unreadable)
        );
    }

    #[test]
    fn no_builds_flags_only_a_jar_that_makes_no_bounded_statement() {
        let fits = PlatformVerdict::Fits;
        let unknown = PlatformVerdict::Unknown;
        // Jade 1.20.1 on MC 1.21: open range, nothing published → flagged.
        assert_eq!(
            classify(&facts(&fits, LiveAvailability::NoBuilds)),
            ModPlatformClass::NoPlatformBuild
        );
        assert_eq!(
            classify(&facts(&unknown, LiveAvailability::NoBuilds)),
            ModPlatformClass::NoPlatformBuild
        );
        // Eating Animations: `[1.21.0,1.22)` on 1.21.1, page tagged `1.21` only → fits.
        assert_eq!(
            classify(&ClassifyFacts {
                mc_fit_bounded: true,
                ..facts(&fits, LiveAvailability::NoBuilds)
            }),
            ModPlatformClass::Fits
        );
    }

    #[test]
    fn a_fits_verdict_is_never_relabelled_by_a_weak_or_failed_answer() {
        // (pin under the stub)
        let fits = PlatformVerdict::Fits;
        for a in [
            LiveAvailability::FileListed,
            LiveAvailability::OtherBuildsOnly,
            LiveAvailability::Unreachable,
            LiveAvailability::NotAsked,
        ] {
            assert_eq!(classify(&facts(&fits, a)), ModPlatformClass::Fits, "{a:?}");
        }
    }

    #[test]
    fn an_unknown_jar_is_judged_by_the_listing_and_otherwise_named_with_the_true_reason() {
        let u = PlatformVerdict::Unknown;
        assert_eq!(
            classify(&facts(&u, LiveAvailability::FileListed)),
            ModPlatformClass::Fits
        );
        assert_eq!(
            classify(&facts(&u, LiveAvailability::OtherBuildsOnly)),
            ModPlatformClass::Unjudged(UnjudgedReason::FileNotListed)
        );
        assert_eq!(
            classify(&facts(&u, LiveAvailability::Unreachable)),
            ModPlatformClass::Unjudged(UnjudgedReason::PlatformUnavailable)
        );
        for (identity, reason) in [
            (IdentityKind::PackOwned, UnjudgedReason::PackOwned),
            (IdentityKind::NoModPage, UnjudgedReason::NoModPage),
            // A known project that was NOT asked: only a Vanilla instance does that.
            (IdentityKind::Project, UnjudgedReason::NoLoader),
        ] {
            assert_eq!(
                classify(&ClassifyFacts {
                    identity,
                    ..facts(&u, LiveAvailability::NotAsked)
                }),
                ModPlatformClass::Unjudged(reason)
            );
        }
    }

    #[test]
    fn the_chip_flags_exactly_the_no_build_class() {
        assert_eq!(
            compat_status(ModPlatformClass::NoPlatformBuild, None),
            ModCompatStatus::Incompatible
        );
        assert_eq!(
            compat_status(ModPlatformClass::Fits, Some("1.2.0".into())),
            ModCompatStatus::Compatible {
                available_version: Some("1.2.0".into())
            }
        );
        for c in [
            ModPlatformClass::Rejected,
            ModPlatformClass::Violated,
            ModPlatformClass::Unjudged(UnjudgedReason::NoModPage),
        ] {
            assert_eq!(compat_status(c, None), ModCompatStatus::Unknown, "{c:?}");
            // (pin under the stub)
        }
    }
}
