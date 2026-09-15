//! The loader's own tolerance table — FML's `VersionSupportMatrix`. Pure — no
//! I/O, no network.
//!
//! FML does not measure a `[[dependencies]]` range against the running version
//! alone. `ModSorter` counts a declaration as CONTAINED when the running
//! version is in range OR any version the loader lists for the running
//! Minecraft is — which is why NeoForge 1.21.1 loads a jar declaring
//! `minecraft [1.21,1.21.1)`. A verdict that skips this table calls such a jar
//! incompatible, gates Play on it, and offers to delete it (reported on a
//! NeoForge 1.21.1 pack whose seven "incompatible" mods all loaded in-game).
//!
//! Every entry was read off the loader's class files on Maven, including the
//! first build that carries it: Forge added its entries after the first builds
//! for a Minecraft version had shipped, and those builds refuse what later ones
//! accept. The `languageloader.javafml` overrides are not modelled — nothing
//! here judges `loaderVersion`.

use crate::instances::schema::LoaderKind;
use crate::mods::mc_compat::PlatformAxis;
use crate::mods::version_range::{satisfies, RangeFamily, Satisfaction};

/// Whether the instance's loader build carries an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Applicability {
    Applies,
    /// The entry starts at a particular build and the instance's loader version
    /// is absent or not comparable — the loader may or may not honour it.
    Unknown,
}

/// The extra versions the loader accepts on one axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Overrides {
    pub(crate) versions: &'static [&'static str],
    pub(crate) applicability: Applicability,
}

struct Entry {
    loader: LoaderKind,
    /// The running Minecraft version the loader keys the entry on — exactly:
    /// NeoForge tests `[x]`, Forge tests string equality.
    mc: &'static str,
    /// First loader build carrying the entry; `None` when every build for this
    /// Minecraft version does.
    since: Option<&'static str>,
    /// `mod.minecraft` overrides.
    minecraft: &'static [&'static str],
    /// `mod.neoforge` / `mod.forge` overrides — the loader's own id.
    loader_ids: &'static [&'static str],
}

const MATRIX: &[Entry] = &[
    // FML 4.0.24 (NeoForge 21.1.1, the first 1.21.1 build) through 4.0.42
    // (21.1.235); FancyModLoader `6a1f952`.
    Entry {
        loader: LoaderKind::NeoForge,
        mc: "1.21.1",
        since: None,
        minecraft: &["1.21"],
        loader_ids: &["21.0.166"],
    },
    // FML 9.0.16 (NeoForge 21.8.0-beta, the first 1.21.8 build) onward;
    // FancyModLoader `610108b`.
    Entry {
        loader: LoaderKind::NeoForge,
        mc: "1.21.8",
        since: None,
        minecraft: &["1.21.7"],
        loader_ids: &["21.7.26-beta"],
    },
    // fmlloader 1.21.1-52.0.1; absent from 52.0.0.
    Entry {
        loader: LoaderKind::Forge,
        mc: "1.21.1",
        since: Some("52.0.1"),
        minecraft: &["1.21"],
        loader_ids: &["51.0.33"],
    },
    // fmlloader 1.20.1-47.2.16; absent from 47.2.14, the previous published
    // build. Forge only — no Minecraft override on this one.
    Entry {
        loader: LoaderKind::Forge,
        mc: "1.20.1",
        since: Some("47.2.16"),
        minecraft: &[],
        loader_ids: &["47.1.79"],
    },
    // fmlloader 1.19.2-43.0.3; absent from 43.0.2.
    Entry {
        loader: LoaderKind::Forge,
        mc: "1.19.2",
        since: Some("43.0.3"),
        minecraft: &["1.19.1"],
        loader_ids: &["42.0.9"],
    },
];

/// The extra versions the loader running this instance accepts on `axis`, or
/// `None` when it carries no override there.
pub(crate) fn overrides_for(
    loader: LoaderKind,
    instance_mc: &str,
    loader_version: Option<&str>,
    axis: PlatformAxis,
) -> Option<Overrides> {
    let entry = MATRIX
        .iter()
        .find(|e| e.loader == loader && e.mc == instance_mc.trim())?;
    let versions = match axis {
        PlatformAxis::Minecraft => entry.minecraft,
        PlatformAxis::Loader => entry.loader_ids,
    };
    if versions.is_empty() {
        return None;
    }
    let applicability = match (entry.since, loader_version) {
        (None, _) => Applicability::Applies,
        (Some(_), None) => Applicability::Unknown,
        (Some(first), Some(lv)) => build_carries(entry.mc, lv, first)?,
    };
    Some(Overrides {
        versions,
        applicability,
    })
}

/// Whether loader build `loader_version` is `first` or later; `None` when it
/// is provably older.
///
/// Forge's own Maven coordinate is `<mc>-<build>`, so this instance's
/// Minecraft prefix is dropped before comparing. A prefix naming ANOTHER
/// Minecraft version is not a build number this can order — measured as-is it
/// would compare `1` against `47` and confidently call every build older — so
/// it is `Unknown`. (No Forge build number itself starts with `1.`.)
fn build_carries(instance_mc: &str, loader_version: &str, first: &str) -> Option<Applicability> {
    let lv = loader_version.trim();
    let build = lv
        .strip_prefix(instance_mc)
        .and_then(|rest| rest.strip_prefix('-'))
        .unwrap_or(lv);
    if build
        .split_once('-')
        .is_some_and(|(head, _)| head.starts_with("1."))
    {
        return Some(Applicability::Unknown);
    }
    match satisfies(build, &format!("[{first},)"), RangeFamily::Maven) {
        Satisfaction::Satisfied => Some(Applicability::Applies),
        Satisfaction::Violated => None,
        Satisfaction::Unknown => Some(Applicability::Unknown),
    }
}

/// FML's containment test for a Maven `range` (`ModSorter` →
/// `VersionSupportMatrix.testVersionSupportMatrix`): contained when the
/// running version is (`direct`) OR the build carries an override version that
/// is. Three-valued, so "could not tell" never becomes either answer on its own.
pub(crate) fn widen(
    direct: Satisfaction,
    range: &str,
    overrides: Option<Overrides>,
) -> Satisfaction {
    let Some(o) = overrides else {
        return direct;
    };
    let via_override = o
        .versions
        .iter()
        .map(|v| satisfies(v, range, RangeFamily::Maven))
        .fold(Satisfaction::Violated, any);
    let via_override = match (o.applicability, via_override) {
        (Applicability::Unknown, Satisfaction::Satisfied) => Satisfaction::Unknown,
        (_, s) => s,
    };
    any(direct, via_override)
}

/// Three-valued OR.
fn any(a: Satisfaction, b: Satisfaction) -> Satisfaction {
    match (a, b) {
        (Satisfaction::Satisfied, _) | (_, Satisfaction::Satisfied) => Satisfaction::Satisfied,
        (Satisfaction::Unknown, _) | (_, Satisfaction::Unknown) => Satisfaction::Unknown,
        (Satisfaction::Violated, Satisfaction::Violated) => Satisfaction::Violated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mc(loader: LoaderKind, instance_mc: &str, lv: Option<&str>) -> Option<Overrides> {
        overrides_for(loader, instance_mc, lv, PlatformAxis::Minecraft)
    }

    fn ld(loader: LoaderKind, instance_mc: &str, lv: Option<&str>) -> Option<Overrides> {
        overrides_for(loader, instance_mc, lv, PlatformAxis::Loader)
    }

    const APPLIES: Applicability = Applicability::Applies;

    fn o(versions: &'static [&'static str], applicability: Applicability) -> Option<Overrides> {
        Some(Overrides {
            versions,
            applicability,
        })
    }

    // ── the table ───────────────────────────────────────────────────────────
    #[test]
    fn neoforge_1_21_1_accepts_1_21_on_every_build() {
        for lv in [Some("21.1.1"), Some("21.1.235"), None] {
            assert_eq!(
                mc(LoaderKind::NeoForge, "1.21.1", lv),
                o(&["1.21"], APPLIES)
            );
            assert_eq!(
                ld(LoaderKind::NeoForge, "1.21.1", lv),
                o(&["21.0.166"], APPLIES)
            );
        }
    }

    #[test]
    fn neoforge_1_21_8_accepts_1_21_7_on_every_build() {
        assert_eq!(
            mc(LoaderKind::NeoForge, "1.21.8", Some("21.8.0-beta")),
            o(&["1.21.7"], APPLIES)
        );
        assert_eq!(
            ld(LoaderKind::NeoForge, "1.21.8", None),
            o(&["21.7.26-beta"], APPLIES)
        );
    }

    #[test]
    fn forge_1_21_1_entry_starts_at_52_0_1() {
        assert_eq!(mc(LoaderKind::Forge, "1.21.1", Some("52.0.0")), None);
        assert_eq!(
            mc(LoaderKind::Forge, "1.21.1", Some("52.0.1")),
            o(&["1.21"], APPLIES)
        );
        assert_eq!(
            ld(LoaderKind::Forge, "1.21.1", Some("52.1.16")),
            o(&["51.0.33"], APPLIES)
        );
    }

    #[test]
    fn an_unknown_build_makes_a_floored_entry_unknown_not_absent() {
        assert_eq!(
            mc(LoaderKind::Forge, "1.21.1", None),
            o(&["1.21"], Applicability::Unknown)
        );
        assert_eq!(
            mc(LoaderKind::Forge, "1.21.1", Some("")),
            o(&["1.21"], Applicability::Unknown)
        );
    }

    #[test]
    fn a_minecraft_prefixed_build_is_read_as_its_build() {
        // Forge's own Maven coordinate is `<mc>-<build>`. No writer stores that
        // shape today, but a comparison of `1` against `47` must not be allowed
        // to decide "this build predates the entry" if one ever does.
        assert_eq!(
            ld(LoaderKind::Forge, "1.20.1", Some("1.20.1-47.4.10")),
            o(&["47.1.79"], APPLIES)
        );
        assert_eq!(
            ld(LoaderKind::Forge, "1.20.1", Some("1.20.1-47.2.14")),
            None
        );
    }

    #[test]
    fn a_build_prefixed_with_another_minecraft_is_unknown_not_old() {
        assert_eq!(
            ld(LoaderKind::Forge, "1.20.1", Some("1.20-47.4.10")),
            o(&["47.1.79"], Applicability::Unknown)
        );
        assert_eq!(
            mc(LoaderKind::Forge, "1.21.1", Some("forge-52.1.0")),
            o(&["1.21"], Applicability::Unknown)
        );
    }

    #[test]
    fn forge_1_20_1_relaxes_only_the_loader_axis_from_47_2_16() {
        assert_eq!(mc(LoaderKind::Forge, "1.20.1", Some("47.4.10")), None);
        assert_eq!(ld(LoaderKind::Forge, "1.20.1", Some("47.2.14")), None);
        assert_eq!(
            ld(LoaderKind::Forge, "1.20.1", Some("47.2.16")),
            o(&["47.1.79"], APPLIES)
        );
    }

    #[test]
    fn forge_1_19_2_entry_starts_at_43_0_3() {
        assert_eq!(mc(LoaderKind::Forge, "1.19.2", Some("43.0.2")), None);
        assert_eq!(
            mc(LoaderKind::Forge, "1.19.2", Some("43.0.3")),
            o(&["1.19.1"], APPLIES)
        );
        assert_eq!(
            ld(LoaderKind::Forge, "1.19.2", Some("43.5.2")),
            o(&["42.0.9"], APPLIES)
        );
    }

    #[test]
    fn no_entry_for_other_versions_or_loaders() {
        assert_eq!(mc(LoaderKind::NeoForge, "1.21", Some("21.0.167")), None);
        assert_eq!(mc(LoaderKind::NeoForge, "1.21.11", Some("21.11.45")), None);
        assert_eq!(mc(LoaderKind::Forge, "1.21.11", Some("61.1.0")), None);
        assert_eq!(mc(LoaderKind::Fabric, "1.21.1", Some("0.16.0")), None);
        assert_eq!(mc(LoaderKind::Quilt, "1.21.1", Some("0.26.0")), None);
        assert_eq!(mc(LoaderKind::Vanilla, "1.21.1", None), None);
    }

    // ── the containment rule ───────────────────────────────────────────────
    #[test]
    fn widen_is_the_direct_answer_without_overrides() {
        for d in [
            Satisfaction::Satisfied,
            Satisfaction::Violated,
            Satisfaction::Unknown,
        ] {
            assert_eq!(widen(d, "[1.21,1.21.1)", None), d);
        }
    }

    #[test]
    fn an_applying_override_inside_the_range_contains_it() {
        let over = o(&["1.21"], APPLIES);
        assert_eq!(
            widen(Satisfaction::Violated, "[1.21,1.21.1)", over),
            Satisfaction::Satisfied
        );
        assert_eq!(
            widen(Satisfaction::Violated, "[1.21]", over),
            Satisfaction::Satisfied
        );
        assert_eq!(
            widen(Satisfaction::Unknown, "[1.21]", over),
            Satisfaction::Satisfied
        );
    }

    #[test]
    fn an_override_outside_the_range_changes_nothing() {
        let over = o(&["1.21"], APPLIES);
        assert_eq!(
            widen(Satisfaction::Violated, "[1.20,1.21)", over),
            Satisfaction::Violated
        );
        assert_eq!(
            widen(Satisfaction::Unknown, "[1.20,1.21)", over),
            Satisfaction::Unknown
        );
    }

    #[test]
    fn an_unknown_build_can_only_soften_a_violation_to_unknown() {
        let over = o(&["1.21"], Applicability::Unknown);
        assert_eq!(
            widen(Satisfaction::Violated, "[1.21,1.21.1)", over),
            Satisfaction::Unknown
        );
        assert_eq!(
            widen(Satisfaction::Violated, "[1.20,1.21)", over),
            Satisfaction::Violated
        );
    }

    #[test]
    fn an_undecidable_override_comparison_is_unknown() {
        // `21.7.26-beta` against a bound equal up to its qualifier: the
        // evaluator cannot order the qualifier, so it must not decide.
        assert_eq!(
            widen(
                Satisfaction::Violated,
                "[21.7.26,21.8)",
                o(&["21.7.26-beta"], APPLIES)
            ),
            Satisfaction::Unknown
        );
    }

    #[test]
    fn a_satisfied_direct_answer_is_never_touched() {
        assert_eq!(
            widen(Satisfaction::Satisfied, "[1.21.1]", o(&["1.21"], APPLIES)),
            Satisfaction::Satisfied
        );
    }
}
