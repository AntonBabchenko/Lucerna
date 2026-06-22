//! Pure dependency version-selection decision (pin → range → newest) and the
//! normalized name-match used to accept a relevance-search hit for a bare
//! loader id. No network; the single source of truth for "which build, why".

use crate::mods::platform::{ModVersion, SelectionReason};
use crate::mods::version_range::RangeFamily;

/// Pick a dependency build from `candidates` (newest-first) and report why.
///
/// Precedence (the reason is the strongest signal actually honored):
/// 1. `pin_version_id` present AND among the candidates → that build, `PinHonored`.
/// 2. `pin_version_id` present but absent → fall through, reason forced to
///    `FellBackFromPin` (the salient fact is the pin was unavailable here).
/// 3. `range` present and satisfied by some build → newest satisfying build;
///    `RangeConstrained` iff that pushed the pick off newest (index > 0), else
///    `NewestNoPin`.
/// 4. Otherwise → newest (index 0), `NewestNoPin`.
///
/// The returned `usize` is an index into the passed `candidates` slice — callers
/// must index that same slice to retrieve the chosen build.
///
/// Returns `None` only when `candidates` is empty.
pub fn select_dep_version(
    candidates: &[ModVersion],
    pin_version_id: Option<&str>,
    range: Option<(&str, RangeFamily)>,
) -> Option<(usize, SelectionReason)> {
    use crate::mods::version_range::satisfying_indices;
    if candidates.is_empty() {
        return None;
    }
    // 1. Honor an author pin when the exact build is among the compatible candidates.
    let mut pin_overridden = false;
    if let Some(pin) = pin_version_id {
        if let Some(i) = candidates.iter().position(|c| c.version_id == pin) {
            return Some((i, SelectionReason::PinHonored));
        }
        pin_overridden = true; // pin present but not compatible here
    }
    // 2/3. Range-aware newest-satisfying.
    if let Some((range, family)) = range {
        let nums: Vec<&str> = candidates
            .iter()
            .map(|c| c.version_number.as_str())
            .collect();
        if let Some(&i) = satisfying_indices(&nums, range, family).first() {
            // Reason precedence is top-down: an overridden pin stays the headline.
            let reason = if pin_overridden {
                SelectionReason::FellBackFromPin
            } else if i > 0 {
                SelectionReason::RangeConstrained
            } else {
                SelectionReason::NewestNoPin
            };
            return Some((i, reason));
        }
    }
    // 4. Newest (range absent or unsatisfiable).
    let reason = if pin_overridden {
        SelectionReason::FellBackFromPin
    } else {
        SelectionReason::NewestNoPin
    };
    Some((0, reason))
}

/// Normalized (`[^a-z0-9]` → `_`, lowercased, trimmed) containment test used to
/// accept a relevance-search hit as a match for a bare loader id. The dep id
/// matches when its normalized form is a substring of the normalized slug or
/// display name. An all-separator `dep_id` (empty needle) never matches.
///
/// This is a deliberately loose substring match: its false positives are
/// contained by the downstream `jar_provides` verification gate at install time.
pub fn name_matches(dep_id: &str, slug: Option<&str>, name: &str) -> bool {
    // Normalize: non-alphanumerics collapse to a single `_`, lowercased, trimmed.
    fn norm(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut prev_sep = false;
        for ch in s.chars() {
            if ch.is_ascii_alphanumeric() {
                out.push(ch.to_ascii_lowercase());
                prev_sep = false;
            } else if !prev_sep {
                out.push('_');
                prev_sep = true;
            }
        }
        out.trim_matches('_').to_string()
    }
    let needle = norm(dep_id);
    if needle.is_empty() {
        return false;
    }
    if let Some(s) = slug {
        if norm(s).contains(&needle) {
            return true;
        }
    }
    norm(name).contains(&needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::platform::{LoaderKind, ModFile, ModSource};

    fn mv(version_id: &str, version_number: &str) -> ModVersion {
        ModVersion {
            source: ModSource::Modrinth,
            project_id: "p".into(),
            version_id: version_id.into(),
            name: "P".into(),
            version_number: version_number.into(),
            mc_versions: vec!["1.20.1".into()],
            loaders: vec![LoaderKind::Forge],
            primary_file: ModFile {
                filename: format!("{version_id}.jar"),
                url: "https://cdn/x.jar".into(),
                sha1: Some("aa".into()),
                size: 1.0,
                distribution_allowed: true,
            },
            deps: vec![],
            published_at: None,
        }
    }

    // candidates newest-first
    fn cands() -> Vec<ModVersion> {
        vec![mv("v3", "3.1.11"), mv("v2", "2.0.41"), mv("v1", "2.0.39")]
    }

    #[test]
    fn empty_candidates_yield_none() {
        assert_eq!(select_dep_version(&[], None, None), None);
    }

    #[test]
    fn no_pin_no_range_picks_newest() {
        let r = select_dep_version(&cands(), None, None).unwrap();
        assert_eq!(r, (0, SelectionReason::NewestNoPin));
    }

    #[test]
    fn pin_present_and_compatible_is_honored() {
        let r = select_dep_version(&cands(), Some("v2"), None).unwrap();
        assert_eq!(r, (1, SelectionReason::PinHonored));
    }

    #[test]
    fn pin_absent_falls_back_to_newest_marked() {
        let r = select_dep_version(&cands(), Some("does-not-exist"), None).unwrap();
        assert_eq!(r, (0, SelectionReason::FellBackFromPin));
    }

    #[test]
    fn pin_absent_with_range_keeps_fell_back_reason() {
        // range would pick v2 (idx 1); reason stays FellBackFromPin (pin is the
        // salient overridden signal).
        let r = select_dep_version(
            &cands(),
            Some("nope"),
            Some(("[2.0.39,2.1)", RangeFamily::Maven)),
        )
        .unwrap();
        assert_eq!(r, (1, SelectionReason::FellBackFromPin));
    }

    #[test]
    fn bounded_range_pushes_off_newest_is_range_constrained() {
        // [2.0.39,2.1) excludes 3.1.11; newest satisfying is 2.0.41 (idx 1).
        let r =
            select_dep_version(&cands(), None, Some(("[2.0.39,2.1)", RangeFamily::Maven))).unwrap();
        assert_eq!(r, (1, SelectionReason::RangeConstrained));
    }

    #[test]
    fn open_range_that_does_not_bite_is_newest_no_pin() {
        // [2.0.39,) accepts 3.1.11 (idx 0) — range did not constrain.
        let r =
            select_dep_version(&cands(), None, Some(("[2.0.39,)", RangeFamily::Maven))).unwrap();
        assert_eq!(r, (0, SelectionReason::NewestNoPin));
    }

    #[test]
    fn unsatisfiable_range_falls_back_to_newest() {
        // No candidate satisfies [9.0,10.0) → newest (idx 0), NewestNoPin.
        let r =
            select_dep_version(&cands(), None, Some(("[9.0,10.0)", RangeFamily::Maven))).unwrap();
        assert_eq!(r, (0, SelectionReason::NewestNoPin));
    }

    #[test]
    fn name_matches_id_against_slug_and_display_name() {
        assert!(name_matches(
            "spore",
            Some("fungal-infection-spore"),
            "Fungal Infection: Spore"
        ));
        assert!(name_matches("spore", None, "Fungal Infection:Spore"));
        assert!(!name_matches(
            "jei",
            Some("fungal-infection-spore"),
            "Fungal Infection:Spore"
        ));
        assert!(!name_matches("   ", Some("anything"), "Anything"));
    }
}
