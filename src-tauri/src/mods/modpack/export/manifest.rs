//! Pure manifest serialization.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::instances::schema::LoaderKind;
use crate::mods::modpack::export::types::ExportMetadata;

/// One referenced file in a `.mrpack`. `path` is relative to `.minecraft/`
/// (e.g. `mods/sodium.jar`). Hashes are lowercase hex computed from the
/// local jar; `url` is the resolved canonical download.
#[derive(Debug, Clone, PartialEq)]
pub struct MrpackRef {
    pub path: String,
    pub sha1: String,
    pub sha512: String,
    pub url: String,
    pub size: u64,
}

#[derive(Serialize)]
struct MrpackOut {
    #[serde(rename = "formatVersion")]
    format_version: u32,
    game: String,
    #[serde(rename = "versionId")]
    version_id: String,
    name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    summary: String,
    files: Vec<MrpackFileOut>,
    dependencies: BTreeMap<String, String>,
}

#[derive(Serialize)]
struct MrpackFileOut {
    path: String,
    hashes: MrpackHashesOut,
    downloads: Vec<String>,
    #[serde(rename = "fileSize")]
    file_size: u64,
}

#[derive(Serialize)]
struct MrpackHashesOut {
    sha1: String,
    sha512: String,
}

/// Map a loader to its `.mrpack` dependency key. Vanilla -> `None`.
fn mrpack_loader_key(loader: LoaderKind) -> Option<&'static str> {
    match loader {
        LoaderKind::Vanilla => None,
        LoaderKind::Fabric => Some("fabric-loader"),
        LoaderKind::Quilt => Some("quilt-loader"),
        LoaderKind::Forge => Some("forge"),
        LoaderKind::NeoForge => Some("neoforge"),
    }
}

/// Build the pretty-printed `modrinth.index.json`. Pure.
pub fn build_mrpack_index(
    meta: &ExportMetadata,
    mc_version: &str,
    loader: LoaderKind,
    loader_version: Option<&str>,
    files: &[MrpackRef],
) -> Result<String, serde_json::Error> {
    let mut dependencies = BTreeMap::new();
    dependencies.insert("minecraft".to_string(), mc_version.to_string());
    if let (Some(key), Some(ver)) = (mrpack_loader_key(loader), loader_version) {
        dependencies.insert(key.to_string(), ver.to_string());
    }
    let out = MrpackOut {
        format_version: 1,
        game: "minecraft".into(),
        version_id: meta.version.clone(),
        name: meta.name.clone(),
        summary: meta.summary.clone(),
        files: files
            .iter()
            .map(|f| MrpackFileOut {
                path: f.path.clone(),
                hashes: MrpackHashesOut {
                    sha1: f.sha1.clone(),
                    sha512: f.sha512.clone(),
                },
                downloads: vec![f.url.clone()],
                file_size: f.size,
            })
            .collect(),
        dependencies,
    };
    serde_json::to_string_pretty(&out)
}

#[cfg(test)]
mod mrpack_tests {
    use super::*;
    use serde_json::Value;

    fn meta() -> ExportMetadata {
        ExportMetadata {
            name: "My Pack".into(),
            version: "1.0.0".into(),
            author: String::new(),
            summary: String::new(),
        }
    }

    fn sample_ref() -> MrpackRef {
        MrpackRef {
            path: "mods/sodium.jar".into(),
            sha1: "aa".into(),
            sha512: "bb".into(),
            url: "https://cdn.modrinth.com/x/sodium.jar".into(),
            size: 1024,
        }
    }

    #[test]
    fn emits_required_top_level_fields() {
        let json =
            build_mrpack_index(&meta(), "1.21.1", LoaderKind::Fabric, Some("0.16.0"), &[]).unwrap();
        let v: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["formatVersion"], 1);
        assert_eq!(v["game"], "minecraft");
        assert_eq!(v["name"], "My Pack");
        assert_eq!(v["versionId"], "1.0.0");
        assert_eq!(v["dependencies"]["minecraft"], "1.21.1");
        assert_eq!(v["dependencies"]["fabric-loader"], "0.16.0");
    }

    #[test]
    fn omits_summary_when_empty() {
        let json = build_mrpack_index(&meta(), "1.21.1", LoaderKind::Vanilla, None, &[]).unwrap();
        let v: Value = serde_json::from_str(&json).unwrap();
        assert!(v.get("summary").is_none());
    }

    #[test]
    fn vanilla_has_no_loader_dependency() {
        let json = build_mrpack_index(&meta(), "1.21.1", LoaderKind::Vanilla, None, &[]).unwrap();
        let v: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["dependencies"].as_object().unwrap().len(), 1);
    }

    #[test]
    fn neoforge_uses_neoforge_key() {
        let json = build_mrpack_index(&meta(), "1.21.1", LoaderKind::NeoForge, Some("21.1.0"), &[])
            .unwrap();
        let v: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["dependencies"]["neoforge"], "21.1.0");
    }

    #[test]
    fn file_entry_has_hashes_downloads_size() {
        let json = build_mrpack_index(
            &meta(),
            "1.21.1",
            LoaderKind::Fabric,
            Some("0.16.0"),
            &[sample_ref()],
        )
        .unwrap();
        let v: Value = serde_json::from_str(&json).unwrap();
        let f = &v["files"][0];
        assert_eq!(f["path"], "mods/sodium.jar");
        assert_eq!(f["hashes"]["sha1"], "aa");
        assert_eq!(f["hashes"]["sha512"], "bb");
        assert_eq!(f["downloads"][0], "https://cdn.modrinth.com/x/sodium.jar");
        assert_eq!(f["fileSize"], 1024);
    }
}
