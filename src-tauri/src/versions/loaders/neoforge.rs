//! NeoForge loader shim. Delegates to `crate::forge::meta::list_versions`
//! and `crate::forge::install_forge` with `ForgeFlavor::NeoForge`. Mirrors
//! `forge.rs`.
//!
//! See `docs/superpowers/specs/2026-05-17-neoforge-design.md`.

use crate::error::Result;
use crate::forge::ForgeFlavor;
use crate::versions::loaders::LoaderVersion;
use crate::versions::version_json::VersionDetails;

pub(super) async fn list(mc: &str) -> Result<Vec<LoaderVersion>> {
    crate::forge::meta::list_versions(ForgeFlavor::NeoForge, mc).await
}

pub(super) async fn profile(
    mc: &str,
    neoforge_ver: &str,
    app: &tauri::AppHandle,
) -> Result<VersionDetails> {
    crate::forge::install_forge(ForgeFlavor::NeoForge, mc, neoforge_ver, app).await
}
