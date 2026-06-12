//! Best-effort resolution of a `CitedMod` (a bare mod-id from a server
//! reject log) to a downloadable Modrinth/CurseForge version. mod-id is
//! NOT a platform slug/id, so resolution is fuzzy and tiered:
//!   Exact      — an unambiguous slug match with a compatible version.
//!   Fuzzy      — plausible hits, user picks.
//!   Unresolved — nothing usable; user searches manually.
//! Pure tier-selection lives here; the network orchestration that feeds
//! it (search + versions) lives in `commands::logs::build_repair_plan`.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::logs::diagnose::server_mods::CitedMod;
use crate::mods::platform::{ModSummary, ModVersion, VersionRef};

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ResolvedCandidate {
    pub target: VersionRef,
    pub display: ModSummary,
    /// Human label, e.g. the version_number "0.5.3".
    pub version_label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "tier", rename_all = "snake_case")]
pub enum ResolveTier {
    Exact { candidate: Box<ResolvedCandidate> },
    Fuzzy { candidates: Vec<ResolvedCandidate> },
    Unresolved,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ResolvedMod {
    pub cited: CitedMod,
    pub tier: ResolveTier,
}

/// True when a search hit is an unambiguous match for the cited id: its
/// slug equals the id case-insensitively. (`project_id` is never compared —
/// mod-ids are not platform ids.)
pub fn is_exact_slug_match(cited_id: &str, hit: &ModSummary) -> bool {
    hit.slug
        .as_deref()
        .map(|s| s.eq_ignore_ascii_case(cited_id))
        .unwrap_or(false)
}

/// Pick the version to install from a project's compatible versions. When
/// `want_version` is set (mismatch case), prefer the exact `version_number`;
/// otherwise (and as fallback) take the first (newest) compatible version.
/// `versions` MUST already be filtered to the instance's mc+loader by the
/// caller. Returns `None` only when `versions` is empty.
pub fn pick_version<'a>(
    versions: &'a [ModVersion],
    want_version: Option<&str>,
) -> Option<&'a ModVersion> {
    if let Some(w) = want_version {
        if let Some(v) = versions.iter().find(|v| v.version_number == w) {
            return Some(v);
        }
    }
    versions.first()
}

/// Build a candidate from a hit + chosen version.
pub fn make_candidate(hit: &ModSummary, version: &ModVersion) -> ResolvedCandidate {
    ResolvedCandidate {
        target: VersionRef {
            source: version.source,
            project_id: version.project_id.clone(),
            version_id: version.version_id.clone(),
        },
        display: hit.clone(),
        version_label: version.version_number.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::platform::{LoaderKind, ModFile, ModSource};

    fn hit(slug: Option<&str>, pid: &str) -> ModSummary {
        ModSummary {
            source: ModSource::Modrinth,
            project_id: pid.into(),
            slug: slug.map(Into::into),
            name: "X".into(),
            summary: "".into(),
            icon_url: None,
            downloads: 1.0,
            author: "a".into(),
            updated_at: None,
        }
    }

    fn ver(num: &str) -> ModVersion {
        ModVersion {
            source: ModSource::Modrinth,
            project_id: "pid".into(),
            version_id: format!("v-{num}"),
            name: num.into(),
            version_number: num.into(),
            mc_versions: vec!["1.20.1".into()],
            loaders: vec![LoaderKind::Forge],
            primary_file: ModFile {
                filename: format!("x-{num}.jar"),
                url: "https://example/x.jar".into(),
                sha1: Some("aa".into()),
                size: 1.0,
                distribution_allowed: true,
            },
            deps: vec![],
            published_at: None,
        }
    }

    #[test]
    fn exact_slug_match_is_case_insensitive() {
        assert!(is_exact_slug_match("JEI", &hit(Some("jei"), "p")));
        assert!(!is_exact_slug_match(
            "jei",
            &hit(Some("just-enough-items"), "p")
        ));
        assert!(!is_exact_slug_match("jei", &hit(None, "p")));
    }

    #[test]
    fn pick_version_prefers_exact_then_newest() {
        let vs = vec![ver("0.5.3"), ver("0.5.2"), ver("0.4.0")];
        assert_eq!(
            pick_version(&vs, Some("0.4.0")).unwrap().version_number,
            "0.4.0"
        );
        assert_eq!(
            pick_version(&vs, Some("9.9.9")).unwrap().version_number,
            "0.5.3"
        );
        assert_eq!(pick_version(&vs, None).unwrap().version_number, "0.5.3");
        assert!(pick_version(&[], None).is_none());
    }
}
