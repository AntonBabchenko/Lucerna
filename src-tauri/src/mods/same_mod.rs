//! Is the jar an install is about to write a mod the instance already has? Asked by the ids the
//! loader reads, because the loader's own rule is the one that stops the game: a mod-id present
//! twice (spec 2026-10-09 same-mod-by-id §2).
//!
//! Two questions, two answers:
//! - a **conflict** — the two jars share any id: both copies cannot stand, whatever the reason;
//! - an **identity** — the two jars carry the same ids: the planned project IS the installed mod,
//!   which is worth remembering as an alias (`cross_ids::remember`). A `provides` overlap or a
//!   multi-mod jar meeting one of its modules is a conflict, never an identity.
//!
//! Pure: the callers read the jars (`preflight::parse_instance`, `preflight::scan_loose_jar`).

use std::collections::BTreeSet;

use crate::instances::schema::LoaderKind;
use crate::mods::local::{DescriptorEra, DescriptorSource, ManifestDeps};
use crate::mods::platform::ModSource;
use crate::mods::preflight::{canon_id, effective_rank};

/// Ids no mod owns — the game, the runtime, the loaders. A descriptor may name one; two mods naming
/// it never collide on it.
const PLATFORM_IDS: &[&str] = &[
    "minecraft",
    "java",
    "forge",
    "neoforge",
    "fml",
    "mcp",
    "fabricloader",
    "fabric_loader",
    "quilt_loader",
];

/// The ids a jar answers for to this instance's loader, canonical (`preflight::canon_id`):
/// - only from a descriptor the loader opens for THIS jar (`effective_rank`, shadowing included) —
///   a NeoForge instance never reads `fabric.mod.json`; the union preflight uses for «is it
///   provided» over-claims here, where a claim refuses an install;
/// - on the legacy era the `@Mod` annotation's ids, `mcmod.info`'s only when the jar has none —
///   `mcmod.info` is display metadata, and a suite may list every module in each jar's copy;
/// - Fabric/Quilt `provides` included: the loader treats a provided id as present;
/// - never Jar-in-Jar (`ManifestDeps::provided` holds none — the scan keeps it apart) and never
///   a platform id.
pub(crate) fn loader_ids(
    manifest: &ManifestDeps,
    loader: LoaderKind,
    era: DescriptorEra,
) -> BTreeSet<String> {
    let read = |source: DescriptorSource| {
        effective_rank(source, &manifest.sources_present, loader, era).is_some()
    };
    let annotated = manifest
        .provided
        .iter()
        .any(|p| p.source == DescriptorSource::McmodAnnotation && read(p.source));
    manifest
        .provided
        .iter()
        .filter(|p| read(p.source))
        .filter(|p| !(annotated && p.source == DescriptorSource::McmodInfo))
        .map(|p| canon_id(&p.mod_id))
        .filter(|id| !id.is_empty() && !PLATFORM_IDS.contains(&id.as_str()))
        .collect()
}

/// An id both sets carry — the reason two jars cannot stand together.
pub(crate) fn shared_id(a: &BTreeSet<String>, b: &BTreeSet<String>) -> Option<String> {
    a.intersection(b).next().cloned()
}

/// The same mod: the same ids, and some.
pub(crate) fn same_mod(a: &BTreeSet<String>, b: &BTreeSet<String>) -> bool {
    !a.is_empty() && a == b
}

/// What a planned jar conflicts with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Meeting {
    /// An installed row (its index in the list handed in) and the id they share.
    Installed { row: usize, id: String },
    /// A planned item kept before it (its index in the plan) and the id they share.
    Planned { item: usize, id: String },
}

/// For each planned item, in plan order: what it conflicts with, or `None` when it may be
/// installed. An installed row wins over a planned item; among planned items the earlier KEPT one
/// wins (a dropped item shadows nothing — it is not installed). `None` ids (a jar that would not
/// open) never conflict and never shadow: «could not tell» is not «the same mod».
pub(crate) fn plan_conflicts(
    planned: &[Option<BTreeSet<String>>],
    installed: &[BTreeSet<String>],
) -> Vec<Option<Meeting>> {
    let mut kept: Vec<usize> = Vec::new();
    planned
        .iter()
        .enumerate()
        .map(|(i, ids)| {
            let ids = ids.as_ref()?;
            let met = installed
                .iter()
                .enumerate()
                .find_map(|(row, have)| {
                    shared_id(ids, have).map(|id| Meeting::Installed { row, id })
                })
                .or_else(|| {
                    kept.iter().find_map(|&item| {
                        let earlier = planned[item].as_ref()?;
                        shared_id(ids, earlier).map(|id| Meeting::Planned { item, id })
                    })
                });
            if met.is_none() {
                kept.push(i);
            }
            met
        })
        .collect()
}

/// The alias an identity teaches: the installed row's own identity and the planned project, when
/// the two are the same mod and one is on Modrinth, the other on CurseForge. `None` otherwise — an
/// alias maps one platform to the other, and a conflict is no identity.
pub(crate) fn alias_to_teach(
    row: (Option<ModSource>, Option<&str>, &BTreeSet<String>),
    planned: (ModSource, &str, &BTreeSet<String>),
) -> Option<((ModSource, String), (ModSource, String))> {
    let (Some(row_source), Some(row_id), row_ids) = row else {
        return None;
    };
    let (planned_source, planned_id, planned_ids) = planned;
    let platforms = matches!(
        (row_source, planned_source),
        (ModSource::Modrinth, ModSource::Curseforge) | (ModSource::Curseforge, ModSource::Modrinth)
    );
    (platforms && same_mod(row_ids, planned_ids)).then(|| {
        (
            (row_source, row_id.to_string()),
            (planned_source, planned_id.to_string()),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::local::{DescriptorSource as S, ProvidedMod};

    fn provided(id: &str, source: S) -> ProvidedMod {
        ProvidedMod {
            mod_id: id.to_string(),
            version: Some("1".into()),
            source,
        }
    }

    fn manifest(provided: Vec<ProvidedMod>, present: &[S]) -> ManifestDeps {
        ManifestDeps {
            provided,
            sources_present: present.to_vec(),
            ..Default::default()
        }
    }

    fn ids(list: &[&str]) -> BTreeSet<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    // ── loader_ids ──

    #[test]
    fn only_the_descriptors_this_loader_opens_count() {
        // A multi-loader jar: NeoForge reads its toml, never its fabric.mod.json.
        let m = manifest(
            vec![
                provided("jei", S::NeoForgeToml),
                provided("jei-fabric", S::FabricJson),
            ],
            &[S::NeoForgeToml, S::FabricJson],
        );
        assert_eq!(
            loader_ids(&m, LoaderKind::NeoForge, DescriptorEra::Modern),
            ids(&["jei"])
        );
        assert_eq!(
            loader_ids(&m, LoaderKind::Fabric, DescriptorEra::Modern),
            ids(&["jei_fabric"])
        );
    }

    #[test]
    fn a_shadowed_descriptor_does_not_count() {
        // NeoForge opens neoforge.mods.toml and, only without it, mods.toml.
        let m = manifest(
            vec![
                provided("new", S::NeoForgeToml),
                provided("old", S::ModsToml),
            ],
            &[S::NeoForgeToml, S::ModsToml],
        );
        assert_eq!(
            loader_ids(&m, LoaderKind::NeoForge, DescriptorEra::Modern),
            ids(&["new"])
        );
    }

    #[test]
    fn on_the_legacy_era_the_annotation_outranks_mcmod_info() {
        // A suite's shared mcmod.info lists every module; the class annotation is the jar's own.
        let m = manifest(
            vec![
                provided("mekanism", S::McmodAnnotation),
                provided("mekanism", S::McmodInfo),
                provided("mekanismgenerators", S::McmodInfo),
            ],
            &[S::McmodInfo],
        );
        assert_eq!(
            loader_ids(&m, LoaderKind::Forge, DescriptorEra::Legacy),
            ids(&["mekanism"])
        );
    }

    #[test]
    fn mcmod_info_answers_when_the_jar_has_no_annotation() {
        let m = manifest(vec![provided("oldmod", S::McmodInfo)], &[S::McmodInfo]);
        assert_eq!(
            loader_ids(&m, LoaderKind::Forge, DescriptorEra::Legacy),
            ids(&["oldmod"])
        );
    }

    #[test]
    fn provides_count_platform_ids_do_not_and_ids_are_canonical() {
        let m = manifest(
            vec![
                provided("Quilted-Fabric-API", S::QuiltJson),
                provided("fabric-api", S::QuiltJson),
                provided("minecraft", S::QuiltJson),
                provided("quilt_loader", S::QuiltJson),
            ],
            &[S::QuiltJson],
        );
        assert_eq!(
            loader_ids(&m, LoaderKind::Quilt, DescriptorEra::Modern),
            ids(&["quilted_fabric_api", "fabric_api"])
        );
    }

    #[test]
    fn a_jar_that_declares_nothing_answers_for_nothing() {
        let m = manifest(Vec::new(), &[]);
        assert!(loader_ids(&m, LoaderKind::Fabric, DescriptorEra::Modern).is_empty());
    }

    // ── shared_id / same_mod ──

    #[test]
    fn any_shared_id_is_a_conflict_only_equal_sets_are_the_same_mod() {
        let qfapi = ids(&["quilted_fabric_api", "fabric_api"]);
        let fapi = ids(&["fabric_api"]);
        assert_eq!(shared_id(&qfapi, &fapi), Some("fabric_api".to_string()));
        assert!(!same_mod(&qfapi, &fapi));

        let balm_mr = ids(&["balm"]);
        let balm_cf = ids(&["balm"]);
        assert!(same_mod(&balm_mr, &balm_cf));

        let suite = ids(&["a", "b"]);
        let module = ids(&["a"]);
        assert_eq!(shared_id(&suite, &module), Some("a".to_string()));
        assert!(!same_mod(&suite, &module));

        assert_eq!(shared_id(&ids(&["x"]), &ids(&["y"])), None);
        assert!(!same_mod(&BTreeSet::new(), &BTreeSet::new()));
    }

    // ── plan_conflicts ──

    #[test]
    fn a_planned_jar_meeting_an_installed_one_is_dropped_with_that_row() {
        let planned = vec![Some(ids(&["create"])), Some(ids(&["balm"]))];
        let installed = vec![ids(&["jei"]), ids(&["balm"])];
        assert_eq!(
            plan_conflicts(&planned, &installed),
            vec![
                None,
                Some(Meeting::Installed {
                    row: 1,
                    id: "balm".into()
                })
            ]
        );
    }

    #[test]
    fn of_two_planned_jars_with_one_id_the_earlier_is_kept() {
        let planned = vec![
            Some(ids(&["waystones"])),
            Some(ids(&["balm"])),
            Some(ids(&["balm"])),
        ];
        assert_eq!(
            plan_conflicts(&planned, &[]),
            vec![
                None,
                None,
                Some(Meeting::Planned {
                    item: 1,
                    id: "balm".into()
                })
            ]
        );
    }

    #[test]
    fn a_dropped_item_shadows_nothing() {
        // Item 1 is dropped for the installed balm; item 2 shares only item 1's other id.
        let planned = vec![
            Some(ids(&["primary"])),
            Some(ids(&["balm", "balm_extra"])),
            Some(ids(&["balm_extra"])),
        ];
        let installed = vec![ids(&["balm"])];
        assert_eq!(
            plan_conflicts(&planned, &installed),
            vec![
                None,
                Some(Meeting::Installed {
                    row: 0,
                    id: "balm".into()
                }),
                None
            ]
        );
    }

    #[test]
    fn a_jar_that_would_not_open_is_never_dropped_and_never_shadows() {
        let planned = vec![None, Some(ids(&["balm"]))];
        assert_eq!(plan_conflicts(&planned, &[]), vec![None, None]);
        let installed = vec![ids(&["balm"])];
        assert_eq!(plan_conflicts(&[None], &installed), vec![None]);
    }

    // ── alias_to_teach ──

    #[test]
    fn an_identity_across_modrinth_and_curseforge_teaches_the_alias() {
        let balm = ids(&["balm"]);
        assert_eq!(
            alias_to_teach(
                (Some(ModSource::Modrinth), Some("MBA"), &balm),
                (ModSource::Curseforge, "531761", &balm),
            ),
            Some((
                (ModSource::Modrinth, "MBA".to_string()),
                (ModSource::Curseforge, "531761".to_string())
            ))
        );
    }

    #[test]
    fn a_conflict_the_same_platform_or_a_sourceless_row_teaches_nothing() {
        let balm = ids(&["balm"]);
        let qfapi = ids(&["quilted_fabric_api", "fabric_api"]);
        let fapi = ids(&["fabric_api"]);
        // A conflict is no identity.
        assert_eq!(
            alias_to_teach(
                (Some(ModSource::Modrinth), Some("QSL"), &qfapi),
                (ModSource::Curseforge, "306612", &fapi),
            ),
            None
        );
        // An alias maps one platform to the other.
        assert_eq!(
            alias_to_teach(
                (Some(ModSource::Modrinth), Some("MBA"), &balm),
                (ModSource::Modrinth, "FORK", &balm),
            ),
            None
        );
        // A jar dropped in by hand has no platform identity to hang an alias on.
        assert_eq!(
            alias_to_teach(
                (None, None, &balm),
                (ModSource::Curseforge, "531761", &balm)
            ),
            None
        );
    }
}
