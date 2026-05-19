//! Parser for CurseForge modpacks (zip with `manifest.json`).
//! Manifest format: https://docs.curseforge.com/?http#tocS_File

use std::io::{Cursor, Read};

use reqwest::Client;
use serde::Deserialize;

use crate::error::Error;
use crate::mods::curseforge::keyring;
use crate::mods::modpack::schema::*;
use crate::mods::platform::{LoaderKind, ModSource};

#[derive(Debug, Deserialize)]
struct CfManifest {
    minecraft: CfMinecraft,
    #[serde(rename = "manifestType")]
    manifest_type: String,
    #[serde(rename = "manifestVersion")]
    manifest_version: u32,
    name: String,
    version: String,
    files: Vec<CfManifestFile>,
}

#[derive(Debug, Deserialize)]
struct CfMinecraft {
    version: String,
    #[serde(rename = "modLoaders")]
    mod_loaders: Vec<CfModLoader>,
}

#[derive(Debug, Deserialize)]
struct CfModLoader {
    id: String,
    primary: bool,
}

#[derive(Debug, Deserialize)]
struct CfManifestFile {
    #[serde(rename = "fileID")]
    file_id: u64,
    required: bool,
}

#[derive(Debug, Deserialize)]
struct CfBulkResp {
    data: Vec<CfFile>,
}

#[derive(Debug, Deserialize)]
struct CfFile {
    id: u64,
    #[serde(rename = "modId")]
    mod_id: u64,
    #[serde(rename = "fileName")]
    file_name: String,
    #[serde(rename = "downloadUrl")]
    download_url: Option<String>,
    #[serde(rename = "fileLength")]
    file_length: u64,
    #[serde(rename = "displayName")]
    display_name: String,
    hashes: Vec<CfHash>,
}

#[derive(Debug, Deserialize)]
struct CfHash {
    value: String,
    algo: u32, // 1=SHA-1, 2=MD5 per CF docs
}

pub async fn parse(
    bytes: &[u8],
    http: &Client,
    base_url: &str,
) -> Result<ModpackSummary, Error> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| Error::ModpackInvalidArchive { details: e.to_string() })?;

    let manifest_bytes = {
        let mut f = zip.by_name("manifest.json")
            .map_err(|_| Error::ModpackFormatUnknown)?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).map_err(|e| Error::ModpackInvalidArchive { details: e.to_string() })?;
        buf
    };
    let m: CfManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|e| Error::ModpackManifestInvalid { format: "curseforge".into(), details: e.to_string() })?;

    if m.manifest_version != 1 {
        return Err(Error::ModpackUnsupportedManifestVersion {
            format: "curseforge".into(),
            version: m.manifest_version,
        });
    }
    if m.manifest_type != "minecraftModpack" {
        return Err(Error::ModpackManifestInvalid {
            format: "curseforge".into(),
            details: format!("manifestType={} unsupported", m.manifest_type),
        });
    }

    let primary = m.minecraft.mod_loaders.iter().find(|l| l.primary).ok_or(Error::ModpackManifestInvalid {
        format: "curseforge".into(),
        details: "no primary modLoader".into(),
    })?;
    let (loader, loader_version) = parse_cf_loader_id(&primary.id)?;

    // Bulk-resolve every fileID.
    let key = keyring::get()?
        .ok_or(Error::ModsPlatformAuth { kind: crate::error::ModsAuthKind::Missing })?;
    let url = format!("{}/v1/mods/files", base_url);
    let body = serde_json::json!({
        "fileIds": m.files.iter().map(|f| f.file_id).collect::<Vec<_>>()
    });
    let resp = http.post(&url)
        .header("x-api-key", &key)
        .json(&body)
        .send()
        .await
        .map_err(|e| Error::ModsNetwork { url: url.clone(), details: e.to_string() })?;
    if !resp.status().is_success() {
        return Err(Error::ModsNetwork { url, details: format!("HTTP {}", resp.status()) });
    }
    let bulk: CfBulkResp = resp.json().await.map_err(|e| Error::ModsDecode {
        platform: "curseforge".into(),
        details: e.to_string(),
    })?;

    let mut files = vec![];
    let mut unresolvable = vec![];

    for entry in &m.files {
        let detail = bulk.data.iter().find(|f| f.id == entry.file_id).ok_or(Error::ModpackManifestInvalid {
            format: "curseforge".into(),
            details: format!("file id {} not resolved by Eternal API", entry.file_id),
        })?;
        let env_client = if entry.required { EnvSupport::Required } else { EnvSupport::Optional };
        let url = match &detail.download_url {
            Some(u) => u.clone(),
            None => {
                unresolvable.push(ModpackUnresolvable {
                    reason: UnresolvableReason::DistributionDisabled,
                    mod_name: detail.display_name.clone(),
                    manual_action_url: format!("https://www.curseforge.com/projects/{}", detail.mod_id),
                });
                continue;
            }
        };
        let sha1 = detail.hashes.iter().find(|h| h.algo == 1).map(|h| h.value.to_ascii_lowercase())
            .ok_or(Error::ModpackSha1Unavailable { mod_name: detail.display_name.clone() })?;
        files.push(ModpackFile {
            project_id: detail.mod_id.to_string(),
            version_id: detail.id.to_string(),
            name: detail.display_name.clone(),
            filename: detail.file_name.clone(),
            install_path: format!("mods/{}", detail.file_name),
            sha1,
            url,
            size: detail.file_length as f64,
            env_client,
            source: ModSource::Curseforge,
        });
    }

    let entries: Vec<String> = (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok().map(|f| f.name().to_string()))
        .collect();
    let has_overrides = entries.iter().any(|n| n.starts_with("overrides/") && n != "overrides/");
    let has_saves_in_overrides = entries.iter().any(|n| n.starts_with("overrides/saves/"));

    Ok(ModpackSummary {
        format: ModpackFormat::Curseforge,
        name: m.name,
        version: m.version,
        game_version: m.minecraft.version,
        loader,
        loader_version,
        files,
        unresolvable,
        has_overrides,
        has_client_overrides: false,
        has_saves_in_overrides,
    })
}

fn parse_cf_loader_id(id: &str) -> Result<(LoaderKind, Option<String>), Error> {
    if let Some(rest) = id.strip_prefix("forge-") {
        return Ok((LoaderKind::Forge, Some(rest.to_string())));
    }
    if let Some(rest) = id.strip_prefix("neoforge-") {
        return Ok((LoaderKind::NeoForge, Some(rest.to_string())));
    }
    if let Some(rest) = id.strip_prefix("fabric-") {
        return Ok((LoaderKind::Fabric, Some(rest.to_string())));
    }
    if let Some(rest) = id.strip_prefix("quilt-") {
        return Ok((LoaderKind::Quilt, Some(rest.to_string())));
    }
    Err(Error::ModpackUnsupportedLoader { format: "curseforge".into(), loader_id: id.into() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::Mutex;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    use zip::write::SimpleFileOptions;

    // The CurseForge API key lives in a single OS keyring slot shared by
    // every test in this process. Without serialization, one test's
    // `clear_test_key()` races another test's first `keyring::get()` call
    // and yields `ModsPlatformAuth::Missing`. The mutex below funnels every
    // CF parser test through a single critical section so the install/clear
    // pair happens atomically per test.
    static KEYRING_LOCK: Mutex<()> = Mutex::new(());

    fn make_cf_zip(manifest: &str) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(Cursor::new(&mut buf));
            w.start_file("manifest.json", SimpleFileOptions::default()).unwrap();
            w.write_all(manifest.as_bytes()).unwrap();
            w.finish().unwrap();
        }
        buf
    }

    fn sample_manifest() -> String {
        r#"{
            "minecraft": {
                "version": "1.20.1",
                "modLoaders": [{ "id": "forge-47.2.0", "primary": true }]
            },
            "manifestType": "minecraftModpack",
            "manifestVersion": 1,
            "name": "Test Pack",
            "version": "1.0.0",
            "files": [
                { "projectID": 238222, "fileID": 4499899, "required": true },
                { "projectID": 222880, "fileID": 4567890, "required": false }
            ]
        }"#.into()
    }

    fn install_test_key() {
        let _ = keyring::set("test-key");
    }
    fn clear_test_key() {
        let _ = keyring::clear();
    }

    #[tokio::test]
    async fn parses_minimal_cf_pack() {
        let _g = KEYRING_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        install_test_key();
        let s = MockServer::start().await;
        let resp = serde_json::json!({
            "data": [
                {
                    "id": 4499899, "modId": 238222, "fileName": "jei-1.20.1.jar",
                    "displayName": "JEI 1.20.1", "fileLength": 999,
                    "downloadUrl": "https://edge.forgecdn.net/files/4/4/jei.jar",
                    "hashes": [{ "value": "abc", "algo": 1 }]
                },
                {
                    "id": 4567890, "modId": 222880, "fileName": "mod-b.jar",
                    "displayName": "Mod B", "fileLength": 500,
                    "downloadUrl": "https://edge.forgecdn.net/files/4/5/mod-b.jar",
                    "hashes": [{ "value": "def", "algo": 1 }]
                }
            ]
        });
        Mock::given(method("POST")).and(path("/v1/mods/files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(resp))
            .mount(&s).await;

        let zip = make_cf_zip(&sample_manifest());
        let r = parse(&zip, &Client::new(), &s.uri()).await.unwrap();
        clear_test_key();
        assert_eq!(r.format, ModpackFormat::Curseforge);
        assert_eq!(r.loader, LoaderKind::Forge);
        assert_eq!(r.loader_version.as_deref(), Some("47.2.0"));
        assert_eq!(r.files.len(), 2);
        assert_eq!(r.files[0].env_client, EnvSupport::Required);
        assert_eq!(r.files[1].env_client, EnvSupport::Optional);
        assert_eq!(r.files[0].install_path, "mods/jei-1.20.1.jar");
    }

    #[tokio::test]
    async fn distribution_disabled_lands_in_unresolvable() {
        let _g = KEYRING_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        install_test_key();
        let s = MockServer::start().await;
        let resp = serde_json::json!({
            "data": [
                {
                    "id": 4499899, "modId": 238222, "fileName": "jei.jar",
                    "displayName": "JEI", "fileLength": 999, "downloadUrl": null,
                    "hashes": [{ "value": "abc", "algo": 1 }]
                },
                {
                    "id": 4567890, "modId": 222880, "fileName": "mod-b.jar",
                    "displayName": "Mod B", "fileLength": 500,
                    "downloadUrl": "https://edge.forgecdn.net/files/4/5/mod-b.jar",
                    "hashes": [{ "value": "def", "algo": 1 }]
                }
            ]
        });
        Mock::given(method("POST")).and(path("/v1/mods/files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(resp))
            .mount(&s).await;

        let zip = make_cf_zip(&sample_manifest());
        let r = parse(&zip, &Client::new(), &s.uri()).await.unwrap();
        clear_test_key();
        assert_eq!(r.files.len(), 1);
        assert_eq!(r.unresolvable.len(), 1);
        assert!(matches!(r.unresolvable[0].reason, UnresolvableReason::DistributionDisabled));
        assert!(r.unresolvable[0].manual_action_url.contains("238222"));
    }

    #[tokio::test]
    async fn rejects_manifest_version_other_than_1() {
        let _g = KEYRING_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        install_test_key();
        let zip = make_cf_zip(&sample_manifest().replace("\"manifestVersion\": 1", "\"manifestVersion\": 2"));
        let r = parse(&zip, &Client::new(), "http://unused").await;
        clear_test_key();
        assert!(matches!(r, Err(Error::ModpackUnsupportedManifestVersion { version: 2, .. })));
    }

    #[tokio::test]
    async fn rejects_unknown_loader_id() {
        let _g = KEYRING_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        install_test_key();
        let zip = make_cf_zip(&sample_manifest().replace("forge-47.2.0", "junk-1.0"));
        let r = parse(&zip, &Client::new(), "http://unused").await;
        clear_test_key();
        assert!(matches!(r, Err(Error::ModpackUnsupportedLoader { .. })));
    }

    #[tokio::test]
    async fn missing_sha1_fails() {
        let _g = KEYRING_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        install_test_key();
        let s = MockServer::start().await;
        let resp = serde_json::json!({
            "data": [{
                "id": 4499899, "modId": 238222, "fileName": "x.jar",
                "displayName": "X", "fileLength": 1,
                "downloadUrl": "https://edge.forgecdn.net/files/x.jar",
                "hashes": [{ "value": "deadbeef", "algo": 2 }]
            },{
                "id": 4567890, "modId": 222880, "fileName": "y.jar",
                "displayName": "Y", "fileLength": 1,
                "downloadUrl": "https://edge.forgecdn.net/files/y.jar",
                "hashes": [{ "value": "feedface", "algo": 1 }]
            }]
        });
        Mock::given(method("POST")).and(path("/v1/mods/files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(resp))
            .mount(&s).await;

        let zip = make_cf_zip(&sample_manifest());
        let r = parse(&zip, &Client::new(), &s.uri()).await;
        clear_test_key();
        assert!(matches!(r, Err(Error::ModpackSha1Unavailable { .. })));
    }
}
