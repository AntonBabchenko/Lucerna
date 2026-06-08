//! ATLauncher API client. Catalogue via `api.atlauncher.com`, per-version
//! install manifest (`Configs.json`) via `download.nodecdn.net`. Pure
//! networking + deserialization; mapping to shared schema is `atl_map`.
//!
//! All HTTP routes through `network::request::get` per CLAUDE.md rules.

use serde::Deserialize;

use crate::error::Error;

const UA: &str = "AntonBabchenko/Lucerna (github.com/AntonBabchenko/Lucerna)";
const NODECDN_BASE: &str = "https://download.nodecdn.net/containers/atl";

// ---- catalogue shapes -------------------------------------------------------

#[derive(Debug, Deserialize)]
struct AtlPacksResp {
    #[serde(default)]
    data: Vec<AtlPack>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AtlPack {
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub name: String,
    #[serde(default, rename = "safeName")]
    pub safe_name: String,
    #[serde(default, rename = "type")]
    pub pack_type: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, rename = "websiteURL")]
    pub website_url: Option<String>,
    #[serde(default)]
    pub versions: Vec<AtlVersionRef>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AtlVersionRef {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub minecraft: String,
    #[serde(default)]
    pub published: i64,
}

// ---- Configs.json shapes ----------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct AtlConfigs {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub minecraft: String,
    #[serde(default)]
    pub loader: Option<AtlLoader>,
    #[serde(default)]
    pub mods: Vec<AtlMod>,
}

#[derive(Debug, Deserialize)]
pub struct AtlLoader {
    #[serde(default, rename = "type")]
    pub loader_type: String,
    #[serde(default)]
    pub metadata: Option<AtlLoaderMeta>,
}

#[derive(Debug, Deserialize)]
pub struct AtlLoaderMeta {
    #[serde(default)]
    pub version: String,
}

#[derive(Debug, Deserialize)]
pub struct AtlMod {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub file: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub md5: String,
    #[serde(default)]
    pub filesize: f64,
    #[serde(default, rename = "type")]
    pub mod_type: String,
    #[serde(default)]
    pub download: String,
    #[serde(default = "default_true")]
    pub client: bool,
    #[serde(default)]
    pub optional: bool,
    #[serde(default, rename = "curseForgeProjectId")]
    pub cf_project_id: u64,
    #[serde(default, rename = "curseForgeFileId")]
    pub cf_file_id: u64,
}

fn default_true() -> bool {
    true
}

// ---- public API functions ---------------------------------------------------

/// Fetch the full ATLauncher catalogue (public + private packs).
/// `GET {base}/v1/packs/full/all`.
pub async fn all_packs(base: &str) -> Result<Vec<AtlPack>, Error> {
    let url = format!("{base}/v1/packs/full/all");
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
    let r: AtlPacksResp = serde_json::from_slice(&resp.body).map_err(|e| Error::ModsDecode {
        platform: "atlauncher".into(),
        details: e.to_string(),
    })?;
    Ok(r.data)
}

/// Fetch the `Configs.json` install manifest for a pack version.
/// `cdn_base` is injectable for tests; production passes `nodecdn_base()`.
pub async fn configs(cdn_base: &str, safe_name: &str, version: &str) -> Result<AtlConfigs, Error> {
    let url = format!("{cdn_base}/packs/{safe_name}/versions/{version}/Configs.json");
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
        platform: "atlauncher".into(),
        details: e.to_string(),
    })
}

/// Production nodecdn base.
pub fn nodecdn_base() -> &'static str {
    NODECDN_BASE
}

// ---- tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::test_env_lock()
    }

    #[tokio::test]
    async fn all_packs_parses_catalogue() {
        let _g = test_lock();
        let s = MockServer::start().await;
        let body = serde_json::json!({
            "error": false, "code": 200, "message": null,
            "data": [{
                "id": 8, "name": "Resonant Rise", "safeName": "ResonantRise",
                "type": "public", "description": "desc", "websiteURL": "https://x",
                "versions": [{ "version": "5.0.0-pre.3", "minecraft": "1.12.2", "published": 1594588700 }]
            }, {
                "id": 9, "name": "Secret", "safeName": "Secret", "type": "private",
                "description": "", "versions": []
            }]
        });
        Mock::given(method("GET"))
            .and(path("/v1/packs/full/all"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&s)
            .await;
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let packs = all_packs(&s.uri()).await.unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert_eq!(packs.len(), 2);
        assert_eq!(packs[0].safe_name, "ResonantRise");
        assert_eq!(packs[0].pack_type, "public");
        assert_eq!(packs[0].versions[0].minecraft, "1.12.2");
    }

    #[tokio::test]
    async fn configs_parses_loader_and_mods() {
        let _g = test_lock();
        let s = MockServer::start().await;
        let body = serde_json::json!({
            "version": "5.0.0-pre.3", "minecraft": "1.12.2",
            "loader": { "type": "forge", "metadata": { "version": "14.23.5.2847" } },
            "mods": [
                { "name": "JEI", "file": "jei.jar", "url": "https://download.nodecdn.net/x/jei.jar",
                  "md5": "0123abcd", "filesize": 512000.0, "type": "mods", "download": "server",
                  "client": true, "optional": false },
                { "name": "CfMod", "file": "cf.jar", "type": "mods", "download": "direct",
                  "curseForgeProjectId": 238222, "curseForgeFileId": 4499899 }
            ]
        });
        Mock::given(method("GET"))
            .and(path(
                "/packs/ResonantRise/versions/5.0.0-pre.3/Configs.json",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&s)
            .await;
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let c = configs(&s.uri(), "ResonantRise", "5.0.0-pre.3")
            .await
            .unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert_eq!(c.loader.as_ref().unwrap().loader_type, "forge");
        assert_eq!(
            c.loader
                .as_ref()
                .unwrap()
                .metadata
                .as_ref()
                .unwrap()
                .version,
            "14.23.5.2847"
        );
        assert_eq!(c.mods.len(), 2);
        assert_eq!(c.mods[0].md5, "0123abcd");
        assert_eq!(c.mods[1].cf_project_id, 238222);
    }
}
