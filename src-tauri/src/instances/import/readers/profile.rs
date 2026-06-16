use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::Result;
use crate::instances::import::model::{scan_content, ForeignInstance};
use crate::instances::import::readers::loader_sniff::sniff_loader_from_mods;
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

/// Like `resolve_mc_version` but, when the JSON's own fields don't yield a
/// version, mine the MC version from a loader library coordinate
/// (`net.minecraftforge:forge:1.21.1-52.1.0` → `1.21.1`).
fn resolve_mc_version_with_libs(minecraft_root: &Path, vj: Option<&VersionJson>) -> String {
    let base = resolve_mc_version(minecraft_root, vj);
    if !base.is_empty() {
        return base;
    }
    if let Some(vj) = vj {
        for lib in &vj.libraries {
            if let Some(name) = lib.name.as_deref() {
                if let Some(mc) = mc_version_from_lib_coord(name) {
                    return mc;
                }
            }
        }
    }
    String::new()
}

/// MC version embedded in a loader library coordinate
/// (`group:artifact:<mc>-<loader>`). Returns the version-like prefix before
/// the first `-` in the coordinate's version segment.
fn mc_version_from_lib_coord(coord: &str) -> Option<String> {
    let version = coord.splitn(3, ':').nth(2)?;
    // Only compound loader coords (`<mc>-<loader>`) carry the MC version. A
    // plain dependency version (`com.mojang:logging:1.2.1`, no `-`) must NOT
    // be mistaken for it, even though it is version-like.
    if !version.contains('-') {
        return None;
    }
    let head = version.split('-').next()?;
    is_version_like(head).then(|| head.to_string())
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

/// The enclosing `.minecraft` for a game dir. If the dir itself holds a
/// `launcher_profiles.json` it IS the root (shared-dir case); otherwise it
/// is a `versions/<name>` dir and the root is two levels up.
fn minecraft_root_of(game_dir: &Path) -> PathBuf {
    if game_dir.join("launcher_profiles.json").is_file() {
        return game_dir.to_path_buf();
    }
    game_dir
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| game_dir.to_path_buf())
}

/// `Tlauncher` when a TLauncher marker file sits at the `.minecraft` root,
/// else the official `MojangLauncher`.
fn source_for_root(minecraft_root: &Path) -> ForeignLauncher {
    let marker = ["TlauncherProfiles.json", "TLauncherAdditional.json"]
        .iter()
        .any(|m| minecraft_root.join(m).is_file());
    if marker {
        ForeignLauncher::Tlauncher
    } else {
        ForeignLauncher::MojangLauncher
    }
}

/// True iff `dir` has any importable content (mods / saves / resourcepacks /
/// shaderpacks). `config`/`options.txt`-only installs are intentionally
/// excluded — they would clutter the list with essentially-empty folders.
fn has_real_content(dir: &Path) -> bool {
    use crate::instances::import::model::ContentCategory;
    scan_content(dir).iter().any(|c| {
        matches!(
            c.category,
            ContentCategory::Mods
                | ContentCategory::Saves
                | ContentCategory::ResourcePacks
                | ContentCategory::Shaderpacks
        )
    })
}

/// Locate the single resolvable `versions/<name>/<name>.json` for a shared
/// `.minecraft`: used when there is exactly one version dir carrying a JSON.
/// Returns its parsed JSON. `None` when zero or many candidates exist.
fn sole_version_json(minecraft_root: &Path) -> Option<VersionJson> {
    let mut hit: Option<VersionJson> = None;
    let rd = std::fs::read_dir(minecraft_root.join("versions")).ok()?;
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() {
            if let Some(vj) = read_version_json(&p) {
                if hit.is_some() {
                    return None; // ambiguous: more than one
                }
                hit = Some(vj);
            }
        }
    }
    hit
}

/// A sensible instance name: a meaningful folder name (e.g. `test`) used
/// directly; a `.minecraft` falls back to `Minecraft <version>`.
fn instance_name_for(game_dir: &Path, mc_version: &str) -> String {
    if let Some(name) = game_dir.file_name().and_then(|s| s.to_str()) {
        if !name.eq_ignore_ascii_case(".minecraft") && !name.is_empty() {
            return name.to_string();
        }
    }
    if mc_version.is_empty() {
        "Minecraft".to_string()
    } else {
        format!("Minecraft {mc_version}")
    }
}

impl LauncherReader for ProfileReader {
    fn launcher(&self) -> ForeignLauncher {
        // Discovery uses `expand_root` + `read`, not `launcher()`; report the
        // primary (official) variant. The per-instance source is set in `read`.
        ForeignLauncher::MojangLauncher
    }

    fn default_roots(&self) -> Vec<PathBuf> {
        crate::platform::default_launcher_roots()
            .into_iter()
            .filter(|p| p.ends_with(".minecraft") || p.ends_with("minecraft"))
            .collect()
    }

    fn detect(&self, dir: &Path) -> bool {
        if scan_content(dir).is_empty() {
            return false;
        }
        let is_shared = dir.join("launcher_profiles.json").is_file();
        let has_version_json = dir
            .file_name()
            .and_then(|s| s.to_str())
            .map(|n| !n.is_empty() && dir.join(format!("{n}.json")).is_file())
            .unwrap_or(false);
        is_shared || has_version_json
    }

    fn read(&self, dir: &Path) -> Result<ForeignInstance> {
        let minecraft_root = minecraft_root_of(dir);
        let vj = read_version_json(dir).or_else(|| {
            // Shared `.minecraft` (no per-dir <name>.json): borrow the sole
            // version JSON under versions/ for version + loader.
            if dir.join("launcher_profiles.json").is_file() {
                sole_version_json(&minecraft_root)
            } else {
                None
            }
        });
        let mc_version = resolve_mc_version_with_libs(&minecraft_root, vj.as_ref());
        let (mut loader, loader_version) = vj
            .as_ref()
            .map(detect_loader)
            .unwrap_or((LoaderKind::Vanilla, None));
        // When the version JSON names no loader, fall back to sniffing the
        // mods folder. The sniffed loader carries no version (resolved
        // downstream from kind + MC). A confident version-JSON loader is left
        // untouched.
        if loader == LoaderKind::Vanilla {
            if let Some(sniffed) = sniff_loader_from_mods(&dir.join("mods")) {
                loader = sniffed;
            }
        }
        let source = source_for_root(&minecraft_root);
        let name = instance_name_for(dir, &mc_version);
        Ok(ForeignInstance {
            source,
            name,
            root: dir.to_path_buf(),
            minecraft_dir: dir.to_path_buf(),
            mc_version,
            loader,
            loader_version,
            max_heap_mb: None,
            extra_jvm_args: None,
            content: scan_content(dir),
            known_mods: vec![],
        })
    }

    fn expand_root(&self, root: &Path) -> Vec<ForeignInstance> {
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();

        // The shared `.minecraft` itself — when it carries any importable
        // content directly (worlds-only vanilla as well as the modded
        // TLauncher pattern). Bare/config-only installs stay hidden.
        if has_real_content(root) && self.detect(root) {
            if let Ok(fi) = self.read(root) {
                if seen.insert(fi.minecraft_dir.clone()) {
                    out.push(fi);
                }
            }
        }

        // Each `versions/<name>` that has its own game content.
        if let Ok(rd) = std::fs::read_dir(root.join("versions")) {
            for entry in rd.flatten() {
                let p = entry.path();
                if p.is_dir() && self.detect(&p) {
                    if let Ok(fi) = self.read(&p) {
                        if seen.insert(fi.minecraft_dir.clone()) {
                            out.push(fi);
                        }
                    }
                }
            }
        }
        out
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
        assert_eq!(
            detect_loader(&vj),
            (LoaderKind::Forge, Some("47.2.0".into()))
        );
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
        assert_eq!(
            detect_loader(&vj),
            (LoaderKind::Fabric, Some("0.15.7".into()))
        );
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

    #[test]
    fn root_of_shared_minecraft_is_itself() {
        let tmp = tempfile::tempdir().unwrap();
        let mc = tmp.path().join(".minecraft");
        std::fs::create_dir_all(&mc).unwrap();
        std::fs::write(mc.join("launcher_profiles.json"), "{}").unwrap();
        assert_eq!(minecraft_root_of(&mc), mc);
    }

    #[test]
    fn root_of_versions_dir_is_two_up() {
        let tmp = tempfile::tempdir().unwrap();
        let game = tmp.path().join(".minecraft/versions/test");
        std::fs::create_dir_all(&game).unwrap();
        assert_eq!(minecraft_root_of(&game), tmp.path().join(".minecraft"));
    }

    #[test]
    fn source_is_tlauncher_when_marker_present() {
        let tmp = tempfile::tempdir().unwrap();
        let mc = tmp.path().join(".minecraft");
        std::fs::create_dir_all(&mc).unwrap();
        std::fs::write(mc.join("TlauncherProfiles.json"), "{}").unwrap();
        assert_eq!(source_for_root(&mc), ForeignLauncher::Tlauncher);
    }

    #[test]
    fn source_is_mojang_without_marker() {
        let tmp = tempfile::tempdir().unwrap();
        let mc = tmp.path().join(".minecraft");
        std::fs::create_dir_all(&mc).unwrap();
        assert_eq!(source_for_root(&mc), ForeignLauncher::MojangLauncher);
    }

    use crate::instances::import::model::ContentCategory;

    /// A `versions/<name>` modded game dir: own mods + a version JSON.
    fn versions_game_dir() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let game = tmp.path().join(".minecraft/versions/test");
        std::fs::create_dir_all(game.join("mods")).unwrap();
        std::fs::write(game.join("mods/a.jar"), b"x").unwrap();
        std::fs::write(
            game.join("test.json"),
            r#"{"id":"test","inheritsFrom":"1.20.1","libraries":[{"name":"net.minecraftforge:forge:1.20.1-47.2.0"}]}"#,
        )
        .unwrap();
        tmp
    }

    #[test]
    fn detects_versions_game_dir_with_content() {
        let tmp = versions_game_dir();
        assert!(ProfileReader.detect(&tmp.path().join(".minecraft/versions/test")));
    }

    #[test]
    fn rejects_bare_vanilla_version_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let game = tmp.path().join(".minecraft/versions/1.20.1");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(game.join("1.20.1.json"), r#"{"id":"1.20.1"}"#).unwrap();
        std::fs::write(game.join("1.20.1.jar"), b"x").unwrap();
        assert!(!ProfileReader.detect(&game));
    }

    #[test]
    fn detects_shared_minecraft_with_mods() {
        let tmp = tempfile::tempdir().unwrap();
        let mc = tmp.path().join(".minecraft");
        std::fs::create_dir_all(mc.join("mods")).unwrap();
        std::fs::write(mc.join("mods/a.jar"), b"x").unwrap();
        std::fs::write(mc.join("launcher_profiles.json"), "{}").unwrap();
        assert!(ProfileReader.detect(&mc));
    }

    #[test]
    fn reads_versions_dir_with_forge_and_inherited_version() {
        let tmp = versions_game_dir();
        let fi = ProfileReader
            .read(&tmp.path().join(".minecraft/versions/test"))
            .unwrap();
        assert_eq!(fi.name, "test");
        assert_eq!(fi.mc_version, "1.20.1");
        assert_eq!(fi.loader, LoaderKind::Forge);
        assert_eq!(fi.loader_version.as_deref(), Some("47.2.0"));
        assert_eq!(fi.source, ForeignLauncher::MojangLauncher);
        assert!(fi
            .content
            .iter()
            .any(|c| c.category == ContentCategory::Mods));
    }

    #[test]
    fn expand_root_finds_versions_and_shared_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let mc = tmp.path().join(".minecraft");
        std::fs::create_dir_all(mc.join("mods")).unwrap();
        std::fs::write(mc.join("mods/shared.jar"), b"x").unwrap();
        std::fs::write(mc.join("launcher_profiles.json"), "{}").unwrap();
        let game = mc.join("versions/test");
        std::fs::create_dir_all(game.join("saves/World")).unwrap();
        std::fs::write(game.join("saves/World/level.dat"), b"x").unwrap();
        std::fs::write(game.join("test.json"), r#"{"id":"1.20.1"}"#).unwrap();
        let v = mc.join("versions/1.20.1");
        std::fs::create_dir_all(&v).unwrap();
        std::fs::write(v.join("1.20.1.json"), r#"{"id":"1.20.1"}"#).unwrap();
        std::fs::write(v.join("1.20.1.jar"), b"x").unwrap();

        let found = ProfileReader.expand_root(&mc);
        let names: Vec<_> = found.iter().map(|f| f.name.clone()).collect();
        assert!(names.contains(&"test".to_string()), "got: {names:?}");
        assert!(
            names.iter().any(|n| n.starts_with("Minecraft")),
            "shared dir expected, got: {names:?}"
        );
        assert!(
            !names.contains(&"1.20.1".to_string()),
            "vanilla leaked: {names:?}"
        );
    }

    #[test]
    fn expand_root_surfaces_shared_dir_with_only_saves() {
        // Previously skipped (only mods triggered import); now worlds-only
        // installs must be detected too (the whole point of Task B1).
        let tmp = tempfile::tempdir().unwrap();
        let mc = tmp.path().join(".minecraft");
        std::fs::create_dir_all(mc.join("saves/New World")).unwrap();
        std::fs::write(mc.join("saves/New World/level.dat"), b"x").unwrap();
        std::fs::write(mc.join("launcher_profiles.json"), "{}").unwrap();
        let v = mc.join("versions/26.1.2");
        std::fs::create_dir_all(&v).unwrap();
        std::fs::write(v.join("26.1.2.json"), r#"{"id":"26.1.2"}"#).unwrap();
        std::fs::write(v.join("26.1.2.jar"), b"x").unwrap();

        let found = ProfileReader.expand_root(&mc);
        let shared = found
            .iter()
            .find(|f| f.minecraft_dir == mc)
            .expect("worlds-only shared dir expected");
        assert!(shared.name.starts_with("Minecraft"));
        // Version resolves from the sole versions/<v>/<v>.json (id 26.1.2).
        assert_eq!(shared.mc_version, "26.1.2");
    }

    #[test]
    fn expand_root_skips_shared_dir_with_no_real_content() {
        // A shared dir with only config/options.txt (no mods, saves, RPs, or
        // shaders) must remain hidden — it's effectively a bare install.
        let tmp = tempfile::tempdir().unwrap();
        let mc = tmp.path().join(".minecraft");
        std::fs::create_dir_all(mc.join("config")).unwrap();
        std::fs::write(mc.join("config/some.cfg"), b"x").unwrap();
        std::fs::write(mc.join("launcher_profiles.json"), "{}").unwrap();

        assert!(ProfileReader.expand_root(&mc).is_empty());
    }

    #[test]
    fn expand_root_includes_shared_dir_with_only_a_world() {
        // A worlds-only TLauncher/official .minecraft (no mods) must surface.
        let tmp = tempfile::tempdir().unwrap();
        let mc = tmp.path().join(".minecraft");
        std::fs::create_dir_all(mc.join("saves/World")).unwrap();
        std::fs::write(mc.join("saves/World/level.dat"), b"x").unwrap();
        std::fs::write(mc.join("launcher_profiles.json"), "{}").unwrap();

        let found = ProfileReader.expand_root(&mc);
        assert!(
            found.iter().any(|f| f.name.starts_with("Minecraft")),
            "worlds-only shared dir expected, got: {:?}",
            found.iter().map(|f| &f.name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn shared_dir_resolves_version_and_loader_from_single_version_json() {
        // TLauncher: version dir name is not version-like and the JSON has no
        // inheritsFrom; MC version comes from the forge library coordinate.
        let tmp = tempfile::tempdir().unwrap();
        let mc = tmp.path().join(".minecraft");
        std::fs::create_dir_all(mc.join("mods")).unwrap();
        std::fs::write(mc.join("mods/a.jar"), b"x").unwrap();
        std::fs::write(mc.join("launcher_profiles.json"), "{}").unwrap();
        let v = mc.join("versions/Forge 1.21.1");
        std::fs::create_dir_all(&v).unwrap();
        std::fs::write(
            v.join("Forge 1.21.1.json"),
            r#"{"id":"Forge 1.21.1","mainClass":"net.minecraftforge.bootstrap.ForgeBootstrap","libraries":[{"name":"net.minecraftforge:forge:1.21.1-52.1.0"}]}"#,
        )
        .unwrap();

        let found = ProfileReader.expand_root(&mc);
        let shared = found
            .iter()
            .find(|f| f.minecraft_dir == mc)
            .expect("shared dir present");
        assert_eq!(shared.mc_version, "1.21.1");
        assert_eq!(shared.loader, LoaderKind::Forge);
    }

    #[test]
    fn read_falls_back_to_mods_sniff_when_version_json_lacks_loader() {
        use std::io::{Cursor, Write};
        use zip::write::SimpleFileOptions;
        fn fabric_jar() -> Vec<u8> {
            let mut buf = Vec::new();
            {
                let mut w = zip::ZipWriter::new(Cursor::new(&mut buf));
                w.start_file("fabric.mod.json", SimpleFileOptions::default())
                    .unwrap();
                w.write_all(br#"{"id":"sodium","name":"Sodium"}"#).unwrap();
                w.finish().unwrap();
            }
            buf
        }
        let tmp = tempfile::tempdir().unwrap();
        let game = tmp.path().join(".minecraft/versions/test");
        std::fs::create_dir_all(game.join("mods")).unwrap();
        std::fs::write(game.join("mods/sodium.jar"), fabric_jar()).unwrap();
        // version JSON inherits a version but names NO loader (lib/mainClass).
        std::fs::write(
            game.join("test.json"),
            r#"{"id":"test","inheritsFrom":"1.20.1"}"#,
        )
        .unwrap();
        let fi = ProfileReader.read(&game).unwrap();
        assert_eq!(fi.mc_version, "1.20.1");
        assert_eq!(fi.loader, LoaderKind::Fabric);
        assert_eq!(fi.loader_version, None);
    }
}
