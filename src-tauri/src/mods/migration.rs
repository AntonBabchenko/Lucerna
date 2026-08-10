//! Plan and settle a Minecraft-version-change mod migration.
//!
//! When a user changes an instance's MC version / loader, jars built for the
//! old platform stay in `mods/` and the game dies at pre-load. This module
//! PLANS remediation — deciding which installed mods can be re-fetched for
//! the instance's CURRENT platform — and separately maps the user's settled
//! decisions over that plan to the concrete steps applying it requires. It
//! never does the applying itself: no I/O beyond what's needed to unit-test
//! it lives here.
//!
//! [`build_migration_plan`], [`fold_new_dependencies`], and
//! [`resolve_migration_selections`] are pure: they take already-computed
//! [`PlatformVerdict`]s, already-fetched platform responses, or an
//! already-settled [`McMigrationSelections`], so they are unit-testable with
//! no I/O. The command layer (`commands::mods::mods_plan_mc_migration` for
//! planning, `commands::mods::mods_apply_mc_migration` for applying) does the
//! jar reads, network calls, and filesystem writes and feeds the results in.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::mods::deps::ProjectKey;
use crate::mods::mc_compat::{PlatformAxis, PlatformVerdict};
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
    /// The jar is violated on the LOADER-version axis (e.g. it needs Forge 52+
    /// but the instance runs an older Forge build), and the platform's build for
    /// this MC + loader-kind is the SAME jar already installed — so no reinstall
    /// can fix it. The remedy is raising the instance's loader build, which this
    /// flow does not do. Surfaced as stranded (disable/remove/keep) instead of a
    /// no-op "replaceable" reinstall that the post-apply rescan would re-flag.
    LoaderTooOld,
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
    /// `u32` not `usize`: specta forbids exporting BigInt-style types to TS
    /// (see the same rule applied to every other count field in this
    /// codebase — `usize`/`u64` counters are cast down, byte sizes go to
    /// `f64`); a bounded per-instance mod count never approaches `u32::MAX`.
    pub unjudged: u32,
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
            PlatformVerdict::Violated { axis, .. } => {
                let reason = match input.identity {
                    Err(Ineligible::PackOrigin) => Some(StrandedReason::PackOrigin),
                    Err(Ineligible::NoProject) => Some(StrandedReason::NoProjectToAsk),
                    Ok((source, project_id)) => match input.candidate {
                        Some(CandidateQuery::Found(versions)) => {
                            match versions.into_iter().next() {
                                // A candidate whose file is byte-identical to the
                                // installed jar is never a fix. The versions query
                                // filters by MC + loader-KIND only (blind to loader
                                // VERSION), so a loader-version violation gets back
                                // the same build already on disk; reinstalling it
                                // changes nothing and the post-apply rescan re-flags
                                // it — the "press Fix forever" loop. Strand it with a
                                // reason that names the real remedy (raise the loader
                                // build) rather than offer a no-op reinstall. Guarding
                                // on the file sha keeps the OTHER loader-axis direction
                                // (instance loader too new, a genuinely newer build
                                // exists) as a legitimate replacement.
                                Some(target)
                                    if target.primary_file.sha1.as_deref()
                                        == Some(input.sha1.as_str()) =>
                                {
                                    Some(match axis {
                                        PlatformAxis::Loader => StrandedReason::LoaderTooOld,
                                        PlatformAxis::Minecraft => StrandedReason::NoBuildForTarget,
                                    })
                                }
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

// =========================================================================
// Apply — settled user selections over an already-shown plan
// =========================================================================

/// Disposition the user chose for one [`StrandedRow`]. Deliberately carries
/// no `Default` impl — the jar behind a stranded row is PROVEN incompatible
/// (`Violated` verdict, no replacement plan), so there is no safe "the user
/// didn't say" fallback the way an un-selected `replaceable` row has (leaving
/// it installed is fine there — it is not proven broken by the plan itself,
/// only unconfirmed). Every [`StrandedSelection`] carries this as a mandatory
/// field (no `#[serde(default)]`), so an omitted `disposition` fails
/// deserialization at the IPC boundary rather than silently landing on
/// `Keep`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum StrandedDisposition {
    Disable,
    Remove,
    Keep,
}

/// One `replaceable` row the user approved for replacement. Carries the
/// EXACT `target` [`McMigrationPlan::replaceable`] showed — see
/// [`McMigrationSelections`] for why apply never re-derives it.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ReplaceSelection {
    pub old_sha1: String,
    pub target: ModVersion,
}

/// One `stranded` row's chosen disposition.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StrandedSelection {
    pub sha1: String,
    pub disposition: StrandedDisposition,
}

/// The user's full set of decisions over one [`McMigrationPlan`], submitted
/// to `mods_apply_mc_migration`.
///
/// Every field is self-contained: `replace` and `new_dependencies` carry the
/// full [`ModVersion`] target the plan already resolved, rather than a row
/// key the apply command would look back up. This is deliberate, not an
/// oversight:
///
/// - The plan already did the network work for these targets —
///   `ModPlatform::versions` per violated mod and `ModPlatform::resolve_deps`
///   per replaceable target, both in `mods_plan_mc_migration`. Re-deriving
///   that inside apply would repeat the work AND risk installing a build
///   different from the one the user actually reviewed (a newer version
///   could have been published in between) — breaking the very invariant
///   this module's doc comment states: "every jar an apply would touch must
///   appear in the plan before the user accepts anything".
/// - It structurally forecloses the bug this task exists to fix.
///   [`resolve_migration_selections`] never reads [`ModVersion::deps`] on any
///   target — there is no step where a target's OWN declared dependencies
///   (e.g. BiomesOPlenty mandatorily declaring `terrablender` +
///   `glitchcore`) could be consulted to silently add installs. Every
///   install this produces traces back to an explicit field the user (or
///   `mods_plan_mc_migration`'s prior run) already decided on.
///
/// Full array-level completeness for `stranded` (every row the plan showed
/// has a matching entry here) is intentionally NOT cross-checked against a
/// plan at apply time, for the same reason: telling a `stranded` row apart
/// from a `replaceable` one requires the platform query
/// `mods_plan_mc_migration` already ran, and re-running it here would be
/// exactly the "fresh resolution at apply time" this design avoids. What IS
/// enforced: [`StrandedDisposition`] has no default, so any row that DOES
/// appear in `stranded` carries an explicit choice — never a silently
/// defaulted one. Presenting one control per stranded row and requiring a
/// choice before enabling Apply is the UI's job.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
pub struct McMigrationSelections {
    /// `replaceable` rows approved for replacement. A row NOT listed here is
    /// left exactly as-is — its old, incompatible jar stays installed. That
    /// is a legitimate choice ("fix this one later"), unlike a stranded
    /// row's disposition, which has no safe default.
    pub replace: Vec<ReplaceSelection>,
    /// `new_dependencies` rows approved for install alongside whichever
    /// replacement(s) need them.
    pub new_dependencies: Vec<ModVersion>,
    /// One entry per `stranded` row the user decided on.
    pub stranded: Vec<StrandedSelection>,
}

/// One concrete step [`resolve_migration_selections`] derives from a settled
/// [`McMigrationSelections`]. The apply command executes these one at a
/// time — never as one atomic batch — so a single row's failure never
/// withdraws a sibling row's already-applied change.
#[derive(Debug, Clone, PartialEq)]
pub enum MigrationAction {
    /// Remove `old_sha1`'s jar and install `target` in its place. No
    /// dependency resolution: the plan already resolved and pruned what this
    /// replacement needs; anything more is a `new_dependencies` row the user
    /// separately accepted (or didn't).
    Replace {
        old_sha1: String,
        target: ModVersion,
    },
    /// Install a genuinely new jar for a project the instance has none of
    /// today (an accepted `new_dependencies` row).
    InstallNewDependency { target: ModVersion },
    /// Disable a stranded mod's jar in place (`.jar` → `.jar.disabled`).
    DisableStranded { sha1: String },
    /// Remove a stranded mod's jar and registry record outright.
    RemoveStranded { sha1: String },
    // `Keep` produces no action — see `resolve_migration_selections`.
}

/// Map a settled [`McMigrationSelections`] to the concrete steps the apply
/// command must execute. Pure — no I/O, no dependency resolution — so it is
/// unit-testable without an `AppHandle` or network.
///
/// Deliberately never reads `target.deps` on any [`ModVersion`] passed in:
/// expanding a target's own declared dependencies into extra installs here
/// would silently reintroduce the exact bug this module exists to fix (see
/// `replace_never_pulls_in_a_kept_dependency_from_target_deps` below). Every
/// action this schedules comes from a field the caller already decided on —
/// `replace` and `new_dependencies` — never inferred from a target's own
/// metadata.
pub fn resolve_migration_selections(selections: &McMigrationSelections) -> Vec<MigrationAction> {
    let mut actions = Vec::with_capacity(
        selections.replace.len() + selections.new_dependencies.len() + selections.stranded.len(),
    );
    for r in &selections.replace {
        actions.push(MigrationAction::Replace {
            old_sha1: r.old_sha1.clone(),
            target: r.target.clone(),
        });
    }
    for dep in &selections.new_dependencies {
        actions.push(MigrationAction::InstallNewDependency {
            target: dep.clone(),
        });
    }
    for s in &selections.stranded {
        match s.disposition {
            StrandedDisposition::Disable => actions.push(MigrationAction::DisableStranded {
                sha1: s.sha1.clone(),
            }),
            StrandedDisposition::Remove => actions.push(MigrationAction::RemoveStranded {
                sha1: s.sha1.clone(),
            }),
            // Explicit no-op: `Keep` means "leave the jar exactly as it is".
            // No `_ =>` wildcard above — a future disposition variant must be
            // handled here explicitly, not silently fall through to a no-op.
            StrandedDisposition::Keep => {}
        }
    }
    actions
}

// =========================================================================
// Apply report
// =========================================================================

/// What happened when one [`MigrationAction`] was executed.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum McMigrationRowOutcome {
    Replaced {
        old_sha1: String,
        name: String,
        new_sha1: String,
    },
    InstalledDependency {
        name: String,
        sha1: String,
    },
    Disabled {
        sha1: String,
        name: String,
    },
    Removed {
        sha1: String,
        name: String,
    },
    /// The action failed. Every OTHER row's outcome in the same report is
    /// unaffected — apply never withdraws a sibling row's already-applied
    /// change because one row failed.
    Failed {
        name: String,
        error: crate::error::Error,
    },
}

/// Full result of one `mods_apply_mc_migration` call: what happened to every
/// action the settled selection produced, success and failure both.
#[derive(Debug, Clone, Serialize, Type, Default)]
pub struct McMigrationReport {
    pub outcomes: Vec<McMigrationRowOutcome>,
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

    fn version_with_sha(
        source: ModSource,
        project_id: &str,
        version_id: &str,
        sha: &str,
    ) -> ModVersion {
        let mut v = version(source, project_id, version_id);
        v.primary_file.sha1 = Some(sha.into());
        v
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

    fn violated_loader() -> PlatformVerdict {
        PlatformVerdict::Violated {
            axis: PlatformAxis::Loader,
            declared: "[52,)".into(),
            actual: "47".into(),
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
    fn loader_axis_violation_whose_only_build_is_the_installed_jar_is_stranded_not_replaceable() {
        // Xaero's Minimap: declares forge >= 52, instance runs an older Forge.
        // The MC + loader-KIND query returns the same build already installed
        // (file sha "aa"), so a reinstall is a no-op that the post-apply rescan
        // re-flags — the reported "press Fix forever" loop. Must be stranded
        // (LoaderTooOld), never replaceable.
        let target = version_with_sha(ModSource::Modrinth, "xaero", "26.4.2", "aa");
        let plan = build_migration_plan(vec![input(
            "aa", // installed jar sha == the candidate's file sha
            "Xaero's Minimap",
            violated_loader(),
            Ok((ModSource::Modrinth, "xaero".to_string())),
            Some(CandidateQuery::Found(vec![target])),
        )]);
        assert!(
            plan.replaceable.is_empty(),
            "a same-file reinstall must never be offered as replaceable"
        );
        assert_eq!(plan.stranded.len(), 1);
        assert!(matches!(
            plan.stranded[0].reason,
            StrandedReason::LoaderTooOld
        ));
    }

    #[test]
    fn loader_axis_violation_with_a_genuinely_different_build_stays_replaceable() {
        // The other loader-axis direction: the instance loader is fine for a
        // NEWER mod build that differs from what's installed (sha "bb" != "aa").
        // That IS fixable by replacement, so it must stay replaceable — the
        // same-file guard must not strand a real update.
        let target = version_with_sha(ModSource::Modrinth, "mymod", "2.0", "bb");
        let plan = build_migration_plan(vec![input(
            "aa",
            "My Mod",
            violated_loader(),
            Ok((ModSource::Modrinth, "mymod".to_string())),
            Some(CandidateQuery::Found(vec![target.clone()])),
        )]);
        assert_eq!(plan.replaceable.len(), 1);
        assert_eq!(plan.replaceable[0].target, target);
        assert!(plan.stranded.is_empty());
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

    // -- resolve_migration_selections ---------------------------------------

    #[test]
    fn stranded_keep_disposition_produces_no_action() {
        let selections = McMigrationSelections {
            replace: vec![],
            new_dependencies: vec![],
            stranded: vec![StrandedSelection {
                sha1: "s1".into(),
                disposition: StrandedDisposition::Keep,
            }],
        };
        assert!(resolve_migration_selections(&selections).is_empty());
    }

    #[test]
    fn stranded_disable_and_remove_dispositions_map_to_their_actions() {
        let selections = McMigrationSelections {
            replace: vec![],
            new_dependencies: vec![],
            stranded: vec![
                StrandedSelection {
                    sha1: "disable-me".into(),
                    disposition: StrandedDisposition::Disable,
                },
                StrandedSelection {
                    sha1: "remove-me".into(),
                    disposition: StrandedDisposition::Remove,
                },
                StrandedSelection {
                    sha1: "keep-me".into(),
                    disposition: StrandedDisposition::Keep,
                },
            ],
        };
        let actions = resolve_migration_selections(&selections);
        assert_eq!(actions.len(), 2, "Keep must not produce a third action");
        assert!(actions.contains(&MigrationAction::DisableStranded {
            sha1: "disable-me".into()
        }));
        assert!(actions.contains(&MigrationAction::RemoveStranded {
            sha1: "remove-me".into()
        }));
    }

    #[test]
    fn replace_selection_produces_a_replace_action() {
        let target = version(ModSource::Modrinth, "bop", "v-1201");
        let selections = McMigrationSelections {
            replace: vec![ReplaceSelection {
                old_sha1: "bop-old".into(),
                target: target.clone(),
            }],
            new_dependencies: vec![],
            stranded: vec![],
        };
        let actions = resolve_migration_selections(&selections);
        assert_eq!(
            actions,
            vec![MigrationAction::Replace {
                old_sha1: "bop-old".into(),
                target,
            }]
        );
    }

    #[test]
    fn new_dependency_selection_produces_an_install_action() {
        let target = version(ModSource::Modrinth, "glitchcore", "g2");
        let selections = McMigrationSelections {
            replace: vec![],
            new_dependencies: vec![target.clone()],
            stranded: vec![],
        };
        let actions = resolve_migration_selections(&selections);
        assert_eq!(
            actions,
            vec![MigrationAction::InstallNewDependency { target }]
        );
    }

    /// THE LOAD-BEARING TEST. Reproduces the reported instance's shape:
    /// BiomesOPlenty is a `replaceable` row the user approved, and its
    /// chosen `target` — exactly like the real Modrinth listing — declares a
    /// MANDATORY required dependency on `terrablender` in `ModVersion::deps`.
    /// TerraBlender's own (separate) stranded row is explicitly set to
    /// `Keep`, and no `new_dependencies` row for it was accepted.
    ///
    /// A per-row `mods_update_one` call would resolve BoP's declared deps
    /// fresh and install `terrablender` regardless of the user's choice —
    /// landing a second `terrablender` jar next to the one the user chose to
    /// keep, which is the exact duplicate-modId FML abort this task exists
    /// to prevent. The produced action set must contain ONLY the BoP
    /// replacement — nothing that installs, mentions, or otherwise touches
    /// terrablender.
    #[test]
    fn replace_never_pulls_in_a_kept_dependency_from_target_deps() {
        use crate::mods::platform::{DepKind, DepProjectRef, ModDepLink};

        let mut bop_target = version(ModSource::Modrinth, "bop", "v-1201");
        bop_target.deps = vec![ModDepLink {
            kind: DepKind::Required,
            project_ref: DepProjectRef::Modrinth {
                project_id: "terrablender".into(),
                version_id: None,
            },
        }];

        let selections = McMigrationSelections {
            replace: vec![ReplaceSelection {
                old_sha1: "bop-old".into(),
                target: bop_target.clone(),
            }],
            new_dependencies: vec![], // nothing accepted for terrablender
            stranded: vec![StrandedSelection {
                sha1: "terrablender-old".into(),
                disposition: StrandedDisposition::Keep,
            }],
        };

        let actions = resolve_migration_selections(&selections);

        assert_eq!(
            actions,
            vec![MigrationAction::Replace {
                old_sha1: "bop-old".into(),
                target: bop_target,
            }],
            "expected only the BoP replace action — no terrablender install"
        );
        assert!(
            !actions.iter().any(|a| matches!(
                a,
                MigrationAction::InstallNewDependency { target }
                    if target.project_id == "terrablender"
            )),
            "terrablender must never be installed when its stranded row is Keep"
        );
    }

    #[test]
    fn empty_selections_produce_no_actions() {
        assert!(resolve_migration_selections(&McMigrationSelections::default()).is_empty());
    }

    #[test]
    fn per_mod_failure_is_representable_alongside_successes_in_the_report() {
        // The report type must be able to say "this row succeeded, this row
        // did not" within the SAME report — never an all-or-nothing shape.
        let report = McMigrationReport {
            outcomes: vec![
                McMigrationRowOutcome::Replaced {
                    old_sha1: "bop-old".into(),
                    name: "Biomes O' Plenty".into(),
                    new_sha1: "bop-new".into(),
                },
                McMigrationRowOutcome::Failed {
                    name: "Glitchcore".into(),
                    error: crate::error::Error::ModsSha1Unavailable,
                },
            ],
        };
        assert_eq!(report.outcomes.len(), 2);
        assert!(matches!(
            report.outcomes[0],
            McMigrationRowOutcome::Replaced { .. }
        ));
        assert!(matches!(
            report.outcomes[1],
            McMigrationRowOutcome::Failed { .. }
        ));
    }
}
