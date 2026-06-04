use async_trait::async_trait;

use crate::error::Error;
use crate::mods::modpack::schema::{
    ModpackProject, ModpackSearchPage, ModpackSort, ModpackVersionEntry,
};
use crate::mods::modpack::source::{ModpackSource, SourceCaps};
use crate::mods::platform::LoaderKind;

const MR_BASE: &str = "https://api.modrinth.com";

pub struct ModrinthModpackSource;

#[async_trait]
impl ModpackSource for ModrinthModpackSource {
    fn caps(&self) -> SourceCaps {
        SourceCaps {
            needs_api_key: false,
            supports_server_filter: true,
            can_export: true,
        }
    }

    async fn search(
        &self,
        query: &str,
        page: u32,
        mc_version: Option<&str>,
        loader: Option<LoaderKind>,
        sort: ModpackSort,
        page_size: u32,
    ) -> Result<ModpackSearchPage, Error> {
        crate::mods::modpack::search::search(
            MR_BASE, query, page, mc_version, loader, sort, page_size,
        )
        .await
    }

    async fn get_versions(&self, project_id: &str) -> Result<Vec<ModpackVersionEntry>, Error> {
        crate::commands::fetch_modpack_versions(MR_BASE, project_id).await
    }

    async fn get_project(&self, project_id: &str) -> Result<ModpackProject, Error> {
        crate::commands::fetch_modrinth_modpack_project(MR_BASE, project_id).await
    }

    async fn stage_version_to_temp(
        &self,
        app: &tauri::AppHandle,
        project_id: &str,
        version_id: &str,
    ) -> Result<String, Error> {
        crate::mods::modpack::source::stage::download_modrinth_mrpack(
            app, MR_BASE, project_id, version_id,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify the adapter's capability descriptor is unchanged — the trait
    /// methods delegate to `search::search`, `commands::fetch_modpack_versions`,
    /// and `commands::fetch_modrinth_modpack_project`, all of which carry their
    /// own wiremock coverage in `mods::modpack::search` and `commands` tests.
    /// A base-injection seam is intentionally absent from the public trait
    /// signature (it would leak test concerns); adapter-level delegation is
    /// therefore verified by construction: if the delegation target's signature
    /// changes without updating the call site here, this file will fail to
    /// compile, which is the strongest guarantee possible without test-seams.
    #[test]
    fn modrinth_adapter_caps_unchanged() {
        assert_eq!(
            ModrinthModpackSource.caps(),
            SourceCaps {
                needs_api_key: false,
                supports_server_filter: true,
                can_export: true,
            }
        );
    }
}
