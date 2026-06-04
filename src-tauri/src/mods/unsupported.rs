//! Fallback `ModPlatform` implementation for sources that have no
//! per-mod browser (e.g. FTB, which is a modpack-only source).

use async_trait::async_trait;

use crate::error::Error;
use crate::mods::platform::{
    LoaderKind, ModPlatform, ModProject, ModSearchPage, ModSearchQuery, ModSource, ModVersion,
    ResolvedDeps,
};

/// A no-op platform handle returned by `platform_for` when the source has no
/// per-mod browser.  Every trait method returns
/// `Error::ModsPlatformUnsupported` so the caller gets a typed error rather
/// than a panic.
pub struct UnsupportedModPlatform {
    pub source: ModSource,
}

#[async_trait]
impl ModPlatform for UnsupportedModPlatform {
    async fn search(&self, _q: &ModSearchQuery) -> Result<ModSearchPage, Error> {
        Err(Error::ModsPlatformUnsupported {
            platform: self.source,
        })
    }

    async fn project(&self, _project_id: &str) -> Result<ModProject, Error> {
        Err(Error::ModsPlatformUnsupported {
            platform: self.source,
        })
    }

    async fn versions(
        &self,
        _project_id: &str,
        _mc_version: Option<&str>,
        _loader: Option<LoaderKind>,
    ) -> Result<Vec<ModVersion>, Error> {
        Err(Error::ModsPlatformUnsupported {
            platform: self.source,
        })
    }

    async fn resolve_deps(
        &self,
        _version: &ModVersion,
        _mc_version: &str,
        _loader: LoaderKind,
    ) -> Result<ResolvedDeps, Error> {
        Err(Error::ModsPlatformUnsupported {
            platform: self.source,
        })
    }
}
