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

/// Best-effort loader + version from a version JSON. Library coordinates
/// give an exact version; `mainClass` / `id` markers give the kind only.
/// Order matters: NeoForge before Forge ("neoforge" contains "forge").
fn detect_loader(vj: &VersionJson) -> (LoaderKind, Option<String>) {
    for lib in &vj.libraries {
        let Some(name) = lib.name.as_deref() else {
            continue;
        };
        if let Some(found) = loader_from_coord(name) {
            return found;
        }
    }
    let hay = format!(
        "{} {} {}",
        vj.main_class.as_deref().unwrap_or(""),
        vj.id.as_deref().unwrap_or(""),
        vj.inherits_from.as_deref().unwrap_or("")
    )
    .to_lowercase();
    if hay.contains("neoforge") || hay.contains("neoforged") {
        (LoaderKind::NeoForge, None)
    } else if hay.contains("forge") {
        (LoaderKind::Forge, None)
    } else if hay.contains("fabric") {
        (LoaderKind::Fabric, None)
    } else if hay.contains("quilt") {
        (LoaderKind::Quilt, None)
    } else {
        (LoaderKind::Vanilla, None)
    }
}

/// Match a Maven coordinate `group:artifact:version` against known loader
/// libraries. The Forge version is the part after the last `-` (coordinates
/// look like `1.20.1-47.2.0`); other loaders use the version verbatim.
fn loader_from_coord(coord: &str) -> Option<(LoaderKind, Option<String>)> {
    let mut parts = coord.splitn(3, ':');
    let group = parts.next()?;
    let artifact = parts.next()?;
    let version = parts.next().map(str::to_string);
    match (group, artifact) {
        ("net.neoforged", "neoforge") => Some((LoaderKind::NeoForge, version)),
        ("net.minecraftforge", "forge") => {
            let v = version.map(|v| v.rsplit('-').next().unwrap_or(&v).to_string());
            Some((LoaderKind::Forge, v))
        }
        ("net.fabricmc", "fabric-loader") => Some((LoaderKind::Fabric, version)),
        ("org.quiltmc", "quilt-loader") => Some((LoaderKind::Quilt, version)),
        _ => None,
    }
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

    #[test]
    fn loader_forge_from_library_with_version() {
        let vj = VersionJson {
            libraries: vec![VersionLib {
                name: Some("net.minecraftforge:forge:1.20.1-47.2.0".into()),
            }],
            ..Default::default()
        };
        assert_eq!(detect_loader(&vj), (LoaderKind::Forge, Some("47.2.0".into())));
    }

    #[test]
    fn loader_neoforge_from_library() {
        let vj = VersionJson {
            libraries: vec![VersionLib {
                name: Some("net.neoforged:neoforge:21.1.66".into()),
            }],
            ..Default::default()
        };
        assert_eq!(
            detect_loader(&vj),
            (LoaderKind::NeoForge, Some("21.1.66".into()))
        );
    }

    #[test]
    fn loader_fabric_from_library() {
        let vj = VersionJson {
            libraries: vec![VersionLib {
                name: Some("net.fabricmc:fabric-loader:0.15.7".into()),
            }],
            ..Default::default()
        };
        assert_eq!(detect_loader(&vj), (LoaderKind::Fabric, Some("0.15.7".into())));
    }

    #[test]
    fn loader_quilt_from_main_class_without_version() {
        let vj = VersionJson {
            main_class: Some("org.quiltmc.loader.impl.launch.knot.KnotClient".into()),
            ..Default::default()
        };
        assert_eq!(detect_loader(&vj), (LoaderKind::Quilt, None));
    }

    #[test]
    fn loader_vanilla_when_no_marker() {
        let vj = VersionJson {
            main_class: Some("net.minecraft.client.main.Main".into()),
            ..Default::default()
        };
        assert_eq!(detect_loader(&vj), (LoaderKind::Vanilla, None));
    }
}
