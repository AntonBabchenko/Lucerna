//! Types crossing the IPC boundary for modpack export. All derive
//! `specta::Type`.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::mods::modpack::schema::ModpackFormat;
use crate::mods::platform::ModSource;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExportMode {
    /// Manifest references mods by URL/ID; recipient's launcher downloads them.
    Lightweight,
    /// Every jar travels inside `overrides/`; nothing is downloaded on import.
    Full,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
pub struct ExportMetadata {
    pub name: String,
    pub version: String,
    /// Empty string when unset.
    pub author: String,
    /// Empty string when unset.
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
pub struct ExportOptions {
    pub format: ModpackFormat,
    pub mode: ExportMode,
    pub include_config: bool,
    pub include_resourcepacks: bool,
    pub include_shaderpacks: bool,
    pub include_worlds: bool,
    /// SHA-1 (lowercased) of each unresolvable mod the user chose to BUNDLE.
    /// Ignored in `Full` mode (everything is bundled regardless).
    pub bundle_shas: Vec<String>,
    pub metadata: ExportMetadata,
}

/// One installed mod as the dialog sees it. The frontend computes, per
/// chosen format + mode, which mods are unresolvable.
#[derive(Debug, Clone, Serialize, Type, PartialEq)]
pub struct ExportModInfo {
    pub sha1: String,
    pub name: String,
    pub filename: String,
    pub source: Option<ModSource>,
    /// `project_id` AND `version_id` both present in the registry.
    pub has_ids: bool,
}

#[derive(Debug, Clone, Serialize, Type, PartialEq)]
pub struct ExportPreview {
    /// Enabled mods only — disabled mods are never exported.
    pub mods: Vec<ExportModInfo>,
    pub has_config: bool,
    pub has_resourcepacks: bool,
    pub has_shaderpacks: bool,
    pub has_saves: bool,
    /// Total bytes under `saves/`, for the privacy/size warning. f64 per
    /// the specta no-BigInt rule.
    pub saves_size_bytes: f64,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum ModpackExportProgress {
    Resolving { current: u32, total: u32 },
    Hashing { current: u32, total: u32 },
    Bundling { current: u32, total: u32 },
    Writing,
    Done { path: String },
}
