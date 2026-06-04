//! Modpack source abstraction. Twin of `mods::platform::ModPlatform`
//! (the per-mod browser). `modpack_source_for` is the twin of
//! `commands::platform_for`. Adding a source = one impl + one match arm.

use async_trait::async_trait;
use serde::Serialize;
use specta::Type;

use crate::error::Error;
use crate::mods::modpack::schema::{
    ModpackProject, ModpackSearchPage, ModpackSort, ModpackVersionEntry,
};
use crate::mods::platform::{LoaderKind, ModSource};

pub mod curseforge;
pub mod ftb;
pub mod modrinth;

pub use curseforge::CurseforgeModpackSource;
pub use ftb::FtbModpackSource;
pub use modrinth::ModrinthModpackSource;

/// Capability descriptor read by the UI to drive source-specific affordances
/// without hardcoding `if source == ftb`. Each field maps to a *present*
/// divergence between the three sources (Principle B.2 — no speculative fields).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
pub struct SourceCaps {
    /// CurseForge requires a stored API key; Modrinth/FTB do not.
    pub needs_api_key: bool,
    /// Modrinth/CF filter by MC version + loader server-side; FTB search
    /// returns IDs only, so its filters are applied client-side.
    pub supports_server_filter: bool,
    /// Modrinth/CF support export; FTB packs are curated (nowhere to upload).
    pub can_export: bool,
}

#[async_trait]
pub trait ModpackSource: Send + Sync {
    fn caps(&self) -> SourceCaps;
    async fn search(
        &self,
        query: &str,
        page: u32,
        mc_version: Option<&str>,
        loader: Option<LoaderKind>,
        sort: ModpackSort,
        page_size: u32,
    ) -> Result<ModpackSearchPage, Error>;
    async fn get_versions(&self, project_id: &str) -> Result<Vec<ModpackVersionEntry>, Error>;
    async fn get_project(&self, project_id: &str) -> Result<ModpackProject, Error>;
    /// Stage a chosen version into the OS temp dir; return the path the
    /// browse flow hands to `modpack_inspect`/`modpack_import`. Modrinth/CF
    /// write the archive (`.mrpack`/`.zip`); FTB writes a `.ftbpack.json`
    /// sidecar (serialized `ModpackSummary`).
    async fn stage_version_to_temp(
        &self,
        app: &tauri::AppHandle,
        project_id: &str,
        version_id: &str,
    ) -> Result<String, Error>;
}

pub fn modpack_source_for(source: ModSource) -> Box<dyn ModpackSource> {
    match source {
        ModSource::Modrinth => Box::new(ModrinthModpackSource),
        ModSource::Curseforge => Box::new(CurseforgeModpackSource),
        ModSource::Ftb => Box::new(FtbModpackSource),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_match_each_source() {
        assert_eq!(
            ModrinthModpackSource.caps(),
            SourceCaps {
                needs_api_key: false,
                supports_server_filter: true,
                can_export: true
            }
        );
        assert_eq!(
            CurseforgeModpackSource.caps(),
            SourceCaps {
                needs_api_key: true,
                supports_server_filter: true,
                can_export: true
            }
        );
        assert_eq!(
            FtbModpackSource.caps(),
            SourceCaps {
                needs_api_key: false,
                supports_server_filter: false,
                can_export: false
            }
        );
    }
}
