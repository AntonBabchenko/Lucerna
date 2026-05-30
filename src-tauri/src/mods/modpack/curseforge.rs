//! Parser for CurseForge modpacks (zip with `manifest.json`).
//! Manifest format: https://docs.curseforge.com/?http#tocS_File

use std::io::{Cursor, Read};

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

pub async fn parse(bytes: &[u8], base_url: &str) -> Result<ModpackSummary, Error> {
    let mut zip =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| Error::ModpackInvalidArchive {
            details: e.to_string(),
        })?;

    let manifest_bytes = {
        let mut f = zip
            .by_name("manifest.json")
            .map_err(|_| Error::ModpackFormatUnknown)?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)
            .map_err(|e| Error::ModpackInvalidArchive {
                details: e.to_string(),
            })?;
        buf
    };
    let m: CfManifest =
        serde_json::from_slice(&manifest_bytes).map_err(|e| Error::ModpackManifestInvalid {
            format: "curseforge".into(),
            details: e.to_string(),
        })?;

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

    let primary = m.minecraft.mod_loaders.iter().find(|l| l.primary).ok_or(
        Error::ModpackManifestInvalid {
            format: "curseforge".into(),
            details: "no primary modLoader".into(),
        },
    )?;
    let (loader, loader_version) = parse_cf_loader_id(&primary.id)?;

    // Bulk-resolve every fileID.
    let key = keyring::get()?.ok_or(Error::ModsPlatformAuth {
        kind: crate::error::ModsAuthKind::Missing,
    })?;
    let url = format!("{}/v1/mods/files", base_url);
    let body = serde_json::json!({
        "fileIds": m.files.iter().map(|f| f.file_id).collect::<Vec<_>>()
    });
    // `request::post` takes a raw byte body — serialise the JSON here and
    // set content-type explicitly (reqwest's `.json()` did both).
    // unreachable: serialising a `serde_json::Value` to bytes is infallible
    // (no non-string map keys, no IO). Per CLAUDE.md `.unwrap()` rule.
    let body_bytes = serde_json::to_vec(&body).unwrap();
    let resp = crate::network::request::post(
        &url,
        &[
            ("x-api-key", key.as_str()),
            ("content-type", "application/json"),
        ],
        &body_bytes,
        "modpacks",
    )
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
    let bulk: CfBulkResp = serde_json::from_slice(&resp.body).map_err(|e| Error::ModsDecode {
        platform: "curseforge".into(),
        details: e.to_string(),
    })?;

    let mod_ids: Vec<u64> = bulk.data.iter().map(|f| f.mod_id).collect();
    let class_ids = fetch_class_ids(base_url, &key, &mod_ids)
        .await
        .unwrap_or_default();

    let mut files = vec![];
    let mut unresolvable = vec![];

    for entry in &m.files {
        let detail = bulk.data.iter().find(|f| f.id == entry.file_id).ok_or(
            Error::ModpackManifestInvalid {
                format: "curseforge".into(),
                details: format!("file id {} not resolved by Eternal API", entry.file_id),
            },
        )?;
        let env_client = if entry.required {
            EnvSupport::Required
        } else {
            EnvSupport::Optional
        };
        let url = match &detail.download_url {
            Some(u) => u.clone(),
            None => {
                unresolvable.push(ModpackUnresolvable {
                    reason: UnresolvableReason::DistributionDisabled,
                    mod_name: detail.display_name.clone(),
                    manual_action_url: format!(
                        "https://www.curseforge.com/projects/{}",
                        detail.mod_id
                    ),
                    filename: detail.file_name.clone(),
                    size: detail.file_length as f64,
                    sha1: detail
                        .hashes
                        .iter()
                        .find(|h| h.algo == 1)
                        .map(|h| h.value.to_ascii_lowercase()),
                    project_id: Some(detail.mod_id.to_string()),
                });
                continue;
            }
        };
        let sha1 = detail
            .hashes
            .iter()
            .find(|h| h.algo == 1)
            .map(|h| h.value.to_ascii_lowercase())
            .ok_or(Error::ModpackSha1Unavailable {
                mod_name: detail.display_name.clone(),
            })?;
        files.push(ModpackFile {
            project_id: detail.mod_id.to_string(),
            version_id: detail.id.to_string(),
            name: detail.display_name.clone(),
            filename: detail.file_name.clone(),
            install_path: format!(
                "{}/{}",
                install_dir(class_ids.get(&detail.mod_id).copied()),
                detail.file_name
            ),
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
    let has_overrides = entries
        .iter()
        .any(|n| n.starts_with("overrides/") && n != "overrides/");
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

/// CurseForge `classId` → the `.minecraft` subdirectory the file
/// installs into. 12 = Resource Packs, 6552 = Shaders; everything else
/// (mods, unknown, or a failed class lookup) → `mods/`.
fn install_dir(class_id: Option<u32>) -> &'static str {
    match class_id {
        Some(12) => "resourcepacks",
        Some(6552) => "shaderpacks",
        _ => "mods",
    }
}

/// Best-effort bulk lookup of project `classId`s via `POST /v1/mods`.
/// A failure resolves to an empty map at the call site — every file
/// then defaults to `mods/` (the pre-fix behaviour), so a class-lookup
/// hiccup never aborts an import.
async fn fetch_class_ids(
    base_url: &str,
    key: &str,
    mod_ids: &[u64],
) -> Result<std::collections::HashMap<u64, u32>, Error> {
    if mod_ids.is_empty() {
        return Ok(Default::default());
    }
    #[derive(Deserialize)]
    struct ModsResp {
        data: Vec<ModInfo>,
    }
    #[derive(Deserialize)]
    struct ModInfo {
        id: u64,
        #[serde(rename = "classId")]
        class_id: Option<u32>,
    }
    let url = format!("{}/v1/mods", base_url);
    let body = serde_json::json!({ "modIds": mod_ids });
    // unreachable: serialising a `serde_json::Value` to bytes is infallible.
    let body_bytes = serde_json::to_vec(&body).unwrap();
    let resp = crate::network::request::post(
        &url,
        &[("x-api-key", key), ("content-type", "application/json")],
        &body_bytes,
        "modpacks",
    )
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
    let parsed: ModsResp = serde_json::from_slice(&resp.body).map_err(|e| Error::ModsDecode {
        platform: "curseforge".into(),
        details: e.to_string(),
    })?;
    Ok(parsed
        .data
        .into_iter()
        .filter_map(|m| m.class_id.map(|c| (m.id, c)))
        .collect())
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
    Err(Error::ModpackUnsupportedLoader {
        format: "curseforge".into(),
        loader_id: id.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    use zip::write::SimpleFileOptions;

    // The CurseForge API key and LUCERNA_EXTRA_ALLOWED_HOSTS env var are
    // both process-global. Use the crate-wide env-lock so these tests don't
    // race with wiremock tests in other modules.
    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::test_env_lock()
    }

    fn make_cf_zip(manifest: &str) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(Cursor::new(&mut buf));
            w.start_file("manifest.json", SimpleFileOptions::default())
                .unwrap();
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
        }"#
        .into()
    }

    fn install_test_key() {
        let _ = keyring::set("test-key");
    }
    fn clear_test_key() {
        let _ = keyring::clear();
    }

    #[tokio::test]
    async fn parses_minimal_cf_pack() {
        let _g = test_lock();
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
        Mock::given(method("POST"))
            .and(path("/v1/mods/files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(resp))
            .mount(&s)
            .await;

        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let zip = make_cf_zip(&sample_manifest());
        let r = parse(&zip, &s.uri()).await.unwrap();
        clear_test_key();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
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
        let _g = test_lock();
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
        Mock::given(method("POST"))
            .and(path("/v1/mods/files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(resp))
            .mount(&s)
            .await;

        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let zip = make_cf_zip(&sample_manifest());
        let r = parse(&zip, &s.uri()).await.unwrap();
        clear_test_key();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert_eq!(r.files.len(), 1);
        assert_eq!(r.unresolvable.len(), 1);
        let u = &r.unresolvable[0];
        assert!(matches!(u.reason, UnresolvableReason::DistributionDisabled));
        assert!(u.manual_action_url.contains("238222"));
        assert_eq!(u.filename, "jei.jar");
        assert_eq!(u.size, 999.0);
        assert_eq!(u.sha1.as_deref(), Some("abc"));
        assert_eq!(u.project_id.as_deref(), Some("238222"));
    }

    #[tokio::test]
    async fn distribution_disabled_without_sha1_records_none() {
        let _g = test_lock();
        install_test_key();
        let s = MockServer::start().await;
        let resp = serde_json::json!({
            "data": [
                { "id": 4499899, "modId": 238222, "fileName": "jei.jar",
                  "displayName": "JEI", "fileLength": 10, "downloadUrl": null,
                  "hashes": [{ "value": "deadbeef", "algo": 2 }] },
                { "id": 4567890, "modId": 222880, "fileName": "mod-b.jar",
                  "displayName": "Mod B", "fileLength": 500,
                  "downloadUrl": "https://edge.forgecdn.net/files/4/5/mod-b.jar",
                  "hashes": [{ "value": "def", "algo": 1 }] }
            ]
        });
        Mock::given(method("POST"))
            .and(path("/v1/mods/files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(resp))
            .mount(&s)
            .await;
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let zip = make_cf_zip(&sample_manifest());
        let r = parse(&zip, &s.uri()).await.unwrap();
        clear_test_key();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert_eq!(r.unresolvable.len(), 1);
        assert_eq!(r.unresolvable[0].sha1, None);
    }

    #[tokio::test]
    async fn rejects_manifest_version_other_than_1() {
        let _g = test_lock();
        install_test_key();
        let zip = make_cf_zip(
            &sample_manifest().replace("\"manifestVersion\": 1", "\"manifestVersion\": 2"),
        );
        let r = parse(&zip, "http://unused").await;
        clear_test_key();
        assert!(matches!(
            r,
            Err(Error::ModpackUnsupportedManifestVersion { version: 2, .. })
        ));
    }

    #[tokio::test]
    async fn rejects_unknown_loader_id() {
        let _g = test_lock();
        install_test_key();
        let zip = make_cf_zip(&sample_manifest().replace("forge-47.2.0", "junk-1.0"));
        let r = parse(&zip, "http://unused").await;
        clear_test_key();
        assert!(matches!(r, Err(Error::ModpackUnsupportedLoader { .. })));
    }

    #[tokio::test]
    async fn resource_pack_file_routes_to_resourcepacks_dir() {
        let _g = test_lock();
        install_test_key();
        let s = MockServer::start().await;
        let files = serde_json::json!({
            "data": [
                { "id": 4499899, "modId": 238222, "fileName": "jei-1.20.1.jar",
                  "displayName": "JEI", "fileLength": 999,
                  "downloadUrl": "https://edge.forgecdn.net/files/4/4/jei.jar",
                  "hashes": [{ "value": "abc", "algo": 1 }] },
                { "id": 4567890, "modId": 222880, "fileName": "FancyTextures.zip",
                  "displayName": "Fancy Textures", "fileLength": 500,
                  "downloadUrl": "https://edge.forgecdn.net/files/4/5/ft.zip",
                  "hashes": [{ "value": "def", "algo": 1 }] }
            ]
        });
        let mods = serde_json::json!({
            "data": [
                { "id": 238222, "classId": 6 },
                { "id": 222880, "classId": 12 }
            ]
        });
        Mock::given(method("POST"))
            .and(path("/v1/mods/files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(files))
            .mount(&s)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/mods"))
            .respond_with(ResponseTemplate::new(200).set_body_json(mods))
            .mount(&s)
            .await;
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let zip = make_cf_zip(&sample_manifest());
        let r = parse(&zip, &s.uri()).await.unwrap();
        clear_test_key();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        let jei = r
            .files
            .iter()
            .find(|f| f.filename == "jei-1.20.1.jar")
            .unwrap();
        let tex = r
            .files
            .iter()
            .find(|f| f.filename == "FancyTextures.zip")
            .unwrap();
        assert_eq!(jei.install_path, "mods/jei-1.20.1.jar");
        assert_eq!(tex.install_path, "resourcepacks/FancyTextures.zip");
    }

    #[tokio::test]
    async fn missing_sha1_fails() {
        let _g = test_lock();
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
        Mock::given(method("POST"))
            .and(path("/v1/mods/files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(resp))
            .mount(&s)
            .await;

        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let zip = make_cf_zip(&sample_manifest());
        let r = parse(&zip, &s.uri()).await;
        clear_test_key();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert!(matches!(r, Err(Error::ModpackSha1Unavailable { .. })));
    }
}
