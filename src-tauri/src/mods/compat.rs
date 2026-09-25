//! Per-mod compatibility classification for a target (MC version, loader).
//!
//! Pure logic — no I/O. The command layer in `commands.rs` is the thin
//! orchestrator that calls the platform and feeds results here.

use crate::mods::platform::{LoaderKind, ModVersion};

/// The LIVE half of one installed mod's compatibility — the chip's projection
/// of [`ModPlatformClass`] (see [`compat_status`]). The offline half
/// (`loader_mismatch` / `platform_mismatch`) travels in [`ModLocalCompat`].
#[derive(Debug, Clone, serde::Serialize, specta::Type, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ModCompatStatus {
    /// The mod fits: its jar says so, or it declares nothing and the platform
    /// lists this exact file for the instance's (mc, loader).
    /// `available_version` is the newest listed version number, when the
    /// platform was asked and answered with any.
    Compatible { available_version: Option<String> },
    /// The mod's page lists no build for the instance's (mc, loader) AND the
    /// jar makes no bounded statement of its own. Probable, never proven —
    /// the UI words it as «no release», not as «the loader will reject it».
    Incompatible,
    /// Nothing to add from the live side: the query failed or was never made
    /// (no identity, pack-owned, unreadable jar, Vanilla instance), builds
    /// exist but not this file, or the offline scan already flags the jar. A
    /// failed query must NOT be read as incompatible.
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

/// D4. «Builds exist» is not a judgement on the FILE; «this file is one of
/// them» is. Judged against a FRESH list for the instance's CURRENT platform —
/// a stored `version_id` alone is never evidence: it was decided at enrich
/// time and goes stale across the version change this feature exists for.
pub fn live_availability(file: &InstalledFile<'_>, answer: ProbeAnswer<'_>) -> LiveAvailability {
    let versions = match answer {
        ProbeAnswer::NotAsked => return LiveAvailability::NotAsked,
        ProbeAnswer::Failed => return LiveAvailability::Unreachable,
        ProbeAnswer::Found([]) => return LiveAvailability::NoBuilds,
        ProbeAnswer::Found(v) => v,
    };
    // (a) the primary file IS the file on disk. Case-insensitive: our digest
    //     is lowercase hex, the platforms do not normalise theirs.
    let by_sha = file.on_disk_sha1.is_some_and(|disk| {
        versions.iter().any(|v| {
            v.primary_file
                .sha1
                .as_deref()
                .is_some_and(|s| s.eq_ignore_ascii_case(disk))
        })
    });
    // (b) the registry's version is listed AND the record still describes the
    //     bytes on disk. A full alternative, not a fallback: it is what covers
    //     a non-primary file of a multi-file version and a CurseForge file
    //     published without a sha1.
    let record_describes_disk = file
        .on_disk_sha1
        .is_some_and(|disk| disk.eq_ignore_ascii_case(file.registry_sha1));
    let by_version = record_describes_disk
        && file
            .registry_version_id
            .is_some_and(|vid| versions.iter().any(|v| v.version_id == vid));
    if by_sha || by_version {
        LiveAvailability::FileListed
    } else {
        LiveAvailability::OtherBuildsOnly
    }
}

/// Decide one enabled mod's class. Pure; the order of the arms IS the policy.
pub fn classify(f: &ClassifyFacts<'_>) -> ModPlatformClass {
    use crate::mods::mc_compat::PlatformVerdict;
    // Nothing could be read: nothing may be claimed, in either direction.
    if !f.readable {
        return ModPlatformClass::Unjudged(UnjudgedReason::Unreadable);
    }
    // Offline proof outranks every live answer — a project-level «compatible»
    // says nothing about the FILE that will actually be launched.
    if f.rejected {
        return ModPlatformClass::Rejected;
    }
    if matches!(f.verdict, PlatformVerdict::Violated { .. }) {
        return ModPlatformClass::Violated;
    }
    // The page lists nothing for this platform AND the jar makes no bounded
    // statement of its own (D5). Probable, never proven — shown as such.
    if f.availability == LiveAvailability::NoBuilds && !f.mc_fit_bounded {
        return ModPlatformClass::NoPlatformBuild;
    }
    if matches!(f.verdict, PlatformVerdict::Fits) {
        // A failed or weak answer never relabels a fit mod.
        return ModPlatformClass::Fits;
    }
    // `Unknown` verdict: the jar declares nothing decidable.
    match f.availability {
        LiveAvailability::FileListed => ModPlatformClass::Fits,
        LiveAvailability::OtherBuildsOnly => {
            ModPlatformClass::Unjudged(UnjudgedReason::FileNotListed)
        }
        LiveAvailability::Unreachable => {
            ModPlatformClass::Unjudged(UnjudgedReason::PlatformUnavailable)
        }
        // Reached only with `mc_fit_bounded`, which an `Unknown` verdict never
        // has (bounded requires a declaration that held, i.e. `Fits`). Kept for
        // exhaustiveness, and it errs toward telling the user.
        LiveAvailability::NoBuilds => ModPlatformClass::NoPlatformBuild,
        LiveAvailability::NotAsked => ModPlatformClass::Unjudged(match f.identity {
            IdentityKind::PackOwned => UnjudgedReason::PackOwned,
            IdentityKind::NoModPage => UnjudgedReason::NoModPage,
            // A known project that was not asked: only a Vanilla instance.
            IdentityKind::Project => UnjudgedReason::NoLoader,
        }),
    }
}

/// The chip's projection of a class. `Rejected` / `Violated` are flagged by the
/// OFFLINE scan, so their live status is deliberately silent.
pub fn compat_status(class: ModPlatformClass, newest: Option<String>) -> ModCompatStatus {
    match class {
        ModPlatformClass::NoPlatformBuild => ModCompatStatus::Incompatible,
        ModPlatformClass::Fits => ModCompatStatus::Compatible {
            available_version: newest,
        },
        ModPlatformClass::Rejected | ModPlatformClass::Violated | ModPlatformClass::Unjudged(_) => {
            ModCompatStatus::Unknown
        }
    }
}

// =========================================================================
// Batch answers (2026-09-21 spec): exact, or decline
// =========================================================================

/// What the batch endpoints settle for ONE existence question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatchAnswer {
    /// Provably what the per-project listing would have produced;
    /// `newest` = that listing's first version number.
    Exact {
        availability: LiveAvailability,
        newest: Option<String>,
    },
    /// At least one listed build exists; whether THIS file is among them is
    /// not settled (S1: the same bytes may be published under a version the
    /// batch did not name).
    BuildsListed { newest: String },
    /// The batch says nothing usable — ask the per-project listing.
    Undecided,
}

/// §5.1 of the spec, rows B0–B7. `own` = `version_files[h]`; `latest` =
/// `update_many[h]` (every project owning the bytes). Pure.
pub fn batch_answer(
    file: &InstalledFile<'_>,
    project_id: &str,
    mc: &str,
    loader: LoaderKind,
    own: Option<&ModVersion>,
    latest: &[ModVersion],
) -> BatchAnswer {
    let _ = (file, project_id, mc, loader, own, latest);
    BatchAnswer::Undecided // stub: red round
}

/// §5.3: what a batch answer settles WITHOUT a listing, for a jar whose own
/// verdict is `Fits` (`verdict_fits`) or not. `None` = ask the listing.
pub fn settle_without_listing(
    answer: &BatchAnswer,
    verdict_fits: bool,
) -> Option<(LiveAvailability, Option<String>)> {
    let _ = (answer, verdict_fits);
    None // stub: red round
}

/// A tiny model of Modrinth for the equivalence property (spec §8): the
/// per-project listing and the two batch endpoints over one set of versions,
/// per S1/S2/S5. Later in the vector = newer. Deliberately independent of
/// `tagged_for` / `listed_for` — a model built on the code under test would
/// share its mistakes.
#[cfg(test)]
pub(crate) mod batch_model {
    use crate::mods::platform::{
        drop_filename_loader_mismatches, LoaderKind, ModFile, ModSource, ModVersion,
    };

    pub(crate) struct Build {
        pub(crate) version: ModVersion,
        /// Every file's sha1, primary first.
        pub(crate) files: Vec<&'static str>,
    }

    pub(crate) fn build(
        project: &str,
        id: &str,
        mc: &[&str],
        loaders: &[LoaderKind],
        filename: &str,
        files: &[&'static str],
    ) -> Build {
        Build {
            version: ModVersion {
                source: ModSource::Modrinth,
                project_id: project.into(),
                version_id: id.into(),
                name: id.into(),
                version_number: id.into(),
                mc_versions: mc.iter().map(|s| s.to_string()).collect(),
                loaders: loaders.to_vec(),
                primary_file: ModFile {
                    filename: filename.into(),
                    url: "https://cdn.modrinth.com/data/x.jar".into(),
                    sha1: files.first().map(|s| s.to_string()),
                    size: 1.0,
                    distribution_allowed: true,
                    sha256: None,
                },
                deps: vec![],
                published_at: None,
            },
            files: files.to_vec(),
        }
    }

    fn server_tags(v: &ModVersion, mc: &str, loader: LoaderKind) -> bool {
        v.mc_versions.iter().any(|g| g == mc) && v.loaders.contains(&loader)
    }

    pub(crate) struct Platform(pub(crate) Vec<Build>);

    impl Platform {
        /// The listing for (project, mc, loader) after our filename rule, newest first.
        pub(crate) fn listing(
            &self,
            project: &str,
            mc: &str,
            loader: LoaderKind,
        ) -> Vec<ModVersion> {
            let tagged: Vec<ModVersion> = self
                .0
                .iter()
                .rev()
                .filter(|b| b.version.project_id == project && server_tags(&b.version, mc, loader))
                .map(|b| b.version.clone())
                .collect();
            drop_filename_loader_mismatches(tagged, Some(loader))
        }

        /// Every version carrying the bytes `h`. `version_files` returns ONE of
        /// them and which one is arbitrary (S1) — tests try each.
        pub(crate) fn owners(&self, h: &str) -> Vec<&ModVersion> {
            self.0
                .iter()
                .filter(|b| b.files.iter().any(|f| *f == h))
                .map(|b| &b.version)
                .collect()
        }

        /// `update_many`: per version carrying `h`, the newest version of ITS
        /// project tagged for (mc, loader) — tags only (S2).
        pub(crate) fn update_many(&self, h: &str, mc: &str, loader: LoaderKind) -> Vec<ModVersion> {
            self.owners(h)
                .iter()
                .filter_map(|o| {
                    self.0
                        .iter()
                        .rev()
                        .find(|b| {
                            b.version.project_id == o.project_id
                                && server_tags(&b.version, mc, loader)
                        })
                        .map(|b| b.version.clone())
                })
                .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    // ── Batch answers (2026-09-21 spec §5.1) ────────────────────────────────
    use super::batch_model::{build, Build, Platform};

    const NF: LoaderKind = LoaderKind::NeoForge;

    fn at<'a>(h: Option<&'a str>, vid: Option<&'a str>) -> InstalledFile<'a> {
        InstalledFile {
            on_disk_sha1: h,
            registry_sha1: h.unwrap_or("00"),
            registry_version_id: vid,
        }
    }

    #[test]
    fn b0_to_b2_decline() {
        // (pin under the stub)
        let own = build("p", "v1", &["1.21.1"], &[NF], "p-1.jar", &["aa"]).version;
        let latest = vec![own.clone()];
        // B0: no on-disk digest.
        assert_eq!(
            batch_answer(
                &at(None, Some("v1")),
                "p",
                "1.21.1",
                NF,
                Some(&own),
                &latest
            ),
            BatchAnswer::Undecided
        );
        // B1: the platform does not know the bytes.
        assert_eq!(
            batch_answer(&at(Some("aa"), None), "p", "1.21.1", NF, None, &latest),
            BatchAnswer::Undecided
        );
        // B2: the bytes are filed under another project.
        assert_eq!(
            batch_answer(
                &at(Some("aa"), None),
                "q",
                "1.21.1",
                NF,
                Some(&own),
                &latest
            ),
            BatchAnswer::Undecided
        );
    }

    #[test]
    fn b3_no_build_is_exact_only_when_the_file_itself_is_not_tagged() {
        let old = build("p", "v1", &["1.21"], &[NF], "p-1.jar", &["aa"]).version;
        assert_eq!(
            batch_answer(&at(Some("aa"), None), "p", "1.21.1", NF, Some(&old), &[]),
            BatchAnswer::Exact {
                availability: LiveAvailability::NoBuilds,
                newest: None
            }
        );
        // B4: this very file is tagged for (mc, loader), yet no build came
        // back — a contradiction, resolved toward asking, never toward «no build».
        let tagged = build("p", "v1", &["1.21.1"], &[NF], "p-1.jar", &["aa"]).version;
        assert_eq!(
            batch_answer(&at(Some("aa"), None), "p", "1.21.1", NF, Some(&tagged), &[]),
            BatchAnswer::Undecided
        );
    }

    #[test]
    fn b5_a_newest_build_the_filename_rule_drops_declines() {
        // (pin under the stub)
        let fo = LoaderKind::Forge;
        let own = build("p", "v1", &["1.20.4"], &[fo], "p-forge-1.jar", &["aa"]).version;
        let newest = build("p", "v2", &["1.20.4"], &[fo], "p-neoforge-2.jar", &["bb"]).version;
        assert_eq!(
            batch_answer(
                &at(Some("aa"), None),
                "p",
                "1.20.4",
                fo,
                Some(&own),
                &[newest]
            ),
            BatchAnswer::Undecided
        );
    }

    #[test]
    fn b6_the_file_is_confirmed_by_its_primary_digest_or_by_a_record_that_still_describes_it() {
        let own = build("p", "v1", &["1.21.1"], &[NF], "p-1.jar", &["aa"]).version;
        let newest = build("p", "v2", &["1.21.1"], &[NF], "p-2.jar", &["bb"]).version;
        let confirmed = BatchAnswer::Exact {
            availability: LiveAvailability::FileListed,
            newest: Some("v2".into()),
        };
        // (a) the primary file's digest, compared ignoring case.
        assert_eq!(
            batch_answer(
                &at(Some("AA"), None),
                "p",
                "1.21.1",
                NF,
                Some(&own),
                &[newest.clone()]
            ),
            confirmed
        );
        // (b) the registry's version id, while the record describes the disk.
        let non_primary = build("p", "v1", &["1.21.1"], &[NF], "p-1.jar", &["cc", "aa"]).version;
        let record = InstalledFile {
            on_disk_sha1: Some("aa"),
            registry_sha1: "aa",
            registry_version_id: Some("v1"),
        };
        assert_eq!(
            batch_answer(
                &record,
                "p",
                "1.21.1",
                NF,
                Some(&non_primary),
                &[newest.clone()]
            ),
            confirmed
        );
        // A record of a replaced file vouches for nothing.
        let replaced = InstalledFile {
            on_disk_sha1: Some("aa"),
            registry_sha1: "ff",
            registry_version_id: Some("v1"),
        };
        assert_eq!(
            batch_answer(&replaced, "p", "1.21.1", NF, Some(&non_primary), &[newest]),
            BatchAnswer::BuildsListed {
                newest: "v2".into()
            }
        );
    }

    #[test]
    fn b7_builds_are_listed_but_this_file_is_not_settled() {
        let own = build("p", "v1", &["1.21"], &[NF], "p-1.jar", &["aa"]).version;
        let newest = build("p", "v2", &["1.21.1"], &[NF], "p-2.jar", &["bb"]).version;
        assert_eq!(
            batch_answer(
                &at(Some("aa"), None),
                "p",
                "1.21.1",
                NF,
                Some(&own),
                &[newest]
            ),
            BatchAnswer::BuildsListed {
                newest: "v2".into()
            }
        );
    }

    #[test]
    fn the_projects_own_entry_is_picked_from_a_multi_project_answer() {
        let own = build("p", "v1", &["1.21.1"], &[NF], "p-1.jar", &["aa"]).version;
        let other = build("q", "w9", &["1.21.1"], &[NF], "q-9.jar", &["aa"]).version;
        let mine = build("p", "v2", &["1.21.1"], &[NF], "p-2.jar", &["bb"]).version;
        assert_eq!(
            batch_answer(
                &at(Some("aa"), None),
                "p",
                "1.21.1",
                NF,
                Some(&own),
                &[other, mine]
            ),
            BatchAnswer::Exact {
                availability: LiveAvailability::FileListed,
                newest: Some("v2".into())
            }
        );
    }

    #[test]
    fn only_a_fits_jar_is_settled_by_builds_existing() {
        let listed = BatchAnswer::BuildsListed {
            newest: "v2".into(),
        };
        assert_eq!(
            settle_without_listing(&listed, true),
            Some((LiveAvailability::OtherBuildsOnly, Some("v2".into())))
        );
        // An undeclared jar gets the full listing: «this exact file is not on
        // the page» is a claim the user reads (`FileNotListed`).
        assert_eq!(settle_without_listing(&listed, false), None);
        assert_eq!(settle_without_listing(&BatchAnswer::Undecided, true), None);
        let exact = BatchAnswer::Exact {
            availability: LiveAvailability::NoBuilds,
            newest: None,
        };
        assert_eq!(
            settle_without_listing(&exact, false),
            Some((LiveAvailability::NoBuilds, None))
        );
    }

    struct Case {
        name: &'static str,
        platform: Platform,
        on_disk: Option<&'static str>,
        project: &'static str,
        version_id: Option<&'static str>,
        mc: &'static str,
        loader: LoaderKind,
    }

    fn case(
        name: &'static str,
        builds: Vec<Build>,
        on_disk: Option<&'static str>,
        version_id: Option<&'static str>,
        mc: &'static str,
        loader: LoaderKind,
    ) -> Case {
        Case {
            name,
            platform: Platform(builds),
            on_disk,
            project: "p",
            version_id,
            mc,
            loader,
        }
    }

    fn cases() -> Vec<Case> {
        let (fo, fa) = (LoaderKind::Forge, LoaderKind::Fabric);
        vec![
            case(
                "the listed primary file",
                vec![build("p", "v1", &["1.21.1"], &[NF], "p-1.jar", &["a1"])],
                Some("a1"),
                Some("v1"),
                "1.21.1",
                NF,
            ),
            case(
                "an older file that is still listed",
                vec![
                    build("p", "v1", &["1.21.1"], &[NF], "p-1.jar", &["b1"]),
                    build("p", "v2", &["1.21.1"], &[NF], "p-2.jar", &["b2"]),
                ],
                Some("b1"),
                None,
                "1.21.1",
                NF,
            ),
            case(
                "one jar re-published for the next Minecraft version",
                vec![
                    build("p", "v1", &["1.21"], &[NF], "p-1.jar", &["c1"]),
                    build("p", "v2", &["1.21.1"], &[NF], "p-1.jar", &["c1"]),
                ],
                Some("c1"),
                None,
                "1.21.1",
                NF,
            ),
            case(
                "the listed owner is not the newest build",
                vec![
                    build("p", "v1", &["1.21.1"], &[NF], "p-1.jar", &["d1"]),
                    build("p", "v2", &["1.21"], &[NF], "p-1.jar", &["d1"]),
                    build("p", "v3", &["1.21.1"], &[NF], "p-3.jar", &["d3"]),
                ],
                Some("d1"),
                None,
                "1.21.1",
                NF,
            ),
            case(
                "a non-primary file the registry names",
                vec![build(
                    "p",
                    "v1",
                    &["1.21.1"],
                    &[NF],
                    "p-1.jar",
                    &["e0", "e1"],
                )],
                Some("e1"),
                Some("v1"),
                "1.21.1",
                NF,
            ),
            case(
                "a non-primary file the registry does not name",
                vec![build(
                    "p",
                    "v1",
                    &["1.21.1"],
                    &[NF],
                    "p-1.jar",
                    &["f0", "f1"],
                )],
                Some("f1"),
                None,
                "1.21.1",
                NF,
            ),
            case(
                "the newest build is dropped by the filename rule",
                vec![
                    build("p", "v1", &["1.20.4"], &[fo], "p-forge-1.jar", &["g1"]),
                    build("p", "v2", &["1.20.4"], &[fo], "p-neoforge-2.jar", &["g2"]),
                ],
                Some("g1"),
                None,
                "1.20.4",
                fo,
            ),
            case(
                "the user holds the mis-tagged jar",
                vec![
                    build("p", "v1", &["1.20.4"], &[fo], "p-forge-1.jar", &["h1"]),
                    build("p", "v2", &["1.20.4"], &[fo], "p-neoforge-2.jar", &["h2"]),
                ],
                Some("h2"),
                None,
                "1.20.4",
                fo,
            ),
            case(
                "the only build is a mis-tagged NeoForge jar",
                vec![build(
                    "p",
                    "v1",
                    &["1.20.4"],
                    &[fo],
                    "p-neoforge-1.jar",
                    &["i1"],
                )],
                Some("i1"),
                None,
                "1.20.4",
                fo,
            ),
            case(
                "tagged for a neighbouring Minecraft version only",
                vec![build("p", "v1", &["1.21"], &[NF], "p-1.jar", &["j1"])],
                Some("j1"),
                None,
                "1.21.1",
                NF,
            ),
            case(
                "builds for another loader only",
                vec![build("p", "v1", &["1.21.1"], &[fa], "p-1.jar", &["k1"])],
                Some("k1"),
                None,
                "1.21.1",
                NF,
            ),
            case(
                "a Fabric jar asked under fabric (Connector)",
                vec![build("p", "v1", &["1.21.1"], &[fa], "p-1.jar", &["l1"])],
                Some("l1"),
                None,
                "1.21.1",
                fa,
            ),
            case(
                "bytes the platform does not know",
                vec![build("p", "v1", &["1.21.1"], &[NF], "p-1.jar", &["m1"])],
                Some("m9"),
                None,
                "1.21.1",
                NF,
            ),
            case(
                "bytes filed under another project",
                vec![
                    build("q", "w1", &["1.21.1"], &[NF], "q-1.jar", &["n1"]),
                    build("p", "v1", &["1.21.1"], &[NF], "p-1.jar", &["n2"]),
                ],
                Some("n1"),
                None,
                "1.21.1",
                NF,
            ),
            case(
                "no on-disk digest",
                vec![build("p", "v1", &["1.21.1"], &[NF], "p-1.jar", &["o1"])],
                None,
                Some("v1"),
                "1.21.1",
                NF,
            ),
        ]
    }

    #[test]
    fn a_batch_answer_is_what_the_listing_would_say_or_it_declines() {
        let mut decided = 0;
        for c in cases() {
            let listing = c.platform.listing(c.project, c.mc, c.loader);
            let file = at(c.on_disk, c.version_id);
            let expected = live_availability(&file, ProbeAnswer::Found(&listing));
            let first = listing.first().map(|v| v.version_number.clone());
            let owners = c.on_disk.map(|h| c.platform.owners(h)).unwrap_or_default();
            // Every owner `version_files` could name (S1) — or «unknown bytes».
            let picks: Vec<Option<&ModVersion>> = if owners.is_empty() {
                vec![None]
            } else {
                owners.into_iter().map(Some).collect()
            };
            let latest = c
                .on_disk
                .map(|h| c.platform.update_many(h, c.mc, c.loader))
                .unwrap_or_default();
            for own in picks {
                match batch_answer(&file, c.project, c.mc, c.loader, own, &latest) {
                    BatchAnswer::Exact {
                        availability,
                        newest,
                    } => {
                        decided += 1;
                        assert_eq!(
                            availability, expected,
                            "{}: differs from the listing",
                            c.name
                        );
                        assert_eq!(newest, first, "{}: newest", c.name);
                    }
                    BatchAnswer::BuildsListed { newest } => {
                        decided += 1;
                        assert!(
                            !listing.is_empty(),
                            "{}: «builds listed», listing empty",
                            c.name
                        );
                        assert_eq!(Some(newest), first, "{}: newest", c.name);
                    }
                    BatchAnswer::Undecided => {}
                }
            }
        }
        // Vacuity guard: an always-declining batch satisfies the property.
        assert!(
            decided >= 10,
            "only {decided} decided answers — the batch is not being exercised"
        );
    }
}
