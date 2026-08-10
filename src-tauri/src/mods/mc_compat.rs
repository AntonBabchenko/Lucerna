//! Whether an installed jar's declared platform matches what an instance
//! provides. Pure — no I/O, no network.
//!
//! Two axes: the jar's `minecraft` dependency against the instance's MC
//! version, and its loader dependency (`forge` / `neoforge` / `fabricloader` /
//! `quilt_loader`) against the instance's loader version. The loader axis is
//! the load-bearing one: a `minecraft` block is a CONVENTION, not a
//! requirement, and half the jars in the reported repro declare none at all.
//!
//! The enforcement rules are `preflight::resolve`'s, reused verbatim rather
//! than re-derived — side filtering, descriptor authority, the `discouraged`
//! skip, and the INVERTED polarity of an `incompatible` declaration. Deriving
//! them a second time is how two consumers of one dependency list come to
//! disagree.

use crate::instances::schema::LoaderKind;
use crate::mods::local::{DepSide, DependencyKind, DescriptorEra, DescriptorSource, ManifestDeps};
use crate::mods::preflight::effective_rank;
use crate::mods::version_range::{satisfies, RangeFamily, Satisfaction};

/// Which half of the platform a verdict is about. Two unit variants, no
/// payload — crosses IPC directly (a later task puts `Option<PlatformAxis>`
/// on `ModLocalCompat`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum PlatformAxis {
    Minecraft,
    Loader,
}

/// The verdict for one jar against one instance.
//
// Deliberately not `specta::Type`: `Violated` carries `source: DescriptorSource`
// for internal diagnostics, and `DescriptorSource` is not an IPC type. This
// enum never crosses IPC itself — the command layer projects the parts the UI
// needs (`platform_mismatch: bool`, `platform_axis: Option<PlatformAxis>`,
// `platform_declared: Option<String>`) into a specta type instead.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlatformVerdict {
    /// At least one declaration was decided and held; none fired.
    Fits,
    /// A declaration fired. The loader will refuse this jar.
    Violated {
        axis: PlatformAxis,
        /// The range the jar declared, verbatim.
        declared: String,
        /// What the instance actually provides on that axis.
        actual: String,
        source: DescriptorSource,
        /// The declaration's own range grammar (Maven / Fabric / Quilt) — the
        /// verdict carries no other source of truth for it, so a consumer that
        /// needs to render `declared` (`range_describe::describe`) must not
        /// assume Maven just because most fired declarations are.
        family: RangeFamily,
    },
    /// No admitted declaration reached a decided outcome. Covers both
    /// "nothing was declared" and "something was declared and admitted but
    /// every one of those declarations evaluated to `Satisfaction::Unknown`"
    /// (a wildcard range, an unparseable bracket, a snapshot instance
    /// version). NEVER flags, never gates.
    Unknown,
}

/// What this instance provides for a declared platform id, plus the axis it
/// belongs to. `None` when the pairing is not comparable, which is the
/// conservative answer rather than a guess.
///
/// The family gate matters: NeoForge restarted its version numbering at 20.x
/// while MinecraftForge is at 47-61, so measuring a `forge` range against a
/// NeoForge version would manufacture violations out of an unrelated number
/// line. Same for `fabricloader` against Quilt.
fn actual_for<'a>(
    dep_id: &str,
    instance_mc: &'a str,
    loader: LoaderKind,
    loader_version: Option<&'a str>,
) -> Option<(PlatformAxis, &'a str)> {
    let id = dep_id.trim().to_ascii_lowercase();
    if id == "minecraft" {
        return Some((PlatformAxis::Minecraft, instance_mc));
    }
    let lv = loader_version?;
    let same_family = matches!(
        (id.as_str(), loader),
        ("forge", LoaderKind::Forge)
            | ("neoforge", LoaderKind::NeoForge)
            | ("fabricloader", LoaderKind::Fabric)
            | ("fabric-loader", LoaderKind::Fabric)
            | ("quilt_loader", LoaderKind::Quilt)
    );
    same_family.then_some((PlatformAxis::Loader, lv))
}

/// `fires` and `holds` jointly cover six of the nine `(kind, satisfaction)`
/// pairs — the three kinds `{Required, Optional, Incompatible}` crossed with
/// `{Satisfied, Violated}`. The remaining three, each kind paired with
/// `Satisfaction::Unknown`, are deliberately claimed by neither: that
/// combination means "undecided", not "healthy" or "failed", which is why
/// `platform_verdict` reports `Unknown` rather than `Fits` when nothing else
/// decided the outcome.
///
/// True when this (kind, satisfaction) pair is one the loader aborts on —
/// `preflight::resolve`'s own match arms, including the inverted `Incompatible`.
fn fires(kind: DependencyKind, sat: Satisfaction) -> bool {
    matches!(
        (kind, sat),
        (DependencyKind::Required, Satisfaction::Violated)
            | (DependencyKind::Optional, Satisfaction::Violated)
            | (DependencyKind::Incompatible, Satisfaction::Satisfied)
    )
}

/// True when this pair is a decided, healthy declaration — used only to tell
/// `Fits` from `Unknown`.
fn holds(kind: DependencyKind, sat: Satisfaction) -> bool {
    matches!(
        (kind, sat),
        (DependencyKind::Required, Satisfaction::Satisfied)
            | (DependencyKind::Optional, Satisfaction::Satisfied)
            | (DependencyKind::Incompatible, Satisfaction::Violated)
    )
}

/// Judge a jar's platform declarations against one instance.
pub fn platform_verdict(
    manifest: &ManifestDeps,
    instance_mc: &str,
    loader: LoaderKind,
    loader_version: Option<&str>,
    era: DescriptorEra,
) -> PlatformVerdict {
    let mut mc_violation: Option<PlatformVerdict> = None;
    let mut loader_violation: Option<PlatformVerdict> = None;
    let mut any_holds = false;

    for d in &manifest.platform {
        // 1. Side first, matching resolve() and ModSorter.java:275 — a
        //    side-filtered declaration is invisible to every check.
        if d.side == DepSide::Server {
            continue;
        }
        // 2. Only declarations from a descriptor this loader opens for this jar.
        if effective_rank(d.source, &manifest.sources_present, loader, era).is_none() {
            continue;
        }
        // 3. The loader only logs a warning for `discouraged` and carries on.
        //    Currently redundant: `fires`/`holds` below are exhaustive over
        //    {Required, Optional, Incompatible}, so a `Discouraged` kind never
        //    matches either and this `continue` cannot change any outcome
        //    today. Kept as the structural counterpart to `resolve`'s own
        //    filter, and as the guard that would matter if `fires`/`holds`
        //    ever grow a `Discouraged` arm. `discouraged_declaration_never_gates`
        //    pins the end-to-end property, not this line.
        if d.kind == DependencyKind::Discouraged {
            continue;
        }
        // 4. Map the declared id onto what this instance provides.
        let Some((axis, actual)) = actual_for(&d.dep_id, instance_mc, loader, loader_version)
        else {
            continue;
        };
        // 5. resolve()'s own (kind, satisfaction) arms.
        let sat = satisfies(actual, &d.range, d.family);
        if fires(d.kind, sat) {
            let v = PlatformVerdict::Violated {
                axis,
                declared: d.range.clone(),
                actual: actual.to_string(),
                source: d.source,
                family: d.family,
            };
            // The first admitted, firing declaration on an axis supplies the
            // reported `declared`/`actual` strings. That choice is arbitrary:
            // `effective_rank` admits per `DescriptorSource`, not per
            // `[[dependencies.<modid>]]` table, so a single `mods.toml`
            // bundling submodules can legitimately declare two different
            // `forge` ranges and have both admitted. The load-bearing
            // property is the axis determination itself, not which
            // declaration's strings win — `resolve`'s own semantics are an
            // OR of violations, so the axis is order-independent regardless
            // of which admitted declaration the loop reaches first (see
            // `same_axis_multiple_firing_declarations_is_order_independent`).
            match axis {
                PlatformAxis::Minecraft if mc_violation.is_none() => mc_violation = Some(v),
                PlatformAxis::Loader if loader_violation.is_none() => loader_violation = Some(v),
                _ => {}
            }
        } else if holds(d.kind, sat) {
            any_holds = true;
        }
    }

    // The Minecraft axis is reported in preference to the loader axis: "built
    // for Minecraft 1.21" is the more actionable sentence than "needs Forge 61".
    mc_violation.or(loader_violation).unwrap_or(if any_holds {
        PlatformVerdict::Fits
    } else {
        PlatformVerdict::Unknown
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::local::{
        DeclaredDep, DepSide, DependencyKind, DescriptorSource, ManifestDeps,
    };
    use crate::mods::version_range::RangeFamily;

    fn dep(id: &str, range: &str, source: DescriptorSource, family: RangeFamily) -> DeclaredDep {
        DeclaredDep {
            dep_id: id.into(),
            range: range.into(),
            kind: DependencyKind::Required,
            side: DepSide::Both,
            family,
            source,
        }
    }

    fn manifest(deps: Vec<DeclaredDep>, present: Vec<DescriptorSource>) -> ManifestDeps {
        ManifestDeps {
            provided: vec![],
            deps: vec![],
            sources_present: present,
            platform: deps,
        }
    }

    fn forge_manifest(deps: Vec<DeclaredDep>) -> ManifestDeps {
        manifest(deps, vec![DescriptorSource::ModsToml])
    }

    // ── the MC axis ────────────────────────────────────────────────────────
    #[test]
    fn closed_mc_range_below_the_instance_is_violated() {
        let m = forge_manifest(vec![dep(
            "minecraft",
            "[1.21.11,1.21.12)",
            DescriptorSource::ModsToml,
            RangeFamily::Maven,
        )]);
        let v = platform_verdict(
            &m,
            "1.20.1",
            LoaderKind::Forge,
            Some("47.4.10"),
            DescriptorEra::Modern,
        );
        assert!(
            matches!(
                v,
                PlatformVerdict::Violated {
                    axis: PlatformAxis::Minecraft,
                    ..
                }
            ),
            "{v:?}"
        );
    }

    #[test]
    fn open_ended_mc_range_below_the_instance_fits() {
        let m = forge_manifest(vec![dep(
            "minecraft",
            "[1.20.1,)",
            DescriptorSource::ModsToml,
            RangeFamily::Maven,
        )]);
        assert_eq!(
            platform_verdict(
                &m,
                "1.21",
                LoaderKind::Forge,
                Some("47.4.10"),
                DescriptorEra::Modern
            ),
            PlatformVerdict::Fits
        );
    }

    #[test]
    fn bare_maven_spec_is_a_soft_requirement_and_never_flags() {
        // Xaero's World Map declares exactly this. Maven — and FML, which
        // delegates to it — accepts any version for a bracket-less spec.
        let m = forge_manifest(vec![dep(
            "minecraft",
            "1.21.11",
            DescriptorSource::ModsToml,
            RangeFamily::Maven,
        )]);
        assert_eq!(
            platform_verdict(
                &m,
                "1.20.1",
                LoaderKind::Forge,
                Some("47.4.10"),
                DescriptorEra::Modern
            ),
            PlatformVerdict::Fits
        );
    }

    #[test]
    fn wildcard_range_is_unknown_not_a_violation() {
        let m = manifest(
            vec![dep(
                "minecraft",
                "1.20.x",
                DescriptorSource::FabricJson,
                RangeFamily::FabricPredicate,
            )],
            vec![DescriptorSource::FabricJson],
        );
        assert_eq!(
            platform_verdict(
                &m,
                "1.21",
                LoaderKind::Fabric,
                Some("0.15.11"),
                DescriptorEra::Modern
            ),
            PlatformVerdict::Unknown
        );
    }

    #[test]
    fn unparseable_range_and_snapshot_instance_are_unknown() {
        let m = forge_manifest(vec![dep(
            "minecraft",
            "[#compatible#)",
            DescriptorSource::ModsToml,
            RangeFamily::Maven,
        )]);
        assert_eq!(
            platform_verdict(
                &m,
                "1.20.1",
                LoaderKind::Forge,
                Some("47.4.10"),
                DescriptorEra::Modern
            ),
            PlatformVerdict::Unknown
        );

        let m2 = forge_manifest(vec![dep(
            "minecraft",
            "[1.21.11,1.21.12)",
            DescriptorSource::ModsToml,
            RangeFamily::Maven,
        )]);
        assert_eq!(
            platform_verdict(
                &m2,
                "24w14a",
                LoaderKind::Forge,
                Some("47.4.10"),
                DescriptorEra::Modern
            ),
            PlatformVerdict::Unknown
        );
    }

    #[test]
    fn no_platform_declaration_at_all_is_unknown() {
        let m = forge_manifest(vec![]);
        assert_eq!(
            platform_verdict(
                &m,
                "1.20.1",
                LoaderKind::Forge,
                Some("47.4.10"),
                DescriptorEra::Modern
            ),
            PlatformVerdict::Unknown
        );
    }

    /// The verdict must report the declaration's OWN family, not assume Maven.
    /// A Fabric mod built for 1.21 sitting in a 1.20.1 instance declares
    /// `"minecraft": ">=1.21"` in `fabric.mod.json` — a `FabricPredicate`, not
    /// Maven bracket notation. A consumer that hardcoded `RangeFamily::Maven`
    /// here would feed `>=1.21` to a Maven-grammar renderer and produce
    /// nonsense for the user, defeating this feature's whole purpose.
    #[test]
    fn a_fabric_predicate_violation_carries_its_own_family() {
        let m = manifest(
            vec![dep(
                "minecraft",
                ">=1.21",
                DescriptorSource::FabricJson,
                RangeFamily::FabricPredicate,
            )],
            vec![DescriptorSource::FabricJson],
        );
        let v = platform_verdict(
            &m,
            "1.20.1",
            LoaderKind::Fabric,
            Some("0.15.11"),
            DescriptorEra::Modern,
        );
        assert!(
            matches!(
                v,
                PlatformVerdict::Violated {
                    axis: PlatformAxis::Minecraft,
                    family: RangeFamily::FabricPredicate,
                    ..
                }
            ),
            "{v:?}"
        );
    }

    // ── the loader axis ────────────────────────────────────────────────────
    #[test]
    fn forge_range_above_the_instance_loader_is_violated() {
        // BiomesOPlenty, TerraBlender and GlitchCore declare no `minecraft`
        // block at all — this axis is the only one that catches them.
        let m = forge_manifest(vec![dep(
            "forge",
            "[61.0.2,)",
            DescriptorSource::ModsToml,
            RangeFamily::Maven,
        )]);
        let v = platform_verdict(
            &m,
            "1.20.1",
            LoaderKind::Forge,
            Some("47.4.10"),
            DescriptorEra::Modern,
        );
        assert!(
            matches!(
                v,
                PlatformVerdict::Violated {
                    axis: PlatformAxis::Loader,
                    ..
                }
            ),
            "{v:?}"
        );
    }

    #[test]
    fn forge_range_the_instance_satisfies_fits() {
        let m = forge_manifest(vec![dep(
            "forge",
            "[47.0.0,)",
            DescriptorSource::ModsToml,
            RangeFamily::Maven,
        )]);
        assert_eq!(
            platform_verdict(
                &m,
                "1.20.1",
                LoaderKind::Forge,
                Some("47.4.10"),
                DescriptorEra::Modern
            ),
            PlatformVerdict::Fits
        );
    }

    #[test]
    fn forge_range_on_a_neoforge_instance_is_unknown() {
        // NeoForge restarted its numbering at 20.x while Forge is at 47-61.
        // Comparing them would manufacture violations off an unrelated number line.
        let m = forge_manifest(vec![dep(
            "forge",
            "[61.0.2,)",
            DescriptorSource::ModsToml,
            RangeFamily::Maven,
        )]);
        assert_eq!(
            platform_verdict(
                &m,
                "1.21.1",
                LoaderKind::NeoForge,
                Some("21.1.5"),
                DescriptorEra::Modern
            ),
            PlatformVerdict::Unknown
        );
    }

    #[test]
    fn absent_loader_version_is_unknown_never_a_violation() {
        // `raw_minecraft.rs:49` writes `loader_version: None` on a real import.
        let m = forge_manifest(vec![dep(
            "forge",
            "[61.0.2,)",
            DescriptorSource::ModsToml,
            RangeFamily::Maven,
        )]);
        assert_eq!(
            platform_verdict(&m, "1.20.1", LoaderKind::Forge, None, DescriptorEra::Modern),
            PlatformVerdict::Unknown
        );
    }

    #[test]
    fn vanilla_instance_never_flags() {
        // NOT because `actual_for` is Vanilla-aware — its `minecraft` branch
        // fires for any loader. This is `Unknown` because `effective_rank`
        // returns `None` for every descriptor when the loader is Vanilla
        // (`descriptor_rank`'s `L::Vanilla => None` arm), so the declaration
        // never survives step 2 and `actual_for` is never even reached.
        let m = forge_manifest(vec![dep(
            "minecraft",
            "[1.21.11,1.21.12)",
            DescriptorSource::ModsToml,
            RangeFamily::Maven,
        )]);
        assert_eq!(
            platform_verdict(
                &m,
                "1.20.1",
                LoaderKind::Vanilla,
                None,
                DescriptorEra::Modern
            ),
            PlatformVerdict::Unknown
        );
    }

    // ── enforcement-rule parity with preflight::resolve ────────────────────
    #[test]
    fn server_side_declaration_does_not_gate_a_client_launch() {
        let mut d = dep(
            "minecraft",
            "[1.21.11,1.21.12)",
            DescriptorSource::ModsToml,
            RangeFamily::Maven,
        );
        d.side = DepSide::Server;
        let m = forge_manifest(vec![d]);
        assert_eq!(
            platform_verdict(
                &m,
                "1.20.1",
                LoaderKind::Forge,
                Some("47.4.10"),
                DescriptorEra::Modern
            ),
            PlatformVerdict::Unknown
        );
    }

    #[test]
    fn discouraged_declaration_never_gates() {
        let mut d = dep(
            "minecraft",
            "[1.21.11,1.21.12)",
            DescriptorSource::ModsToml,
            RangeFamily::Maven,
        );
        d.kind = DependencyKind::Discouraged;
        let m = forge_manifest(vec![d]);
        assert_eq!(
            platform_verdict(
                &m,
                "1.20.1",
                LoaderKind::Forge,
                Some("47.4.10"),
                DescriptorEra::Modern
            ),
            PlatformVerdict::Unknown
        );
    }

    #[test]
    fn incompatible_declaration_is_inverted() {
        // An `incompatible` range fires when the actual version IS inside it —
        // the opposite of Required. A re-derived fold gets this exactly backwards.
        let mut inside = dep(
            "minecraft",
            "[1.20,1.21)",
            DescriptorSource::ModsToml,
            RangeFamily::Maven,
        );
        inside.kind = DependencyKind::Incompatible;
        let m = forge_manifest(vec![inside]);
        let v = platform_verdict(
            &m,
            "1.20.1",
            LoaderKind::Forge,
            Some("47.4.10"),
            DescriptorEra::Modern,
        );
        assert!(matches!(v, PlatformVerdict::Violated { .. }), "{v:?}");

        let mut outside = dep(
            "minecraft",
            "[1.21,1.22)",
            DescriptorSource::ModsToml,
            RangeFamily::Maven,
        );
        outside.kind = DependencyKind::Incompatible;
        let m2 = forge_manifest(vec![outside]);
        assert_eq!(
            platform_verdict(
                &m2,
                "1.20.1",
                LoaderKind::Forge,
                Some("47.4.10"),
                DescriptorEra::Modern
            ),
            PlatformVerdict::Fits
        );
    }

    // ── descriptor authority ───────────────────────────────────────────────
    #[test]
    fn multi_loader_jar_is_judged_on_the_descriptor_its_loader_reads() {
        // Same jar, same Minecraft, opposite verdicts — isolating effective_rank.
        let both = vec![
            dep(
                "minecraft",
                ">=1.21",
                DescriptorSource::FabricJson,
                RangeFamily::FabricPredicate,
            ),
            dep(
                "minecraft",
                "[1.20.1,]",
                DescriptorSource::ModsToml,
                RangeFamily::Maven,
            ),
        ];
        let present = vec![DescriptorSource::FabricJson, DescriptorSource::ModsToml];

        assert_eq!(
            platform_verdict(
                &manifest(both.clone(), present.clone()),
                "1.20.1",
                LoaderKind::Forge,
                Some("47.4.10"),
                DescriptorEra::Modern
            ),
            PlatformVerdict::Fits,
            "Forge reads mods.toml: [1.20.1,] holds"
        );
        let v = platform_verdict(
            &manifest(both, present),
            "1.20.1",
            LoaderKind::Fabric,
            Some("0.15.11"),
            DescriptorEra::Modern,
        );
        assert!(
            matches!(v, PlatformVerdict::Violated { .. }),
            "Fabric reads fabric.mod.json: >=1.21 fails on 1.20.1; got {v:?}"
        );
    }

    // ── axis precedence when both fire ──────────────────────────────────────
    #[test]
    fn mc_axis_is_reported_when_both_axes_violate() {
        // Both a `minecraft` and a `forge` declaration are admitted and
        // fire; the MC axis is reported because "built for Minecraft 1.21"
        // is the more actionable sentence than "needs Forge 61" (module
        // doc). A refactor of the `.or()` chain in `platform_verdict` could
        // silently invert this precedence while every other test stayed
        // green — this test exists to catch exactly that.
        let m = forge_manifest(vec![
            dep(
                "minecraft",
                "[1.21.11,1.21.12)",
                DescriptorSource::ModsToml,
                RangeFamily::Maven,
            ),
            dep(
                "forge",
                "[61.0.2,)",
                DescriptorSource::ModsToml,
                RangeFamily::Maven,
            ),
        ]);
        let v = platform_verdict(
            &m,
            "1.20.1",
            LoaderKind::Forge,
            Some("47.4.10"),
            DescriptorEra::Modern,
        );
        assert!(
            matches!(
                v,
                PlatformVerdict::Violated {
                    axis: PlatformAxis::Minecraft,
                    ..
                }
            ),
            "{v:?}"
        );
    }

    // ── same-axis tie-break: arbitrary string, deterministic axis ──────────
    #[test]
    fn same_axis_multiple_firing_declarations_is_order_independent() {
        // `effective_rank` admits per `DescriptorSource`, not per
        // `[[dependencies.<modid>]]` table — a `mods.toml` bundling
        // submodules can legitimately declare two different `forge` ranges
        // and have both admitted. Decision: keep first-wins for the
        // reported `declared`/`actual` strings; do not add ordering
        // cleverness. What this test pins is narrower and load-bearing: the
        // AXIS the verdict reports must not depend on declaration order,
        // because `resolve`'s own semantics are an OR of violations.
        let a = dep(
            "forge",
            "[61.0.2,)",
            DescriptorSource::ModsToml,
            RangeFamily::Maven,
        );
        let b = dep(
            "forge",
            "[60.0.0,)",
            DescriptorSource::ModsToml,
            RangeFamily::Maven,
        );

        let forward = forge_manifest(vec![a.clone(), b.clone()]);
        let v1 = platform_verdict(
            &forward,
            "1.20.1",
            LoaderKind::Forge,
            Some("47.4.10"),
            DescriptorEra::Modern,
        );
        assert!(
            matches!(
                v1,
                PlatformVerdict::Violated {
                    axis: PlatformAxis::Loader,
                    ..
                }
            ),
            "{v1:?}"
        );

        let reversed = forge_manifest(vec![b, a]);
        let v2 = platform_verdict(
            &reversed,
            "1.20.1",
            LoaderKind::Forge,
            Some("47.4.10"),
            DescriptorEra::Modern,
        );
        assert!(
            matches!(
                v2,
                PlatformVerdict::Violated {
                    axis: PlatformAxis::Loader,
                    ..
                }
            ),
            "{v2:?}"
        );
    }
}
