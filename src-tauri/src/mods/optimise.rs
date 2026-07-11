//! Curated, loader- and version-aware "one-click Optimise" performance-mod set.
//!
//! A hardcoded CATALOG of well-known Modrinth performance mods is resolved
//! against an instance's (loader, mc_version) at request time — the general-
//! ization of the shader-loader-hint precedent. Nothing is installed here;
//! `resolve` classifies each candidate so the UI can preview the set, and the
//! frontend installs the `WillInstall` entries through the existing
//! `mods_install_with_deps` pipeline.

use serde::Serialize;
use specta::Type;

use crate::error::Error;
use crate::instances::schema::LoaderKind;
use crate::mods::platform::{InstalledMod, ModSource, ModVersion, VersionRef};

/// Mutually-exclusive candidate group. Only the first catalog candidate in a
/// group that reaches a shown terminal state (install / already-installed /
/// optifine-conflict) claims the group; later members are omitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptimiseGroup {
    Renderer,
}

/// A per-entry advisory surfaced in the dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum OptimiseNote {
    /// Tick-optimizer: strongest in singleplayer / LAN (the client hosts the
    /// integrated server); on a remote dedicated server it does not speed the
    /// server up but is harmless.
    SinglePlayerTick,
}

/// A hardcoded catalog entry. `loaders` is where the mod publishes at all;
/// actual availability for a given MC is decided live in `resolve`.
#[derive(Debug, Clone, Copy)]
pub struct OptimiseCandidate {
    pub key: &'static str,
    pub title: &'static str,
    pub modrinth_id: &'static str,
    pub loaders: &'static [LoaderKind],
    pub group: Option<OptimiseGroup>,
    pub note: Option<OptimiseNote>,
}

use LoaderKind::{Fabric, Forge, NeoForge, Quilt};

/// Verified against the live Modrinth API 2026-07-11 (ids + loader coverage).
/// Renderer group priority is catalog order: Sodium preferred, Embeddium is the
/// Forge/legacy fallback. Canary (dead) is intentionally excluded.
pub static CATALOG: &[OptimiseCandidate] = &[
    OptimiseCandidate {
        key: "sodium",
        title: "Sodium",
        modrinth_id: "AANobbMI",
        loaders: &[Fabric, Quilt, NeoForge],
        group: Some(OptimiseGroup::Renderer),
        note: None,
    },
    OptimiseCandidate {
        key: "embeddium",
        title: "Embeddium",
        modrinth_id: "sk9rgfiA",
        loaders: &[Fabric, Forge, NeoForge],
        group: Some(OptimiseGroup::Renderer),
        note: None,
    },
    OptimiseCandidate {
        key: "lithium",
        title: "Lithium",
        modrinth_id: "gvQqBUqZ",
        loaders: &[Fabric, Quilt, NeoForge],
        group: None,
        note: Some(OptimiseNote::SinglePlayerTick),
    },
    OptimiseCandidate {
        key: "immediatelyfast",
        title: "ImmediatelyFast",
        modrinth_id: "5ZwdcRci",
        loaders: &[Fabric, Forge, NeoForge, Quilt],
        group: None,
        note: None,
    },
    OptimiseCandidate {
        key: "entityculling",
        title: "Entity Culling",
        modrinth_id: "NNAgCjsB",
        loaders: &[Fabric, Forge, NeoForge, Quilt],
        group: None,
        note: None,
    },
    OptimiseCandidate {
        key: "ferritecore",
        title: "FerriteCore",
        modrinth_id: "uXXizFIs",
        loaders: &[Fabric, Forge, NeoForge, Quilt],
        group: None,
        note: None,
    },
    OptimiseCandidate {
        key: "dynamic-fps",
        title: "Dynamic FPS",
        modrinth_id: "LQ3K71Q1",
        loaders: &[Fabric, Forge, NeoForge, Quilt],
        group: None,
        note: None,
    },
];

/// Per-entry classification against the instance.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum OptimiseEntryStatus {
    /// Has a build for this loader+MC and is not installed — will be installed.
    WillInstall,
    /// Already present on the instance (matched by Modrinth project id).
    AlreadyInstalled,
    /// Publishes to this loader but has no build for this MC version.
    UnavailableForVersion,
    /// A renderer suppressed because OptiFine is installed (incompatible).
    ConflictOptifine,
    /// The platform query errored — surfaced non-fatally, not installed.
    Unknown,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct OptimiseEntry {
    pub key: String,
    pub title: String,
    pub status: OptimiseEntryStatus,
    pub note: Option<OptimiseNote>,
    /// Present only for `WillInstall` — the exact build the FE installs.
    pub version: Option<VersionRef>,
    /// Present only for `WillInstall` — the display version string.
    pub version_number: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct OptimisePlan {
    /// True when the instance has no mod loader (vanilla) — nothing to install.
    pub loader_unsupported: bool,
    pub entries: Vec<OptimiseEntry>,
    /// Count of `WillInstall` entries (drives the confirm button label/enabled).
    pub install_count: u32,
}

/// OptiFine ships no Modrinth/CF project (site-only), so an anchored filename
/// heuristic is the only signal. Matches the shader-loader-hint precedent:
/// "OptiFine" is a distinctive prefix (real jars: `OptiFine_1.20.1_HD_U_I6.jar`,
/// incl. the `_MOD` Forge variant); the anchor avoids `optifabric*` false hits.
fn has_optifine(installed: &[InstalledMod]) -> bool {
    installed.iter().any(|m| {
        let base = m.filename.rsplit(['/', '\\']).next().unwrap_or(&m.filename);
        let lower = base.to_ascii_lowercase();
        lower.starts_with("optifine-")
            || lower.starts_with("optifine_")
            || lower.starts_with("optifine.")
    })
}

/// Public wrapper so the command layer can compute OptiFine presence with the
/// same heuristic `resolve` relies on.
pub fn has_optifine_public(installed: &[InstalledMod]) -> bool {
    has_optifine(installed)
}

fn entry(
    c: &OptimiseCandidate,
    status: OptimiseEntryStatus,
    version: Option<VersionRef>,
    version_number: Option<String>,
) -> OptimiseEntry {
    OptimiseEntry {
        key: c.key.into(),
        title: c.title.into(),
        status,
        note: c.note,
        version,
        version_number,
    }
}

/// Classify every catalog candidate against the instance. `fetch(modrinth_id)`
/// must return the platform's newest-first versions already filtered to the
/// instance loader+MC (empty = no build for this version). Pure over `fetch`,
/// so unit-testable without the network.
pub async fn resolve<F, Fut>(
    loader: LoaderKind,
    mc_version: &str,
    installed: &[InstalledMod],
    optifine_present: bool,
    mut fetch: F,
) -> OptimisePlan
where
    F: FnMut(&'static str) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<ModVersion>, Error>>,
{
    let _ = mc_version; // fetch is expected to be pre-filtered to this MC
    if loader == LoaderKind::Vanilla {
        return OptimisePlan {
            loader_unsupported: true,
            entries: vec![],
            install_count: 0,
        };
    }

    let mut entries: Vec<OptimiseEntry> = Vec::new();
    let mut renderer_claimed = false;

    for c in CATALOG {
        if !c.loaders.contains(&loader) {
            continue;
        }
        let is_renderer = c.group == Some(OptimiseGroup::Renderer);
        if is_renderer && renderer_claimed {
            continue; // a prior renderer already owns the group — hide the rest
        }

        let already = installed.iter().any(|m| {
            m.source == Some(ModSource::Modrinth) && m.project_id.as_deref() == Some(c.modrinth_id)
        });

        // A renderer is incompatible with OptiFine — claim the group so no other
        // renderer is offered, and surface exactly one conflict row.
        if is_renderer && optifine_present {
            renderer_claimed = true;
            entries.push(entry(c, OptimiseEntryStatus::ConflictOptifine, None, None));
            continue;
        }

        if already {
            if is_renderer {
                renderer_claimed = true;
            }
            entries.push(entry(c, OptimiseEntryStatus::AlreadyInstalled, None, None));
            continue;
        }

        match fetch(c.modrinth_id).await {
            Ok(vers) if !vers.is_empty() => {
                if is_renderer {
                    renderer_claimed = true;
                }
                let v = &vers[0];
                let vr = VersionRef {
                    source: v.source,
                    project_id: v.project_id.clone(),
                    version_id: v.version_id.clone(),
                };
                entries.push(entry(
                    c,
                    OptimiseEntryStatus::WillInstall,
                    Some(vr),
                    Some(v.version_number.clone()),
                ));
            }
            // Not available / errored: show the row but DON'T claim the group,
            // so a later renderer may still resolve.
            Ok(_) => entries.push(entry(
                c,
                OptimiseEntryStatus::UnavailableForVersion,
                None,
                None,
            )),
            Err(_) => entries.push(entry(c, OptimiseEntryStatus::Unknown, None, None)),
        }
    }

    let install_count = entries
        .iter()
        .filter(|e| e.status == OptimiseEntryStatus::WillInstall)
        .count() as u32;
    OptimisePlan {
        loader_unsupported: false,
        entries,
        install_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::platform::{ModFile, ModSource};
    use std::collections::HashMap;

    fn installed_mod(
        filename: &str,
        source: Option<ModSource>,
        project_id: Option<&str>,
    ) -> InstalledMod {
        InstalledMod {
            filename: filename.into(),
            sha1: "aa".into(),
            source,
            project_id: project_id.map(Into::into),
            version_id: None,
            name: filename.into(),
            version_number: None,
            installed_at: "2026-01-01T00:00:00Z".into(),
            enabled: true,
            enrich_attempted: false,
            requires: vec![],
        }
    }

    fn mv(id: &str) -> ModVersion {
        ModVersion {
            source: ModSource::Modrinth,
            project_id: id.into(),
            version_id: format!("{id}-v1"),
            name: id.into(),
            version_number: "1.0.0".into(),
            mc_versions: vec!["1.21.1".into()],
            loaders: vec![],
            primary_file: ModFile {
                filename: format!("{id}.jar"),
                url: format!("https://cdn.modrinth.com/{id}.jar"),
                sha1: Some("aa".into()),
                size: 1.0,
                distribution_allowed: true,
                sha256: None,
            },
            deps: vec![],
            published_at: None,
        }
    }

    fn err() -> Error {
        Error::ModsNetwork {
            url: "test".into(),
            details: "boom".into(),
        }
    }

    /// Resolve with a fixed set of "available" ids (each returns one build);
    /// everything else is unavailable.
    async fn resolve_with(
        loader: LoaderKind,
        installed: &[InstalledMod],
        optifine: bool,
        available: &[&'static str],
    ) -> OptimisePlan {
        let map: HashMap<&'static str, Vec<ModVersion>> =
            available.iter().map(|id| (*id, vec![mv(id)])).collect();
        let fetch = move |id: &'static str| {
            std::future::ready(Ok(map.get(id).cloned().unwrap_or_default()))
        };
        resolve(loader, "1.21.1", installed, optifine, fetch).await
    }

    fn status_of<'a>(plan: &'a OptimisePlan, key: &str) -> Option<&'a OptimiseEntryStatus> {
        plan.entries
            .iter()
            .find(|e| e.key == key)
            .map(|e| &e.status)
    }

    #[test]
    fn detects_optifine_by_filename_prefix() {
        let mods = vec![installed_mod("OptiFine_1.20.1_HD_U_I6.jar", None, None)];
        assert!(has_optifine(&mods));
    }

    #[test]
    fn ignores_lookalike_names() {
        let mods = vec![
            installed_mod("optifabric-1.14.2.jar", None, None),
            installed_mod(
                "sodium-fabric-0.6.0.jar",
                Some(ModSource::Modrinth),
                Some("AANobbMI"),
            ),
        ];
        assert!(!has_optifine(&mods));
    }

    #[tokio::test]
    async fn vanilla_is_unsupported() {
        let plan = resolve_with(LoaderKind::Vanilla, &[], false, &[]).await;
        assert!(plan.loader_unsupported);
        assert!(plan.entries.is_empty());
        assert_eq!(plan.install_count, 0);
    }

    #[tokio::test]
    async fn neoforge_picks_sodium_and_hides_embeddium() {
        let plan = resolve_with(
            LoaderKind::NeoForge,
            &[],
            false,
            &["AANobbMI", "sk9rgfiA", "gvQqBUqZ"],
        )
        .await;
        assert_eq!(
            status_of(&plan, "sodium"),
            Some(&OptimiseEntryStatus::WillInstall)
        );
        assert!(
            status_of(&plan, "embeddium").is_none(),
            "embeddium hidden by group"
        );
        assert_eq!(
            status_of(&plan, "lithium"),
            Some(&OptimiseEntryStatus::WillInstall)
        );
    }

    #[tokio::test]
    async fn modern_forge_has_no_renderer() {
        let plan = resolve_with(
            LoaderKind::Forge,
            &[],
            false,
            &["5ZwdcRci", "NNAgCjsB", "uXXizFIs", "LQ3K71Q1"],
        )
        .await;
        // Embeddium is not in `available` here -> no build for this MC.
        assert_eq!(
            status_of(&plan, "embeddium"),
            Some(&OptimiseEntryStatus::UnavailableForVersion)
        );
        assert!(
            status_of(&plan, "sodium").is_none(),
            "sodium not applicable to forge"
        );
        assert!(
            status_of(&plan, "lithium").is_none(),
            "lithium not applicable to forge"
        );
        assert_eq!(
            status_of(&plan, "immediatelyfast"),
            Some(&OptimiseEntryStatus::WillInstall)
        );
    }

    #[tokio::test]
    async fn already_installed_sodium_is_skipped_and_claims_group() {
        let mods = vec![installed_mod(
            "sodium-fabric-0.6.0.jar",
            Some(ModSource::Modrinth),
            Some("AANobbMI"),
        )];
        let plan = resolve_with(LoaderKind::Fabric, &mods, false, &["AANobbMI", "sk9rgfiA"]).await;
        assert_eq!(
            status_of(&plan, "sodium"),
            Some(&OptimiseEntryStatus::AlreadyInstalled)
        );
        assert!(status_of(&plan, "embeddium").is_none());
    }

    #[tokio::test]
    async fn optifine_suppresses_the_renderer_only() {
        let mods = vec![installed_mod("OptiFine_1.20.1_HD_U_I6.jar", None, None)];
        let plan = resolve_with(LoaderKind::Fabric, &mods, true, &["AANobbMI", "NNAgCjsB"]).await;
        assert_eq!(
            status_of(&plan, "sodium"),
            Some(&OptimiseEntryStatus::ConflictOptifine)
        );
        assert!(status_of(&plan, "embeddium").is_none());
        assert_eq!(
            status_of(&plan, "entityculling"),
            Some(&OptimiseEntryStatus::WillInstall)
        );
    }

    #[tokio::test]
    async fn platform_error_is_unknown_not_fatal() {
        let map: HashMap<&'static str, Vec<ModVersion>> =
            [("NNAgCjsB", vec![mv("NNAgCjsB")])].into_iter().collect();
        let fetch = move |id: &'static str| {
            if id == "uXXizFIs" {
                std::future::ready(Err(err()))
            } else {
                std::future::ready(Ok(map.get(id).cloned().unwrap_or_default()))
            }
        };
        let plan = resolve(LoaderKind::Fabric, "1.21.1", &[], false, fetch).await;
        assert_eq!(
            status_of(&plan, "ferritecore"),
            Some(&OptimiseEntryStatus::Unknown)
        );
        assert_eq!(
            status_of(&plan, "entityculling"),
            Some(&OptimiseEntryStatus::WillInstall)
        );
    }

    #[tokio::test]
    async fn install_count_counts_only_will_install() {
        let plan = resolve_with(LoaderKind::Fabric, &[], false, &["AANobbMI", "gvQqBUqZ"]).await;
        assert_eq!(plan.install_count, 2);
    }

    #[tokio::test]
    async fn will_install_carries_version_ref() {
        let plan = resolve_with(LoaderKind::Fabric, &[], false, &["AANobbMI"]).await;
        let sodium = plan.entries.iter().find(|e| e.key == "sodium").unwrap();
        let vr = sodium
            .version
            .as_ref()
            .expect("WillInstall carries a VersionRef");
        assert_eq!(vr.project_id, "AANobbMI");
        assert_eq!(vr.source, ModSource::Modrinth);
        assert_eq!(sodium.version_number.as_deref(), Some("1.0.0"));
    }
}
