//! Best-effort детект (loader, mc_version, loader_version) импортируемого
//! сервера + `can_launch_as_is`. Несработавшее поле → `None`; юзер правит в
//! визарде (Слайс 2b). Никогда не паникует на странном дереве.
//!
//! Which installed loader version the tree runs comes from
//! `servers_runtime::installed_loader` — the rule the launch uses too — and
//! never from folder-name order.

use crate::forge::ForgeFlavor;
use crate::servers_runtime::installed_loader::{
    self, Answer, ArgsOs, FabricKind, ForgeInstall, LaunchEntry,
};
use crate::servers_runtime::schema::ServerCore;
use std::path::Path;

/// Результат детекта. Любое поле может быть `None` (юзер уточнит).
#[derive(Debug, Clone, PartialEq)]
pub struct Detected {
    pub loader: Option<ServerCore>,
    pub mc_version: Option<String>,
    pub loader_version: Option<String>,
}

/// Детект (loader, mc_version, loader_version) по содержимому `root`.
/// Порядок: серверные паки (CF/Modrinth манифест) → NeoForge/Forge (their
/// install as `installed_loader` resolves it: run scripts, args-file
/// folders, a pre-1.17 Forge server jar in the root) → Quilt/Fabric (маркеры;
/// versions from the launcher's own records) → Paper/Purpur (purpur.yml /
/// config/paper-global.yml / version_history.json) → Vanilla (server.jar +
/// version.json). Любая ветка может вернуть частичный результат.
pub fn detect(root: &Path) -> Detected {
    // Server packs declare loader + MC in their manifest — trust that over jar
    // sniffing (the pack's mods aren't even on disk yet for Modrinth) (#10).
    match crate::servers_runtime::import::pack::detect_pack(root) {
        Some(crate::servers_runtime::import::pack::PackKind::Modrinth) => {
            if let Ok(p) = crate::servers_runtime::import::pack::parse_modrinth(root) {
                return Detected {
                    loader: Some(ServerCore::from_loader_kind(p.loader)),
                    mc_version: Some(p.mc_version),
                    loader_version: p.loader_version,
                };
            }
        }
        Some(crate::servers_runtime::import::pack::PackKind::Curseforge) => {
            if let Ok(p) = crate::servers_runtime::import::pack::parse_cf(root) {
                return Detected {
                    loader: Some(ServerCore::from_loader_kind(p.loader)),
                    mc_version: Some(p.mc_version),
                    loader_version: p.loader_version,
                };
            }
        }
        None => {}
    }
    for flavor in [ForgeFlavor::NeoForge, ForgeFlavor::Forge] {
        if let Some(found) = detect_forge_family(root, flavor) {
            return found;
        }
    }
    if quilt_marker(root) {
        return detect_fabric_family(root, ServerCore::Quilt, FabricKind::Quilt);
    }
    if fabric_marker(root) {
        return detect_fabric_family(root, ServerCore::Fabric, FabricKind::Fabric);
    }
    // Bukkit-family cores. Purpur first (a Purpur tree also has Paper's
    // config/, so the more specific marker wins). Checked before the vanilla
    // server.jar fallback: a Paper tree has server.jar too, and labeling it
    // vanilla would make "redownload jar" clobber the Paper jar with a
    // vanilla one. version_history.json is read+parsed once for both arms.
    let vh_current = version_history_current(root);
    let vh_mc = vh_current.as_deref().and_then(mc_from_current_version);
    if root.join("purpur.yml").exists() {
        return Detected {
            loader: Some(ServerCore::Purpur),
            mc_version: vh_mc.or_else(|| mc_from_logs(root)),
            loader_version: None,
        };
    }
    if root.join("config").join("paper-global.yml").exists()
        || vh_current.as_deref().is_some_and(|v| v.contains("Paper"))
    {
        return Detected {
            loader: Some(ServerCore::Paper),
            mc_version: vh_mc.or_else(|| mc_from_logs(root)),
            loader_version: None,
        };
    }
    if root.join("server.jar").exists() || has_vanilla_named_jar(root) {
        // An old vanilla server keeps the version in its jar's name
        // (`minecraft_server.1.12.2.jar`); several such jars cannot be told
        // apart, and then a log from an earlier run cannot either.
        let named = || match installed_loader::vanilla_root_jar_mcs(root) {
            Ok(mcs) => answer_or_logs(Answer::of(mcs), root),
            // The folder could not be listed: which jar is there, and so
            // which version, cannot be told.
            Err(_) => None,
        };
        return Detected {
            loader: Some(ServerCore::Vanilla),
            mc_version: mc_from_server_jar(root).or_else(named),
            loader_version: None,
        };
    }
    Detected {
        loader: None,
        mc_version: mc_from_logs(root),
        loader_version: None,
    }
}

/// `true` если staged-дерево уже запускаемо нашим `build_launch_argv`:
/// V/F — есть `server.jar` и НЕТ отдельного чужого лаунчер-jar (иначе
/// `server.jar` — ванильный, и `-jar server.jar` запустил бы ваниль);
/// Quilt — `quilt::launchable_as_is`: the launcher launch would pick, its game
/// jar and every library its `Class-Path` names are all present;
/// Forge/NeoForge — `installed_loader` resolves exactly one install (the same
/// rule the launch uses), and a pre-1.17 Forge jar has every file its
/// `Class-Path` needs.
pub fn can_launch_as_is(root: &Path, loader: ServerCore) -> bool {
    match loader {
        // Paper/Purpur launch exactly like vanilla (`-jar server.jar`).
        // `detect()` tells them apart via their own markers (purpur.yml,
        // config/paper-global.yml, version_history.json); launchability
        // only needs the jar.
        ServerCore::Vanilla | ServerCore::Paper | ServerCore::Purpur => {
            root.join("server.jar").exists()
        }
        ServerCore::Fabric => {
            root.join("server.jar").exists()
                && !root.join("fabric-server-launch.jar").exists()
                && !root.join("fabric-server-launcher.jar").exists()
                && !root.join("quilt-server-launch.jar").exists()
        }
        // Quilt: the launcher launch itself would pick, plus the game jar and
        // every library it names. A tree whose server.jar is plain vanilla,
        // or that lacks a piece, is reprovisioned instead — it would start
        // Minecraft without Quilt, or crash at boot.
        ServerCore::Quilt => crate::servers_runtime::quilt::launchable_as_is(root),
        ServerCore::Forge | ServerCore::NeoForge => {
            let Some(flavor) = installed_loader::forge_flavor(loader) else {
                return false;
            };
            match installed_loader::resolve_forge_family(root, flavor, ArgsOs::current()) {
                ForgeInstall::Found(LaunchEntry::ArgsFile { .. }) => true,
                ForgeInstall::Found(LaunchEntry::RootJar { file }) => {
                    installed_loader::root_jar_missing(root, &file).is_empty()
                }
                ForgeInstall::Ambiguous { .. } | ForgeInstall::Absent => false,
            }
        }
    }
}

/// Forge or NeoForge, when its `libraries/` folder holds a version or (Forge)
/// a pre-1.17 Forge server jar sits in the root. `None` when there is no
/// trace of it — or an unknown loader when a folder that would hold one (its
/// `libraries/` folder, or for Forge the server folder itself) cannot be
/// listed, since then it cannot be told apart from any other.
fn detect_forge_family(root: &Path, flavor: ForgeFlavor) -> Option<Detected> {
    let install = installed_loader::resolve_forge_family(root, flavor, ArgsOs::current());
    let dirs = installed_loader::version_dirs(root, flavor);
    let has_dirs = matches!(&dirs, Ok(names) if !names.is_empty());
    if install == ForgeInstall::Absent && !has_dirs {
        let unlistable = dirs.is_err()
            || (flavor == ForgeFlavor::Forge && installed_loader::root_jar_names(root).is_err());
        return unlistable.then(|| unknown_loader(root));
    }
    let core = match flavor {
        ForgeFlavor::Forge => ServerCore::Forge,
        ForgeFlavor::NeoForge => ServerCore::NeoForge,
    };
    let (mc_version, loader_version) = match (&install, dirs.as_deref()) {
        (ForgeInstall::Found(entry), _) => live_versions(root, flavor, entry),
        // Nothing launchable and exactly one version folder: the best
        // evidence there is, and the version a reprovision would install.
        (ForgeInstall::Absent, Ok([only])) => dir_versions(root, flavor, only),
        // Several installs (or folders) and nothing names the live one. A log
        // from an earlier run cannot say which either.
        _ => (None, None),
    };
    Some(Detected {
        loader: Some(core),
        mc_version,
        loader_version,
    })
}

/// `(mc, loader version)` of the install the server runs.
fn live_versions(
    root: &Path,
    flavor: ForgeFlavor,
    entry: &LaunchEntry,
) -> (Option<String>, Option<String>) {
    match entry {
        LaunchEntry::ArgsFile { version_dir, .. } => dir_versions(root, flavor, version_dir),
        LaunchEntry::RootJar { file } => {
            let (mc, build) = installed_loader::root_jar_versions(root, file);
            (answer_or_logs(mc, root), build)
        }
    }
}

/// `(mc, loader version)` named by a version folder.
fn dir_versions(root: &Path, flavor: ForgeFlavor, dir: &str) -> (Option<String>, Option<String>) {
    match flavor {
        ForgeFlavor::Forge => match installed_loader::split_forge_raw(dir) {
            Some((mc, build)) => (Some(mc), Some(build)),
            // Not `<mc>-<build>` (a branch build): only a log is left.
            None => (mc_from_logs(root), None),
        },
        ForgeFlavor::NeoForge => (neoforge_mc(root, dir), Some(dir.to_string())),
    }
}

/// Minecraft version of the NeoForge install in `version_dir`: the
/// installer's own `--fml.mcVersion`, then the shared NeoForge numbering rule,
/// then a log from an earlier run.
fn neoforge_mc(root: &Path, version_dir: &str) -> Option<String> {
    let dir = root
        .join("libraries/net/neoforged/neoforge")
        .join(version_dir);
    mc_from_args_files(&dir)
        .or_else(|| neoforge_mc_from_version(version_dir))
        .or_else(|| mc_from_logs(root))
}

/// `--fml.mcVersion <id>` from the args files the NeoForge installer writes
/// into the version dir (`win_args.txt`, then `unix_args.txt`). This is the
/// installer's own answer, so it wins over reading the version number.
fn mc_from_args_files(version_dir: &Path) -> Option<String> {
    ["win_args.txt", "unix_args.txt"]
        .into_iter()
        .find_map(|name| {
            // Absent or unreadable: this file says nothing; the next source may.
            let text = installed_loader::read_text(&version_dir.join(name))
                .ok()
                .flatten()?;
            let mut tokens = text.split_whitespace();
            tokens.find(|t| *t == "--fml.mcVersion")?;
            tokens.next().map(str::to_string)
        })
}

/// The Minecraft version a NeoForge version number is for, by the shared rule
/// in `forge::meta`. A server is what it is whether or not Lucerna offers that
/// build, so a not-offered build's id counts. A shape the rule does not know
/// is `None`, never a guess.
fn neoforge_mc_from_version(v: &str) -> Option<String> {
    use crate::forge::meta::{neoforge_mc_for, NeoForgeMc};
    match neoforge_mc_for(v) {
        NeoForgeMc::Minecraft(id) | NeoForgeMc::NotOffered(id) => Some(id),
        NeoForgeMc::Unrecognized => None,
    }
}

/// Fabric or Quilt, versions from the launcher's own records first.
fn detect_fabric_family(root: &Path, core: ServerCore, kind: FabricKind) -> Detected {
    let versions = installed_loader::fabric_family_versions(root, kind);
    Detected {
        loader: Some(core),
        mc_version: answer_or_logs(versions.mc, root),
        loader_version: versions.loader.known(),
    }
}

/// A resolved answer, or — only when no source had evidence — a log line.
/// Sources that disagree stay unanswered: a log from an earlier run cannot
/// settle which install is live.
fn answer_or_logs(answer: Answer, root: &Path) -> Option<String> {
    match answer {
        Answer::Known(value) => Some(value),
        Answer::CannotTell => None,
        Answer::NoEvidence => mc_from_logs(root),
    }
}

fn unknown_loader(root: &Path) -> Detected {
    Detected {
        loader: None,
        mc_version: mc_from_logs(root),
        loader_version: None,
    }
}

fn fabric_marker(root: &Path) -> bool {
    root.join(".fabric").is_dir()
        || root.join("fabric-server-launch.jar").exists()
        || root.join("fabric-server-launcher.jar").exists()
        || root.join("libraries/net/fabricmc").is_dir()
        // A Fabric server that never started: the bundled launcher is all of it.
        || installed_loader::has_bundled_fabric_launcher(root)
}

fn quilt_marker(root: &Path) -> bool {
    root.join(".quilt").is_dir()
        || root.join("quilt-server-launch.jar").exists()
        || root.join("libraries/org/quiltmc").is_dir()
}

fn has_vanilla_named_jar(root: &Path) -> bool {
    std::fs::read_dir(root)
        .ok()
        .map(|rd| {
            rd.flatten().any(|e| {
                e.file_name()
                    .to_str()
                    .map(|n| n.starts_with("minecraft_server.") && n.ends_with(".jar"))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

/// Read `version.json` (`{"id":"1.20.4"}`) embedded in `server.jar`.
fn mc_from_server_jar(root: &Path) -> Option<String> {
    let jar = root.join("server.jar");
    let file = std::fs::File::open(&jar).ok()?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file)).ok()?;
    let mut entry = zip.by_name("version.json").ok()?;
    let mut s = String::new();
    std::io::Read::read_to_string(&mut entry, &mut s).ok()?;
    let v: serde_json::Value = serde_json::from_str(&s).ok()?;
    v.get("id").and_then(|x| x.as_str()).map(String::from)
}

/// Paper-family `version_history.json`. After an update it holds BOTH
/// `oldVersion` and `currentVersion` (old is serialized first), so substring
/// scanning the raw file would grab the stale entry — parse and use
/// `currentVersion` only.
#[derive(serde::Deserialize)]
struct VersionHistory {
    #[serde(rename = "currentVersion")]
    current_version: Option<String>,
}

/// `currentVersion` from `version_history.json` (written by Paper/Purpur),
/// e.g. "git-Paper-129 (MC: 1.21.4)".
fn version_history_current(root: &Path) -> Option<String> {
    let text = std::fs::read_to_string(root.join("version_history.json")).ok()?;
    let vh: VersionHistory = serde_json::from_str(&text).ok()?;
    vh.current_version
}

/// Extract the "(MC: x)" suffix from a Paper-family version string like
/// "git-Paper-129 (MC: 1.21.4)".
fn mc_from_current_version(current: &str) -> Option<String> {
    let marker = "(MC: ";
    let idx = current.find(marker)? + marker.len();
    let rest = &current[idx..];
    let end = rest.find(')')?;
    let mc = rest[..end].trim();
    (!mc.is_empty()).then(|| mc.to_string())
}

/// Fallback: parse "Starting minecraft server version X" from a log.
fn mc_from_logs(root: &Path) -> Option<String> {
    for rel in ["logs/latest.log", "logs/server-latest.log"] {
        if let Ok(text) = std::fs::read_to_string(root.join(rel)) {
            if let Some(v) = parse_mc_from_log(&text) {
                return Some(v);
            }
        }
    }
    None
}

fn parse_mc_from_log(text: &str) -> Option<String> {
    let marker = "Starting minecraft server version ";
    let idx = text.find(marker)? + marker.len();
    let rest = &text[idx..];
    let end = rest
        .find(|c: char| c == '\n' || c == '\r' || c == ' ')
        .unwrap_or(rest.len());
    let v = rest[..end].trim();
    (!v.is_empty()).then(|| v.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn touch(p: &Path) {
        if let Some(d) = p.parent() {
            fs::create_dir_all(d).unwrap();
        }
        fs::write(p, b"x").unwrap();
    }

    /// The real 1.7.10 server layout the official installer writes: the
    /// universal jar in the root (with its `version.json`), the vanilla jar
    /// on its Class-Path, and no Forge folder under libraries/.
    fn legacy_forge_1_7_10_tree(root: &Path) {
        use crate::servers_runtime::installed_loader::test_jars::{jar, manifest};
        let mf = manifest(&[
            ("Main-Class", "cpw.mods.fml.relauncher.ServerLaunchWrapper"),
            (
                "Class-Path",
                "libraries/net/minecraft/launchwrapper/1.12/launchwrapper-1.12.jar minecraft_server.1.7.10.jar",
            ),
        ]);
        jar(
            &root.join("forge-1.7.10-10.13.4.1614-1.7.10-universal.jar"),
            &[
                ("META-INF/MANIFEST.MF", mf.as_str()),
                (
                    "version.json",
                    r#"{"id":"1.7.10-Forge10.13.4.1614-1.7.10","libraries":[{"name":"net.minecraftforge:forge:1.7.10-10.13.4.1614-1.7.10"}]}"#,
                ),
            ],
        );
        touch(&root.join("minecraft_server.1.7.10.jar"));
        touch(&root.join("libraries/net/minecraft/launchwrapper/1.12/launchwrapper-1.12.jar"));
    }

    #[test]
    fn legacy_forge_1_7_10_tree_is_forge_not_vanilla() {
        let d = tempdir().unwrap();
        legacy_forge_1_7_10_tree(d.path());
        let r = detect(d.path());
        assert_eq!(r.loader, Some(ServerCore::Forge));
        assert_eq!(r.mc_version.as_deref(), Some("1.7.10"));
        assert_eq!(r.loader_version.as_deref(), Some("10.13.4.1614"));
        assert!(can_launch_as_is(d.path(), ServerCore::Forge));
    }

    fn forge_args_dir(root: &Path, dir: &str) {
        for f in ["win_args.txt", "unix_args.txt"] {
            touch(&root.join(format!("libraries/net/minecraftforge/forge/{dir}/{f}")));
        }
    }

    #[test]
    fn in_place_upgraded_forge_follows_its_run_script() {
        let d = tempdir().unwrap();
        forge_args_dir(d.path(), "1.21.9-58.1.0");
        forge_args_dir(d.path(), "1.21.11-61.1.0");
        fs::write(
            d.path().join("run.bat"),
            "java @user_jvm_args.txt @libraries/net/minecraftforge/forge/1.21.11-61.1.0/win_args.txt %*\r\n",
        )
        .unwrap();
        let r = detect(d.path());
        assert_eq!(r.loader, Some(ServerCore::Forge));
        assert_eq!(r.mc_version.as_deref(), Some("1.21.11"));
        assert_eq!(r.loader_version.as_deref(), Some("61.1.0"));
        assert!(can_launch_as_is(d.path(), ServerCore::Forge));
    }

    #[test]
    fn forge_installs_nothing_names_leave_the_versions_empty() {
        let d = tempdir().unwrap();
        forge_args_dir(d.path(), "1.21.9-58.1.0");
        forge_args_dir(d.path(), "1.21.11-61.1.0");
        fs::create_dir_all(d.path().join("logs")).unwrap();
        fs::write(
            d.path().join("logs/latest.log"),
            "[main/INFO]: Starting minecraft server version 1.21.9\n",
        )
        .unwrap();
        let r = detect(d.path());
        assert_eq!(r.loader, Some(ServerCore::Forge));
        // An earlier run's log cannot say which install is live.
        assert_eq!(r.mc_version, None);
        assert_eq!(r.loader_version, None);
        assert!(!can_launch_as_is(d.path(), ServerCore::Forge));
    }

    #[test]
    fn client_layout_forge_folder_reads_from_its_single_folder() {
        // A `.minecraft` holds the Forge library but nothing launchable.
        let d = tempdir().unwrap();
        touch(&d.path().join(
            "libraries/net/minecraftforge/forge/1.7.10-10.13.4.1614-1.7.10/forge-1.7.10-10.13.4.1614-1.7.10.jar",
        ));
        let r = detect(d.path());
        assert_eq!(r.loader, Some(ServerCore::Forge));
        assert_eq!(r.mc_version.as_deref(), Some("1.7.10"));
        assert_eq!(r.loader_version.as_deref(), Some("10.13.4.1614"));
        assert!(!can_launch_as_is(d.path(), ServerCore::Forge));
    }

    #[test]
    fn fabric_26_bundled_server_jar_names_its_versions() {
        use crate::servers_runtime::installed_loader::test_jars::jar;
        let d = tempdir().unwrap();
        jar(
            &d.path().join("server.jar"),
            &[(
                "install.properties",
                "fabric-loader-version=0.19.5\ngame-version=26.1",
            )],
        );
        touch(&d.path().join(".fabric/server/26.1-server.jar"));
        let r = detect(d.path());
        assert_eq!(r.loader, Some(ServerCore::Fabric));
        assert_eq!(r.mc_version.as_deref(), Some("26.1"));
        assert_eq!(r.loader_version.as_deref(), Some("0.19.5"));
    }

    #[test]
    fn fabric_stale_intermediary_folders_leave_the_mc_empty() {
        let d = tempdir().unwrap();
        touch(&d.path().join(".fabric/x"));
        touch(
            &d.path()
                .join("libraries/net/fabricmc/intermediary/1.21.9/x.jar"),
        );
        touch(
            &d.path()
                .join("libraries/net/fabricmc/intermediary/1.21.10/x.jar"),
        );
        fs::create_dir_all(d.path().join("logs")).unwrap();
        fs::write(
            d.path().join("logs/latest.log"),
            "[main/INFO]: Starting minecraft server version 1.21.9\n",
        )
        .unwrap();
        let r = detect(d.path());
        assert_eq!(r.loader, Some(ServerCore::Fabric));
        assert_eq!(r.mc_version, None);
    }

    #[test]
    fn old_vanilla_server_takes_the_mc_from_its_jar_name() {
        let d = tempdir().unwrap();
        touch(&d.path().join("minecraft_server.1.12.2.jar"));
        let r = detect(d.path());
        assert_eq!(r.loader, Some(ServerCore::Vanilla));
        assert_eq!(r.mc_version.as_deref(), Some("1.12.2"));
    }

    #[test]
    fn detects_neoforge_and_mc_from_libraries() {
        // Installers write both OS args files; with both present the live
        // install resolves on every CI OS (not the one-folder fallback).
        let d = tempdir().unwrap();
        for f in ["win_args.txt", "unix_args.txt"] {
            touch(
                &d.path()
                    .join(format!("libraries/net/neoforged/neoforge/20.4.237/{f}")),
            );
        }
        let r = detect(d.path());
        assert_eq!(r.loader, Some(ServerCore::NeoForge));
        assert_eq!(r.loader_version.as_deref(), Some("20.4.237"));
        assert_eq!(r.mc_version.as_deref(), Some("1.20.4"));
        assert!(can_launch_as_is(d.path(), ServerCore::NeoForge));
    }

    fn write(p: &Path, text: &str) {
        if let Some(d) = p.parent() {
            fs::create_dir_all(d).unwrap();
        }
        fs::write(p, text).unwrap();
    }

    #[test]
    fn neoforge_mc_comes_from_the_installer_args_file_first() {
        // The version dir's shape is unknown to the mapping; the installer's
        // own `--fml.mcVersion` still answers.
        let d = tempdir().unwrap();
        write(
            &d.path()
                .join("libraries/net/neoforged/neoforge/27.1.0.0.0/unix_args.txt"),
            "-p libraries/a.jar\n--fml.neoForgeVersion 27.1.0.0.0\n--fml.mcVersion 27.1\n",
        );
        let r = detect(d.path());
        assert_eq!(r.loader, Some(ServerCore::NeoForge));
        assert_eq!(r.mc_version.as_deref(), Some("27.1"));
        assert_eq!(r.loader_version.as_deref(), Some("27.1.0.0.0"));
    }

    #[test]
    fn neoforge_mc_from_the_version_number_when_no_args_file_says() {
        for (dir, mc) in [
            ("26.2.0.59", "26.2"),
            ("21.0.167", "1.21"),
            ("0.25w14craftmine.5-beta", "25w14craftmine"),
        ] {
            let d = tempdir().unwrap();
            touch(&d.path().join(format!(
                "libraries/net/neoforged/neoforge/{dir}/win_args.txt"
            )));
            let r = detect(d.path());
            assert_eq!(r.mc_version.as_deref(), Some(mc), "{dir}");
        }
    }

    #[test]
    fn neoforge_mc_falls_back_to_the_server_log_then_to_none() {
        let d = tempdir().unwrap();
        touch(
            &d.path()
                .join("libraries/net/neoforged/neoforge/27.1.0.0.0/win_args.txt"),
        );
        let r = detect(d.path());
        assert_eq!(r.loader, Some(ServerCore::NeoForge));
        assert_eq!(r.mc_version, None, "an unknown number is not guessed at");

        write(
            &d.path().join("logs/latest.log"),
            "[12:00:00] [main/INFO]: Starting minecraft server version 27.1\n",
        );
        assert_eq!(detect(d.path()).mc_version.as_deref(), Some("27.1"));
    }

    #[test]
    fn detects_forge_split_version() {
        let d = tempdir().unwrap();
        forge_args_dir(d.path(), "1.20.1-47.2.0");
        let r = detect(d.path());
        assert_eq!(r.loader, Some(ServerCore::Forge));
        assert_eq!(r.mc_version.as_deref(), Some("1.20.1"));
        assert_eq!(r.loader_version.as_deref(), Some("47.2.0"));
        assert!(can_launch_as_is(d.path(), ServerCore::Forge));
    }

    #[test]
    fn a_corrupt_fabric_launch_jar_does_not_vouch_for_its_vanilla_jar() {
        use crate::servers_runtime::installed_loader::test_jars::jar;
        let d = tempdir().unwrap();
        // Truncated download: not a readable jar.
        fs::write(
            d.path().join("fabric-server-launch.jar"),
            b"PK\x03\x04trunc",
        )
        .unwrap();
        jar(
            &d.path().join("server.jar"),
            &[("version.json", r#"{"id":"1.21.1"}"#)],
        );
        touch(
            &d.path()
                .join("libraries/net/fabricmc/intermediary/1.20.4/x.jar"),
        );
        let r = detect(d.path());
        assert_eq!(r.loader, Some(ServerCore::Fabric));
        // Only the version folder speaks; the vanilla jar a broken launcher
        // would run says nothing.
        assert_eq!(r.mc_version.as_deref(), Some("1.20.4"));
    }

    #[test]
    fn detects_fabric_from_marker_and_intermediary() {
        let d = tempdir().unwrap();
        touch(&d.path().join("fabric-server-launch.jar"));
        touch(
            &d.path()
                .join("libraries/net/fabricmc/intermediary/1.20.4/intermediary-1.20.4.jar"),
        );
        touch(
            &d.path()
                .join("libraries/net/fabricmc/fabric-loader/0.16.5/fabric-loader-0.16.5.jar"),
        );
        let r = detect(d.path());
        assert_eq!(r.loader, Some(ServerCore::Fabric));
        assert_eq!(r.mc_version.as_deref(), Some("1.20.4"));
        assert_eq!(r.loader_version.as_deref(), Some("0.16.5"));
    }

    #[test]
    fn detects_quilt_marker() {
        let d = tempdir().unwrap();
        touch(&d.path().join(".quilt/x"));
        let r = detect(d.path());
        assert_eq!(r.loader, Some(ServerCore::Quilt));
    }

    /// What Lucerna's own Fabric server is until its first start: Fabric's
    /// bundled server launcher saved as `server.jar` and nothing it downloads
    /// yet (no `.fabric/`, no `libraries/`). It imported as Vanilla with an
    /// empty version (2026-10-02 regression F09).
    #[test]
    fn a_fabric_server_that_never_started_is_fabric() {
        use crate::servers_runtime::installed_loader::test_jars::jar;
        let d = tempdir().unwrap();
        jar(
            &d.path().join("server.jar"),
            &[(
                "install.properties",
                "fabric-loader-version=0.16.5\ngame-version=1.20.4",
            )],
        );
        fs::write(d.path().join("eula.txt"), "eula=true\n").unwrap();
        fs::write(d.path().join("server.properties"), "server-port=25565\n").unwrap();
        let r = detect(d.path());
        assert_eq!(r.loader, Some(ServerCore::Fabric));
        assert_eq!(r.mc_version.as_deref(), Some("1.20.4"));
        assert_eq!(r.loader_version.as_deref(), Some("0.16.5"));
        assert!(can_launch_as_is(d.path(), ServerCore::Fabric));
    }

    #[test]
    fn vanilla_when_only_server_jar_no_markers() {
        let d = tempdir().unwrap();
        // server.jar with an embedded version.json {"id":"1.20.4"}
        write_jar_with_version_json(&d.path().join("server.jar"), "1.20.4");
        let r = detect(d.path());
        assert_eq!(r.loader, Some(ServerCore::Vanilla));
        assert_eq!(r.mc_version.as_deref(), Some("1.20.4"));
    }

    #[test]
    fn unknown_when_nothing_recognizable() {
        let d = tempdir().unwrap();
        touch(&d.path().join("readme.txt"));
        let r = detect(d.path());
        assert_eq!(r.loader, None);
        assert_eq!(r.mc_version, None);
    }

    #[test]
    fn can_launch_vanilla_with_server_jar() {
        let d = tempdir().unwrap();
        touch(&d.path().join("server.jar"));
        assert!(can_launch_as_is(d.path(), ServerCore::Vanilla));
    }

    #[test]
    fn cannot_launch_fabric_when_foreign_launcher_present() {
        // Foreign fabric: server.jar is the VANILLA jar; launcher is separate.
        let d = tempdir().unwrap();
        touch(&d.path().join("server.jar"));
        touch(&d.path().join("fabric-server-launch.jar"));
        assert!(!can_launch_as_is(d.path(), ServerCore::Fabric));
    }

    #[test]
    fn can_launch_a_standard_quilt_tree() {
        // quilt-installer's layout: the launch jar next to the vanilla
        // server.jar. This launch jar names no Class-Path (libraries bundled,
        // as before 2022); a Class-Path is checked in `quilt`'s own tests.
        use std::io::Write;
        let d = tempdir().unwrap();
        touch(&d.path().join("server.jar"));
        let jar = fs::File::create(d.path().join("quilt-server-launch.jar")).unwrap();
        let mut zw = zip::ZipWriter::new(jar);
        zw.start_file(
            "META-INF/MANIFEST.MF",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        write!(
            zw,
            "Manifest-Version: 1.0\r\nMain-Class: org.quiltmc.loader.impl.launch.server.QuiltServerLauncher\r\n\r\n"
        )
        .unwrap();
        zw.finish().unwrap();
        assert!(can_launch_as_is(d.path(), ServerCore::Quilt));
    }

    #[test]
    fn cannot_launch_a_quilt_tree_whose_server_jar_is_vanilla() {
        // No launch jar: server.jar would start Minecraft without Quilt.
        let d = tempdir().unwrap();
        touch(&d.path().join(".quilt/x"));
        write_jar_with_version_json(&d.path().join("server.jar"), "1.20.4");
        assert!(!can_launch_as_is(d.path(), ServerCore::Quilt));
    }

    #[test]
    fn can_launch_forge_when_args_file_present() {
        let d = tempdir().unwrap();
        touch(
            &d.path()
                .join("libraries/net/neoforged/neoforge/20.4.237/win_args.txt"),
        );
        touch(
            &d.path()
                .join("libraries/net/neoforged/neoforge/20.4.237/unix_args.txt"),
        );
        assert!(can_launch_as_is(d.path(), ServerCore::NeoForge));
    }

    // Helper: write a minimal zip (jar) containing version.json at the root.
    fn write_jar_with_version_json(path: &Path, mc_id: &str) {
        use std::io::Write;
        let f = fs::File::create(path).unwrap();
        let mut zw = zip::ZipWriter::new(f);
        zw.start_file("version.json", zip::write::SimpleFileOptions::default())
            .unwrap();
        write!(zw, "{{\"id\":\"{mc_id}\"}}").unwrap();
        zw.finish().unwrap();
    }

    #[test]
    fn parses_mc_from_log_line() {
        let log = "[12:00:00] [main/INFO]: Starting minecraft server version 1.20.4\n";
        assert_eq!(super::parse_mc_from_log(log).as_deref(), Some("1.20.4"));
    }

    #[test]
    fn detects_purpur_tree_before_vanilla_fallback() {
        let d = tempdir().unwrap();
        touch(&d.path().join("server.jar"));
        touch(&d.path().join("purpur.yml"));
        let r = detect(d.path());
        assert_eq!(r.loader, Some(ServerCore::Purpur));
    }

    #[test]
    fn detects_paper_tree_via_config_dir() {
        let d = tempdir().unwrap();
        touch(&d.path().join("server.jar"));
        touch(&d.path().join("config/paper-global.yml"));
        let r = detect(d.path());
        assert_eq!(r.loader, Some(ServerCore::Paper));
    }

    #[test]
    fn paper_mc_version_parsed_from_version_history() {
        let d = tempdir().unwrap();
        touch(&d.path().join("server.jar"));
        touch(&d.path().join("config/paper-global.yml"));
        fs::write(
            d.path().join("version_history.json"),
            br#"{"currentVersion":"git-Paper-129 (MC: 1.21.4)"}"#,
        )
        .unwrap();
        let r = detect(d.path());
        assert_eq!(r.loader, Some(ServerCore::Paper));
        assert_eq!(r.mc_version.as_deref(), Some("1.21.4"));
    }

    #[test]
    fn updated_paper_history_takes_current_version_not_old() {
        // Real Paper serializes oldVersion BEFORE currentVersion after an
        // update; a naive first-match scan of the raw file would report the
        // stale 1.19.2.
        let d = tempdir().unwrap();
        touch(&d.path().join("server.jar"));
        touch(&d.path().join("config/paper-global.yml"));
        fs::write(
            d.path().join("version_history.json"),
            br#"{"oldVersion":"git-Paper-497 (MC: 1.19.2)","currentVersion":"git-Paper-152 (MC: 1.19.3)"}"#,
        )
        .unwrap();
        let r = detect(d.path());
        assert_eq!(r.loader, Some(ServerCore::Paper));
        assert_eq!(r.mc_version.as_deref(), Some("1.19.3"));
    }

    #[test]
    fn plain_vanilla_tree_still_detects_vanilla() {
        let d = tempdir().unwrap();
        touch(&d.path().join("server.jar"));
        let r = detect(d.path());
        assert_eq!(r.loader, Some(ServerCore::Vanilla));
    }

    #[test]
    fn purpur_tree_without_purpur_yml_falls_back_to_vanilla() {
        // A genuine Purpur server writes purpur.yml on first boot; before that
        // boot (or if the file was stripped during import), a Purpur tree is
        // indistinguishable from vanilla by these markers alone — acceptable,
        // since purpur.yml is the authoritative, always-present-after-boot
        // marker. version_history's "git-Purpur-*" string does NOT contain
        // "Paper" (case-sensitive), so it also doesn't accidentally match the
        // Paper arm.
        let d = tempdir().unwrap();
        touch(&d.path().join("server.jar"));
        fs::write(
            d.path().join("version_history.json"),
            br#"{"currentVersion":"git-Purpur-2321 (MC: 1.21.4)"}"#,
        )
        .unwrap();
        let r = detect(d.path());
        assert_eq!(r.loader, Some(ServerCore::Vanilla));
    }

    #[test]
    fn version_history_current_reads_current_version_field() {
        let d = tempdir().unwrap();
        fs::write(
            d.path().join("version_history.json"),
            br#"{"oldVersion":"git-Paper-497 (MC: 1.19.2)","currentVersion":"git-Paper-152 (MC: 1.19.3)"}"#,
        )
        .unwrap();
        assert_eq!(
            version_history_current(d.path()).as_deref(),
            Some("git-Paper-152 (MC: 1.19.3)")
        );
    }

    #[test]
    fn version_history_current_none_when_file_missing() {
        let d = tempdir().unwrap();
        assert_eq!(version_history_current(d.path()), None);
    }

    #[test]
    fn version_history_current_none_when_malformed_json() {
        let d = tempdir().unwrap();
        fs::write(d.path().join("version_history.json"), b"not json").unwrap();
        assert_eq!(version_history_current(d.path()), None);
    }

    #[test]
    fn mc_from_current_version_parses_suffix() {
        assert_eq!(
            mc_from_current_version("git-Paper-129 (MC: 1.21.4)").as_deref(),
            Some("1.21.4")
        );
    }

    #[test]
    fn mc_from_current_version_none_when_marker_absent() {
        assert_eq!(mc_from_current_version("git-Paper-129"), None);
    }
}
