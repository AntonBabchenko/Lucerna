//! Modpack import and discovery. Two formats supported: Modrinth
//! `.mrpack` (zip with `modrinth.index.json`) and CurseForge `.zip`
//! (zip with `manifest.json`). See
//! `docs/superpowers/specs/2026-05-19-v0.5.0-modpack-import-design.md`.

pub mod cf_api;
pub mod curseforge;
pub mod detect;
pub mod import;
pub mod modrinth;
pub mod overrides;
pub mod path_safety;
pub mod schema;
pub mod search;

pub use detect::detect_format;
pub use schema::{
    EnvSupport, ModpackFile, ModpackFormat, ModpackHit, ModpackProgress, ModpackSearchPage,
    ModpackSort, ModpackSummary, ModpackUnresolvable, UnresolvableReason,
};
