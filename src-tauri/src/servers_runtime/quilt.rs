//! Quilt dedicated server layout.
//!
//! Quilt publishes no prebuilt server jar: its meta serves `profile/json` and
//! `server/json`, never a `server/jar` (Fabric has one; Quilt never did). A
//! Quilt server is therefore assembled the way `quilt-installer install
//! server` lays it out:
//!
//! - `libraries/<maven path>` — every library in the loader's `server/json`
//!   (the loader, its dependencies, the `hashed` + `intermediary` mappings);
//! - `server.jar` — the vanilla Mojang server jar (SHA-1 verified by
//!   `create::create_quilt_server`);
//! - `quilt-server-launcher.properties` — `serverJar=server.jar`, the jar
//!   Quilt's launcher loads as the game (also its default, but an imported
//!   or repaired tree may carry one naming another jar);
//! - `quilt-server-launch.jar` — a jar holding only a manifest:
//!   `Main-Class: <launcherMainClass>`, `Class-Path: libraries/…`.
//!
//! The server starts with `-jar quilt-server-launch.jar`.
//!
//! Library downloads are trust-on-first-use over HTTPS: Quilt meta publishes
//! no per-file checksums (PRINCIPLES.md Part B 6, the exception the client's
//! Quilt path already uses).

use crate::error::{Error, Result};
use crate::servers_runtime::installed_loader;
use crate::versions::version_json::Library;
use futures_util::stream::{self, StreamExt};
use std::path::Path;

/// The launch jar's file name — Quilt's own, so a folder Lucerna assembles
/// reads the same as one `quilt-installer` made.
pub const LAUNCH_JAR: &str = "quilt-server-launch.jar";

/// `launcherMainClass` of Quilt's server profiles: the class a launch jar's
/// manifest names. Also how a host-renamed launch jar is recognised.
pub const SERVER_LAUNCHER_MAIN: &str = "org.quiltmc.loader.impl.launch.server.QuiltServerLauncher";

const LIBRARIES_DIR: &str = "libraries";
const SERVER_JAR: &str = "server.jar";
/// Where Quilt's server launcher reads which jar is the game.
const LAUNCHER_PROPERTIES: &str = "quilt-server-launcher.properties";
const MANIFEST_ENTRY: &str = "META-INF/MANIFEST.MF";
/// `java.util.jar.Manifest` line limit, in bytes.
const MANIFEST_LINE_BYTES: usize = 72;
/// Parallel library downloads: a Quilt server needs about a dozen small jars.
const DOWNLOAD_CONCURRENCY: usize = 6;

/// One library the server needs: its path under `libraries/` and where to
/// fetch it. `sha1` is empty for Quilt's maven libraries (see the module doc).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuiltServerLibrary {
    pub rel_path: String,
    pub url: String,
    pub sha1: String,
}

/// What Quilt's `server/json` says a server needs, validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuiltServerProfile {
    pub launcher_main_class: String,
    pub libraries: Vec<QuiltServerLibrary>,
}

#[derive(serde::Deserialize)]
struct RawServerProfile {
    #[serde(rename = "launcherMainClass", default)]
    launcher_main_class: Option<String>,
    #[serde(default)]
    libraries: Vec<Library>,
}

fn unavailable(mc: &str, reason: impl Into<String>) -> Error {
    Error::ServerJarUnavailable {
        loader: "quilt".into(),
        mc_version: mc.into(),
        reason: reason.into(),
    }
}

/// Validate Quilt's `server/json` into what the assembly needs. Its content
/// becomes paths on disk and manifest headers, so it is checked here, before
/// anything is written: the launcher class must be a Java class name (a
/// newline in it would inject manifest headers); every library must name its
/// own maven (`url`) or carry a `downloads` block — never the silent
/// libraries.minecraft.net fallback — and must resolve to a download (a
/// library dropped here would leave the launch jar's Class-Path short); every
/// library path must stay under `libraries/` and be writable into a
/// space-separated `Class-Path`. A profile without libraries cannot start a
/// server.
pub fn parse_server_profile(json: serde_json::Value, mc: &str) -> Result<QuiltServerProfile> {
    use crate::versions::libraries::{artifacts_to_install, should_install};
    let raw: RawServerProfile = serde_json::from_value(json)
        .map_err(|e| unavailable(mc, format!("server profile is not valid: {e}")))?;
    let main_class = raw.launcher_main_class.unwrap_or_default();
    if !is_java_class_name(&main_class) {
        return Err(unavailable(
            mc,
            format!("server profile names no valid launcher class: {main_class:?}"),
        ));
    }
    let os = crate::versions::install::current_os();
    let arch = crate::versions::install::current_arch();
    let mut seen = std::collections::HashSet::new();
    let mut libraries = Vec::new();
    for lib in raw.libraries.iter().filter(|l| should_install(l, os, arch)) {
        let names_its_source =
            lib.url.as_deref().is_some_and(|u| !u.is_empty()) || lib.downloads.is_some();
        let artifacts = artifacts_to_install(lib, os, arch);
        if !names_its_source || artifacts.is_empty() {
            return Err(unavailable(
                mc,
                format!("server profile library {:?} has no download", lib.name),
            ));
        }
        for (rel_path, url, sha1, _size) in artifacts {
            if url.is_empty() || !is_class_path_safe(&rel_path) {
                return Err(unavailable(
                    mc,
                    format!("server profile has an unusable library path: {rel_path:?}"),
                ));
            }
            if seen.insert(rel_path.clone()) {
                libraries.push(QuiltServerLibrary {
                    rel_path,
                    url,
                    sha1,
                });
            }
        }
    }
    if libraries.is_empty() {
        return Err(unavailable(mc, "server profile lists no libraries"));
    }
    Ok(QuiltServerProfile {
        launcher_main_class: main_class,
        libraries,
    })
}

/// A Java binary class name: dot-separated identifiers of ASCII letters,
/// digits, `_` and `$`, none starting with a digit.
fn is_java_class_name(s: &str) -> bool {
    !s.is_empty()
        && s.split('.').all(|segment| {
            let mut chars = segment.chars();
            matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_' || c == '$')
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
        })
}

/// A library path safe both to join under `libraries/` and to write into a
/// manifest `Class-Path` (space-separated relative URLs): `/`-separated
/// segments of `[A-Za-z0-9._+-]`, none empty, `.` or `..`.
fn is_class_path_safe(rel_path: &str) -> bool {
    rel_path.split('/').all(|segment| {
        !segment.is_empty()
            && segment != "."
            && segment != ".."
            && segment
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-'))
    })
}

/// `META-INF/MANIFEST.MF` for the launch jar, byte-compatible with
/// `java.util.jar.Manifest`: CRLF line ends, header lines split at 72 bytes
/// with each continuation led by one space, and the section closed by an
/// empty line (Java drops a last header that lacks its line end).
pub fn manifest_bytes(profile: &QuiltServerProfile) -> Vec<u8> {
    let class_path: Vec<String> = profile
        .libraries
        .iter()
        .map(|l| format!("{LIBRARIES_DIR}/{}", l.rel_path))
        .collect();
    let mut out = String::new();
    push_manifest_header(&mut out, "Manifest-Version", "1.0");
    push_manifest_header(&mut out, "Main-Class", &profile.launcher_main_class);
    push_manifest_header(&mut out, "Class-Path", &class_path.join(" "));
    out.push_str("\r\n");
    out.into_bytes()
}

fn push_manifest_header(out: &mut String, name: &str, value: &str) {
    let line = format!("{name}: {value}");
    let mut rest = line.as_str();
    let mut limit = MANIFEST_LINE_BYTES;
    loop {
        let mut cut = rest.len().min(limit);
        while !rest.is_char_boundary(cut) {
            cut -= 1;
        }
        out.push_str(&rest[..cut]);
        out.push_str("\r\n");
        rest = &rest[cut..];
        if rest.is_empty() {
            return;
        }
        out.push(' ');
        limit = MANIFEST_LINE_BYTES - 1;
    }
}

/// The launch jar: a zip holding only the manifest, stored uncompressed.
fn launch_jar_bytes(profile: &QuiltServerProfile) -> Result<Vec<u8>> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    let zip_err = |e: zip::result::ZipError| Error::io(LAUNCH_JAR, format!("zip: {e}"));
    let mut zw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zw.start_file(MANIFEST_ENTRY, opts).map_err(zip_err)?;
    zw.write_all(&manifest_bytes(profile))
        .map_err(|e| Error::io(LAUNCH_JAR, e))?;
    Ok(zw.finish().map_err(zip_err)?.into_inner())
}

/// Download every library into `runtime/libraries/`, up to
/// `DOWNLOAD_CONCURRENCY` at a time. Each download is atomic
/// (`network::download`): it promotes or discards its own `.part` file. So
/// every started download runs to its end even after another fails —
/// dropping one mid-flight would skip that cleanup and leave a temp file in
/// `libraries/`, which a re-download keeps. The first failure is returned.
/// Always downloads: a re-download is the repair path, so a present file is
/// not trusted.
pub async fn download_libraries(runtime: &Path, profile: &QuiltServerProfile) -> Result<()> {
    let root = runtime.join(LIBRARIES_DIR);
    // Owned work items: a future borrowing `profile` is not general enough
    // to be `Send` inside the Tauri command that awaits this.
    let jobs: Vec<(String, std::path::PathBuf, String)> = profile
        .libraries
        .iter()
        .map(|lib| (lib.url.clone(), root.join(&lib.rel_path), lib.sha1.clone()))
        .collect();
    let results: Vec<Result<()>> = stream::iter(jobs)
        .map(|(url, dest, sha1)| async move {
            crate::network::download::download_no_emit_with(
                &url,
                &dest,
                crate::network::download::Checksum::Sha1(sha1),
                "servers",
            )
            .await
        })
        .buffer_unordered(DOWNLOAD_CONCURRENCY)
        .collect()
        .await;
    results.into_iter().collect()
}

/// Write what starts the server, once its libraries and `server.jar` are in
/// place: `quilt-server-launcher.properties` naming `server.jar` as the game
/// (so a repair also resets a file that pointed elsewhere), then the launch
/// jar LAST — its presence means everything it names is on disk. Both writes
/// are atomic, so a file being replaced stays intact until the new one is
/// complete.
pub async fn write_launcher(runtime: &Path, profile: &QuiltServerProfile) -> Result<()> {
    let properties = format!(
        "# Written by Lucerna: the jar Quilt's server launcher loads as Minecraft.\n\
         serverJar={SERVER_JAR}\n"
    );
    replace_file(&runtime.join(LAUNCHER_PROPERTIES), properties.into_bytes()).await?;
    replace_file(&runtime.join(LAUNCH_JAR), launch_jar_bytes(profile)?).await
}

/// Write `bytes` to `<path>.part`, then rename it over `path` (checked).
async fn replace_file(path: &Path, bytes: Vec<u8>) -> Result<()> {
    let mut part = path.as_os_str().to_owned();
    part.push(".part");
    let part = std::path::PathBuf::from(part);
    if let Err(e) = tokio::fs::write(&part, bytes).await {
        discard_part(&part).await;
        return Err(Error::io(part.display().to_string(), e));
    }
    if let Err(e) = tokio::fs::rename(&part, path).await {
        discard_part(&part).await;
        return Err(Error::io(path.display().to_string(), e));
    }
    Ok(())
}

/// Remove a temp file after a failed write or rename. The caller already
/// returns that failure; a removal that fails too is logged with the path so
/// the stray file can be found. It is never mistaken for the real file:
/// readers look only at the final names.
async fn discard_part(part: &Path) {
    match tokio::fs::remove_file(part).await {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => crate::diag!(
            "servers: could not remove {} after a failed write: {e}",
            part.display()
        ),
    }
}

/// Which jar starts this Quilt server. `runtime::build_launch_argv` launches
/// it; `launchable_as_is` builds on it for import.
///
/// 1. `quilt-server-launch.jar` is a file → it (Quilt's layout; Lucerna's too);
/// 2. else `server.jar` is a file whose manifest `Main-Class` is Quilt's
///    server launcher → `server.jar` (a host renamed the launch jar);
/// 3. else no Quilt launcher → `ServerSpawnFailed`. Running `server.jar`
///    here would start plain Minecraft and silently ignore every mod.
///
/// A check that cannot be made (IO error other than "not found", a jar that
/// cannot be read) is an error, never a guess.
pub fn launch_jar(runtime: &Path) -> Result<&'static str> {
    if is_file(&runtime.join(LAUNCH_JAR))? {
        return Ok(LAUNCH_JAR);
    }
    let server_jar = runtime.join(SERVER_JAR);
    if is_file(&server_jar)? && main_class(&server_jar)?.as_deref() == Some(SERVER_LAUNCHER_MAIN) {
        return Ok(SERVER_JAR);
    }
    Err(Error::ServerSpawnFailed {
        details: format!(
            "no Quilt launcher found: {LAUNCH_JAR} is missing and {SERVER_JAR} is not a Quilt launcher"
        ),
    })
}

/// Import's "keep it as it is" test for a Quilt tree: `launch_jar` finds the
/// launcher AND what it needs is there — the game jar (`serverJar=` in
/// `quilt-server-launcher.properties`, else `server.jar`, and not the
/// launcher itself) and every library its manifest's `Class-Path` names (a
/// pre-2022 launch jar bundles its libraries and has no `Class-Path`).
/// Anything missing, or any check that cannot be made, answers `false`: the
/// import then reinstalls Quilt rather than keep a tree that crashes at boot.
pub fn launchable_as_is(root: &Path) -> bool {
    // An error here is "could not tell", and false (reinstall) is the
    // restrictive answer to it.
    launch_needs_met(root).unwrap_or(false)
}

fn launch_needs_met(root: &Path) -> Result<bool> {
    let jar = launch_jar(root)?;
    let Some(game) = game_jar(root) else {
        return Ok(false);
    };
    if game == jar || !is_file(&root.join(&game))? {
        return Ok(false);
    }
    // An unreadable launch jar is reported missing too.
    Ok(installed_loader::root_jar_missing(root, jar).is_empty())
}

/// The jar Quilt's launcher loads as the game: `serverJar=` from
/// `quilt-server-launcher.properties`, else `server.jar` (the launcher's own
/// default, with or without the file). `None` when the file cannot be read or
/// names something other than a plain relative path — which jar it means
/// cannot be told.
fn game_jar(root: &Path) -> Option<String> {
    let jar = match installed_loader::read_text(&root.join(LAUNCHER_PROPERTIES)) {
        Ok(Some(text)) => {
            installed_loader::property(&text, "serverJar").unwrap_or_else(|| SERVER_JAR.into())
        }
        Ok(None) => SERVER_JAR.into(),
        Err(installed_loader::Unreadable) => return None,
    };
    installed_loader::is_plain_relative(&jar).then_some(jar)
}

fn is_file(path: &Path) -> Result<bool> {
    match std::fs::metadata(path) {
        Ok(meta) => Ok(meta.is_file()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(Error::io(path.display().to_string(), e)),
    }
}

/// A jar's manifest `Main-Class`; `None` when it has no manifest or names no
/// main class. A jar that cannot be opened or read is an error: "could not
/// tell" is not "not a Quilt launcher".
fn main_class(jar: &Path) -> Result<Option<String>> {
    match installed_loader::read_jar_text(jar, MANIFEST_ENTRY) {
        Ok(manifest) => {
            Ok(manifest.and_then(|m| installed_loader::manifest_attr(&m, "Main-Class")))
        }
        Err(installed_loader::Unreadable) => Err(Error::io(
            jar.display().to_string(),
            "the jar could not be read",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Live `server/json` for MC 1.21.1 + Quilt Loader 0.30.1 (2026-09-30),
    /// trimmed of its timestamps.
    const SERVER_JSON_1_21_1: &str = r#"{
      "id": "quilt-loader-0.30.1-1.21.1",
      "inheritsFrom": "1.21.1",
      "type": "release",
      "mainClass": "org.quiltmc.loader.impl.launch.knot.KnotServer",
      "launcherMainClass": "org.quiltmc.loader.impl.launch.server.QuiltServerLauncher",
      "arguments": { "game": [] },
      "libraries": [
        { "name": "net.fabricmc:sponge-mixin:0.17.3+mixin.0.8.7", "url": "https://maven.fabricmc.net/" },
        { "name": "org.quiltmc:quilt-json5:1.0.4+final", "url": "https://maven.quiltmc.org/repository/release/" },
        { "name": "org.ow2.asm:asm:9.10.1", "url": "https://maven.fabricmc.net/" },
        { "name": "org.ow2.asm:asm-analysis:9.10.1", "url": "https://maven.fabricmc.net/" },
        { "name": "org.ow2.asm:asm-commons:9.10.1", "url": "https://maven.fabricmc.net/" },
        { "name": "org.ow2.asm:asm-tree:9.10.1", "url": "https://maven.fabricmc.net/" },
        { "name": "org.ow2.asm:asm-util:9.10.1", "url": "https://maven.fabricmc.net/" },
        { "name": "org.quiltmc:quilt-config:1.3.3", "url": "https://maven.quiltmc.org/repository/release/" },
        { "name": "org.quiltmc:quilt-loader:0.30.1", "url": "https://maven.quiltmc.org/repository/release/" },
        { "name": "org.quiltmc:hashed:1.21.1", "url": "https://maven.quiltmc.org/repository/release/" },
        { "name": "net.fabricmc:intermediary:1.21.1", "url": "https://maven.fabricmc.net/" }
      ]
    }"#;

    fn live_profile() -> QuiltServerProfile {
        parse_server_profile(serde_json::from_str(SERVER_JSON_1_21_1).unwrap(), "1.21.1").unwrap()
    }

    fn with_libraries(main: &str, names: &[&str]) -> serde_json::Value {
        let libs: Vec<serde_json::Value> = names
            .iter()
            .map(|n| serde_json::json!({ "name": n, "url": "https://maven.quiltmc.org/repository/release/" }))
            .collect();
        serde_json::json!({ "launcherMainClass": main, "libraries": libs })
    }

    fn assert_unavailable(r: Result<QuiltServerProfile>) {
        assert!(
            matches!(&r, Err(Error::ServerJarUnavailable { loader, .. }) if loader == "quilt"),
            "got {r:?}"
        );
    }

    #[test]
    fn live_server_profile_parses_to_launcher_class_and_every_library() {
        let p = live_profile();
        assert_eq!(p.launcher_main_class, SERVER_LAUNCHER_MAIN);
        assert_eq!(p.libraries.len(), 11);
        let loader = p
            .libraries
            .iter()
            .find(|l| l.rel_path == "org/quiltmc/quilt-loader/0.30.1/quilt-loader-0.30.1.jar")
            .expect("loader library");
        assert_eq!(
            loader.url,
            "https://maven.quiltmc.org/repository/release/org/quiltmc/quilt-loader/0.30.1/quilt-loader-0.30.1.jar"
        );
        assert_eq!(loader.sha1, "", "Quilt meta publishes no checksums");
        assert!(p
            .libraries
            .iter()
            .any(|l| l.rel_path == "net/fabricmc/intermediary/1.21.1/intermediary-1.21.1.jar"));
    }

    #[test]
    fn a_profile_without_a_launcher_class_is_refused() {
        let mut json: serde_json::Value = serde_json::from_str(SERVER_JSON_1_21_1).unwrap();
        json.as_object_mut().unwrap().remove("launcherMainClass");
        assert_unavailable(parse_server_profile(json, "1.21.1"));
    }

    #[test]
    fn a_launcher_class_that_would_inject_manifest_headers_is_refused() {
        let json = with_libraries(
            "org.quiltmc.X\r\nClass-Path: evil.jar",
            &["org.quiltmc:quilt-loader:0.30.1"],
        );
        assert_unavailable(parse_server_profile(json, "1.21.1"));
    }

    #[test]
    fn a_library_coordinate_that_climbs_out_of_libraries_is_refused() {
        let json = with_libraries(SERVER_LAUNCHER_MAIN, &["org.quiltmc:..:1"]);
        assert_unavailable(parse_server_profile(json, "1.21.1"));
    }

    #[test]
    fn a_library_path_with_a_space_is_refused() {
        // A space would split one Class-Path entry into two.
        let json = with_libraries(SERVER_LAUNCHER_MAIN, &["org.quiltmc:quilt-loader:0.30 1"]);
        assert_unavailable(parse_server_profile(json, "1.21.1"));
    }

    #[test]
    fn a_profile_without_libraries_is_refused() {
        let json = with_libraries(SERVER_LAUNCHER_MAIN, &[]);
        assert_unavailable(parse_server_profile(json, "1.21.1"));
    }

    #[test]
    fn a_library_that_cannot_resolve_to_a_download_is_refused_not_dropped() {
        // Two segments only: no maven path. Dropping it would leave the
        // launch jar's Class-Path one library short.
        let json = with_libraries(
            SERVER_LAUNCHER_MAIN,
            &["org.quiltmc:quilt-loader:0.30.1", "org.quiltmc:broken"],
        );
        assert_unavailable(parse_server_profile(json, "1.21.1"));
    }

    #[test]
    fn a_library_that_names_no_source_is_refused() {
        // Without `url` or `downloads` the shared resolver would fall back to
        // libraries.minecraft.net unverified; a Quilt profile always names its maven.
        let json = serde_json::json!({
            "launcherMainClass": SERVER_LAUNCHER_MAIN,
            "libraries": [{ "name": "org.quiltmc:quilt-loader:0.30.1" }]
        });
        assert_unavailable(parse_server_profile(json, "1.21.1"));
    }

    #[test]
    fn manifest_lines_fit_72_bytes_and_unwrap_to_the_headers() {
        let p = live_profile();
        let text = String::from_utf8(manifest_bytes(&p)).unwrap();
        assert!(
            text.ends_with("\r\n\r\n"),
            "section must close with an empty line"
        );
        for line in text.split("\r\n") {
            assert!(
                line.len() <= MANIFEST_LINE_BYTES,
                "{} bytes: {line:?}",
                line.len()
            );
        }
        let continuations = text.split("\r\n").filter(|l| l.starts_with(' ')).count();
        assert!(
            continuations > 0,
            "the Class-Path of 11 libraries must wrap"
        );

        assert_eq!(
            installed_loader::manifest_attr(&text, "Main-Class").as_deref(),
            Some(SERVER_LAUNCHER_MAIN)
        );
        let unwrapped = text.replace("\r\n ", "");
        let class_path = unwrapped
            .lines()
            .find_map(|l| l.strip_prefix("Class-Path: "))
            .expect("Class-Path header");
        let expected: Vec<String> = p
            .libraries
            .iter()
            .map(|l| format!("libraries/{}", l.rel_path))
            .collect();
        assert_eq!(class_path, expected.join(" "));
    }

    fn jar_with_manifest(path: &Path, manifest: &str) {
        use std::io::Write;
        let mut zw = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        zw.start_file(MANIFEST_ENTRY, zip::write::SimpleFileOptions::default())
            .unwrap();
        zw.write_all(manifest.as_bytes()).unwrap();
        zw.finish().unwrap();
    }

    #[test]
    fn the_launch_jar_starts_the_server_when_present() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join(LAUNCH_JAR),
            launch_jar_bytes(&live_profile()).unwrap(),
        )
        .unwrap();
        jar_with_manifest(
            &d.path().join(SERVER_JAR),
            "Main-Class: net.minecraft.bundler.Main\r\n\r\n",
        );
        assert_eq!(launch_jar(d.path()).unwrap(), LAUNCH_JAR);
    }

    #[test]
    fn a_host_renamed_launch_jar_starts_the_server() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join(SERVER_JAR),
            launch_jar_bytes(&live_profile()).unwrap(),
        )
        .unwrap();
        assert_eq!(launch_jar(d.path()).unwrap(), SERVER_JAR);
    }

    #[test]
    fn a_vanilla_server_jar_is_not_a_quilt_launcher() {
        let d = tempfile::tempdir().unwrap();
        jar_with_manifest(
            &d.path().join(SERVER_JAR),
            "Main-Class: net.minecraft.bundler.Main\r\n\r\n",
        );
        assert!(matches!(
            launch_jar(d.path()),
            Err(Error::ServerSpawnFailed { .. })
        ));
    }

    #[test]
    fn an_empty_folder_has_no_quilt_launcher() {
        let d = tempfile::tempdir().unwrap();
        assert!(matches!(
            launch_jar(d.path()),
            Err(Error::ServerSpawnFailed { .. })
        ));
    }

    #[test]
    fn an_unreadable_server_jar_is_could_not_tell_not_no() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join(SERVER_JAR), b"not a zip").unwrap();
        assert!(matches!(launch_jar(d.path()), Err(Error::Io { .. })));
    }

    /// A tree laid out the way `write_launcher` + the downloads leave it.
    async fn complete_tree(root: &Path) -> QuiltServerProfile {
        let p = live_profile();
        for lib in &p.libraries {
            let dest = root.join(LIBRARIES_DIR).join(&lib.rel_path);
            std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
            std::fs::write(dest, b"lib").unwrap();
        }
        jar_with_manifest(
            &root.join(SERVER_JAR),
            "Main-Class: net.minecraft.bundler.Main\r\n\r\n",
        );
        write_launcher(root, &p).await.unwrap();
        p
    }

    #[tokio::test]
    async fn write_launcher_names_server_jar_as_the_game_and_writes_the_launch_jar() {
        let d = tempfile::tempdir().unwrap();
        complete_tree(d.path()).await;
        assert_eq!(game_jar(d.path()).unwrap(), SERVER_JAR);
        assert_eq!(launch_jar(d.path()).unwrap(), LAUNCH_JAR);
        assert!(!d.path().join(format!("{LAUNCH_JAR}.part")).exists());
    }

    #[tokio::test]
    async fn a_complete_quilt_tree_is_launchable_as_is() {
        let d = tempfile::tempdir().unwrap();
        complete_tree(d.path()).await;
        assert!(launchable_as_is(d.path()));
    }

    #[tokio::test]
    async fn a_quilt_tree_missing_a_library_is_not_launchable_as_is() {
        let d = tempfile::tempdir().unwrap();
        let p = complete_tree(d.path()).await;
        std::fs::remove_file(d.path().join(LIBRARIES_DIR).join(&p.libraries[3].rel_path)).unwrap();
        assert!(!launchable_as_is(d.path()));
    }

    #[tokio::test]
    async fn a_quilt_tree_missing_its_game_jar_is_not_launchable_as_is() {
        let d = tempfile::tempdir().unwrap();
        complete_tree(d.path()).await;
        std::fs::remove_file(d.path().join(SERVER_JAR)).unwrap();
        assert!(!launchable_as_is(d.path()));
    }

    #[tokio::test]
    async fn the_game_jar_named_in_the_launcher_properties_counts() {
        let d = tempfile::tempdir().unwrap();
        complete_tree(d.path()).await;
        std::fs::rename(
            d.path().join(SERVER_JAR),
            d.path().join("minecraft_server.1.21.1.jar"),
        )
        .unwrap();
        std::fs::write(
            d.path().join(LAUNCHER_PROPERTIES),
            "#Wed Sep 30 00:35:32 CEST 2026\nserverJar=minecraft_server.1.21.1.jar\n",
        )
        .unwrap();
        assert!(launchable_as_is(d.path()));
    }

    #[test]
    fn a_renamed_launcher_that_would_load_itself_is_not_launchable_as_is() {
        // server.jar IS the launcher and no properties file names another
        // game jar: Quilt would load its own launcher as Minecraft.
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join(SERVER_JAR),
            launch_jar_bytes(&live_profile()).unwrap(),
        )
        .unwrap();
        assert!(!launchable_as_is(d.path()));
    }
}
