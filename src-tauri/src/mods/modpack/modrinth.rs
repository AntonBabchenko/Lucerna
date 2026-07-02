//! Parser for Modrinth `.mrpack` (zip with `modrinth.index.json`).
//! Format spec: https://support.modrinth.com/en/articles/8802351

use std::io::{Cursor, Read};

use serde::Deserialize;

use crate::error::Error;
use crate::mods::modpack::schema::*;
use crate::mods::platform::{LoaderKind, ModSource};

#[derive(Debug, Deserialize)]
struct MrpackIndex {
    #[serde(rename = "formatVersion")]
    format_version: u32,
    game: String,
    name: String,
    #[serde(rename = "versionId")]
    version_id: String,
    files: Vec<MrpackFile>,
    dependencies: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct MrpackFile {
    path: String,
    hashes: MrpackHashes,
    env: Option<MrpackEnv>,
    downloads: Vec<String>,
    #[serde(rename = "fileSize")]
    file_size: u64,
}

#[derive(Debug, Deserialize)]
struct MrpackHashes {
    sha1: String,
    #[allow(dead_code)]
    #[serde(default)]
    sha512: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MrpackEnv {
    #[serde(default = "default_required")]
    client: String,
    #[allow(dead_code)]
    #[serde(default = "default_required")]
    server: String,
}

fn default_required() -> String {
    "required".into()
}

const ALLOWED_HOST: &str = "cdn.modrinth.com";

/// Upper bound on the `modrinth.index.json` manifest read. This runs during
/// `modpack_inspect` BEFORE the user confirms, so an unbounded read of an
/// attacker-declared entry would let a zip bomb allocate arbitrarily. A real
/// manifest is a few KB even for a large pack; 16 MiB is a generous ceiling.
const MAX_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;

pub fn parse(bytes: &[u8]) -> Result<ModpackSummary, Error> {
    let mut zip =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| Error::ModpackInvalidArchive {
            details: e.to_string(),
        })?;

    let index_bytes = {
        let mut f = zip
            .by_name("modrinth.index.json")
            .map_err(|_| Error::ModpackFormatUnknown)?;
        // Bound the read: the manifest entry's declared size is attacker
        // controlled and this runs pre-confirm. Read at most
        // MAX_MANIFEST_BYTES + 1 and reject a manifest that exceeds the cap.
        let mut buf = Vec::new();
        let read = f
            .by_ref()
            .take(MAX_MANIFEST_BYTES + 1)
            .read_to_end(&mut buf)
            .map_err(|e| Error::ModpackInvalidArchive {
                details: e.to_string(),
            })? as u64;
        if read > MAX_MANIFEST_BYTES {
            return Err(Error::ModpackManifestInvalid {
                format: "modrinth".into(),
                details: format!("modrinth.index.json exceeds {MAX_MANIFEST_BYTES} bytes"),
            });
        }
        buf
    };
    let index: MrpackIndex =
        serde_json::from_slice(&index_bytes).map_err(|e| Error::ModpackManifestInvalid {
            format: "modrinth".into(),
            details: e.to_string(),
        })?;

    if index.format_version != 1 {
        return Err(Error::ModpackUnsupportedManifestVersion {
            format: "modrinth".into(),
            version: index.format_version,
        });
    }
    if index.game != "minecraft" {
        return Err(Error::ModpackManifestInvalid {
            format: "modrinth".into(),
            details: format!("game={} unsupported", index.game),
        });
    }

    let mc_version =
        index
            .dependencies
            .get("minecraft")
            .cloned()
            .ok_or(Error::ModpackManifestInvalid {
                format: "modrinth".into(),
                details: "dependencies.minecraft missing".into(),
            })?;
    let (loader, loader_version) = resolve_loader(&index.dependencies);

    let mut files = vec![];
    let mut unresolvable = vec![];

    for f in &index.files {
        // Reject paths that would escape `.minecraft/` (`..`, absolute,
        // drive letter, backslash). The whole pack is still importable;
        // just this file is flagged. install_asset re-checks at install
        // time (defense in depth).
        if !crate::mods::modpack::path_safety::is_safe_relative_path(&f.path) {
            unresolvable.push(ModpackUnresolvable {
                reason: UnresolvableReason::UnsafePath,
                mod_name: f.path.clone(),
                manual_action_url: String::new(),
                filename: f.path.rsplit('/').next().unwrap_or(&f.path).to_string(),
                size: f.file_size as f64,
                sha1: Some(f.hashes.sha1.to_ascii_lowercase()),
                project_id: None,
            });
            continue;
        }
        let env_client = match f
            .env
            .as_ref()
            .map(|e| e.client.as_str())
            .unwrap_or("required")
        {
            "required" => EnvSupport::Required,
            "optional" => EnvSupport::Optional,
            "unsupported" => continue,
            _ => EnvSupport::Required,
        };
        let url = f
            .downloads
            .first()
            .cloned()
            .ok_or(Error::ModpackManifestInvalid {
                format: "modrinth".into(),
                details: format!("file {} has no downloads", f.path),
            })?;
        let host = url::Url::parse(&url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_ascii_lowercase));
        if host.as_deref() != Some(ALLOWED_HOST) {
            unresolvable.push(ModpackUnresolvable {
                reason: UnresolvableReason::HostNotAllowed,
                mod_name: f.path.clone(),
                manual_action_url: url.clone(),
                filename: f.path.rsplit('/').next().unwrap_or(&f.path).to_string(),
                size: f.file_size as f64,
                sha1: Some(f.hashes.sha1.to_ascii_lowercase()),
                project_id: None,
            });
            continue;
        }
        let (project_id, version_id) = parse_modrinth_cdn_url(&url);
        let filename = f.path.rsplit('/').next().unwrap_or(&f.path).to_string();
        files.push(ModpackFile {
            project_id,
            version_id,
            name: filename.clone(),
            filename,
            install_path: f.path.clone(),
            sha1: f.hashes.sha1.to_ascii_lowercase(),
            md5: None,
            url,
            size: f.file_size as f64,
            env_client,
            source: ModSource::Modrinth,
        });
    }

    let entries: Vec<String> = (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok().map(|f| f.name().to_string()))
        .collect();
    let has_overrides = entries
        .iter()
        .any(|n| n.starts_with("overrides/") && n != "overrides/");
    let has_client_overrides = entries
        .iter()
        .any(|n| n.starts_with("client-overrides/") && n != "client-overrides/");
    let has_saves_in_overrides = entries
        .iter()
        .any(|n| n.starts_with("overrides/saves/") || n.starts_with("client-overrides/saves/"));

    Ok(ModpackSummary {
        format: ModpackFormat::Modrinth,
        name: index.name,
        version: index.version_id,
        game_version: mc_version,
        loader,
        loader_version,
        files,
        unresolvable,
        has_overrides,
        has_client_overrides,
        has_saves_in_overrides,
    })
}

fn resolve_loader(
    deps: &std::collections::BTreeMap<String, String>,
) -> (LoaderKind, Option<String>) {
    for (k, v) in deps {
        match k.as_str() {
            "fabric-loader" => return (LoaderKind::Fabric, Some(v.clone())),
            "quilt-loader" => return (LoaderKind::Quilt, Some(v.clone())),
            "forge" => return (LoaderKind::Forge, Some(v.clone())),
            "neoforge" => return (LoaderKind::NeoForge, Some(v.clone())),
            _ => {}
        }
    }
    (LoaderKind::Vanilla, None)
}

/// `https://cdn.modrinth.com/data/<project_id>/versions/<version_id>/<filename>`.
fn parse_modrinth_cdn_url(url: &str) -> (String, String) {
    let parts: Vec<&str> = url.split('/').collect();
    let data_idx = parts.iter().position(|p| *p == "data");
    let versions_idx = parts.iter().position(|p| *p == "versions");
    match (data_idx, versions_idx) {
        (Some(d), Some(v)) if v > d + 1 && parts.len() > v + 1 => {
            (parts[d + 1].to_string(), parts[v + 1].to_string())
        }
        _ => (String::new(), String::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn make_mrpack(index_json: &str, extra: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(Cursor::new(&mut buf));
            w.start_file("modrinth.index.json", SimpleFileOptions::default())
                .unwrap();
            w.write_all(index_json.as_bytes()).unwrap();
            for (n, b) in extra {
                w.start_file(*n, SimpleFileOptions::default()).unwrap();
                w.write_all(b).unwrap();
            }
            w.finish().unwrap();
        }
        buf
    }

    const SAMPLE_INDEX: &str = r#"{
        "formatVersion": 1,
        "game": "minecraft",
        "name": "Sample Pack",
        "versionId": "1.0.0",
        "dependencies": { "minecraft": "1.20.1", "fabric-loader": "0.15.7" },
        "files": [{
            "path": "mods/sodium.jar",
            "hashes": { "sha1": "abc123", "sha512": "..." },
            "env": { "client": "required", "server": "unsupported" },
            "downloads": ["https://cdn.modrinth.com/data/AANobbMI/versions/abcdef/sodium.jar"],
            "fileSize": 1234567
        }]
    }"#;

    #[test]
    fn parses_minimal_mrpack() {
        let zip = make_mrpack(SAMPLE_INDEX, &[]);
        let s = parse(&zip).unwrap();
        assert_eq!(s.format, ModpackFormat::Modrinth);
        assert_eq!(s.name, "Sample Pack");
        assert_eq!(s.version, "1.0.0");
        assert_eq!(s.game_version, "1.20.1");
        assert_eq!(s.loader, LoaderKind::Fabric);
        assert_eq!(s.loader_version.as_deref(), Some("0.15.7"));
        assert_eq!(s.files.len(), 1);
        assert_eq!(s.files[0].sha1, "abc123");
        assert_eq!(s.files[0].project_id, "AANobbMI");
        assert_eq!(s.files[0].version_id, "abcdef");
    }

    #[test]
    fn rejects_format_version_2() {
        let bad = SAMPLE_INDEX.replace("\"formatVersion\": 1", "\"formatVersion\": 2");
        let zip = make_mrpack(&bad, &[]);
        assert!(matches!(
            parse(&zip),
            Err(Error::ModpackUnsupportedManifestVersion { version: 2, .. })
        ));
    }

    #[test]
    fn rejects_when_dependencies_minecraft_missing() {
        let bad = SAMPLE_INDEX.replace("\"minecraft\": \"1.20.1\", ", "");
        let zip = make_mrpack(&bad, &[]);
        assert!(matches!(
            parse(&zip),
            Err(Error::ModpackManifestInvalid { .. })
        ));
    }

    #[test]
    fn filters_out_unsupported_files() {
        let json = SAMPLE_INDEX.replace("\"client\": \"required\"", "\"client\": \"unsupported\"");
        let zip = make_mrpack(&json, &[]);
        let s = parse(&zip).unwrap();
        assert!(s.files.is_empty());
    }

    #[test]
    fn marks_non_modrinth_host_as_unresolvable() {
        let json = SAMPLE_INDEX.replace("cdn.modrinth.com", "github.com");
        let zip = make_mrpack(&json, &[]);
        let s = parse(&zip).unwrap();
        assert!(s.files.is_empty());
        assert_eq!(s.unresolvable.len(), 1);
        let u = &s.unresolvable[0];
        assert!(matches!(u.reason, UnresolvableReason::HostNotAllowed));
        assert_eq!(u.filename, "sodium.jar");
        assert_eq!(u.size, 1234567.0);
        assert_eq!(u.sha1.as_deref(), Some("abc123"));
        assert_eq!(u.project_id, None);
    }

    #[test]
    fn flags_overrides_and_saves() {
        let zip = make_mrpack(
            SAMPLE_INDEX,
            &[
                ("overrides/config/foo.toml", b"k=v" as &[u8]),
                ("overrides/saves/world/level.dat", b"\x00\x01" as &[u8]),
            ],
        );
        let s = parse(&zip).unwrap();
        assert!(s.has_overrides);
        assert!(!s.has_client_overrides);
        assert!(s.has_saves_in_overrides);
    }

    #[test]
    fn detects_client_overrides_separately() {
        let zip = make_mrpack(
            SAMPLE_INDEX,
            &[("client-overrides/options.txt", b"fov:80" as &[u8])],
        );
        let s = parse(&zip).unwrap();
        assert!(s.has_client_overrides);
    }

    #[test]
    fn keeps_non_mods_path_files() {
        // v0.6.0: non-mods/ files[] entries (resourcepacks, shaders,
        // configs) are installed at their declared install_path.
        let json = SAMPLE_INDEX.replace(
            "\"path\": \"mods/sodium.jar\"",
            "\"path\": \"resourcepacks/RP.zip\"",
        );
        let zip = make_mrpack(&json, &[]);
        let s = parse(&zip).unwrap();
        assert_eq!(s.files.len(), 1);
        assert_eq!(s.files[0].install_path, "resourcepacks/RP.zip");
        assert_eq!(s.files[0].filename, "RP.zip");
    }

    #[test]
    fn unsafe_install_path_lands_in_unresolvable() {
        let json = SAMPLE_INDEX.replace(
            "\"path\": \"mods/sodium.jar\"",
            "\"path\": \"../escape.jar\"",
        );
        let zip = make_mrpack(&json, &[]);
        let s = parse(&zip).unwrap();
        assert!(s.files.is_empty());
        assert_eq!(s.unresolvable.len(), 1);
        assert!(matches!(
            s.unresolvable[0].reason,
            UnresolvableReason::UnsafePath
        ));
    }

    #[test]
    fn vanilla_pack_has_no_loader_dep() {
        let json = r#"{
            "formatVersion": 1,
            "game": "minecraft",
            "name": "Vanilla",
            "versionId": "1.0",
            "dependencies": { "minecraft": "1.20.1" },
            "files": []
        }"#;
        let zip = make_mrpack(json, &[]);
        let s = parse(&zip).unwrap();
        assert_eq!(s.loader, LoaderKind::Vanilla);
        assert_eq!(s.loader_version, None);
    }
}
