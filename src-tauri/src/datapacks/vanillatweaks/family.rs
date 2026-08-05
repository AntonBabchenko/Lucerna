//! Minecraft version → Vanilla Tweaks family.
//!
//! VT publishes per family (`1.21`, `26.2`, …), not per patch, and its oldest
//! family is `1.13` — the same floor as datapacks themselves. A version that
//! maps to no family is refused rather than rounded down to the nearest older
//! one: falling back would promise a compatibility nobody checked.

/// `1.21.4` → `Some("1.21")`. `None` when the version is unparseable or below
/// the datapack floor.
pub fn family_for(mc_version: &str) -> Option<String> {
    if !crate::datapacks::compat::supports_datapacks(mc_version) {
        return None;
    }
    let mut parts = mc_version.split('.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next()?.parse().ok()?;
    Some(format!("{major}.{minor}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn family_takes_major_and_minor() {
        assert_eq!(family_for("1.21.4").as_deref(), Some("1.21"));
        assert_eq!(family_for("1.21").as_deref(), Some("1.21"));
        assert_eq!(family_for("26.2.1").as_deref(), Some("26.2"));
    }

    #[test]
    fn family_is_none_below_the_datapack_floor() {
        // Datapacks start at 1.13, and VT's oldest family is 1.13 too.
        assert_eq!(family_for("1.12.2"), None);
    }

    #[test]
    fn family_is_none_for_a_version_it_cannot_parse() {
        assert_eq!(family_for("23w13a_or_b"), None);
        assert_eq!(family_for(""), None);
    }
}
