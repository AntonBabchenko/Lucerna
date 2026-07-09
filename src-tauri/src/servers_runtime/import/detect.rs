//! Best-effort детект (loader, mc_version, loader_version) импортируемого
//! сервера + `can_launch_as_is`. Несработавшее поле → `None`; юзер правит в
//! визарде (Слайс 2b). Никогда не паникует на странном дереве.

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
/// Порядок: серверные паки (CF/Modrinth манифест) → Forge/NeoForge (по
/// libraries) → Fabric/Quilt (маркеры) → Paper/Purpur (purpur.yml /
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
    if let Some((mc, lv)) = neoforge_from_libraries(root) {
        return Detected {
            loader: Some(ServerCore::NeoForge),
            mc_version: mc,
            loader_version: Some(lv),
        };
    }
    if let Some((mc, lv)) = forge_from_libraries(root) {
        return Detected {
            loader: Some(ServerCore::Forge),
            mc_version: mc,
            loader_version: Some(lv),
        };
    }
    if quilt_marker(root) {
        return Detected {
            loader: Some(ServerCore::Quilt),
            mc_version: fabric_family_mc(root).or_else(|| mc_from_logs(root)),
            loader_version: loader_version_under(root, "org/quiltmc/quilt-loader"),
        };
    }
    if fabric_marker(root) {
        return Detected {
            loader: Some(ServerCore::Fabric),
            mc_version: fabric_family_mc(root).or_else(|| mc_from_logs(root)),
            loader_version: loader_version_under(root, "net/fabricmc/fabric-loader"),
        };
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
        return Detected {
            loader: Some(ServerCore::Vanilla),
            mc_version: mc_from_server_jar(root).or_else(|| mc_from_logs(root)),
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
/// V/Q/F — есть `server.jar` и НЕТ отдельного чужого лаунчер-jar (иначе
/// `server.jar` — ванильный, и `-jar server.jar` запустил бы ваниль);
/// Forge/NeoForge — найден installer args-файл (та же проверка, что у запуска).
pub fn can_launch_as_is(root: &Path, loader: ServerCore) -> bool {
    match loader {
        // Paper/Purpur launch exactly like vanilla (`-jar server.jar`).
        // `detect()` tells them apart via their own markers (purpur.yml,
        // config/paper-global.yml, version_history.json); launchability
        // only needs the jar.
        ServerCore::Vanilla | ServerCore::Paper | ServerCore::Purpur => {
            root.join("server.jar").exists()
        }
        ServerCore::Fabric | ServerCore::Quilt => {
            root.join("server.jar").exists()
                && !root.join("fabric-server-launch.jar").exists()
                && !root.join("fabric-server-launcher.jar").exists()
                && !root.join("quilt-server-launch.jar").exists()
        }
        ServerCore::Forge | ServerCore::NeoForge => {
            crate::servers_runtime::runtime::find_loader_args_file(root).is_some()
        }
    }
}

fn first_subdir_name(dir: &Path) -> Option<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter_map(|e| {
            if e.file_type().ok()?.is_dir() {
                e.file_name().to_str().map(String::from)
            } else {
                None
            }
        })
        .collect();
    names.sort();
    names.into_iter().last()
}

fn neoforge_from_libraries(root: &Path) -> Option<(Option<String>, String)> {
    let v = first_subdir_name(&root.join("libraries/net/neoforged/neoforge"))?;
    Some((neoforge_mc_version(&v), v))
}

fn forge_from_libraries(root: &Path) -> Option<(Option<String>, String)> {
    let dir = first_subdir_name(&root.join("libraries/net/minecraftforge/forge"))?;
    // Forge version dir is "<mc>-<forge>" e.g. "1.20.1-47.2.0".
    match dir.split_once('-') {
        Some((mc, forge)) => Some((Some(mc.to_string()), forge.to_string())),
        None => Some((None, dir)),
    }
}

/// NeoForge `<a>.<b>.<c>` → MC `1.<a>.<b>` (b==0 → `1.<a>`). e.g. 20.4.237 →
/// 1.20.4; 21.0.x → 1.21; 21.1.x → 1.21.1.
fn neoforge_mc_version(v: &str) -> Option<String> {
    let mut it = v.split('.');
    let a: u32 = it.next()?.parse().ok()?;
    let b: u32 = it.next()?.parse().ok()?;
    Some(if b == 0 {
        format!("1.{a}")
    } else {
        format!("1.{a}.{b}")
    })
}

fn fabric_marker(root: &Path) -> bool {
    root.join(".fabric").is_dir()
        || root.join("fabric-server-launch.jar").exists()
        || root.join("fabric-server-launcher.jar").exists()
        || root.join("libraries/net/fabricmc").is_dir()
}

fn quilt_marker(root: &Path) -> bool {
    root.join(".quilt").is_dir()
        || root.join("quilt-server-launch.jar").exists()
        || root.join("libraries/org/quiltmc").is_dir()
}

/// MC version from `libraries/net/fabricmc/intermediary/<mc>/`.
fn fabric_family_mc(root: &Path) -> Option<String> {
    first_subdir_name(&root.join("libraries/net/fabricmc/intermediary"))
}

/// Loader version from `libraries/<rel>/<v>/`.
fn loader_version_under(root: &Path, rel: &str) -> Option<String> {
    first_subdir_name(&root.join("libraries").join(rel))
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

    #[test]
    fn detects_neoforge_and_mc_from_libraries() {
        let d = tempdir().unwrap();
        touch(
            &d.path()
                .join("libraries/net/neoforged/neoforge/20.4.237/win_args.txt"),
        );
        let r = detect(d.path());
        assert_eq!(r.loader, Some(ServerCore::NeoForge));
        assert_eq!(r.loader_version.as_deref(), Some("20.4.237"));
        assert_eq!(r.mc_version.as_deref(), Some("1.20.4"));
    }

    #[test]
    fn detects_forge_split_version() {
        let d = tempdir().unwrap();
        touch(
            &d.path()
                .join("libraries/net/minecraftforge/forge/1.20.1-47.2.0/win_args.txt"),
        );
        let r = detect(d.path());
        assert_eq!(r.loader, Some(ServerCore::Forge));
        assert_eq!(r.mc_version.as_deref(), Some("1.20.1"));
        assert_eq!(r.loader_version.as_deref(), Some("47.2.0"));
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
