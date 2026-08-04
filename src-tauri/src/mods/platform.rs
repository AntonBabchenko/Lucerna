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
    Ftb,
    Atlauncher,
    /// hangar.papermc.io — Bukkit/Spigot/Paper/Purpur plugin registry, served
    /// by `mods::hangar::HangarClient`.
    Hangar,
}

impl ModSource {
    /// Every variant. `tasks::TaskOrigin`'s `From<ModSource>` mapping is
    /// exhaustively tested against this, the same shape as `AiProvider::ALL`
    /// in `instances::schema` — adding a variant without adding it here is
    /// caught by a test rather than by a silently-unmapped source later.
    pub const ALL: [ModSource; 5] = [
        ModSource::Modrinth,
        ModSource::Curseforge,
        ModSource::Ftb,
        ModSource::Atlauncher,
        ModSource::Hangar,
    ];
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
    /// A Bukkit-family server plugin (Paper/Purpur). Searched via Modrinth's
    /// `project_type:plugin` facet and via Hangar (`mods::hangar`).
    Plugin,
    /// A Minecraft data pack. Present for DISCOVERY ONLY — Modrinth's
    /// `project_type:datapack` facet (undocumented but real: verified
    /// 2026-08-04, 13 804 hits, while an unknown project_type returns 0) and
    /// CurseForge class 6945.
    ///
    /// A datapack has no directory under `.minecraft/`: its library lives at
    /// `<instance>/datapacks/` and its real home is
    /// `saves/<world>/datapacks/`, one hardlink per world with its own
    /// enabled state in that world's `level.dat`. So `assets::require_asset_kind`
    /// rejects this variant, and every install/list/update/remove path goes
    /// through `crate::datapacks::*` instead of the asset commands.
    Datapack,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModSort {
    Relevance,
    Downloads,
    Updated,
}

/// Platform-declared server support for a mod. Mirrors Modrinth's `server_side`
/// field (`required` / `optional` / `unsupported` / `unknown`); `Unknown` also
/// covers CurseForge (no clean flag) and any mod we could not identify.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerSideSupport {
    Required,
    Optional,
    Unsupported,
    Unknown,
}

impl ServerSideSupport {
    /// Parse Modrinth's `server_side` string. Unrecognized / absent → `Unknown`.
    pub fn from_modrinth(s: Option<&str>) -> Self {
        match s {
            Some("required") => Self::Required,
            Some("optional") => Self::Optional,
            Some("unsupported") => Self::Unsupported,
            _ => Self::Unknown,
        }
    }
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
    /// Plugin-capable server core driving the plugin-loader facet OR-group
    /// when `kind == Plugin`. Ignored for every other kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_core: Option<crate::servers_runtime::schema::ServerCore>,
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
    /// Loader families this *project* is known to support — a UNION across every
    /// published version and every game version, so it over-states support and
    /// never under-states it. That direction is what makes the dependency
    /// graph's child scoping safe: it can only under-suppress.
    ///
    /// `None` means ONLY "this source cannot report project loaders" (see
    /// [`supplies_project_loaders`]) or "this entry predates the field". It is
    /// never a claim that the project supports nothing. A source that CAN report
    /// always yields `Some`, **empty vec included** — a project whose tags map to
    /// no known loader (a shader, a datapack) is `Some(vec![])`, not `None`.
    /// Storing `None` there would mark the entry permanently stale in
    /// `summary_cache` and re-fetch it on every resolve.
    #[serde(default)]
    pub loaders: Option<Vec<LoaderKind>>,
}

/// Whether `source` can populate [`ModSummary::loaders`].
///
/// Exhaustive on purpose: a new [`ModSource`] must be classified here rather
/// than silently defaulting, and the answer gates a cache-staleness rule — a
/// source wrongly marked `true` would be re-fetched on every dependency-graph
/// resolve, which is the 429 fan-out `summary_cache` exists to prevent.
pub const fn supplies_project_loaders(source: ModSource) -> bool {
    match source {
        // Project-level `loaders`, a union over versions — can only be too wide.
        ModSource::Modrinth => true,
        // Derived from `latestFilesIndexes`, a full-history union verified by
        // exhaustive pagination. Must go through `curseforge::project_loaders`,
        // which returns `None` when the index carries untagged entries.
        ModSource::Curseforge => true,
        // Hangar has no loader concept (Paper/Bukkit plugins). FTB and
        // ATLauncher resolve to `unsupported::UnsupportedModPlatform`, so they
        // never populate a summary at all.
        ModSource::Hangar | ModSource::Ftb | ModSource::Atlauncher => false,
    }
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
    /// Lowercase sha256 hex when the platform publishes one (Hangar-hosted
    /// files). Modrinth/CF fill `sha1` instead; installers prefer sha1 (the
    /// content-addressed cache key) and fall back to sha256 direct download.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
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

/// Why the resolver chose a specific build for a dependency. Surfaced so the UI
/// can later distinguish an author-recommended pin from a "newest compatible"
/// auto-pick. `Copy` because it is a tiny tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SelectionReason {
    /// Author pinned this exact `version_id` (Modrinth) / `file_id` (CF) and it
    /// is compatible — installed verbatim.
    PinHonored,
    /// Author pinned a build, but it is not compatible with this MC/loader —
    /// fell back to a compatible build.
    FellBackFromPin,
    /// A declared version range excluded newer builds and pushed the pick to an
    /// older satisfying build.
    RangeConstrained,
    /// No pin and no range narrowed the choice — newest compatible build.
    NewestNoPin,
}

/// A dependency the launcher plans to install, plus why that build was chosen.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PlannedDep {
    pub version: ModVersion,
    pub selection_reason: SelectionReason,
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
    pub selection_reason: SelectionReason,
}

/// A direct optional dependency of the primary, plus ITS own transitive
/// required closure (`requires`) — so the dialog can reveal sub-deps live
/// when the user opts in. `requires` already excludes the primary's own
/// requireds, already-installed projects, and loaders.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct OptionalDep {
    pub version: ModVersion,
    pub requires: Vec<PlannedDep>,
}

/// The full plan the dependency dialog renders. `required` is the primary's
/// transitive required closure (always installed). `loader_requirements` are
/// loader project refs (informational). `incompatible`/`unresolvable` carry
/// refs for display.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct InstallPlan {
    pub required: Vec<PlannedDep>,
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
    /// One row per installed jar (primary + dependencies), in `install_seq`
    /// order. Unlike the modpack import/update paths — which carry their
    /// per-file report on the terminal `Channel` message because they already
    /// take one — this command has no channel, so the report rides the
    /// return value instead. `InstallSummary` has exactly one producer
    /// (`mods_install_with_deps`) and one consumer (the UI toast), which is
    /// what makes widening the return value cheap here; the same design was
    /// rejected for the modpack paths, where it would have meant inventing an
    /// envelope across three unrelated command signatures.
    pub details: Vec<crate::tasks::TaskDetail>,
}

/// Result of a one-click "install the missing required dependency" action.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InstallMissingOutcome {
    /// The dependency was resolved, verified, and installed. `name` is its display name.
    Installed { name: String },
    /// Could not resolve/verify with confidence — the UI opens a pre-filled
    /// search for `query` (the loader mod-id) so the user can pick it manually.
    OpenSearch { query: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct VersionRef {
    pub source: ModSource,
    pub project_id: String,
    pub version_id: String,
}

/// A hash-recovered identity from enrichment. `version_id` is `None` when
/// the matched Modrinth version's loader/MC tags do not match the instance
/// (a shared "universal" jar attached to several versions) — the project is
/// recorded for icons/name, but a misleading version is not.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedIdentity {
    pub source: ModSource,
    pub project_id: String,
    pub version_id: Option<String>,
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
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
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

    /// Batch-fetch project summaries by id. Returns only the projects the
    /// platform resolved — unknown ids are silently omitted (callers degrade
    /// the corresponding rows). Collapses the per-mod `project()` fan-out (a
    /// 429 rate-limit source on large instances) into a handful of batched
    /// requests. The default implementation falls back to per-id `project()`
    /// so a platform without a batch endpoint still works.
    async fn summaries(&self, ids: &[&str]) -> Result<Vec<ModSummary>, Error> {
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            if let Ok(p) = self.project(id).await {
                out.push(p.summary);
            }
        }
        Ok(out)
    }

    /// Batch-fetch versions by their version id (Modrinth version id /
    /// CurseForge file id). Each returned `ModVersion` carries its declared
    /// `deps`, so the dependency graph can be built from installed versions
    /// without a per-project `versions()` call. Unknown ids are omitted. The
    /// default implementation returns an empty list (sources with no batch
    /// endpoint contribute no version data).
    async fn versions_by_ids(&self, _version_ids: &[&str]) -> Result<Vec<ModVersion>, Error> {
        Ok(Vec::new())
    }

    /// Every release of `project_id` compatible with one of the given plugin
    /// loader slugs (bukkit/spigot/paper/purpur), newest-first. Default:
    /// the platform hosts no plugins.
    async fn plugin_versions(
        &self,
        _project_id: &str,
        _mc_version: Option<&str>,
        _plugin_loaders: &[&str],
    ) -> Result<Vec<ModVersion>, Error> {
        Ok(Vec::new())
    }

    /// Every datapack release of `project_id`, newest-first, filtered by the
    /// raw `datapack` loader slug — the datapack twin of [`plugin_versions`].
    /// A hybrid project (e.g. Terralith) publishes both a datapack and a mod,
    /// and its unfiltered "latest version" is a mod jar, so the unfiltered
    /// [`versions`] listing must never be used for this kind.
    ///
    /// Default: the platform hosts no datapacks. CurseForge deliberately
    /// inherits this default until its own PR lands — its `versions` filters
    /// solely via `modLoaderType`, which cannot express a datapack — and the
    /// UI hides the CurseForge source for the datapack kind over that
    /// interval, so the default is never a visible-but-broken card
    /// (slice-2 design §13.1).
    ///
    /// [`plugin_versions`]: ModPlatform::plugin_versions
    /// [`versions`]: ModPlatform::versions
    async fn datapack_versions(
        &self,
        _project_id: &str,
        _mc_version: Option<&str>,
    ) -> Result<Vec<ModVersion>, Error> {
        Ok(Vec::new())
    }

    /// The cumulative changelog for `(base_version_id, target_version_id]` of
    /// `project_id`, newest→oldest, each section's `body_html` sanitized.
    /// Default: the source has no changelog API — a defensive backstop, since
    /// the UI gates the affordance on [`changelog_supported`].
    async fn changelog_range(
        &self,
        _project_id: &str,
        _target_version_id: &str,
        _base_version_id: Option<&str>,
    ) -> Result<crate::mods::changelog::ChangelogResult, Error> {
        Err(Error::ChangelogUnsupported)
    }
}

/// Sources that implement [`ModPlatform::changelog_range`]. The FE gates the
/// "changelog" affordance on this (re-derived in TS) and the command
/// short-circuits the rest, so the trait default is only a defensive backstop.
pub fn changelog_supported(source: ModSource) -> bool {
    matches!(source, ModSource::Modrinth | ModSource::Curseforge)
}

/// Upper bound on ids per batched `summaries` / `versions_by_ids` request.
/// Keeps Modrinth's `?ids=[…]` query string and CurseForge's POST body within
/// safe limits; 286 mods fan into ~3 requests rather than ~286.
pub const BATCH_CHUNK: usize = 100;

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
    fn mod_source_ftb_round_trips_snake_case() {
        let j = serde_json::to_string(&ModSource::Ftb).unwrap();
        assert_eq!(j, r#""ftb""#);
        let back: ModSource = serde_json::from_str(r#""ftb""#).unwrap();
        assert_eq!(back, ModSource::Ftb);
    }

    #[test]
    fn mod_source_atlauncher_round_trips_snake_case() {
        let j = serde_json::to_string(&ModSource::Atlauncher).unwrap();
        assert_eq!(j, r#""atlauncher""#);
        let back: ModSource = serde_json::from_str(r#""atlauncher""#).unwrap();
        assert_eq!(back, ModSource::Atlauncher);
    }

    #[test]
    fn mod_source_hangar_round_trips_snake_case() {
        let j = serde_json::to_string(&ModSource::Hangar).unwrap();
        assert_eq!(j, r#""hangar""#);
        let back: ModSource = serde_json::from_str(r#""hangar""#).unwrap();
        assert_eq!(back, ModSource::Hangar);
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
                sha256: None,
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
    fn content_kind_plugin_round_trips_snake_case() {
        assert_eq!(
            serde_json::to_string(&ContentKind::Plugin).unwrap(),
            r#""plugin""#
        );
        let back: ContentKind = serde_json::from_str(r#""plugin""#).unwrap();
        assert_eq!(back, ContentKind::Plugin);
    }

    #[test]
    fn content_kind_defaults_to_mod_when_absent() {
        let q: ModSearchQuery = serde_json::from_str(
            r#"{"source":"modrinth","query":"x","mc_version":null,"loader":null,
                "sort":"relevance","page_size":20,"offset":0}"#,
        )
        .unwrap();
        assert_eq!(q.kind, ContentKind::Mod);
        assert_eq!(q.plugin_core, None);
    }
}
