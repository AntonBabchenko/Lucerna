use async_trait::async_trait;

use crate::error::Error;
use crate::mods::modpack::schema::{
    ModpackProject, ModpackSearchPage, ModpackSort, ModpackVersionEntry,
};
use crate::mods::modpack::source::{ModpackSource, SourceCaps};
use crate::mods::platform::LoaderKind;

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
        _query: &str,
        _page: u32,
        _mc_version: Option<&str>,
        _loader: Option<LoaderKind>,
        _sort: ModpackSort,
        _page_size: u32,
    ) -> Result<ModpackSearchPage, Error> {
        todo!("Task 3: ModrinthModpackSource::search")
    }

    async fn get_versions(&self, _project_id: &str) -> Result<Vec<ModpackVersionEntry>, Error> {
        todo!("Task 3: ModrinthModpackSource::get_versions")
    }

    async fn get_project(&self, _project_id: &str) -> Result<ModpackProject, Error> {
        todo!("Task 3: ModrinthModpackSource::get_project")
    }

    async fn stage_version_to_temp(
        &self,
        _app: &tauri::AppHandle,
        _project_id: &str,
        _version_id: &str,
    ) -> Result<String, Error> {
        todo!("Task 3: ModrinthModpackSource::stage_version_to_temp")
    }
}
