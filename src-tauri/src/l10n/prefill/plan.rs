//! Turning "these keys are missing" into "these are the requests to make".
//!
//! Three jobs, in order:
//! 1. **Dedup.** Distinct `(source string, role)` pairs are asked once, no
//!    matter how many mods and keys want them. In a real pack the overlap is
//!    large ("Energy", "Tier", "Durability"), so this is both the biggest
//!    cost saving and the reason repeated terms come back consistent for free.
//! 2. **Two passes.** Names first; the run freezes their answers as the
//!    glossary for the prose pass. This is what stops one term becoming three
//!    different phrases across a mod's tooltips.
//! 3. **Batching.** 20-50 strings per request, one role per batch, so the
//!    prompt can speak to a single register.
//!
//! Note what this deliberately does NOT do: group by mod. Global dedup and
//! per-mod grouping are incompatible, and dedup is worth more.

use crate::l10n::prefill::role::{role_of, UiRole};
use std::collections::BTreeMap;

/// One missing translation, as discovered from the store's key rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefillUnit {
    pub namespace: String,
    pub key: String,
    pub source_en: String,
}

/// Where a single answer must be written back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub namespace: String,
    pub key: String,
}

/// One distinct string to translate, plus every key waiting on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchUnit {
    /// The JSON key in the request and the response. Dense within a batch
    /// (`s0`, `s1`, …). A real translation key cannot be used: after dedup one
    /// unit serves N keys across N namespaces.
    pub id: String,
    pub source_en: String,
    pub targets: Vec<Target>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Batch {
    pub role: UiRole,
    pub units: Vec<BatchUnit>,
}

/// Group missing units into request batches. `max_batch` caps units per
/// request; callers pass 20-50.
#[must_use]
pub fn build_batches(units: &[PrefillUnit], max_batch: usize) -> Vec<Batch> {
    // BTreeMap so batching is deterministic: a stable request order makes a
    // cache hit reproducible run to run.
    let mut grouped: BTreeMap<(UiRole, String), Vec<Target>> = BTreeMap::new();
    for u in units {
        if u.source_en.trim().is_empty() {
            continue;
        }
        grouped
            .entry((role_of(&u.key), u.source_en.clone()))
            .or_default()
            .push(Target {
                namespace: u.namespace.clone(),
                key: u.key.clone(),
            });
    }

    let mut by_role: BTreeMap<UiRole, Vec<(String, Vec<Target>)>> = BTreeMap::new();
    for ((role, source), targets) in grouped {
        by_role.entry(role).or_default().push((source, targets));
    }

    let cap = max_batch.max(1);
    let mut out = Vec::new();
    // BTreeMap over UiRole yields Name, then Prose, then Other — the two-pass
    // ordering falls out of the enum's declaration order.
    for (role, items) in by_role {
        for chunk in items.chunks(cap) {
            out.push(Batch {
                role,
                units: chunk
                    .iter()
                    .enumerate()
                    .map(|(i, (source, targets))| BatchUnit {
                        id: format!("s{i}"),
                        source_en: source.clone(),
                        targets: targets.clone(),
                    })
                    .collect(),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(ns: &str, key: &str, en: &str) -> PrefillUnit {
        PrefillUnit {
            namespace: ns.to_string(),
            key: key.to_string(),
            source_en: en.to_string(),
        }
    }

    #[test]
    fn identical_strings_in_the_same_role_collapse_to_one_request() {
        let units = vec![
            unit("create", "item.create.a", "Energy"),
            unit("ae2", "item.ae2.b", "Energy"),
            unit("create", "item.create.c", "Durability"),
        ];
        let batches = build_batches(&units, 50);
        let total: usize = batches.iter().map(|b| b.units.len()).sum();
        assert_eq!(total, 2, "Energy must be asked once, not twice");
    }

    #[test]
    fn the_same_string_in_two_roles_stays_two_requests() {
        let units = vec![
            unit("create", "item.create.energy", "Energy"),
            unit("create", "create.tooltip.energy", "Energy"),
        ];
        let batches = build_batches(&units, 50);
        let total: usize = batches.iter().map(|b| b.units.len()).sum();
        assert_eq!(
            total, 2,
            "a label and a sentence are different translations"
        );
    }

    #[test]
    fn names_are_batched_before_prose() {
        let units = vec![
            unit("create", "create.tooltip.x", "Hold shift"),
            unit("create", "item.create.y", "Cogwheel"),
        ];
        let batches = build_batches(&units, 50);
        assert_eq!(batches[0].role, UiRole::Name);
        assert_eq!(batches[1].role, UiRole::Prose);
    }

    #[test]
    fn batches_respect_the_size_cap_and_never_mix_roles() {
        let units: Vec<_> = (0..25)
            .map(|i| {
                unit(
                    "create",
                    &format!("item.create.k{i}"),
                    &format!("Thing {i}"),
                )
            })
            .collect();
        let batches = build_batches(&units, 10);
        assert_eq!(batches.len(), 3);
        assert!(batches.iter().all(|b| b.units.len() <= 10));
        assert!(batches.iter().all(|b| b.role == UiRole::Name));
    }

    #[test]
    fn ids_are_dense_within_each_batch() {
        // Ids address units inside ONE request. Numbering them globally would
        // hand a batch s40, s41, s42 — harmless but confusing, and it makes a
        // response impossible to eyeball against its request.
        let units: Vec<_> = (0..25)
            .map(|i| {
                unit(
                    "create",
                    &format!("item.create.k{i}"),
                    &format!("Thing {i}"),
                )
            })
            .collect();
        let batches = build_batches(&units, 10);
        for b in &batches {
            for (i, u) in b.units.iter().enumerate() {
                assert_eq!(u.id, format!("s{i}"));
            }
        }
    }

    #[test]
    fn a_batch_carries_every_namespace_that_wants_the_string_back() {
        let units = vec![
            unit("create", "item.create.a", "Energy"),
            unit("ae2", "item.ae2.b", "Energy"),
        ];
        let batches = build_batches(&units, 50);
        let targets = &batches[0].units[0].targets;
        assert_eq!(
            targets.len(),
            2,
            "one answer must fan back out to both keys"
        );
    }

    #[test]
    fn units_with_an_empty_source_are_dropped() {
        // An empty English value is a real, present key in Minecraft lang
        // files, and it lands in KeyState::Missing. It can never produce a
        // valid translation, so it must not burn a batch slot and a retry.
        let units = vec![unit("create", "gui.create.empty", "")];
        assert!(build_batches(&units, 50).is_empty());
    }
}
