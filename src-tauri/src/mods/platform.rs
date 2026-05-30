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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModSort {
    Relevance,
    Downloads,
    Updated,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ModSearchQuery {
    pub source: ModSource,
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

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
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

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
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

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
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

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
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

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct VersionRef {
    pub source: ModSource,
    pub project_id: String,
    pub version_id: String,
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
}
