use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::instances::import::model::{scan_content, ForeignInstance, KnownMod};
use crate::instances::import::readers::LauncherReader;
use crate::instances::schema::{ForeignLauncher, LoaderKind};
use crate::mods::platform::ModSource;

/// The CurseForge desktop app. One `minecraftinstance.json` per instance,
/// game content directly in the instance dir.
pub struct CurseforgeAppReader;

#[derive(Deserialize)]
struct MinecraftInstance {
    #[serde(default)]
    name: Option<String>,
    #[serde(rename = "gameVersion", default)]
    game_version: Option<String>,
    #[serde(rename = "baseModLoader", default)]
    base_mod_loader: Option<BaseModLoader>,
    #[serde(rename = "installedAddons", default)]
    installed_addons: Vec<InstalledAddon>,
}

#[derive(Deserialize, Default)]
struct BaseModLoader {
    #[serde(default)]
    name: Option<String>,
    #[serde(rename = "type", default)]
    type_: Option<i64>,
    #[serde(rename = "minecraftVersion", default)]
    minecraft_version: Option<String>,
    #[serde(rename = "forgeVersion", default)]
    forge_version: Option<String>,
}

#[derive(Deserialize)]
struct InstalledAddon {
    #[serde(rename = "addonID", default)]
    addon_id: Option<i64>,
    #[serde(rename = "installedFile", default)]
    installed_file: Option<InstalledFile>,
}

#[derive(Deserialize, Default)]
struct InstalledFile {
    #[serde(default)]
    id: Option<i64>,
    #[serde(rename = "fileName", default)]
    file_name: Option<String>,
}

/// CurseForge `modLoaderType`: 1=Forge, 4=Fabric, 5=Quilt, 6=NeoForge.
/// Types 2 (Cauldron) and 3 (LiteLoader) are historical and unsupported here —
/// they intentionally degrade to Vanilla (the user can correct in the wizard).
fn loader_from_type(t: Option<i64>) -> LoaderKind {
    match t {
        Some(1) => LoaderKind::Forge,
        Some(4) => LoaderKind::Fabric,
        Some(5) => LoaderKind::Quilt,
        Some(6) => LoaderKind::NeoForge,
        _ => LoaderKind::Vanilla,
    }
}

/// Loader version: `forgeVersion` if present, else the part after the first
/// `-` in `name` (`forge-52.1.0` -> `52.1.0`, `fabric-0.16.5` -> `0.16.5`).
fn loader_version(bml: &BaseModLoader) -> Option<String> {
    if let Some(fv) = bml.forge_version.as_deref().filter(|s| !s.is_empty()) {
        return Some(fv.to_string());
    }
    bml.name
        .as_deref()
        .and_then(|n| n.split_once('-').map(|(_, v)| v.to_string()))
        .filter(|s| !s.is_empty())
}

impl LauncherReader for CurseforgeAppReader {
    fn launcher(&self) -> ForeignLauncher {
        ForeignLauncher::CurseforgeApp
    }

    fn default_roots(&self) -> Vec<PathBuf> {
        crate::platform::default_launcher_roots()
            .into_iter()
            .filter(|p| {
                let s = p.to_string_lossy().to_lowercase();
                s.contains("curseforge") && p.ends_with("Instances")
            })
            .collect()
    }

    fn detect(&self, dir: &Path) -> bool {
        dir.join("minecraftinstance.json").is_file()
    }

    fn read(&self, dir: &Path) -> Result<ForeignInstance> {
        let invalid = |d: String| Error::ImportInstanceUnreadable {
            launcher: "curseforge_app".into(),
            details: d,
        };
        let raw = std::fs::read_to_string(dir.join("minecraftinstance.json"))
            .map_err(|e| invalid(format!("minecraftinstance.json: {e}")))?;
        let mi: MinecraftInstance = serde_json::from_str(&raw)
            .map_err(|e| invalid(format!("minecraftinstance.json: {e}")))?;

        let bml = mi.base_mod_loader.unwrap_or_default();
        let mc_version = mi
            .game_version
            .filter(|s| !s.is_empty())
            .or_else(|| bml.minecraft_version.clone())
            .unwrap_or_default();
        let loader = loader_from_type(bml.type_);
        let loader_version = if loader == LoaderKind::Vanilla {
            None
        } else {
            loader_version(&bml)
        };
        let name = mi.name.filter(|s| !s.is_empty()).unwrap_or_else(|| {
            dir.file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
        let known_mods = mi
            .installed_addons
            .into_iter()
            .filter_map(|a| {
                let f = a.installed_file?;
                let filename = f.file_name?;
                Some(KnownMod {
                    filename,
                    source: ModSource::Curseforge,
                    project_id: a.addon_id?.to_string(),
                    version_id: f.id.map(|i| i.to_string()),
                })
            })
            .collect();

        Ok(ForeignInstance {
            source: ForeignLauncher::CurseforgeApp,
            name,
            root: dir.to_path_buf(),
            minecraft_dir: dir.to_path_buf(),
            mc_version,
            loader,
            loader_version,
            max_heap_mb: None,
            extra_jvm_args: None,
            content: scan_content(dir),
            known_mods,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/curseforge_app_forge")
    }

    #[test]
    fn detects_a_cf_instance() {
        assert!(CurseforgeAppReader.detect(&fixture()));
    }

    #[test]
    fn rejects_a_non_cf_dir() {
        assert!(!CurseforgeAppReader.detect(Path::new(env!("CARGO_MANIFEST_DIR"))));
    }

    #[test]
    fn reads_version_loader_and_known_mods() {
        let fi = CurseforgeAppReader.read(&fixture()).unwrap();
        assert_eq!(fi.source, ForeignLauncher::CurseforgeApp);
        assert_eq!(fi.name, "My CF Pack");
        assert_eq!(fi.mc_version, "1.21.1");
        assert_eq!(fi.loader, LoaderKind::Forge);
        assert_eq!(fi.loader_version.as_deref(), Some("52.1.0"));
        let km = &fi.known_mods;
        assert_eq!(km.len(), 1);
        assert_eq!(km[0].filename, "jei-1.21.1.jar");
        assert_eq!(km[0].source, ModSource::Curseforge);
        assert_eq!(km[0].project_id, "238222");
        assert_eq!(km[0].version_id.as_deref(), Some("5500001"));
    }

    #[test]
    fn maps_loader_type_for_fabric_quilt_neoforge() {
        assert_eq!(loader_from_type(Some(4)), LoaderKind::Fabric);
        assert_eq!(loader_from_type(Some(5)), LoaderKind::Quilt);
        assert_eq!(loader_from_type(Some(6)), LoaderKind::NeoForge);
        // Unmapped (Cauldron/LiteLoader/absent) degrade to Vanilla.
        assert_eq!(loader_from_type(Some(2)), LoaderKind::Vanilla);
        assert_eq!(loader_from_type(None), LoaderKind::Vanilla);
    }

    #[test]
    fn loader_version_falls_back_to_name_split() {
        let bml = BaseModLoader {
            name: Some("fabric-0.16.5".into()),
            forge_version: None,
            ..Default::default()
        };
        assert_eq!(loader_version(&bml).as_deref(), Some("0.16.5"));
    }
}
