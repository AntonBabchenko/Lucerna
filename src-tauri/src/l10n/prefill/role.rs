//! What a key's string is *for*. Two consumers: the dedup identity (the same
//! English word can translate differently as a label than as prose) and the
//! two-pass ordering (names are decided first and then frozen as the mod's
//! own glossary, which is what stops one term becoming three phrases).

use serde::{Deserialize, Serialize};

/// Ordered so `Name` sorts first — the two-pass order falls out of a BTreeMap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum UiRole {
    /// A short label: item, block, entity, creative tab, enchantment…
    Name,
    /// A sentence or paragraph: tooltips, guidebook pages, messages.
    Prose,
    /// Everything else — UI chrome, config labels, unclassifiable keys.
    Other,
}

const NAME_PREFIXES: &[&str] = &[
    "block.",
    "item.",
    "itemgroup.",
    "entity.",
    "fluid.",
    "enchantment.",
    "effect.",
    "biome.",
    "potion.",
    "attribute.",
];

const PROSE_MARKERS: &[&str] = &[
    ".tooltip",
    "tooltip.",
    ".desc",
    "desc.",
    ".info",
    "info.",
    "book.",
    "patchouli.",
    "guide.",
    "message.",
    "advancements.",
    "quest.",
];

/// Classify a translation key. Case-insensitive: `itemGroup.` and
/// `itemgroup.` are the same thing in the wild.
///
/// Prose markers are checked before name prefixes on purpose — see the
/// `a_tooltip_suffix_beats_a_name_prefix` test for why that order is the safe
/// one.
#[must_use]
pub fn role_of(key: &str) -> UiRole {
    let k = key.to_ascii_lowercase();
    if PROSE_MARKERS.iter().any(|m| k.contains(m)) {
        return UiRole::Prose;
    }
    if NAME_PREFIXES.iter().any(|p| k.starts_with(p)) {
        return UiRole::Name;
    }
    UiRole::Other
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_tooltips_and_prose_are_classified_apart() {
        assert_eq!(role_of("block.create.andesite_alloy"), UiRole::Name);
        assert_eq!(role_of("item.ae2.certus_quartz"), UiRole::Name);
        assert_eq!(role_of("itemGroup.create.base"), UiRole::Name);
        assert_eq!(role_of("entity.minecraft.pig"), UiRole::Name);
        assert_eq!(role_of("item.create.wrench.tooltip"), UiRole::Prose);
        assert_eq!(role_of("create.tooltip.holdKey"), UiRole::Prose);
        assert_eq!(role_of("book.ars.chapter1"), UiRole::Prose);
        assert_eq!(role_of("gui.create.confirm"), UiRole::Other);
    }

    #[test]
    fn a_tooltip_suffix_beats_a_name_prefix() {
        // The prefix says "item name", the suffix says "prose". Prose wins:
        // sending a paragraph through the terse name prompt is the worse error.
        assert_eq!(
            role_of("item.create.goggles.tooltip.summary"),
            UiRole::Prose
        );
    }

    #[test]
    fn name_sorts_before_prose_so_the_two_pass_order_is_free() {
        assert!(UiRole::Name < UiRole::Prose);
        assert!(UiRole::Prose < UiRole::Other);
    }
}
