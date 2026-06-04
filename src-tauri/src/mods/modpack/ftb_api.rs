//! FTB (Feed The Beast) modpacks.ch API client — search, pack detail,
//! and version manifest. Pure networking + deserialization; no mapping
//! to shared schema types (that is Task 6).
//!
//! All HTTP routes through `network::request::get` per CLAUDE.md rules.
//! Structs are internal-only (not crossing IPC) so they only derive
//! `Deserialize`, not `specta::Type`.

use serde::Deserialize;

use crate::error::Error;

const UA: &str = "AntonBabchenko/Lucerna (github.com/AntonBabchenko/Lucerna)";

// ---- response shapes --------------------------------------------------------

#[derive(Debug, Deserialize)]
struct FtbSearchResp {
    #[serde(default)]
    packs: Vec<u64>,
}

#[derive(Debug, Deserialize)]
pub struct FtbPackDetail {
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub synopsis: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub art: Vec<FtbArt>,
    #[serde(default)]
    pub authors: Vec<FtbAuthor>,
    #[serde(default)]
    pub installs: u64,
    #[serde(default)]
    pub versions: Vec<FtbVersionRef>,
}

#[derive(Debug, Deserialize)]
pub struct FtbArt {
    #[serde(default)]
    pub url: String,
    #[serde(default, rename = "type")]
    pub art_type: String,
}

#[derive(Debug, Deserialize)]
pub struct FtbAuthor {
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct FtbVersionRef {
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub name: String,
    #[serde(default, rename = "type")]
    pub release_type: String,
    #[serde(default)]
    pub targets: Vec<FtbTarget>,
    #[serde(default)]
    pub updated: i64,
}

#[derive(Debug, Deserialize)]
pub struct FtbVersion {
    #[serde(default)]
    pub files: Vec<FtbFile>,
    #[serde(default)]
    pub targets: Vec<FtbTarget>,
}

#[derive(Debug, Deserialize)]
pub struct FtbFile {
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub sha1: String,
    #[serde(default)]
    pub size: f64,
    #[serde(default, rename = "type")]
    pub file_type: String,
    #[serde(default)]
    pub clientonly: bool,
    #[serde(default)]
    pub serveronly: bool,
    #[serde(default)]
    pub optional: bool,
    /// Present when the file is distributed via CurseForge rather than the
    /// FTB CDN directly. The `url` field will be empty in this case; the
    /// project/file ids are used to resolve the real download URL via the CF
    /// bulk-files API before install.
    #[serde(default)]
    pub curseforge: Option<FtbCfRef>,
}

/// CurseForge project + file reference embedded in an FTB file manifest entry.
/// Both fields default to 0 when absent; zero values are treated as "not a
/// valid CF ref" throughout the mapper.
#[derive(Debug, Deserialize, Clone)]
pub struct FtbCfRef {
    #[serde(default)]
    pub project: u64,
    #[serde(default)]
    pub file: u64,
}

#[derive(Debug, Deserialize)]
pub struct FtbTarget {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default, rename = "type")]
    pub target_type: String,
}

// ---- public API functions ---------------------------------------------------

/// Search FTB modpacks by keyword; returns a list of FTB pack IDs.
///
/// Calls `GET {base}/public/modpack/search/{limit}?term=<q>`.
pub async fn search_ids(base: &str, query: &str, limit: u32) -> Result<Vec<u64>, Error> {
    let q = crate::mods::modpack::search::urlencode(query);
    let url = format!("{base}/public/modpack/search/{limit}?term={q}");
    let resp = crate::network::request::get(&url, &[("user-agent", UA)], "modpacks")
        .await
        .map_err(|e| Error::ModsNetwork {
            url: url.clone(),
            details: e.to_string(),
        })?;
    if !(200..300).contains(&resp.status) {
        return Err(Error::ModsNetwork {
            url,
            details: format!("HTTP {}", resp.status),
        });
    }
    // Note: FTB returns HTTP 200 with `{"status":"error","message":"Search
    // term too short."}` for empty/too-short terms. That body has no `packs`,
    // so `#[serde(default)]` yields an empty vec — a graceful "no results"
    // while the user is still typing. The empty-query default-browse path uses
    // `popular_ids` instead (see `FtbModpackSource::search`).
    let r: FtbSearchResp = serde_json::from_slice(&resp.body).map_err(|e| Error::ModsDecode {
        platform: "ftb".into(),
        details: e.to_string(),
    })?;
    Ok(r.packs)
}

/// Fetch the most-installed FTB packs; returns a list of FTB pack IDs.
/// Used to populate the default browse view when no search term is entered
/// (FTB's search endpoint rejects empty/short terms).
///
/// Calls `GET {base}/public/modpack/popular/installs/{limit}`. The response
/// shares the `{ "packs": [...] }` shape with the search endpoint.
pub async fn popular_ids(base: &str, limit: u32) -> Result<Vec<u64>, Error> {
    let url = format!("{base}/public/modpack/popular/installs/{limit}");
    let resp = crate::network::request::get(&url, &[("user-agent", UA)], "modpacks")
        .await
        .map_err(|e| Error::ModsNetwork {
            url: url.clone(),
            details: e.to_string(),
        })?;
    if !(200..300).contains(&resp.status) {
        return Err(Error::ModsNetwork {
            url,
            details: format!("HTTP {}", resp.status),
        });
    }
    let r: FtbSearchResp = serde_json::from_slice(&resp.body).map_err(|e| Error::ModsDecode {
        platform: "ftb".into(),
        details: e.to_string(),
    })?;
    Ok(r.packs)
}

/// Fetch the full pack detail for a single FTB pack.
///
/// Calls `GET {base}/public/modpack/{id}`.
pub async fn pack_detail(base: &str, id: u64) -> Result<FtbPackDetail, Error> {
    let url = format!("{base}/public/modpack/{id}");
    let resp = crate::network::request::get(&url, &[("user-agent", UA)], "modpacks")
        .await
        .map_err(|e| Error::ModsNetwork {
            url: url.clone(),
            details: e.to_string(),
        })?;
    if !(200..300).contains(&resp.status) {
        return Err(Error::ModsNetwork {
            url,
            details: format!("HTTP {}", resp.status),
        });
    }
    serde_json::from_slice(&resp.body).map_err(|e| Error::ModsDecode {
        platform: "ftb".into(),
        details: e.to_string(),
    })
}

/// Fetch the file manifest for a specific version of an FTB pack.
///
/// Calls `GET {base}/public/modpack/{id}/{version_id}`.
pub async fn version_manifest(base: &str, id: u64, version_id: u64) -> Result<FtbVersion, Error> {
    let url = format!("{base}/public/modpack/{id}/{version_id}");
    let resp = crate::network::request::get(&url, &[("user-agent", UA)], "modpacks")
        .await
        .map_err(|e| Error::ModsNetwork {
            url: url.clone(),
            details: e.to_string(),
        })?;
    if !(200..300).contains(&resp.status) {
        return Err(Error::ModsNetwork {
            url,
            details: format!("HTTP {}", resp.status),
        });
    }
    serde_json::from_slice(&resp.body).map_err(|e| Error::ModsDecode {
        platform: "ftb".into(),
        details: e.to_string(),
    })
}

// ---- tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::test_env_lock()
    }

    #[tokio::test]
    async fn search_returns_pack_ids() {
        let _g = test_lock();
        let s = MockServer::start().await;
        let resp = serde_json::json!({
            "packs": [91u64, 35u64],
            "curseforge": [],
            "total": 2
        });
        Mock::given(method("GET"))
            .and(path("/public/modpack/search/20"))
            .and(query_param("term", "x"))
            .respond_with(ResponseTemplate::new(200).set_body_json(resp))
            .mount(&s)
            .await;

        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let r = search_ids(&s.uri(), "x", 20).await.unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");

        assert_eq!(r, vec![91, 35]);
    }

    #[tokio::test]
    async fn version_manifest_parses_files_and_targets() {
        let _g = test_lock();
        let s = MockServer::start().await;
        let resp = serde_json::json!({
            "files": [
                {
                    "id": 1001,
                    "name": "JEI.jar",
                    "path": "./mods/",
                    "url": "https://dist.modpacks.ch/modpacks/91/6495/mods/JEI.jar",
                    "sha1": "aabbccdd",
                    "size": 512000.0,
                    "type": "mod",
                    "clientonly": false,
                    "serveronly": false,
                    "optional": false,
                    "mirrors": [],
                    "tags": [],
                    "version": "1.0.0",
                    "updated": 1700000000
                },
                {
                    "id": 1002,
                    "name": "forge.cfg",
                    "path": "./config/",
                    "url": "https://dist.modpacks.ch/modpacks/91/6495/config/forge.cfg",
                    "sha1": "deadbeef",
                    "size": 1024.0,
                    "type": "config",
                    "clientonly": false,
                    "serveronly": true,
                    "optional": false,
                    "mirrors": [],
                    "tags": [],
                    "version": "1.0",
                    "updated": 1700000001
                }
            ],
            "targets": [
                {
                    "version": "36.2.39",
                    "id": 736,
                    "name": "forge",
                    "type": "modloader",
                    "updated": 1700000002
                },
                {
                    "version": "1.16.5",
                    "id": 100,
                    "name": "minecraft",
                    "type": "game",
                    "updated": 1700000003
                },
                {
                    "version": "17",
                    "id": 200,
                    "name": "java",
                    "type": "runtime",
                    "updated": 1700000004
                }
            ]
        });
        Mock::given(method("GET"))
            .and(path("/public/modpack/91/6495"))
            .respond_with(ResponseTemplate::new(200).set_body_json(resp))
            .mount(&s)
            .await;

        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let v = version_manifest(&s.uri(), 91, 6495).await.unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");

        assert_eq!(v.files.len(), 2);
        let mod_file = &v.files[0];
        assert_eq!(mod_file.name, "JEI.jar");
        assert_eq!(mod_file.path, "./mods/");
        assert_eq!(
            mod_file.url,
            "https://dist.modpacks.ch/modpacks/91/6495/mods/JEI.jar"
        );
        assert_eq!(mod_file.sha1, "aabbccdd");
        assert!(!mod_file.serveronly);
        assert_eq!(mod_file.file_type, "mod");

        let cfg_file = &v.files[1];
        assert_eq!(cfg_file.name, "forge.cfg");
        assert_eq!(cfg_file.path, "./config/");
        assert!(cfg_file.serveronly);
        assert_eq!(cfg_file.file_type, "config");

        assert_eq!(v.targets.len(), 3);
        let forge_t = &v.targets[0];
        assert_eq!(forge_t.name, "forge");
        assert_eq!(forge_t.version, "36.2.39");
        assert_eq!(forge_t.target_type, "modloader");

        let mc_t = &v.targets[1];
        assert_eq!(mc_t.name, "minecraft");
        assert_eq!(mc_t.version, "1.16.5");
        assert_eq!(mc_t.target_type, "game");

        let java_t = &v.targets[2];
        assert_eq!(java_t.name, "java");
        assert_eq!(java_t.version, "17");
        assert_eq!(java_t.target_type, "runtime");
    }

    #[tokio::test]
    async fn pack_detail_parses_core_fields() {
        let _g = test_lock();
        let s = MockServer::start().await;
        let resp = serde_json::json!({
            "id": 91,
            "name": "FTB Presents Direwolf20 1.16",
            "synopsis": "Direwolf20's personal modpack for 1.16",
            "description": "Full description here.",
            "art": [
                {
                    "url": "https://dist.modpacks.ch/modpacks/91/art/pack.png",
                    "type": "square"
                }
            ],
            "authors": [
                { "name": "Direwolf20" }
            ],
            "installs": 500000,
            "versions": [
                {
                    "id": 6495,
                    "name": "1.7.0",
                    "type": "Release",
                    "targets": [
                        {
                            "version": "36.2.39",
                            "id": 736,
                            "name": "forge",
                            "type": "modloader",
                            "updated": 1700000002
                        },
                        {
                            "version": "1.16.5",
                            "id": 100,
                            "name": "minecraft",
                            "type": "game",
                            "updated": 1700000003
                        }
                    ],
                    "updated": 1700000010
                }
            ]
        });
        Mock::given(method("GET"))
            .and(path("/public/modpack/91"))
            .respond_with(ResponseTemplate::new(200).set_body_json(resp))
            .mount(&s)
            .await;

        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let detail = pack_detail(&s.uri(), 91).await.unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");

        assert_eq!(detail.id, 91);
        assert_eq!(detail.name, "FTB Presents Direwolf20 1.16");
        assert_eq!(detail.synopsis, "Direwolf20's personal modpack for 1.16");
        assert_eq!(detail.installs, 500_000);

        assert_eq!(detail.art.len(), 1);
        assert_eq!(
            detail.art[0].url,
            "https://dist.modpacks.ch/modpacks/91/art/pack.png"
        );
        assert_eq!(detail.art[0].art_type, "square");

        assert_eq!(detail.versions.len(), 1);
        let v = &detail.versions[0];
        assert_eq!(v.id, 6495);
        assert_eq!(v.name, "1.7.0");
        assert_eq!(v.targets.len(), 2);
        assert_eq!(v.targets[0].name, "forge");
        assert_eq!(v.targets[0].version, "36.2.39");
        assert_eq!(v.targets[1].name, "minecraft");
        assert_eq!(v.targets[1].version, "1.16.5");
    }

    #[tokio::test]
    async fn search_non_2xx_is_error() {
        let _g = test_lock();
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/public/modpack/search/20"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&s)
            .await;

        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let result = search_ids(&s.uri(), "x", 20).await;
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");

        assert!(
            matches!(result, Err(Error::ModsNetwork { .. })),
            "expected ModsNetwork error, got: {result:?}"
        );
    }
}
