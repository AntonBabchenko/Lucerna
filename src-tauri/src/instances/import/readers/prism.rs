use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::instances::import::model::{scan_content, ForeignInstance};
use crate::instances::import::readers::LauncherReader;
use crate::instances::schema::{ForeignLauncher, LoaderKind};

pub struct PrismReader;

#[derive(Deserialize)]
struct MmcPack {
    components: Vec<MmcComponent>,
}
#[derive(Deserialize)]
struct MmcComponent {
    uid: String,
    #[serde(default)]
    version: Option<String>,
}

fn loader_for_uid(uid: &str) -> Option<LoaderKind> {
    match uid {
        "net.fabricmc.fabric-loader" => Some(LoaderKind::Fabric),
        "org.quiltmc.quilt-loader" => Some(LoaderKind::Quilt),
        "net.minecraftforge" => Some(LoaderKind::Forge),
        "net.neoforged" => Some(LoaderKind::NeoForge),
        _ => None,
    }
}

/// Read a single key from a Prism `instance.cfg` (flat `key=value` INI;
/// section headers ignored). Returns the first match.
fn cfg_value(cfg: &str, key: &str) -> Option<String> {
    cfg.lines().find_map(|line| {
        let line = line.trim();
        let (k, v) = line.split_once('=')?;
        (k.trim() == key).then(|| v.trim().to_string())
    })
}

impl LauncherReader for PrismReader {
    fn launcher(&self) -> ForeignLauncher {
        ForeignLauncher::Prism
    }

    fn default_roots(&self) -> Vec<PathBuf> {
        crate::platform::default_launcher_roots()
            .into_iter()
            .filter(|p| {
                p.ends_with("instances") && {
                    let s = p.to_string_lossy().to_lowercase();
                    s.contains("prism") || s.contains("multimc") || s.contains("polymc")
                }
            })
            .collect()
    }

    fn detect(&self, dir: &Path) -> bool {
        dir.join("mmc-pack.json").is_file() && dir.join("instance.cfg").is_file()
    }

    fn read(&self, dir: &Path) -> Result<ForeignInstance> {
        let invalid = |d: String| Error::ImportInstanceUnreadable {
            launcher: "prism".into(),
            details: d,
        };

        let cfg = std::fs::read_to_string(dir.join("instance.cfg"))
            .map_err(|e| invalid(format!("instance.cfg: {e}")))?;
        let pack_raw = std::fs::read_to_string(dir.join("mmc-pack.json"))
            .map_err(|e| invalid(format!("mmc-pack.json: {e}")))?;
        let pack: MmcPack =
            serde_json::from_str(&pack_raw).map_err(|e| invalid(format!("mmc-pack.json: {e}")))?;

        let mc_version = pack
            .components
            .iter()
            .find(|c| c.uid == "net.minecraft")
            .and_then(|c| c.version.clone())
            .ok_or_else(|| invalid("no net.minecraft component".into()))?;

        let (loader, loader_version) = pack
            .components
            .iter()
            .find_map(|c| loader_for_uid(&c.uid).map(|l| (l, c.version.clone())))
            .unwrap_or((LoaderKind::Vanilla, None));

        let name = cfg_value(&cfg, "name").unwrap_or_else(|| {
            dir.file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
        let max_heap_mb = cfg_value(&cfg, "MaxMemAlloc").and_then(|v| v.parse::<u32>().ok());
        let extra_jvm_args = cfg_value(&cfg, "JvmArgs").filter(|s| !s.is_empty());

        let minecraft_dir = dir.join(".minecraft");
        let content = scan_content(&minecraft_dir);

        Ok(ForeignInstance {
            source: ForeignLauncher::Prism,
            name,
            root: dir.to_path_buf(),
            minecraft_dir,
            mc_version,
            loader,
            loader_version,
            max_heap_mb,
            extra_jvm_args,
            content,
            known_mods: vec![],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::schema::LoaderKind;
    use std::path::Path;

    fn fixture(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    #[test]
    fn detects_a_prism_instance() {
        assert!(PrismReader.detect(&fixture("prism_fabric")));
    }

    #[test]
    fn rejects_a_non_prism_dir() {
        assert!(!PrismReader.detect(&fixture("raw_minecraft")));
        assert!(!PrismReader.detect(Path::new(env!("CARGO_MANIFEST_DIR"))));
    }

    #[test]
    fn reads_fabric_instance() {
        let fi = PrismReader.read(&fixture("prism_fabric")).unwrap();
        assert_eq!(fi.name, "Fabulously Optimized");
        assert_eq!(fi.mc_version, "1.20.1");
        assert_eq!(fi.loader, LoaderKind::Fabric);
        assert_eq!(fi.loader_version.as_deref(), Some("0.15.7"));
        assert_eq!(fi.max_heap_mb, Some(6144));
        assert_eq!(fi.extra_jvm_args.as_deref(), Some("-Xmx6144m -XX:+UseG1GC"));
        assert!(fi.content.iter().any(|c| matches!(
            c.category,
            crate::instances::import::model::ContentCategory::Mods
        )));
    }

    #[test]
    fn reads_each_loader_kind() {
        assert_eq!(
            PrismReader.read(&fixture("prism_forge")).unwrap().loader,
            LoaderKind::Forge
        );
        assert_eq!(
            PrismReader.read(&fixture("prism_neoforge")).unwrap().loader,
            LoaderKind::NeoForge
        );
        assert_eq!(
            PrismReader.read(&fixture("prism_quilt")).unwrap().loader,
            LoaderKind::Quilt
        );
    }
}
