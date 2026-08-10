//! Plan a Minecraft-version-change mod migration.
//!
//! When a user changes an instance's MC version / loader, jars built for the
//! old platform stay in `mods/` and the game dies at pre-load. This module
//! PLANS remediation — deciding which installed mods can be re-fetched for
//! the instance's CURRENT platform — but never applies it. Applying is a
//! later task.
//!
//! [`build_migration_plan`] and [`fold_new_dependencies`] are pure: they take
//! already-computed [`PlatformVerdict`]s and already-fetched platform
//! responses, so they are unit-testable with no I/O. The command layer
//! (`commands::mods::mods_plan_mc_migration`) does the jar reads and network
//! calls and feeds the results in.

use std::collections::HashSet;

use serde::Serialize;
use specta::Type;

use crate::mods::deps::ProjectKey;
use crate::mods::mc_compat::PlatformVerdict;
use crate::mods::platform::{ModSource, ModVersion};

// =========================================================================
// Plan shape (crosses IPC)
// =========================================================================

/// An installed mod whose current jar already fits the instance's platform.
/// Nothing to do.
#[derive(Debug, Clone, Serialize, Type)]
pub struct FitsRow {
    pub sha1: String,
    pub name: String,
}

/// A `Violated` mod with a project to ask and a build the platform lists for
/// the instance's CURRENT MC + loader. `target` is what remediation would
/// install in place of `sha1`.
#[derive(Debug, Clone, Serialize, Type)]
pub struct ReplaceableRow {
    pub sha1: String,
    pub name: String,
    pub source: ModSource,
    pub project_id: String,
    pub target: ModVersion,
}

/// Why a `Violated` mod has no replacement plan. The UI shows different copy
/// for each — a failed query must never be read as "no build exists".
#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StrandedReason {
    /// The platform was asked for the instance's current MC + loader and
    /// returned no build.
    NoBuildForTarget,
    /// No project to ask — a hand-dropped jar with no platform identity.
    NoProjectToAsk,
    /// The pack owns this mod's versions; changing it piecemeal is the
    /// modpack version-switch flow, not this one.
    PackOrigin,
    /// The platform query itself failed (network, missing CurseForge key,
    /// project delisted / 404). Distinct from `NoBuildForTarget` on purpose.
    QueryFailed,
}

/// A `Violated` mod with no replacement plan. Carries WHY.
#[derive(Debug, Clone, Serialize, Type)]
pub struct StrandedRow {
    pub sha1: String,
    pub name: String,
    pub reason: StrandedReason,
}

/// A project a chosen target's required-dependency set needs, for which the
/// instance has NO installed jar at all today — fitting, violated, or
/// otherwise. A genuinely new addition the migration would pull in.
///
/// This is how the plan avoids the bug that motivated it: BiomesOPlenty
/// mandatorily requires `terrablender` and `glitchcore`. If a target the plan
/// wants to install needs a project the instance simply doesn't have a jar
/// for, that need must be visible here BEFORE the user accepts anything —
/// every jar an apply would touch must appear in the plan.
#[derive(Debug, Clone, Serialize, Type)]
pub struct NewDependencyRow {
    pub source: ModSource,
    pub project_id: String,
    pub target: ModVersion,
    /// Display names of the replaceable rows whose chosen target requires
    /// this project.
    pub needed_by: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Type, Default)]
pub struct McMigrationPlan {
    /// Nothing to do — the installed file already fits.
    pub fits: Vec<FitsRow>,
    /// Violated, a project to ask, and the platform lists a build for the
    /// instance's current MC + loader. Carries the target so the UI can name it.
    pub replaceable: Vec<ReplaceableRow>,
    /// Projects a chosen target's required-dependency set needs that the
    /// instance has no jar for at all today. See [`NewDependencyRow`].
    pub new_dependencies: Vec<NewDependencyRow>,
    /// Violated with no replacement: no build for the target, no project to
    /// query (hand-dropped), pack-origin, or the query itself failed. Each
    /// row carries WHY.
    pub stranded: Vec<StrandedRow>,
    /// Verdict was `Unknown` — surfaced in the summary, never folded into
    /// `fits`. A check that did not run must not read as a check that passed.
    pub unjudged: usize,
}

// =========================================================================
// Pure bucketing
// =========================================================================

/// Why a mod has no [`replaceable_identity`](crate::mods::updates::replaceable_identity)
/// — the two reasons `replaceable_identity` collapses into one `None`, kept
/// distinct here so bucketing can pick the right [`StrandedReason`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ineligible {
    PackOrigin,
    NoProject,
}

/// The platform's answer, for one project, to "what do you have for the
/// instance's current MC + loader". Already fetched by the command layer —
/// this module never does I/O.
#[derive(Debug, Clone)]
pub enum CandidateQuery {
    /// The platform answered (possibly with zero builds).
    Found(Vec<ModVersion>),
    /// The query itself failed.
    Failed,
}

/// One installed mod's already-computed verdict, identity, and (if
/// applicable) already-fetched candidate query. Feeds [`build_migration_plan`]
/// — every field here is data the caller already has in hand.
#[derive(Debug, Clone)]
pub struct ModMigrationInput {
    pub sha1: String,
    pub name: String,
    pub verdict: PlatformVerdict,
    /// `Ok` mirrors `replaceable_identity`'s `Some`; `Err` distinguishes WHY
    /// it was `None` (pack-origin vs no project at all).
    pub identity: Result<(ModSource, String), Ineligible>,
    /// `Some` only when `verdict` is `Violated` and `identity` is `Ok` — the
    /// command already asked the platform. `None` otherwise (never asked).
    pub candidate: Option<CandidateQuery>,
}

/// Bucket already-computed per-mod inputs into a plan. Pure — no I/O.
pub fn build_migration_plan(inputs: Vec<ModMigrationInput>) -> McMigrationPlan {
    let mut plan = McMigrationPlan::default();
    for input in inputs {
        match input.verdict {
            PlatformVerdict::Fits => plan.fits.push(FitsRow {
                sha1: input.sha1,
                name: input.name,
            }),
            // A check that did not run must not read as one that passed —
            // counted separately, never merged into `fits`.
            PlatformVerdict::Unknown => plan.unjudged += 1,
            PlatformVerdict::Violated { .. } => {
                let reason = match input.identity {
                    Err(Ineligible::PackOrigin) => Some(StrandedReason::PackOrigin),
                    Err(Ineligible::NoProject) => Some(StrandedReason::NoProjectToAsk),
                    Ok((source, project_id)) => match input.candidate {
                        Some(CandidateQuery::Found(versions)) => {
                            match versions.into_iter().next() {
                                Some(target) => {
                                    plan.replaceable.push(ReplaceableRow {
                                        sha1: input.sha1.clone(),
                                        name: input.name.clone(),
                                        source,
                                        project_id,
                                        target,
                                    });
                                    None
                                }
                                None => Some(StrandedReason::NoBuildForTarget),
                            }
                        }
                        Some(CandidateQuery::Failed) => Some(StrandedReason::QueryFailed),
                        // Defensive: a violated, identified mod the command
                        // never queried. Every violated mod must still appear
                        // somewhere in the plan — treat an absent query the
                        // same as a failed one rather than silently drop the row.
                        None => Some(StrandedReason::QueryFailed),
                    },
                };
                if let Some(reason) = reason {
                    plan.stranded.push(StrandedRow {
                        sha1: input.sha1,
                        name: input.name,
                        reason,
                    });
                }
            }
        }
    }
    plan
}

// =========================================================================
// Replacement closure
// =========================================================================

/// One replaceable row's chosen target's required dependencies — already
/// resolved (I/O) by the command via `ModPlatform::resolve_deps`, exactly
/// once per target, the same call `mods_update_one` itself makes.
#[derive(Debug, Clone)]
pub struct TargetRequirement {
    pub row_name: String,
    pub required: Vec<ModVersion>,
}

/// Union every replaceable row's required-dependency list, dedupe by
/// project, and keep only the projects the instance has NO installed jar for
/// at all today — fitting, violated, or otherwise. A project that already has
/// an installed jar already has its own row elsewhere in the plan (`fits` /
/// `replaceable` / `stranded`); this surfaces only what is genuinely new.
/// Pure — no I/O.
pub fn fold_new_dependencies(
    requirements: &[TargetRequirement],
    already_installed: &HashSet<ProjectKey>,
) -> Vec<NewDependencyRow> {
    let mut out: Vec<NewDependencyRow> = Vec::new();
    for req in requirements {
        for dep in &req.required {
            let key = ProjectKey::of_version(dep);
            if already_installed.contains(&key) {
                continue;
            }
            match out
                .iter_mut()
                .find(|row| ProjectKey::of_version(&row.target) == key)
            {
                Some(existing) => {
                    if !existing.needed_by.contains(&req.row_name) {
                        existing.needed_by.push(req.row_name.clone());
                    }
                }
                None => out.push(NewDependencyRow {
                    source: dep.source,
                    project_id: dep.project_id.clone(),
                    target: dep.clone(),
                    needed_by: vec![req.row_name.clone()],
                }),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::mc_compat::PlatformAxis;
    use crate::mods::platform::{LoaderKind, ModFile};

    fn version(source: ModSource, project_id: &str, version_id: &str) -> ModVersion {
        ModVersion {
            source,
            project_id: project_id.into(),
            version_id: version_id.into(),
            name: format!("{project_id}-name"),
            version_number: version_id.into(),
            mc_versions: vec!["1.20.1".into()],
            loaders: vec![LoaderKind::Forge],
            primary_file: ModFile {
                filename: format!("{project_id}-{version_id}.jar"),
                url: "https://example/mod.jar".into(),
                sha1: Some("aa".into()),
                size: 1.0,
                distribution_allowed: true,
                sha256: None,
            },
            deps: vec![],
            published_at: None,
        }
    }

    fn violated() -> PlatformVerdict {
        PlatformVerdict::Violated {
            axis: PlatformAxis::Minecraft,
            declared: "[1.21,1.22)".into(),
            actual: "1.20.1".into(),
            source: crate::mods::local::DescriptorSource::ModsToml,
            family: crate::mods::version_range::RangeFamily::Maven,
        }
    }

    fn input(
        sha1: &str,
        name: &str,
        verdict: PlatformVerdict,
        identity: Result<(ModSource, String), Ineligible>,
        candidate: Option<CandidateQuery>,
    ) -> ModMigrationInput {
        ModMigrationInput {
            sha1: sha1.into(),
            name: name.into(),
            verdict,
            identity,
            candidate,
        }
    }

    // -- build_migration_plan ---------------------------------------------

    #[test]
    fn fits_mod_lands_in_fits() {
        let plan = build_migration_plan(vec![input(
            "s1",
            "Fine Mod",
            PlatformVerdict::Fits,
            Err(Ineligible::NoProject),
            None,
        )]);
        assert_eq!(plan.fits.len(), 1);
        assert_eq!(plan.fits[0].sha1, "s1");
        assert!(plan.replaceable.is_empty());
        assert!(plan.stranded.is_empty());
        assert_eq!(plan.unjudged, 0);
    }

    #[test]
    fn violated_with_identity_and_a_found_build_is_replaceable() {
        let target = version(ModSource::Modrinth, "bop", "v-1201");
        let plan = build_migration_plan(vec![input(
            "s1",
            "Biomes O' Plenty",
            violated(),
            Ok((ModSource::Modrinth, "bop".to_string())),
            Some(CandidateQuery::Found(vec![target.clone()])),
        )]);
        assert!(plan.fits.is_empty());
        assert_eq!(plan.replaceable.len(), 1);
        let row = &plan.replaceable[0];
        assert_eq!(row.sha1, "s1");
        assert_eq!(row.project_id, "bop");
        assert_eq!(row.target, target);
        assert!(plan.stranded.is_empty());
    }

    #[test]
    fn violated_with_no_build_for_target_is_stranded() {
        let plan = build_migration_plan(vec![input(
            "s1",
            "Old Mod",
            violated(),
            Ok((ModSource::Modrinth, "old".to_string())),
            Some(CandidateQuery::Found(vec![])),
        )]);
        assert!(plan.replaceable.is_empty());
        assert_eq!(plan.stranded.len(), 1);
        assert!(matches!(
            plan.stranded[0].reason,
            StrandedReason::NoBuildForTarget
        ));
    }

    #[test]
    fn violated_with_no_project_to_ask_is_stranded() {
        let plan = build_migration_plan(vec![input(
            "s1",
            "Dropped In Jar",
            violated(),
            Err(Ineligible::NoProject),
            None,
        )]);
        assert_eq!(plan.stranded.len(), 1);
        assert!(matches!(
            plan.stranded[0].reason,
            StrandedReason::NoProjectToAsk
        ));
    }

    #[test]
    fn violated_pack_origin_mod_is_stranded() {
        let plan = build_migration_plan(vec![input(
            "s1",
            "Bundled Mod",
            violated(),
            Err(Ineligible::PackOrigin),
            None,
        )]);
        assert_eq!(plan.stranded.len(), 1);
        assert!(matches!(
            plan.stranded[0].reason,
            StrandedReason::PackOrigin
        ));
    }

    #[test]
    fn violated_with_a_failed_query_is_stranded_and_distinct_from_no_build() {
        let plan = build_migration_plan(vec![input(
            "s1",
            "Rate Limited Mod",
            violated(),
            Ok((ModSource::Modrinth, "rl".to_string())),
            Some(CandidateQuery::Failed),
        )]);
        assert_eq!(plan.stranded.len(), 1);
        // Load-bearing: a failed query must never be read as proof no
        // build exists.
        assert!(matches!(
            plan.stranded[0].reason,
            StrandedReason::QueryFailed
        ));
        assert!(!matches!(
            plan.stranded[0].reason,
            StrandedReason::NoBuildForTarget
        ));
    }

    #[test]
    fn unknown_verdict_lands_in_unjudged_not_fits() {
        let plan = build_migration_plan(vec![input(
            "s1",
            "Undeclared Mod",
            PlatformVerdict::Unknown,
            Ok((ModSource::Modrinth, "u".to_string())),
            None,
        )]);
        assert_eq!(plan.unjudged, 1);
        assert!(plan.fits.is_empty());
        assert!(plan.replaceable.is_empty());
        assert!(plan.stranded.is_empty());
    }

    #[test]
    fn mixed_batch_buckets_each_mod_independently() {
        let target = version(ModSource::Modrinth, "bop", "v2");
        let inputs = vec![
            input(
                "f1",
                "Fits",
                PlatformVerdict::Fits,
                Err(Ineligible::NoProject),
                None,
            ),
            input(
                "r1",
                "Replaceable",
                violated(),
                Ok((ModSource::Modrinth, "bop".to_string())),
                Some(CandidateQuery::Found(vec![target])),
            ),
            input(
                "u1",
                "Unjudged",
                PlatformVerdict::Unknown,
                Err(Ineligible::NoProject),
                None,
            ),
            input(
                "s1",
                "Stranded",
                violated(),
                Err(Ineligible::NoProject),
                None,
            ),
        ];
        let plan = build_migration_plan(inputs);
        assert_eq!(plan.fits.len(), 1);
        assert_eq!(plan.replaceable.len(), 1);
        assert_eq!(plan.stranded.len(), 1);
        assert_eq!(plan.unjudged, 1);
    }

    // -- fold_new_dependencies ---------------------------------------------

    #[test]
    fn required_project_already_installed_is_not_a_new_dependency() {
        let terrablender = version(ModSource::Modrinth, "terrablender", "t2");
        let requirements = vec![TargetRequirement {
            row_name: "Biomes O' Plenty".into(),
            required: vec![terrablender.clone()],
        }];
        let mut already: HashSet<ProjectKey> = HashSet::new();
        already.insert(ProjectKey::of_version(&terrablender));
        assert!(fold_new_dependencies(&requirements, &already).is_empty());
    }

    #[test]
    fn required_project_not_installed_anywhere_is_a_new_dependency() {
        let glitchcore = version(ModSource::Modrinth, "glitchcore", "g2");
        let requirements = vec![TargetRequirement {
            row_name: "Biomes O' Plenty".into(),
            required: vec![glitchcore.clone()],
        }];
        let out = fold_new_dependencies(&requirements, &HashSet::new());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].project_id, "glitchcore");
        assert_eq!(out[0].target, glitchcore);
        assert_eq!(out[0].needed_by, vec!["Biomes O' Plenty".to_string()]);
    }

    #[test]
    fn same_new_dependency_needed_by_two_targets_is_deduped_with_both_names() {
        let glitchcore = version(ModSource::Modrinth, "glitchcore", "g2");
        let requirements = vec![
            TargetRequirement {
                row_name: "Biomes O' Plenty".into(),
                required: vec![glitchcore.clone()],
            },
            TargetRequirement {
                row_name: "Terrablender".into(),
                required: vec![glitchcore.clone()],
            },
        ];
        let out = fold_new_dependencies(&requirements, &HashSet::new());
        assert_eq!(out.len(), 1);
        assert_eq!(
            out[0].needed_by,
            vec!["Biomes O' Plenty".to_string(), "Terrablender".to_string()]
        );
    }

    #[test]
    fn no_requirements_yields_no_new_dependencies() {
        assert!(fold_new_dependencies(&[], &HashSet::new()).is_empty());
    }
}
