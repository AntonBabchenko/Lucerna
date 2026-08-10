//! Mod browser: Modrinth + CurseForge integration.
//!
//! Public API is the [`ModPlatform`] trait (see [`platform`]).
//! Two implementations live under [`modrinth`] and [`curseforge`].
//! The install pipeline lives in [`install`].

pub mod asset_local;
pub mod assets;
pub mod cache;
pub mod changelog;
pub mod cited_resolve;
pub mod compat;
pub mod curseforge;
pub mod dep_resolve;
pub mod dep_select;
pub mod depgraph;
pub mod deps;
pub mod enrich;
pub mod fix_resolve;
pub mod forge_descriptor;
pub mod hangar;
pub mod install;
pub mod install_batch;
pub mod installed;
pub mod jar_scan_cache;
pub mod local;
pub mod mc_compat;
pub mod migration;
pub mod modpack;
pub mod modrinth;
pub mod optimise;
pub mod orphans;
pub mod pack_completion;
pub mod platform;
pub mod preflight;
pub mod project_cache;
pub mod range_describe;
pub mod render;
pub mod store;
pub mod summary_cache;
pub mod unsupported;
pub mod updates;
pub mod version_cache;
pub mod version_range;
pub mod word_segment;

pub use platform::*;
