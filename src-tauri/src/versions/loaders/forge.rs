//! Forge loader shim. Delegates to `crate::forge::meta::list_versions`
//! for the version list and to `crate::forge::install_forge` for the
//! installer-driven profile assembly.
//!
//! See `docs/superpowers/specs/2026-05-16-forge-loader-design.md`.

use crate::error::Result;
use crate::forge::ForgeFlavor;
use crate::versions::loaders::LoaderVersion;
use crate::versions::version_json::VersionDetails;

pub(super) async fn list(mc: &str) -> Result<Vec<LoaderVersion>> {
    crate::forge::meta::list_versions(ForgeFlavor::Forge, mc).await
}

pub(super) async fn profile(
    mc: &str,
    forge_ver: &str,
    app: &tauri::AppHandle,
) -> Result<VersionDetails> {
    crate::forge::install_forge(ForgeFlavor::Forge, mc, forge_ver, app).await
}
