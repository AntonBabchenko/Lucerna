//! Normalised types shared between Modrinth and CurseForge modpack
//! parsers and the import orchestrator. Every public type derives
//! `specta::Type` and crosses the IPC boundary as-is.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::mods::installed::{PackOrigin, PackOriginFile};
use crate::mods::platform::{LoaderKind, ModSource};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModpackFormat {
    Modrinth,
    Curseforge,
}

/// Sort order for `/v2/search`. Maps to Modrinth's `index=<value>` query
/// param. `follows` is omitted — not useful for modpack discovery; can
/// be added later if requested.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModpackSort {
    Relevance,
    Downloads,
    Newest,
    Updated,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EnvSupport {
    Required,
    Optional,
    Unsupported,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UnresolvableReason {
    DistributionDisabled,
    HostNotAllowed,
    UnsafePath,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct ModpackFile {
    pub project_id: String,
    pub version_id: String,
    pub name: String,
    pub filename: String,
    pub install_path: String,
    pub sha1: String,
    pub url: String,
    pub size: f64,
    pub env_client: EnvSupport,
    pub source: ModSource,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct ModpackUnresolvable {
    pub reason: UnresolvableReason,
    pub mod_name: String,
    pub manual_action_url: String,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct ModpackSummary {
    pub format: ModpackFormat,
    pub name: String,
    pub version: String,
    pub game_version: String,
    pub loader: LoaderKind,
    pub loader_version: Option<String>,
    pub files: Vec<ModpackFile>,
    pub unresolvable: Vec<ModpackUnresolvable>,
    pub has_overrides: bool,
    pub has_client_overrides: bool,
    pub has_saves_in_overrides: bool,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct ModpackHit {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub icon_url: Option<String>,
    pub downloads: f64,
    pub latest_mc_version: Option<String>,
    pub supported_loaders: Vec<LoaderKind>,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct ModpackSearchPage {
    pub hits: Vec<ModpackHit>,
    pub total: u32,
    pub offset: u32,
    pub limit: u32,
}

/// Coarse-grained phase emitted on the `modpack_progress` channel.
/// Per-mod download/verify/copy ticks continue on the existing
/// `install_progress` channel from sub-3 so PhaseStatusRow + the
/// import progress view can both render fine-grained state.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum ModpackProgress {
    Inspecting,
    CreatingInstance { name: String },
    InstallingFile { current: u32, total: u32, file_name: String },
    ExtractingOverrides { current: u32, total: u32 },
    Done { instance_id: String },
}

/// One-shot diff of "what was the pack at import time" vs "what's
/// actually on disk now". The UI uses this to draw the modified-tag on
/// imported cards and the removed-from-pack section in the drawer.
/// Returned by `modpack_status` for pack-originated instances; the
/// command returns `None` for instances that have no `pack_origin`
/// recorded (= manually created or pre-bundle-2 imports).
#[derive(Debug, Clone, Serialize, Type)]
pub struct ModpackStatus {
    /// The frozen snapshot captured at import time.
    pub origin: PackOrigin,
    /// SHA-1 of every mod currently installed in the instance (the
    /// reconciled `mods_list_installed` set). Lowercased.
    pub installed_shas: Vec<String>,
    /// Pack files whose SHA-1 is no longer present in `installed_shas`
    /// (the user disabled+removed, uninstalled, or never installed).
    pub removed_files: Vec<PackOriginFile>,
    /// Number of installed mods whose SHA-1 is NOT in `origin.files`.
    /// These are user additions made after the import landed.
    pub added_count: u32,
    /// `!removed_files.is_empty() || added_count > 0`. Pre-computed
    /// here so the UI doesn't have to.
    pub is_modified: bool,
}

/// One entry in a modpack project's version list, as shown in the
/// modpack version drawer. Mirrors the subset of the Modrinth
/// `/v2/project/<id>/version` response the UI consumes.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ModpackVersionEntry {
    pub id: String,
    pub name: String,
    pub version_number: String,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub date_published: String,
}
