//! Minimal serde shapes for the CurseForge Eternal API v1.
//! https://docs.curseforge.com/

use serde::Deserialize;

pub const GAME_MINECRAFT: u32 = 432;

// modLoaderType: 0=Any 1=Forge 4=Fabric 5=Quilt 6=NeoForge
pub fn loader_type(loader: crate::mods::platform::LoaderKind) -> u32 {
    use crate::mods::platform::LoaderKind::*;
    match loader {
        Forge => 1,
        Fabric => 4,
        Quilt => 5,
        NeoForge => 6,
        Vanilla => 0,
    }
}

#[derive(Debug, Deserialize)]
pub struct Envelope<T> {
    pub data: T,
}

#[derive(Debug, Deserialize)]
pub struct ListEnvelope<T> {
    pub data: Vec<T>,
    pub pagination: Option<Pagination>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pagination {
    pub index: u32,
    pub page_size: u32,
    #[allow(dead_code)]
    pub result_count: u32,
    pub total_count: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mod {
    pub id: u32,
    pub slug: String,
    pub name: String,
    pub summary: String,
    pub download_count: u64,
    pub authors: Vec<Author>,
    pub logo: Option<Logo>,
    pub date_modified: Option<String>,
    #[allow(dead_code)]
    pub links: Links,
    #[serde(default)]
    pub screenshots: Vec<Screenshot>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Screenshot {
    pub url: String,
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Author {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct Logo {
    pub url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Links {
    #[allow(dead_code)]
    pub website_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct File {
    pub id: u32,
    #[allow(dead_code)]
    pub mod_id: u32,
    pub display_name: String,
    pub file_name: String,
    pub file_length: u64,
    pub hashes: Vec<Hash>,
    pub game_versions: Vec<String>,
    pub download_url: Option<String>,
    pub file_date: Option<String>,
    pub is_available: bool,
    #[allow(dead_code)]
    pub release_type: u8,
    pub dependencies: Vec<Dependency>,
}

#[derive(Debug, Deserialize)]
pub struct Hash {
    pub value: String,
    pub algo: u32,
} // 1 = SHA1, 2 = MD5

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dependency {
    pub mod_id: u32,
    pub relation_type: u8,
}
// relation_type: 1=EmbeddedLibrary 2=OptionalDependency 3=RequiredDependency
//                4=Tool 5=Incompatible 6=Include
