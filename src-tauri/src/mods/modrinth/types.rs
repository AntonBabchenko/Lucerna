//! Minimal serde shapes for the Modrinth v2 endpoints we use.
//! https://docs.modrinth.com/api/

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct SearchResponse {
    pub hits: Vec<SearchHit>,
    pub total_hits: u32,
    pub offset: u32,
    pub limit: u32,
}

#[derive(Debug, Deserialize)]
pub struct SearchHit {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub icon_url: Option<String>,
    pub downloads: u64,
    pub author: String,
    pub date_modified: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Project {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub description: String, // short
    pub body: String,        // long markdown
    pub icon_url: Option<String>,
    pub downloads: u64,
    pub source_url: Option<String>,
    pub wiki_url: Option<String>,
    pub team: String,
    #[serde(default)]
    pub gallery: Vec<GalleryEntry>,
}

#[derive(Debug, Deserialize)]
pub struct GalleryEntry {
    pub url: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub featured: bool,
    #[serde(default)]
    pub ordering: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct Version {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub version_number: String,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub date_published: Option<String>,
    pub files: Vec<VersionFile>,
    pub dependencies: Vec<VersionDep>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct VersionFile {
    pub url: String,
    pub filename: String,
    pub hashes: VersionFileHashes,
    pub size: u64,
    pub primary: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct VersionFileHashes {
    pub sha1: String,
}

#[derive(Debug, Deserialize)]
pub struct VersionDep {
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    pub dependency_type: String, // "required" | "optional" | "incompatible" | "embedded"
}
