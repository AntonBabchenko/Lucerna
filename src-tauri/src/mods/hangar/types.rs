//! Minimal serde shapes for the Hangar api/v1 endpoints we use.
//! https://hangar.papermc.io/api/docs (rendered from Hangar's OpenAPI spec)
//!
//! Every struct is tolerant of extra fields (serde's default "ignore unknown
//! fields" behavior) — we only declare what we consume.

use std::collections::HashMap;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct HangarPagination {
    pub count: u32,
    #[allow(dead_code)] // Kept for shape fidelity; not consumed by callers yet.
    pub limit: u32,
    #[allow(dead_code)]
    pub offset: u32,
}

#[derive(Debug, Deserialize)]
pub struct HangarPage<T> {
    pub pagination: HangarPagination,
    pub result: Vec<T>,
}

#[derive(Debug, Deserialize)]
pub struct HangarNamespace {
    pub owner: String,
    pub slug: String,
}

#[derive(Debug, Deserialize)]
pub struct HangarStats {
    pub downloads: u64,
}

#[derive(Debug, Deserialize)]
pub struct HangarProject {
    pub name: String,
    pub namespace: HangarNamespace,
    pub stats: HangarStats,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, rename = "avatarUrl")]
    pub avatar_url: Option<String>,
    #[serde(default, rename = "lastUpdated")]
    pub last_updated: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct HangarFileInfo {
    pub name: String,
    #[serde(rename = "sizeBytes")]
    pub size_bytes: f64,
    #[serde(rename = "sha256Hash")]
    pub sha256_hash: String,
}

#[derive(Debug, Deserialize)]
pub struct HangarDownload {
    #[serde(default, rename = "downloadUrl")]
    pub download_url: Option<String>,
    #[serde(default, rename = "externalUrl")]
    pub external_url: Option<String>,
    #[serde(default, rename = "fileInfo")]
    pub file_info: Option<HangarFileInfo>,
}

#[derive(Debug, Deserialize)]
pub struct HangarVersion {
    pub name: String,
    #[serde(default, rename = "createdAt")]
    pub created_at: Option<String>,
    #[serde(default)]
    pub downloads: HashMap<String, HangarDownload>,
    #[serde(default, rename = "platformDependencies")]
    pub platform_dependencies: HashMap<String, Vec<String>>,
}
