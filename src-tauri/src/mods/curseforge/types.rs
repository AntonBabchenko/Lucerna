//! Minimal serde shapes for the CurseForge Eternal API v1.
//! https://docs.curseforge.com/

use serde::Deserialize;

pub const GAME_MINECRAFT: u32 = 432;

// CurseForge Minecraft class IDs (https://docs.curseforge.com — categories):
// Mods=6, Resource Packs=12, Shaders=6552, Bukkit Plugins=5. (Datapacks=6945,
// Worlds=17 — unused.) `search()` early-returns for `Plugin` before this is
// ever called (CF is not offered as a plugin source) — the arm exists purely
// so this defensive lookup stays exhaustive.
pub fn class_id(kind: crate::mods::platform::ContentKind) -> u32 {
    use crate::mods::platform::ContentKind::*;
    match kind {
        Mod => 6,
        ResourcePack => 12,
        Shader => 6552,
        Plugin => 5,
    }
}

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

/// Recognize a CurseForge `gameVersions` entry that names a mod loader.
///
/// CF mixes Minecraft versions and loader names in the same array, e.g.
/// `["1.20.1", "NeoForge"]`. This maps the loader names (case-insensitive)
/// to `LoaderKind`; anything else (MC versions, `Client`/`Server`, etc.)
/// returns `None`. NeoForge is distinct from Forge — they are not
/// interchangeable when choosing which file to install.
pub fn loader_from_tag(tag: &str) -> Option<crate::mods::platform::LoaderKind> {
    use crate::mods::platform::LoaderKind::*;
    match tag.trim().to_ascii_lowercase().as_str() {
        "forge" => Some(Forge),
        "fabric" => Some(Fabric),
        "quilt" => Some(Quilt),
        "neoforge" => Some(NeoForge),
        _ => None,
    }
}

/// CF `gameVersions` also carries environment markers (`Client` / `Server`)
/// that are neither Minecraft versions nor loaders. Recognize them so they
/// don't leak into a version's `mc_versions` list.
pub fn is_environment_marker(tag: &str) -> bool {
    matches!(
        tag.trim().to_ascii_lowercase().as_str(),
        "client" | "server"
    )
}

#[cfg(test)]
mod tests {
    use super::loader_from_tag;
    use crate::mods::platform::LoaderKind;

    #[test]
    fn class_id_maps_each_kind() {
        use crate::mods::platform::ContentKind::*;
        assert_eq!(super::class_id(Mod), 6);
        assert_eq!(super::class_id(ResourcePack), 12);
        assert_eq!(super::class_id(Shader), 6552);
    }

    #[test]
    fn maps_known_loader_tags_case_insensitively() {
        assert_eq!(loader_from_tag("Forge"), Some(LoaderKind::Forge));
        assert_eq!(loader_from_tag("forge"), Some(LoaderKind::Forge));
        assert_eq!(loader_from_tag("Fabric"), Some(LoaderKind::Fabric));
        assert_eq!(loader_from_tag("Quilt"), Some(LoaderKind::Quilt));
        assert_eq!(loader_from_tag("NeoForge"), Some(LoaderKind::NeoForge));
        assert_eq!(loader_from_tag("neoforge"), Some(LoaderKind::NeoForge));
    }

    #[test]
    fn forge_and_neoforge_are_distinct() {
        assert_ne!(loader_from_tag("Forge"), loader_from_tag("NeoForge"));
    }

    #[test]
    fn mc_versions_and_markers_are_not_loaders() {
        assert_eq!(loader_from_tag("1.20.1"), None);
        assert_eq!(loader_from_tag("1.21.4"), None);
        assert_eq!(loader_from_tag("Client"), None);
        assert_eq!(loader_from_tag("Server"), None);
        assert_eq!(loader_from_tag(""), None);
    }

    #[test]
    fn recognizes_environment_markers() {
        use super::is_environment_marker;
        assert!(is_environment_marker("Client"));
        assert!(is_environment_marker("server"));
        assert!(!is_environment_marker("1.20.1"));
        assert!(!is_environment_marker("Forge"));
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
    // `index`/`page_size`/`result_count` echo the request; search() drives
    // pagination from the requested offset/page_size and only reads back
    // `total_count`.
    #[allow(dead_code)]
    pub index: u32,
    #[allow(dead_code)]
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
