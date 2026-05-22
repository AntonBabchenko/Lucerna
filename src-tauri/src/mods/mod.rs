//! Mod browser: Modrinth + CurseForge integration.
//!
//! Public API is the [`ModPlatform`] trait (see [`platform`]).
//! Two implementations live under [`modrinth`] and [`curseforge`].
//! The install pipeline lives in [`install`].

pub mod cache;
pub mod curseforge;
pub mod install;
pub mod installed;
pub mod local;
pub mod modpack;
pub mod modrinth;
pub mod platform;
pub mod project_cache;
pub mod updates;

pub use platform::*;
