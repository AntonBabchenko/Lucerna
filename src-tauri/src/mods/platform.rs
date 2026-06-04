//! Normalized cross-platform mod browser API.
//!
//! Two implementations (`modrinth::ModrinthClient`, `curseforge::CurseForgeClient`)
//! satisfy this trait. UI consumes only the types defined here.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::Error;

pub use crate::instances::schema::LoaderKind;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ModSource {
    Modrinth,
    Curseforge,
}

/// What kind of content a search/install targets. `Mod` is the historical
/// default so payloads that omit it keep working (serde `default`).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum ContentKind {
    #[default]
    Mod,
    ResourcePack,
    Shader,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModSort {
    Relevance,
    Downloads,
    Updated,
}

/// Best-effort loader detection from a release filename.
///
/// Defends against upstream loader mis-tagging: some authors publish a
/// NeoForge jar but tag it with the `forge` loader on Modrinth (Xaero's
/// Minimap for 1.20.4 does this), which would otherwise install a NeoForge
/// jar onto a Forge instance and crash at launch. When the platform's loader
/// tag is unreliable, the filename (`...-neoforge-...` vs `...-forge-...`) is
/// the trustworthy signal.
///
/// Returns `None` when the filename names no loader — callers MUST treat that
/// as "trust the platform tag", never as a mismatch.
///
/// The loader name must appear as a delimited token (split on any
/// non-alphanumeric character), NOT as a bare substring: a mod whose *name*
/// embeds a loader word — e.g. `forgero-1.0.jar` (project "Forgero") — must not
/// be mistaken for a Forge build and silently dropped. `neoforge` is matched
/// before `forge` so a literal `neoforge` token never resolves to Forge.
pub fn loader_in_filename(filename: &str) -> Option<LoaderKind> {
    let lower = filename.to_ascii_lowercase();
    let has_token = |name: &str| {
        lower
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|t| t == name)
    };
    if has_token("neoforge") {
        Some(LoaderKind::NeoForge)
    } else if has_token("fabric") {
        Some(LoaderKind::Fabric)
    } else if has_token("quilt") {
        Some(LoaderKind::Quilt)
    } else if has_token("forge") {
        Some(LoaderKind::Forge)
    } else {
        None
    }
}

/// Drop versions whose primary-file filename names a loader different from the
/// requested one. Versions whose filename names no loader are kept (trust the
/// platform tag). No-op when no loader was requested (e.g. the "show all
/// versions" path). Forge and NeoForge are distinct here.
pub fn drop_filename_loader_mismatches(
    versions: Vec<ModVersion>,
    want: Option<LoaderKind>,
) -> Vec<ModVersion> {
    match want {
        Some(want) => versions
            .into_iter()
            .filter(|v| match loader_in_filename(&v.primary_file.filename) {
                Some(found) => found == want,
                None => true,
            })
            .collect(),
        None => versions,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ModSearchQuery {
    pub source: ModSource,
    #[serde(default)]
    pub kind: ContentKind,
    pub query: String,
    pub mc_version: Option<String>,
    pub loader: Option<LoaderKind>,
    pub sort: ModSort,
    pub page_size: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ModSearchPage {
    pub hits: Vec<ModSummary>,
    pub total: u32,
    pub offset: u32,
    pub page_size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ModSummary {
    pub source: ModSource,
    pub project_id: String,
    pub slug: Option<String>,
    pub name: String,
    pub summary: String,
    pub icon_url: Option<String>,
    // f64 not u64: specta forbids exporting BigInt-style types to TS (the
    // existing "f64-not-u64" convention used elsewhere in this crate).
    // 2^53 downloads is far beyond any plausible mod count.
    pub downloads: f64,
    pub author: String,
    pub updated_at: Option<String>,
}

/// One screenshot/gallery image for a mod or modpack detail view.
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
pub struct GalleryImage {
    pub url: String,
    /// Caption / alt text when the platform supplies one.
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ModProject {
    pub summary: ModSummary,
    /// Sanitized HTML of the project's long description (Modrinth `body`
    /// rendered from markdown; CurseForge description endpoint HTML). Empty
    /// when the platform supplies none — the UI falls back to `summary`.
    pub body_html: String,
    /// Screenshots, ordered featured-first then by platform ordering.
    pub gallery: Vec<GalleryImage>,
    pub website_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
pub struct ModVersion {
    pub source: ModSource,
    pub project_id: String,
    pub version_id: String,
    pub name: String,
    pub version_number: String,
    pub mc_versions: Vec<String>,
    pub loaders: Vec<LoaderKind>,
    pub primary_file: ModFile,
    pub deps: Vec<ModDepLink>,
    pub published_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
pub struct ModFile {
    pub filename: String,
    pub url: String,
    pub sha1: Option<String>,
    // f64 not u64: specta forbids exporting BigInt-style types to TS.
    // 2^53 bytes (~9 PiB) is far beyond any realistic mod jar size.
    pub size: f64,
    pub distribution_allowed: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DepKind {
    Required,
    Optional,
    Incompatible,
    Embedded,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum DepProjectRef {
    Modrinth {
        project_id: String,
        version_id: Option<String>,
    },
    Curseforge {
        mod_id: u32,
        file_id: Option<u32>,
    },
}

impl DepProjectRef {
    pub fn display(&self) -> String {
        match self {
            DepProjectRef::Modrinth { project_id, .. } => format!("modrinth:{project_id}"),
            DepProjectRef::Curseforge { mod_id, .. } => format!("curseforge:{mod_id}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
pub struct ModDepLink {
    pub kind: DepKind,
    pub project_ref: DepProjectRef,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ResolvedDeps {
    pub required: Vec<ResolvedDep>,
    pub optional: Vec<ResolvedDep>,
    pub incompatible: Vec<DepProjectRef>,
    pub unresolvable: Vec<DepProjectRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ResolvedDep {
    pub project_ref: DepProjectRef,
    pub version: ModVersion,
}

/// A direct optional dependency of the primary, plus ITS own transitive
/// required closure (`requires`) — so the dialog can reveal sub-deps live
/// when the user opts in. `requires` already excludes the primary's own
/// requireds, already-installed projects, and loaders.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct OptionalDep {
    pub version: ModVersion,
    pub requires: Vec<ModVersion>,
}

/// The full plan the dependency dialog renders. `required` is the primary's
/// transitive required closure (always installed). `loader_requirements` are
/// loader project refs (informational). `incompatible`/`unresolvable` carry
/// refs for display.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct InstallPlan {
    pub required: Vec<ModVersion>,
    pub optional: Vec<OptionalDep>,
    pub incompatible: Vec<DepProjectRef>,
    pub unresolvable: Vec<DepProjectRef>,
    pub loader_requirements: Vec<DepProjectRef>,
}

/// A mod that would no longer be required by anything after a removal.
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
pub struct OrphanRef {
    pub sha1: String,
    pub name: String,
    pub project_id: String,
}

/// Returned by `mods_install_with_deps` so the UI can show a per-mod toast.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct InstallSummary {
    pub primary_name: String,
    /// Display names of dependencies that were newly installed (primary
    /// excluded). Empty when the primary had no missing deps.
    pub installed_dependencies: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct VersionRef {
    pub source: ModSource,
    pub project_id: String,
    pub version_id: String,
}

/// The per-asset update classification (mirrors `ModUpdateState` for mods).
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AssetUpdateState {
    /// The installed version is the newest offered for this MC version.
    UpToDate,
    /// A newer version exists; `latest` is the version to install.
    UpdateAvailable { latest: Box<ModVersion> },
    /// The platform query failed (network error, project delisted, etc.).
    /// Set by the command layer — never produced by `classify_asset_update`.
    CheckFailed { reason: String },
}

/// One installed resource-pack/shader's update-check result.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AssetUpdateCheck {
    pub filename: String,
    pub name: String,
    pub state: AssetUpdateState,
}

/// A resource pack or shader installed into an instance. Unlike `InstalledMod`
/// there is no `enabled`/`requires` — Minecraft owns activation and these have
/// no dependencies. `kind` is `ResourcePack` or `Shader` (never `Mod`).
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
pub struct InstalledAsset {
    pub kind: ContentKind,
    pub filename: String,
    pub sha1: String,
    pub source: Option<ModSource>,
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    pub name: String,
    pub version_number: Option<String>,
    pub installed_at: String, // RFC 3339
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct InstalledMod {
    pub filename: String,
    pub sha1: String,
    pub source: Option<ModSource>,
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    pub name: String,
    pub version_number: Option<String>,
    pub installed_at: String, // RFC 3339
    pub enabled: bool,
    /// `true` once a modpack hash-enrichment pass has tried this mod —
    /// whether or not a platform identified it. Stops the backfill from
    /// re-querying a permanently-unidentifiable jar. `#[serde(default)]`
    /// so registry files written before this feature load as `false`.
    #[serde(default)]
    pub enrich_attempted: bool,
    /// Project IDs of the *required* dependencies this mod pulled in at
    /// install time (transitive required closure of the primary). Powers
    /// offline orphan detection on bulk uninstall. `#[serde(default)]` so
    /// registry files written before schema v4 load with an empty vec.
    /// Optional deps the user opted into are NOT recorded here.
    #[serde(default)]
    pub requires: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KeyStatus {
    Missing,
    Set,
    Invalid,
}

#[async_trait]
pub trait ModPlatform: Send + Sync {
    async fn search(&self, q: &ModSearchQuery) -> Result<ModSearchPage, Error>;
    async fn project(&self, project_id: &str) -> Result<ModProject, Error>;
    /// Fetch every release of `project_id`. When `mc_version` or
    /// `loader` is None, the corresponding facet is omitted from the
    /// upstream query — used by the "Show all versions" toggle in the
    /// mod detail drawer.
    async fn versions(
        &self,
        project_id: &str,
        mc_version: Option<&str>,
        loader: Option<LoaderKind>,
    ) -> Result<Vec<ModVersion>, Error>;
    async fn resolve_deps(
        &self,
        version: &ModVersion,
        mc_version: &str,
        loader: LoaderKind,
    ) -> Result<ResolvedDeps, Error>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mod_source_round_trips_snake_case() {
        let j = serde_json::to_string(&ModSource::Modrinth).unwrap();
        assert_eq!(j, r#""modrinth""#);
        let back: ModSource = serde_json::from_str(r#""curseforge""#).unwrap();
        assert_eq!(back, ModSource::Curseforge);
    }

    #[test]
    fn dep_project_ref_tagged_serialization() {
        let m = DepProjectRef::Modrinth {
            project_id: "abc".into(),
            version_id: None,
        };
        let j = serde_json::to_string(&m).unwrap();
        assert!(j.contains(r#""source":"modrinth""#));
        assert!(j.contains(r#""project_id":"abc""#));
    }

    #[test]
    fn loader_in_filename_detects_neoforge_before_forge() {
        // "forge" is a substring of "neoforge" — must not misfire.
        assert_eq!(
            loader_in_filename("xaerominimap-neoforge-1.20.4-25.3.13.jar"),
            Some(LoaderKind::NeoForge)
        );
        assert_eq!(
            loader_in_filename("xaerominimap-forge-1.20.4-25.3.13.jar"),
            Some(LoaderKind::Forge)
        );
        assert_eq!(
            loader_in_filename("Xaeros_Minimap_24.4.0_NeoForge_1.20.4.jar"),
            Some(LoaderKind::NeoForge)
        );
    }

    #[test]
    fn loader_in_filename_detects_fabric_quilt_and_none() {
        assert_eq!(
            loader_in_filename("sodium-fabric-0.5.jar"),
            Some(LoaderKind::Fabric)
        );
        assert_eq!(
            loader_in_filename("mod-quilt-1.0.jar"),
            Some(LoaderKind::Quilt)
        );
        // No loader token → ambiguous → None (trust the platform tag).
        assert_eq!(loader_in_filename("jei-1.20.1.jar"), None);
        assert_eq!(loader_in_filename(""), None);
    }

    #[test]
    fn loader_in_filename_requires_a_delimited_token_not_a_substring() {
        // A mod whose NAME embeds a loader word must not be classified by it —
        // otherwise it gets silently dropped on a mismatching instance. Only a
        // delimited token counts.
        assert_eq!(loader_in_filename("forgero-1.0.jar"), None);
        assert_eq!(loader_in_filename("fabricated-1.0.jar"), None);
        assert_eq!(loader_in_filename("quiltessential-2.jar"), None);
        // An explicit loader token alongside such a name still classifies.
        assert_eq!(
            loader_in_filename("Forgero-1.20.1-fabric.jar"),
            Some(LoaderKind::Fabric)
        );
    }

    fn version_with_filename(filename: &str) -> ModVersion {
        ModVersion {
            source: ModSource::Modrinth,
            project_id: "p".into(),
            version_id: filename.into(),
            name: filename.into(),
            version_number: filename.into(),
            mc_versions: vec!["1.20.4".into()],
            loaders: vec![LoaderKind::Forge], // mis-tagged as forge on purpose
            primary_file: ModFile {
                filename: filename.into(),
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
    fn drop_filename_loader_mismatches_removes_neoforge_for_forge_request() {
        // Mirrors the live Xaero's Minimap 1.20.4 data: both jars carry the
        // `forge` loader tag, but the neoforge build must be dropped when the
        // user asked for Forge.
        let versions = vec![
            version_with_filename("xaerominimap-neoforge-1.20.4-25.3.13.jar"),
            version_with_filename("xaerominimap-forge-1.20.4-25.3.13.jar"),
        ];
        let kept = drop_filename_loader_mismatches(versions, Some(LoaderKind::Forge));
        assert_eq!(kept.len(), 1);
        assert_eq!(
            kept[0].primary_file.filename,
            "xaerominimap-forge-1.20.4-25.3.13.jar"
        );
    }

    #[test]
    fn drop_filename_loader_mismatches_keeps_untagged_and_is_noop_without_loader() {
        let versions = vec![version_with_filename("jei-1.20.1.jar")];
        // Untagged filename is kept even when a loader is requested.
        assert_eq!(
            drop_filename_loader_mismatches(versions.clone(), Some(LoaderKind::Forge)).len(),
            1
        );
        // No loader requested → no filtering at all.
        let mixed = vec![
            version_with_filename("x-neoforge-1.jar"),
            version_with_filename("x-forge-1.jar"),
        ];
        assert_eq!(drop_filename_loader_mismatches(mixed, None).len(), 2);
    }

    #[test]
    fn content_kind_round_trips_snake_case() {
        assert_eq!(
            serde_json::to_string(&ContentKind::Mod).unwrap(),
            r#""mod""#
        );
        assert_eq!(
            serde_json::to_string(&ContentKind::ResourcePack).unwrap(),
            r#""resource_pack""#
        );
        assert_eq!(
            serde_json::to_string(&ContentKind::Shader).unwrap(),
            r#""shader""#
        );
        let back: ContentKind = serde_json::from_str(r#""shader""#).unwrap();
        assert_eq!(back, ContentKind::Shader);
    }

    #[test]
    fn content_kind_defaults_to_mod_when_absent() {
        let q: ModSearchQuery = serde_json::from_str(
            r#"{"source":"modrinth","query":"x","mc_version":null,"loader":null,
                "sort":"relevance","page_size":20,"offset":0}"#,
        )
        .unwrap();
        assert_eq!(q.kind, ContentKind::Mod);
    }
}
