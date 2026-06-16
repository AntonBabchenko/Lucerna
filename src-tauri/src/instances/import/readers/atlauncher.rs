use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::instances::import::model::{scan_content, ForeignInstance};
use crate::instances::import::readers::LauncherReader;
use crate::instances::schema::{ForeignLauncher, LoaderKind};

/// The ATLauncher desktop app. One `instance.json` (with a `launcher`
/// object) per instance; game content directly in the instance dir.
pub struct AtlauncherReader;

#[derive(Deserialize)]
struct InstanceJson {
    launcher: LauncherSection,
    #[serde(default)]
    id: Option<String>,
}

#[derive(Deserialize)]
struct LauncherSection {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(rename = "loaderVersion", default)]
    loader_version: Option<LoaderVersion>,
    #[serde(rename = "requiredMemory", default)]
    required_memory: Option<u32>,
}

#[derive(Deserialize, Default)]
struct LoaderVersion {
    #[serde(default)]
    version: Option<String>,
    #[serde(rename = "type", default)]
    type_: Option<String>,
}

/// Map ATLauncher's loader `type` string (case-insensitive). NeoForge is
/// checked before Forge because "neoforge" contains "forge".
fn loader_from_type(t: Option<&str>) -> LoaderKind {
    let t = t.unwrap_or("").to_ascii_lowercase();
    if t.contains("neoforge") {
        LoaderKind::NeoForge
    } else if t.contains("forge") {
        LoaderKind::Forge
    } else if t.contains("fabric") {
        LoaderKind::Fabric
    } else if t.contains("quilt") {
        LoaderKind::Quilt
    } else {
        LoaderKind::Vanilla
    }
}

impl LauncherReader for AtlauncherReader {
    fn launcher(&self) -> ForeignLauncher {
        ForeignLauncher::Atlauncher
    }

    fn default_roots(&self) -> Vec<PathBuf> {
        crate::platform::default_launcher_roots()
            .into_iter()
            .filter(|p| {
                let s = p.to_string_lossy().to_lowercase();
                s.contains("atlauncher") && p.ends_with("instances")
            })
            .collect()
    }

    fn detect(&self, dir: &Path) -> bool {
        let path = dir.join("instance.json");
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return false;
        };
        // ATLauncher's instance.json always has a `launcher` object — this
        // distinguishes it from other launchers' files of the same name.
        serde_json::from_str::<InstanceJson>(&raw).is_ok()
    }

    fn read(&self, dir: &Path) -> Result<ForeignInstance> {
        let invalid = |d: String| Error::ImportInstanceUnreadable {
            launcher: "atlauncher".into(),
            details: d,
        };
        let raw = std::fs::read_to_string(dir.join("instance.json"))
            .map_err(|e| invalid(format!("instance.json: {e}")))?;
        let ij: InstanceJson =
            serde_json::from_str(&raw).map_err(|e| invalid(format!("instance.json: {e}")))?;

        let lv = ij.launcher.loader_version.unwrap_or_default();
        let loader = loader_from_type(lv.type_.as_deref());
        let loader_version = if loader == LoaderKind::Vanilla {
            None
        } else {
            lv.version.filter(|s| !s.is_empty())
        };
        let mc_version = ij
            .launcher
            .version
            .filter(|s| !s.is_empty())
            .or(ij.id)
            .unwrap_or_default();
        let name = ij
            .launcher
            .name
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                dir.file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default()
            });
        let max_heap_mb = ij.launcher.required_memory.filter(|m| *m > 0);

        Ok(ForeignInstance {
            source: ForeignLauncher::Atlauncher,
            name,
            root: dir.to_path_buf(),
            minecraft_dir: dir.to_path_buf(),
            mc_version,
            loader,
            loader_version,
            max_heap_mb,
            extra_jvm_args: None,
            content: scan_content(dir),
            known_mods: vec![],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/atlauncher_neoforge")
    }

    #[test]
    fn detects_an_atlauncher_instance() {
        assert!(AtlauncherReader.detect(&fixture()));
    }

    #[test]
    fn rejects_a_non_atlauncher_dir() {
        assert!(!AtlauncherReader.detect(Path::new(env!("CARGO_MANIFEST_DIR"))));
    }

    #[test]
    fn reads_neoforge_version_loader_and_heap() {
        let fi = AtlauncherReader.read(&fixture()).unwrap();
        assert_eq!(fi.source, ForeignLauncher::Atlauncher);
        assert_eq!(fi.name, "Minecraft 1.20.4 with NeoForge");
        assert_eq!(fi.mc_version, "1.20.4");
        assert_eq!(fi.loader, LoaderKind::NeoForge);
        assert_eq!(fi.loader_version.as_deref(), Some("20.4.251"));
        assert_eq!(fi.max_heap_mb, Some(4096));
    }

    #[test]
    fn maps_loader_type_neoforge_before_forge() {
        assert_eq!(loader_from_type(Some("NeoForge")), LoaderKind::NeoForge);
        assert_eq!(loader_from_type(Some("Forge")), LoaderKind::Forge);
        assert_eq!(loader_from_type(Some("Fabric")), LoaderKind::Fabric);
        assert_eq!(loader_from_type(None), LoaderKind::Vanilla);
    }
}
