//! Modpack import and discovery. Two formats supported: Modrinth
//! `.mrpack` (zip with `modrinth.index.json`) and CurseForge `.zip`
//! (zip with `manifest.json`).

pub mod cf_api;
pub mod curseforge;
pub mod detect;
pub mod export;
pub mod import;
pub mod modrinth;
pub mod overrides;
pub mod path_safety;
pub mod schema;
pub mod search;

pub use detect::detect_format;
pub use export::{
    ExportMetadata, ExportMode, ExportModInfo, ExportOptions, ExportPreview, ModpackExportProgress,
};
pub use schema::{
    EnvSupport, ModpackFile, ModpackFormat, ModpackHit, ModpackProgress, ModpackSearchPage,
    ModpackSort, ModpackSummary, ModpackUnresolvable, UnresolvableReason,
};
