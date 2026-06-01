//! Self-update: check GitHub Releases, verify the official installer,
//! and launch it. Network goes through `network::`; the installer
//! launch through `process::`. The install is always user-initiated —
//! there is no silent background update (see docs/ROADMAP.md).

pub mod check;
pub mod install;
pub mod verify;

use serde::Serialize;
use specta::Type;

/// One downloadable release asset. `size` is `f64` because specta maps
/// Rust integers wider than i53 to TS `number` via f64 (project-wide
/// convention; see `InstanceWithStatus.created_unix_ms`).
#[derive(Debug, Clone, Serialize, Type)]
pub struct ReleaseAsset {
    pub name: String,
    pub url: String,
    pub size: f64,
}

/// The result of an update check. `available` is false when up-to-date.
#[derive(Debug, Clone, Serialize, Type)]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub available: bool,
    pub release_url: String,
    pub installer: ReleaseAsset,
    pub sha256sums: ReleaseAsset,
    pub cosign_bundle: ReleaseAsset,
}
