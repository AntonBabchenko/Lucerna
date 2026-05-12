//! Version manifest + install pipeline.

pub mod assets;
pub mod client;
pub mod install;
pub mod libraries;
pub mod manifest;
pub mod version_json;

pub use install::{install_version, InstallPhase, InstallProgress};
pub use manifest::{list_manifest, VersionEntry, VersionType};
pub use version_json::{parse as parse_version_json, VersionDetails};
