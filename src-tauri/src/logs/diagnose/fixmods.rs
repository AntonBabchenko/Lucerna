//! Curated, version-PINNED bug → fix-mod table. Pure data + lookups: no
//! network, no telemetry (detection stays pure; the network resolve happens
//! later in the command layer on user intent).
//!
//! Each entry maps a diagnoser `pattern_id` to a specific third-party fix mod,
//! pinned by `project_id` + an exact `version_id` PER (mc, loader). Pins are
//! changed only via PR/code-review — never fuzzy-resolved by name. Sources may
//! point ONLY at Modrinth/CurseForge (already on the network allowlist).

use crate::instances::schema::LoaderKind;
use crate::mods::platform::ModSource;

/// One curated fix mod for a diagnosed bug.
pub struct FixModRef {
    /// Human title shown in the UI, e.g. "Create: The Factory Must Grow Lag Fix".
    pub human_title: &'static str,
    pub source: ModSource,
    /// Pinned platform project id (numeric for CF, slug/id for Modrinth).
    pub project_id: &'static str,
    /// Project slug — builds the degraded "open project page" link.
    pub slug: &'static str,
    /// Exact version pins per instance configuration.
    pub rows: &'static [FixModPin],
}

/// A pinned version for one (mc, loader) configuration.
pub struct FixModPin {
    pub mc: &'static str,
    pub loader: LoaderKind,
    pub version_id: &'static str,
}

// --- Concrete pins -----------------------------------------------------------

/// CurseForge project id for tfmg-lag-fix (curseforge.com/minecraft/mc-mods/tfmg-lag-fix).
const TFMG_LAG_FIX_PROJECT_ID: &str = "1456944";
/// CurseForge file id of tfmg-lag-fix 1.0.2 for MC 1.20.1 / Forge.
const TFMG_LAG_FIX_VERSION_ID_1201_FORGE: &str = "8007822";

const TFMG_LAG_FIX_ROWS: &[FixModPin] = &[FixModPin {
    mc: "1.20.1",
    loader: LoaderKind::Forge,
    version_id: TFMG_LAG_FIX_VERSION_ID_1201_FORGE,
}];

/// The curated table. Keep deliberately small and high-precision — breadth here
/// is a liability, not a feature. Each entry is a deliberate, reviewed PR.
pub const FIX_MODS: &[(&str, FixModRef)] = &[(
    "create-goggle-overlay-crash",
    FixModRef {
        human_title: "Create: The Factory Must Grow Lag Fix",
        source: ModSource::Curseforge,
        project_id: TFMG_LAG_FIX_PROJECT_ID,
        slug: "tfmg-lag-fix",
        rows: TFMG_LAG_FIX_ROWS,
    },
)];

/// The curated fix mod for a diagnosed `pattern_id`, or `None`.
pub fn fix_mod_for(pattern_id: &str) -> Option<&'static FixModRef> {
    FIX_MODS
        .iter()
        .find(|(id, _)| *id == pattern_id)
        .map(|(_, r)| r)
}

/// The pinned version for an instance configuration, or `None` to degrade
/// (no curated pin for this mc + loader → UI offers the project link instead).
pub fn pin_for<'a>(
    r: &'a FixModRef,
    mc: &str,
    loader: LoaderKind,
) -> Option<&'a FixModPin> {
    r.rows.iter().find(|p| p.mc == mc && p.loader == loader)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fix_mod_for_returns_goggle_entry() {
        let r = fix_mod_for("create-goggle-overlay-crash").expect("entry exists");
        assert_eq!(r.source, ModSource::Curseforge);
        assert_eq!(r.slug, "tfmg-lag-fix");
        assert!(!r.rows.is_empty());
    }

    #[test]
    fn fix_mod_for_unknown_pattern_is_none() {
        assert!(fix_mod_for("out-of-memory").is_none());
        assert!(fix_mod_for("nonexistent").is_none());
    }

    #[test]
    fn pin_for_matches_known_config() {
        let r = fix_mod_for("create-goggle-overlay-crash").unwrap();
        let pin = pin_for(r, "1.20.1", LoaderKind::Forge).expect("1.20.1/forge pinned");
        assert_eq!(pin.mc, "1.20.1");
        assert_eq!(pin.loader, LoaderKind::Forge);
    }

    #[test]
    fn pin_for_degrades_on_unpinned_config() {
        let r = fix_mod_for("create-goggle-overlay-crash").unwrap();
        assert!(pin_for(r, "1.21.1", LoaderKind::Forge).is_none());
        assert!(pin_for(r, "1.20.1", LoaderKind::Fabric).is_none());
    }

    #[test]
    fn all_fix_mod_keys_are_unique() {
        use std::collections::HashSet;
        let ids: HashSet<&str> = FIX_MODS.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids.len(), FIX_MODS.len(), "duplicate fix-mod pattern id");
    }
}
