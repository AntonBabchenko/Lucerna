use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::Result;
use crate::instances::import::model::{scan_content, ForeignInstance};
use crate::instances::import::readers::raw_minecraft::{detect_mc_version_hint, is_version_like};
use crate::instances::import::readers::LauncherReader;
use crate::instances::schema::{ForeignLauncher, LoaderKind};

/// The official / TLauncher launcher profile model: one `.minecraft` with a
/// `launcher_profiles.json`; modded builds live in `versions/<name>/` as
/// standalone game dirs (own mods/saves/config). One reader serves both —
/// the source is tagged `Tlauncher` or `MojangLauncher` by marker files.
pub struct ProfileReader;

/// Minimal view of a `versions/<name>/<name>.json`.
#[derive(Deserialize, Default)]
struct VersionJson {
    #[serde(default)]
    id: Option<String>,
    #[serde(rename = "inheritsFrom", default)]
    inherits_from: Option<String>,
    #[serde(rename = "mainClass", default)]
    main_class: Option<String>,
    #[serde(default)]
    libraries: Vec<VersionLib>,
}

#[derive(Deserialize, Default)]
struct VersionLib {
    #[serde(default)]
    name: Option<String>,
}

/// Parse `game_dir/<dirname>.json` when present (a `versions/<name>` profile
/// dir). The shared `.minecraft` has no such file -> `None`.
fn read_version_json(game_dir: &Path) -> Option<VersionJson> {
    let dirname = game_dir.file_name()?.to_str()?;
    let raw = std::fs::read_to_string(game_dir.join(format!("{dirname}.json"))).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Minecraft version, in priority order: version JSON `inheritsFrom`, then
/// `id` (when version-like), then the `versions/`-folder hint at the
/// enclosing `.minecraft`, then `""` (the user picks in the wizard).
fn resolve_mc_version(minecraft_root: &Path, vj: Option<&VersionJson>) -> String {
    if let Some(vj) = vj {
        if let Some(inh) = vj.inherits_from.as_deref() {
            if is_version_like(inh) {
                return inh.to_string();
            }
        }
        if let Some(id) = vj.id.as_deref() {
            if is_version_like(id) {
                return id.to_string();
            }
        }
    }
    detect_mc_version_hint(minecraft_root).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_prefers_inherits_from() {
        let vj = VersionJson {
            id: Some("test".into()),
            inherits_from: Some("1.20.1".into()),
            ..Default::default()
        };
        assert_eq!(resolve_mc_version(Path::new("/nope"), Some(&vj)), "1.20.1");
    }

    #[test]
    fn version_falls_back_to_id_when_version_like() {
        let vj = VersionJson {
            id: Some("1.20.4".into()),
            inherits_from: None,
            ..Default::default()
        };
        assert_eq!(resolve_mc_version(Path::new("/nope"), Some(&vj)), "1.20.4");
    }

    #[test]
    fn version_uses_versions_hint_when_json_unhelpful() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("versions/1.20.1")).unwrap();
        let vj = VersionJson {
            id: Some("test".into()),
            ..Default::default()
        };
        assert_eq!(resolve_mc_version(root, Some(&vj)), "1.20.1");
    }

    #[test]
    fn version_empty_when_nothing_resolves() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(resolve_mc_version(tmp.path(), None), "");
    }
}
