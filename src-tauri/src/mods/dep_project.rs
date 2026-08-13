//! Resolve a bare loader `dep_id` to the platform project that provides it, by
//! reading the REQUIRING mod's declared platform dependencies.
//!
//! Modrinth names a version's dependencies by project id outright, so for a mod
//! we have already identified there is nothing to guess. Measured 2026-08-12:
//!
//! ```text
//! open-parties-and-claims  b0.25.8 (forge-1.20.6-0.25.8)
//!   dependencies: [ { project_id: "ohNO6lps", dependency_type: "required" } ]
//! ohNO6lps -> slug forge-config-api-port, title "Forge Config API Port"
//! ```
//!
//! That beats searching by the slammed id, which for this very case returns
//! zero hits under the instance's facets and, unfaceted, only the unrelated
//! `kilt-forgeconfigapiport-fix`.
//!
//! **Scope discipline.** The jar descriptor remains the sole authority on
//! WHETHER a dependency is required — that verdict is `preflight`'s. This
//! module answers only "which project is that id", so the known
//! non-loader-scoped platform-dependency problem (a Forgix-merged jar
//! publishing one `[fabric, neoforge, quilt]` version whose dependency list is
//! a flat union) cannot leak in: a union is harmless as a lookup table.

use std::future::Future;

use crate::mods::platform::{DepProjectRef, InstalledMod, ModDepLink, ModSource, ModSummary};

/// The project a loader `dep_id` resolves to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepProject {
    pub source: ModSource,
    pub project_id: String,
    pub name: String,
}

/// The project id a dependency link points at, paired with its source.
fn link_target(link: &ModDepLink) -> (ModSource, String) {
    match &link.project_ref {
        DepProjectRef::Modrinth { project_id, .. } => (ModSource::Modrinth, project_id.clone()),
        DepProjectRef::Curseforge { mod_id, .. } => (ModSource::Curseforge, mod_id.to_string()),
    }
}

/// Pure core: choose the declared dependency whose slug or title identifies
/// `dep_id`. `summaries` must cover the projects named by `links`.
///
/// Uses the STRICT matcher (`dep_resolve::is_exact_project_match`), not the
/// containment-based `name_matches`. The candidate set here is small, but the
/// result is used to print a name in front of the user with no download to
/// verify it against — and containment happily accepts a lookalike whose slug
/// merely embeds the id. Strictness costs nothing when the platform has told us
/// the exact project.
pub(crate) fn pick_declared(
    dep_id: &str,
    links: &[ModDepLink],
    summaries: &[ModSummary],
) -> Option<DepProject> {
    for link in links {
        let (source, pid) = link_target(link);
        let Some(s) = summaries
            .iter()
            .find(|s| s.source == source && s.project_id == pid)
        else {
            continue;
        };
        if crate::mods::dep_resolve::is_exact_project_match(dep_id, s.slug.as_deref(), &s.name) {
            return Some(DepProject {
                source,
                project_id: pid,
                name: s.name.clone(),
            });
        }
    }
    None
}

/// Resolve `dep_id` through the requiring mod's declared platform dependencies.
///
/// Returns `None` whenever the requiring mod has no platform identity, its
/// version cannot be fetched, it declares nothing, or nothing it declares
/// matches. In every one of those cases the caller falls through to the
/// slug/search path — this is an accelerator over that path, not a replacement.
///
/// Closure-injected rather than taking clients, so the whole thing is testable
/// without a network or an `AppHandle`.
pub async fn resolve_via_requiring_mod<VF, VFut, SF, SFut>(
    requiring: &InstalledMod,
    dep_id: &str,
    fetch_version: VF,
    fetch_summaries: SF,
) -> Option<DepProject>
where
    VF: FnOnce(ModSource, String, String) -> VFut,
    VFut: Future<Output = Option<crate::mods::platform::ModVersion>>,
    SF: FnOnce(ModSource, Vec<String>) -> SFut,
    SFut: Future<Output = Vec<ModSummary>>,
{
    let source = requiring.source?;
    let project_id = requiring.project_id.clone()?;
    let version_id = requiring.version_id.clone()?;
    let version = fetch_version(source, project_id, version_id).await?;
    if version.deps.is_empty() {
        return None;
    }
    let mut ids: Vec<String> = Vec::new();
    for link in &version.deps {
        let (_, pid) = link_target(link);
        if !ids.contains(&pid) {
            ids.push(pid);
        }
    }
    let summaries = fetch_summaries(source, ids).await;
    pick_declared(dep_id, &version.deps, &summaries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::platform::{DepKind, LoaderKind, ModFile, ModVersion};

    fn summary(id: &str, slug: &str, name: &str) -> ModSummary {
        ModSummary {
            source: ModSource::Modrinth,
            project_id: id.into(),
            slug: Some(slug.into()),
            name: name.into(),
            summary: String::new(),
            icon_url: None,
            downloads: 0.0,
            author: String::new(),
            updated_at: None,
            loaders: None,
        }
    }

    fn link(pid: &str) -> ModDepLink {
        ModDepLink {
            kind: DepKind::Required,
            project_ref: DepProjectRef::Modrinth {
                project_id: pid.into(),
                version_id: None,
            },
        }
    }

    fn requiring_mod(project_id: Option<&str>, version_id: Option<&str>) -> InstalledMod {
        InstalledMod {
            filename: "opac.jar".into(),
            sha1: "sha-a".into(),
            source: project_id.map(|_| ModSource::Modrinth),
            project_id: project_id.map(str::to_string),
            version_id: version_id.map(str::to_string),
            name: "Open Parties and Claims".into(),
            version_number: Some("forge-1.20.6-0.25.8".into()),
            installed_at: "2026-08-12T00:00:00Z".into(),
            enabled: true,
            enrich_attempted: false,
            requires: Vec::new(),
        }
    }

    fn version_with(deps: Vec<ModDepLink>) -> ModVersion {
        ModVersion {
            source: ModSource::Modrinth,
            project_id: "bo89PdrX".into(),
            version_id: "vid".into(),
            name: "b0.25.8".into(),
            version_number: "forge-1.20.6-0.25.8".into(),
            mc_versions: vec!["1.20.6".into()],
            loaders: vec![LoaderKind::Forge],
            primary_file: ModFile {
                filename: "opac.jar".into(),
                url: String::new(),
                sha1: Some("sha".into()),
                size: 0.0,
                distribution_allowed: true,
                sha256: None,
            },
            deps,
            published_at: None,
        }
    }

    /// The reported case, end to end through the pure core.
    #[test]
    fn picks_the_declared_dependency_whose_slug_identifies_the_loader_id() {
        let got = pick_declared(
            "forgeconfigapiport",
            &[link("ohNO6lps")],
            &[summary(
                "ohNO6lps",
                "forge-config-api-port",
                "Forge Config API Port",
            )],
        );
        assert_eq!(
            got,
            Some(DepProject {
                source: ModSource::Modrinth,
                project_id: "ohNO6lps".into(),
                name: "Forge Config API Port".into(),
            })
        );
    }

    #[test]
    fn disambiguates_among_several_declared_dependencies() {
        let got = pick_declared(
            "glitchcore",
            &[link("aaa"), link("bbb"), link("ccc")],
            &[
                summary("aaa", "balm", "Balm"),
                summary("bbb", "glitchcore", "GlitchCore"),
                summary("ccc", "curios", "Curios API"),
            ],
        );
        assert_eq!(got.map(|d| d.project_id), Some("bbb".to_string()));
    }

    /// A declared dependency whose slug merely EMBEDS the id must not be
    /// accepted. There is no download to re-verify a label against.
    #[test]
    fn a_lookalike_slug_does_not_win() {
        let got = pick_declared(
            "forgeconfigapiport",
            &[link("kilt")],
            &[summary(
                "kilt",
                "kilt-forgeconfigapiport-fix",
                "Kilt FCAP fix",
            )],
        );
        assert_eq!(got, None);
    }

    #[test]
    fn returns_none_when_no_declared_dependency_matches() {
        let got = pick_declared(
            "forgeconfigapiport",
            &[link("aaa")],
            &[summary("aaa", "balm", "Balm")],
        );
        assert_eq!(got, None, "the caller must fall through to the search path");
    }

    #[test]
    fn returns_none_for_a_version_that_declares_nothing() {
        assert_eq!(pick_declared("anything", &[], &[]), None);
    }

    /// A summary the caller could not fetch simply skips its link rather than
    /// aborting the scan of the remaining ones.
    #[test]
    fn a_missing_summary_skips_only_its_own_link() {
        let got = pick_declared(
            "glitchcore",
            &[link("unfetched"), link("bbb")],
            &[summary("bbb", "glitchcore", "GlitchCore")],
        );
        assert_eq!(got.map(|d| d.project_id), Some("bbb".to_string()));
    }

    #[tokio::test]
    async fn resolves_through_the_requiring_mod() {
        let got = resolve_via_requiring_mod(
            &requiring_mod(Some("bo89PdrX"), Some("vid")),
            "forgeconfigapiport",
            |_, pid, vid| async move {
                assert_eq!((pid.as_str(), vid.as_str()), ("bo89PdrX", "vid"));
                Some(version_with(vec![link("ohNO6lps")]))
            },
            |_, ids| async move {
                assert_eq!(ids, vec!["ohNO6lps".to_string()]);
                vec![summary(
                    "ohNO6lps",
                    "forge-config-api-port",
                    "Forge Config API Port",
                )]
            },
        )
        .await;
        assert_eq!(got.map(|d| d.name), Some("Forge Config API Port".into()));
    }

    /// A manually added jar has no platform identity, so there is no dependency
    /// list to read. The version fetch must not even be attempted.
    #[tokio::test]
    async fn a_mod_with_no_platform_identity_short_circuits() {
        let got = resolve_via_requiring_mod(
            &requiring_mod(None, None),
            "forgeconfigapiport",
            |_, _, _| async { panic!("must not fetch a version for an unidentified mod") },
            |_, _| async { Vec::new() },
        )
        .await;
        assert_eq!(got, None);
    }

    /// Registry rows predating version pinning have a project but no version id.
    #[tokio::test]
    async fn a_mod_with_no_version_id_short_circuits() {
        let got = resolve_via_requiring_mod(
            &requiring_mod(Some("bo89PdrX"), None),
            "forgeconfigapiport",
            |_, _, _| async { panic!("must not fetch without a version id") },
            |_, _| async { Vec::new() },
        )
        .await;
        assert_eq!(got, None);
    }

    #[tokio::test]
    async fn an_unfetchable_version_falls_through() {
        let got = resolve_via_requiring_mod(
            &requiring_mod(Some("bo89PdrX"), Some("vid")),
            "forgeconfigapiport",
            |_, _, _| async { None },
            |_, _| async { panic!("no summaries to fetch when the version is unavailable") },
        )
        .await;
        assert_eq!(got, None);
    }

    /// Duplicate links must not produce a duplicated summary request.
    #[tokio::test]
    async fn duplicate_links_are_asked_for_once() {
        let got = resolve_via_requiring_mod(
            &requiring_mod(Some("bo89PdrX"), Some("vid")),
            "glitchcore",
            |_, _, _| async { Some(version_with(vec![link("bbb"), link("bbb")])) },
            |_, ids| async move {
                assert_eq!(ids, vec!["bbb".to_string()], "deduplicated");
                vec![summary("bbb", "glitchcore", "GlitchCore")]
            },
        )
        .await;
        assert_eq!(got.map(|d| d.project_id), Some("bbb".to_string()));
    }
}
