//! Which installed loader version a server tree runs — the one rule shared by
//! import detection (`import::detect`) and launch (`runtime::build_launch_argv`).
//!
//! Loader installers never delete an older version's folder, so after an
//! in-place upgrade `libraries/` holds several. Nothing here picks one by
//! name order. A marker names the live install — the run script, the only
//! launchable candidate, the launcher's own metadata — or the answer is
//! "cannot tell".

use crate::forge::meta::parse_maven_entry;
use crate::forge::ForgeFlavor;
use crate::instances::import::readers::raw_minecraft::is_version_like;
use crate::servers_runtime::schema::ServerCore;
use std::collections::BTreeSet;
use std::io::{ErrorKind, Read};
use std::path::{Component, Path};

/// Run scripts, manifests and embedded metadata are small; reading stops here.
const TEXT_CAP: u64 = 1024 * 1024;

/// `Main-Class` of a pre-1.17 Forge server jar, read from the jars the
/// official installers write for 1.7.10, 1.12.2 and 1.16.5. The 1.17+ shim
/// (`net.minecraftforge.bootstrap.shim.Main`), the installer and the vanilla
/// server jar are deliberately absent: none of them is the Forge server.
const FORGE_SERVER_MAINS: &[&str] = &[
    "cpw.mods.fml.relauncher.ServerLaunchWrapper",
    "net.minecraftforge.fml.relauncher.ServerLaunchWrapper",
    "net.minecraftforge.server.ServerMain",
];

/// Which OS's args file a launch reads. Production passes
/// [`ArgsOs::current`]; tests pass both, so they do not depend on the CI OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArgsOs {
    Windows,
    Unix,
}

impl ArgsOs {
    pub(crate) fn current() -> Self {
        if cfg!(windows) {
            ArgsOs::Windows
        } else {
            ArgsOs::Unix
        }
    }

    fn file_name(self) -> &'static str {
        match self {
            ArgsOs::Windows => "win_args.txt",
            ArgsOs::Unix => "unix_args.txt",
        }
    }
}

/// How the live install is started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LaunchEntry {
    /// 1.17+: `@user_jvm_args.txt @<rel> nogui`. `rel` is root-relative, e.g.
    /// `libraries/net/minecraftforge/forge/1.20.1-47.2.0/win_args.txt`.
    ArgsFile { rel: String, version_dir: String },
    /// Forge before 1.17: `-jar <file> nogui`, `file` a root file name.
    RootJar { file: String },
}

impl LaunchEntry {
    /// The root-relative path that names this install to the user.
    fn shown(&self) -> String {
        match self {
            LaunchEntry::ArgsFile { rel, .. } => rel
                .rsplit_once('/')
                .map_or_else(|| rel.clone(), |(dir, _)| dir.to_string()),
            LaunchEntry::RootJar { file } => file.clone(),
        }
    }
}

/// The answer to "which Forge-family install does this server run".
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ForgeInstall {
    Found(LaunchEntry),
    /// Several installs and nothing names the live one. Root-relative paths,
    /// sorted, for the user-facing message.
    Ambiguous {
        candidates: Vec<String>,
    },
    Absent,
}

/// A folder that could not be listed, or a file that could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Unreadable;

/// One value's answer across a chain of sources.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Answer {
    Known(String),
    /// The sources disagree; a weaker source (a log) must not guess.
    CannotTell,
    /// No source had anything to say.
    NoEvidence,
}

impl Answer {
    /// One distinct value → `Known`, several → `CannotTell`, none → `NoEvidence`.
    pub(crate) fn of(values: BTreeSet<String>) -> Answer {
        let mut values = values.into_iter();
        match (values.next(), values.next()) {
            (None, _) => Answer::NoEvidence,
            (Some(only), None) => Answer::Known(only),
            (Some(_), Some(_)) => Answer::CannotTell,
        }
    }

    /// This answer, or the next source's when this one had no evidence.
    fn or_else(self, next: impl FnOnce() -> Answer) -> Answer {
        match self {
            Answer::NoEvidence => next(),
            decided => decided,
        }
    }

    pub(crate) fn known(self) -> Option<String> {
        match self {
            Answer::Known(value) => Some(value),
            Answer::CannotTell | Answer::NoEvidence => None,
        }
    }
}

/// The Forge flavour a server core launches through, if any.
pub(crate) fn forge_flavor(core: ServerCore) -> Option<ForgeFlavor> {
    match core {
        ServerCore::Forge => Some(ForgeFlavor::Forge),
        ServerCore::NeoForge => Some(ForgeFlavor::NeoForge),
        ServerCore::Vanilla
        | ServerCore::Fabric
        | ServerCore::Quilt
        | ServerCore::Paper
        | ServerCore::Purpur => None,
    }
}

/// The loader's name as shown in messages.
pub(crate) fn flavor_name(flavor: ForgeFlavor) -> &'static str {
    match flavor {
        ForgeFlavor::Forge => "Forge",
        ForgeFlavor::NeoForge => "NeoForge",
    }
}

/// `libraries/…` folder holding one sub-folder per installed version.
fn libraries_rel(flavor: ForgeFlavor) -> &'static str {
    match flavor {
        ForgeFlavor::Forge => "libraries/net/minecraftforge/forge",
        ForgeFlavor::NeoForge => "libraries/net/neoforged/neoforge",
    }
}

// ---- which install runs ------------------------------------------------

/// Resolve the live Forge-family install of the server tree at `root`.
///
/// 1. The run scripts name it (`@libraries/<group>/<dir>/<os>_args.txt`).
/// 2. Otherwise every launchable candidate: a version folder holding this
///    OS's args file, and (Forge only) a root jar whose `Main-Class` is a
///    pre-1.17 Forge server main. One → `Found`, several → `Ambiguous`.
///
/// A file this process cannot read is not a candidate: the JVM is launched as
/// the same user on the same files, so it could not start it either.
pub(crate) fn resolve_forge_family(root: &Path, flavor: ForgeFlavor, os: ArgsOs) -> ForgeInstall {
    let marked = script_marked_dirs(root, flavor, os);
    if !marked.is_empty() {
        return decide(marked);
    }
    let mut candidates = launchable_version_dirs(root, flavor, os);
    if flavor == ForgeFlavor::Forge {
        candidates.extend(
            forge_root_jars(root)
                .into_iter()
                .map(|file| LaunchEntry::RootJar { file }),
        );
    }
    decide(candidates)
}

fn decide(mut candidates: Vec<LaunchEntry>) -> ForgeInstall {
    if candidates.len() > 1 {
        let mut shown: Vec<String> = candidates.iter().map(LaunchEntry::shown).collect();
        shown.sort();
        shown.dedup();
        return ForgeInstall::Ambiguous { candidates: shown };
    }
    candidates
        .pop()
        .map_or(ForgeInstall::Absent, ForgeInstall::Found)
}

/// Step 1: the version folders the run scripts start. A script names its own
/// OS's args file, but either one names the version folder, and the folder
/// counts when it holds THIS OS's args file — installers write both, so a
/// server copied from Linux with only `run.sh` still resolves on Windows.
///
/// A missing or unreadable script names nothing. Scripts only choose among
/// folders that hold an args file, and step 2 sees every such folder, so
/// skipping a script can turn a choice into "cannot tell", never into a wrong
/// answer.
fn script_marked_dirs(root: &Path, flavor: ForgeFlavor, os: ArgsOs) -> Vec<LaunchEntry> {
    let mut dirs = BTreeSet::new();
    for script in ["run.bat", "run.sh"] {
        let text = match read_text(&root.join(script)) {
            Ok(Some(text)) => text,
            Ok(None) => continue,
            // An unreadable script names nothing (see above).
            Err(Unreadable) => continue,
        };
        dirs.extend(script_arg_dirs(&text, flavor));
    }
    dirs.into_iter()
        .filter_map(|dir| args_entry(root, flavor, os, dir))
        .collect()
}

/// Version folders named by `@libraries/<group>/<dir>/{win,unix}_args.txt`
/// tokens on the script's lines that are not comments. A commented-out line
/// is where an old reference is left after an upgrade.
fn script_arg_dirs(text: &str, flavor: ForgeFlavor) -> Vec<String> {
    let prefix = format!("{}/", libraries_rel(flavor));
    text.lines()
        .filter(|line| !is_script_comment(line))
        .flat_map(str::split_whitespace)
        .filter_map(|token| {
            let path = token
                .trim_matches(['"', '\''])
                .strip_prefix('@')?
                .replace('\\', "/");
            let (dir, file) = path.strip_prefix(&prefix)?.split_once('/')?;
            let is_args = file == "win_args.txt" || file == "unix_args.txt";
            (is_args && !dir.is_empty() && dir != "." && dir != "..").then(|| dir.to_string())
        })
        .collect()
}

/// `#` in sh; `REM`, `@REM` and `::` in bat.
fn is_script_comment(line: &str) -> bool {
    let line = line.trim_start().to_ascii_lowercase();
    line.starts_with('#')
        || line.starts_with("::")
        || line == "rem"
        || line.starts_with("rem ")
        || line == "@rem"
        || line.starts_with("@rem ")
}

/// The `ArgsFile` entry for `dir` when it holds this OS's args file.
fn args_entry(root: &Path, flavor: ForgeFlavor, os: ArgsOs, dir: String) -> Option<LaunchEntry> {
    let rel = format!("{}/{dir}/{}", libraries_rel(flavor), os.file_name());
    match std::fs::metadata(root.join(&rel)) {
        Ok(meta) if meta.is_file() => Some(LaunchEntry::ArgsFile {
            rel,
            version_dir: dir,
        }),
        Ok(_) => None,
        // Absent, or unreadable to this user — and so unreadable to the JVM
        // launched as this user too: either way not a launchable install.
        Err(_) => None,
    }
}

/// Step 2a: every version folder holding this OS's args file.
fn launchable_version_dirs(root: &Path, flavor: ForgeFlavor, os: ArgsOs) -> Vec<LaunchEntry> {
    match version_dirs(root, flavor) {
        Ok(dirs) => dirs
            .into_iter()
            .filter_map(|dir| args_entry(root, flavor, os, dir))
            .collect(),
        // Unlistable: nothing in it can be named here. `detect` reports the
        // loader as unknown from the same listing, and the launch reports
        // that no launch file could be found or read.
        Err(Unreadable) => Vec::new(),
    }
}

/// The version sub-folders of the flavour's `libraries/` folder, sorted:
/// empty when the folder does not exist, `Err` when it could not be listed.
pub(crate) fn version_dirs(root: &Path, flavor: ForgeFlavor) -> Result<Vec<String>, Unreadable> {
    subdir_names(&root.join(libraries_rel(flavor)))
}

/// Step 2b: root jars whose manifest names a pre-1.17 Forge server main.
fn forge_root_jars(root: &Path) -> Vec<String> {
    match root_jar_names(root) {
        Ok(names) => names
            .into_iter()
            .filter(|name| is_forge_server_jar(&root.join(name)))
            .collect(),
        // An unlistable server folder: nothing in it can be named here.
        // `detect` reports the loader as unknown from the same listing, and
        // the launch reports that no launch file could be found or read.
        Err(Unreadable) => Vec::new(),
    }
}

/// `true` for a jar whose manifest `Main-Class` is a pre-1.17 Forge server
/// main. A jar that cannot be read is not one: the JVM, launched as the same
/// user, could not start it either.
pub(crate) fn is_forge_server_jar(jar: &Path) -> bool {
    match read_jar_text(jar, "META-INF/MANIFEST.MF") {
        Ok(Some(manifest)) => manifest_attr(&manifest, "Main-Class")
            .is_some_and(|main| FORGE_SERVER_MAINS.contains(&main.as_str())),
        Ok(None) => false,
        // Unreadable means unlaunchable (see above).
        Err(Unreadable) => false,
    }
}

/// `Class-Path` entries of the root jar `file` that are not under `root`:
/// files the JVM needs to start it. Legacy jars list the vanilla
/// `minecraft_server.<mc>.jar` and their libraries. A jar that cannot be read
/// is itself reported missing.
pub(crate) fn root_jar_missing(root: &Path, file: &str) -> Vec<String> {
    let manifest = match read_jar_text(&root.join(file), "META-INF/MANIFEST.MF") {
        Ok(Some(manifest)) => manifest,
        Ok(None) => return Vec::new(),
        Err(Unreadable) => return vec![file.to_string()],
    };
    let Some(class_path) = manifest_attr(&manifest, "Class-Path") else {
        return Vec::new();
    };
    class_path
        .split_whitespace()
        // A stat failure counts as missing: the JVM would fail on it too.
        .filter(|entry| !root.join(entry).is_file())
        .map(str::to_string)
        .collect()
}

// ---- Forge versions ----------------------------------------------------

/// `(mc, build)` from a Forge `<mc>-<build>` string, legacy `<mc>-<build>-<mc>`
/// included. PR #465's rule: a branch build such as `…-1.10.0` or `…-1710ls`
/// parses to a non-version `mc` and yields nothing.
pub(crate) fn split_forge_raw(raw: &str) -> Option<(String, String)> {
    parse_maven_entry(raw)
        .filter(|entry| is_version_like(&entry.mc))
        .map(|entry| (entry.mc, entry.fv))
}

/// `(mc, build)` of the pre-1.17 Forge server jar `file`. The build comes
/// from the jar's embedded `version.json` (1.7.10 carries one), else from the
/// installer-given name `forge-<raw>[-universal].jar`. The vanilla jar it
/// runs also names the MC; when the two disagree the MC is `CannotTell`.
pub(crate) fn root_jar_versions(root: &Path, file: &str) -> (Answer, Option<String>) {
    let declared = match read_jar_text(&root.join(file), "version.json") {
        Ok(Some(json)) => forge_raw_in_version_json(&json).and_then(|raw| split_forge_raw(&raw)),
        Ok(None) => None,
        // Its manifest was readable a moment ago; the name still speaks.
        Err(Unreadable) => None,
    };
    let from_jar =
        declared.or_else(|| forge_raw_in_file_name(file).and_then(|raw| split_forge_raw(&raw)));
    let build = from_jar.as_ref().map(|(_, build)| build.clone());
    let mut mcs: BTreeSet<String> = from_jar.into_iter().map(|(mc, _)| mc).collect();
    match vanilla_jar_mcs(root, file) {
        Ok(vanilla) => mcs.extend(vanilla),
        // The vanilla jars could not be listed, so a disagreement cannot be
        // ruled out.
        Err(Unreadable) => return (Answer::CannotTell, build),
    }
    (Answer::of(mcs), build)
}

/// The Forge coordinate's version in a Forge `version.json`
/// (`net.minecraftforge:forge:<raw>`), classifier and extension dropped.
fn forge_raw_in_version_json(json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    value
        .get("libraries")?
        .as_array()?
        .iter()
        .filter_map(|lib| lib.get("name")?.as_str())
        .find_map(|name| name.strip_prefix("net.minecraftforge:forge:"))
        .and_then(|rest| rest.split([':', '@']).next())
        .map(str::to_string)
}

/// `<raw>` of an installer-named `forge-<raw>.jar` / `forge-<raw>-universal.jar`.
fn forge_raw_in_file_name(file: &str) -> Option<String> {
    let lower = file.to_ascii_lowercase();
    let stem = lower.strip_prefix("forge-")?;
    let raw = stem
        .strip_suffix("-universal.jar")
        .or_else(|| stem.strip_suffix(".jar"))?;
    Some(raw.to_string())
}

/// MC of the vanilla jar a legacy Forge jar runs: the `minecraft_server.<mc>.jar`
/// its own `Class-Path` names (1.7.10, 1.12.2), else every such jar in the root
/// (1.16.5 does not list it).
fn vanilla_jar_mcs(root: &Path, file: &str) -> Result<BTreeSet<String>, Unreadable> {
    let class_path = match read_jar_text(&root.join(file), "META-INF/MANIFEST.MF") {
        Ok(Some(manifest)) => manifest_attr(&manifest, "Class-Path").unwrap_or_default(),
        Ok(None) => String::new(),
        // Its manifest was readable a moment ago; fall back to the root.
        Err(Unreadable) => String::new(),
    };
    let named: BTreeSet<String> = class_path
        .split_whitespace()
        .filter_map(vanilla_jar_mc)
        .collect();
    if !named.is_empty() {
        return Ok(named);
    }
    vanilla_root_jar_mcs(root)
}

/// MC of every root `minecraft_server.<mc>.jar`; `Err` when the folder could
/// not be listed.
pub(crate) fn vanilla_root_jar_mcs(root: &Path) -> Result<BTreeSet<String>, Unreadable> {
    Ok(root_jar_names(root)?
        .iter()
        .filter_map(|name| vanilla_jar_mc(name))
        .collect())
}

/// `<mc>` of `minecraft_server.<mc>.jar`, when it is a Minecraft version id.
fn vanilla_jar_mc(name: &str) -> Option<String> {
    let mc = name
        .strip_prefix("minecraft_server.")?
        .strip_suffix(".jar")?;
    is_version_like(mc).then(|| mc.to_string())
}

// ---- Fabric and Quilt versions -----------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FabricKind {
    Fabric,
    Quilt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FabricVersions {
    pub(crate) mc: Answer,
    pub(crate) loader: Answer,
}

/// MC and loader version of a Fabric or Quilt server tree.
///
/// The launcher's own records are peers: the bundled launcher's
/// `install.properties` (Fabric) and the installer-style launch jar's
/// `Class-Path` with the vanilla jar it runs. One distinct value between them
/// wins; two disagree, and a stale launcher left beside the live one must not
/// win by order. Only when both are silent do the version folders speak.
pub(crate) fn fabric_family_versions(root: &Path, kind: FabricKind) -> FabricVersions {
    let mut mcs = BTreeSet::new();
    let mut loaders = BTreeSet::new();
    if kind == FabricKind::Fabric
        && bundled_launcher_versions(root, &mut mcs, &mut loaders).is_err()
    {
        // The server folder could not be listed: a launcher we cannot see
        // might name other versions, so nothing can be told.
        return FabricVersions {
            mc: Answer::CannotTell,
            loader: Answer::CannotTell,
        };
    }
    launch_jar_versions(root, kind, &mut mcs, &mut loaders);
    FabricVersions {
        mc: Answer::of(mcs).or_else(|| intermediary_dirs(root)),
        loader: Answer::of(loaders).or_else(|| loader_dirs(root, kind)),
    }
}

/// Root jars carrying the bundled launcher's `install.properties`
/// (`game-version`, `fabric-loader-version`) — the jar meta.fabricmc.net
/// serves, which Lucerna itself saves as `server.jar`.
fn bundled_launcher_versions(
    root: &Path,
    mcs: &mut BTreeSet<String>,
    loaders: &mut BTreeSet<String>,
) -> Result<(), Unreadable> {
    for name in root_jar_names(root)? {
        if let Some((game, loader)) = bundled_launcher_props(&root.join(&name)) {
            mcs.insert(game);
            if !loader.is_empty() {
                loaders.insert(loader);
            }
        }
    }
    Ok(())
}

/// Whether a jar in `root` is Fabric's bundled server launcher. Until a Fabric
/// server Lucerna made has started once, that jar is all of Fabric there is:
/// the first start downloads `.fabric/` and `libraries/` beside it, the other
/// signs of a Fabric tree. A folder that cannot be listed vouches for nothing.
pub(crate) fn has_bundled_fabric_launcher(root: &Path) -> bool {
    match root_jar_names(root) {
        Ok(names) => names
            .iter()
            .any(|name| bundled_launcher_props(&root.join(name)).is_some()),
        Err(Unreadable) => false,
    }
}

/// `(game-version, fabric-loader-version)` from `jar`'s `install.properties`,
/// when `jar` is the bundled Fabric server launcher. The loader version may be
/// empty; the game version may not.
fn bundled_launcher_props(jar: &Path) -> Option<(String, String)> {
    let props = match read_jar_text(jar, "install.properties") {
        Ok(Some(text)) => text,
        Ok(None) => return None,
        // A jar this process cannot read cannot be the launcher the server
        // runs either (same user, same file).
        Err(Unreadable) => return None,
    };
    let game = property(&props, "game-version").filter(|v| !v.is_empty())?;
    let loader = property(&props, "fabric-loader-version")?;
    Some((game, loader))
}

/// The installer-style launch jar's `Class-Path`, matched by path and never
/// by position (Quilt's order is not stable), and the vanilla jar it runs.
/// Quilt's `hashed` mapping version can carry a `+build.N` suffix, so only
/// intermediary names the MC. A launch jar without `Class-Path` (Quilt's
/// pre-2022 fat jar) says nothing.
fn launch_jar_versions(
    root: &Path,
    kind: FabricKind,
    mcs: &mut BTreeSet<String>,
    loaders: &mut BTreeSet<String>,
) {
    let (jars, props_file): (&[&str], &str) = match kind {
        FabricKind::Fabric => (
            &["fabric-server-launch.jar", "fabric-server-launcher.jar"],
            "fabric-server-launcher.properties",
        ),
        FabricKind::Quilt => (
            &["quilt-server-launch.jar"],
            "quilt-server-launcher.properties",
        ),
    };
    let loader_prefix = format!("{}/", loader_rel(kind));
    // Set only for a launch jar whose manifest reads: an unreadable one runs
    // nothing, so the vanilla jar it would run says nothing either.
    let mut launch_jar_readable = false;
    for jar in jars {
        let manifest = match read_jar_text(&root.join(jar), "META-INF/MANIFEST.MF") {
            Ok(Some(manifest)) => manifest,
            Ok(None) => continue,
            // Absent, or unreadable: the JVM could not start it either.
            Err(Unreadable) => continue,
        };
        launch_jar_readable = true;
        let Some(class_path) = manifest_attr(&manifest, "Class-Path") else {
            continue;
        };
        for entry in class_path.split_whitespace() {
            if let Some(mc) = dir_after(entry, "libraries/net/fabricmc/intermediary/") {
                if is_version_like(mc) {
                    mcs.insert(mc.to_string());
                }
            }
            if let Some(loader) = dir_after(entry, &loader_prefix) {
                loaders.insert(loader.to_string());
            }
        }
    }
    if launch_jar_readable {
        mcs.extend(vanilla_id(root, props_file));
    }
}

fn loader_rel(kind: FabricKind) -> &'static str {
    match kind {
        FabricKind::Fabric => "libraries/net/fabricmc/fabric-loader",
        FabricKind::Quilt => "libraries/org/quiltmc/quilt-loader",
    }
}

/// The first path segment after `prefix`, e.g. the version folder.
fn dir_after<'a>(entry: &'a str, prefix: &str) -> Option<&'a str> {
    entry
        .strip_prefix(prefix)?
        .split('/')
        .next()
        .filter(|dir| !dir.is_empty())
}

/// `id` from the embedded `version.json` of the vanilla jar an
/// installer-style launch jar runs: `serverJar=` in its properties file,
/// `server.jar` when there is no such file.
fn vanilla_id(root: &Path, props_file: &str) -> Option<String> {
    let jar = match read_text(&root.join(props_file)) {
        Ok(Some(text)) => property(&text, "serverJar")?,
        Ok(None) => "server.jar".to_string(),
        // Unreadable: which jar it names cannot be told.
        Err(Unreadable) => return None,
    };
    if !is_plain_relative(&jar) {
        return None;
    }
    let json = match read_jar_text(&root.join(&jar), "version.json") {
        Ok(Some(json)) => json,
        Ok(None) => return None,
        // Unreadable to this user, so unreadable to the server it would run.
        Err(Unreadable) => return None,
    };
    let value: serde_json::Value = serde_json::from_str(&json).ok()?;
    value
        .get("id")?
        .as_str()
        .filter(|id| !id.is_empty())
        .map(str::to_string)
}

pub(crate) fn is_plain_relative(path: &str) -> bool {
    !path.is_empty()
        && Path::new(path)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

/// Exactly one intermediary version folder names the MC.
fn intermediary_dirs(root: &Path) -> Answer {
    match subdir_names(&root.join("libraries/net/fabricmc/intermediary")) {
        Ok(names) => Answer::of(names.into_iter().filter(|n| is_version_like(n)).collect()),
        // A folder that cannot be listed may hold a second version.
        Err(Unreadable) => Answer::CannotTell,
    }
}

/// Exactly one loader version folder names the loader.
fn loader_dirs(root: &Path, kind: FabricKind) -> Answer {
    match subdir_names(&root.join(loader_rel(kind))) {
        Ok(names) => Answer::of(names.into_iter().collect()),
        // A folder that cannot be listed may hold a second version.
        Err(Unreadable) => Answer::CannotTell,
    }
}

// ---- readers -----------------------------------------------------------

/// A small text file: `Ok(None)` when absent, `Err` when it cannot be read.
pub(crate) fn read_text(path: &Path) -> Result<Option<String>, Unreadable> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(Unreadable),
    };
    let mut bytes = Vec::new();
    file.take(TEXT_CAP)
        .read_to_end(&mut bytes)
        .map_err(|_| Unreadable)?;
    Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
}

/// Text of `entry` inside `jar`: `Ok(None)` when the jar has no such entry,
/// `Err` when the jar cannot be opened or read.
pub(crate) fn read_jar_text(jar: &Path, entry: &str) -> Result<Option<String>, Unreadable> {
    let file = std::fs::File::open(jar).map_err(|_| Unreadable)?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file)).map_err(|_| Unreadable)?;
    let found = match zip.by_name(entry) {
        Ok(found) => found,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(_) => return Err(Unreadable),
    };
    let mut bytes = Vec::new();
    found
        .take(TEXT_CAP)
        .read_to_end(&mut bytes)
        .map_err(|_| Unreadable)?;
    Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
}

/// Names of the regular `*.jar` files directly in `dir`, sorted: empty when
/// `dir` does not exist, `Err` when it could not be listed.
pub(crate) fn root_jar_names(dir: &Path) -> Result<Vec<String>, Unreadable> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(Unreadable),
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_| Unreadable)?;
        if !entry.file_type().map_err(|_| Unreadable)?.is_file() {
            continue;
        }
        if let Some(name) = entry.file_name().to_str() {
            if name.to_ascii_lowercase().ends_with(".jar") {
                names.push(name.to_string());
            }
        }
    }
    names.sort();
    Ok(names)
}

/// Sub-folder names of `dir`, sorted: empty when `dir` does not exist,
/// `Err` when it could not be listed.
fn subdir_names(dir: &Path) -> Result<Vec<String>, Unreadable> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(Unreadable),
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_| Unreadable)?;
        if entry.file_type().map_err(|_| Unreadable)?.is_dir() {
            if let Some(name) = entry.file_name().to_str() {
                names.push(name.to_string());
            }
        }
    }
    names.sort();
    Ok(names)
}

/// A main-section attribute of a jar manifest. `java.util.jar.Manifest`
/// wraps lines at 72 bytes, continuing on a line that starts with one space;
/// those are unwrapped first.
pub(crate) fn manifest_attr(manifest: &str, key: &str) -> Option<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in manifest.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line.is_empty() {
            break; // end of the main section
        }
        match (line.strip_prefix(' '), lines.last_mut()) {
            (Some(continued), Some(previous)) => previous.push_str(continued),
            _ => lines.push(line.to_string()),
        }
    }
    lines.iter().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim()
            .eq_ignore_ascii_case(key)
            .then(|| value.trim().to_string())
    })
}

/// A `key=value` (or `key: value`) entry of a Java properties text.
pub(crate) fn property(text: &str, key: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#') && !line.starts_with('!'))
        .find_map(|line| {
            let (name, value) = line.split_once(['=', ':'])?;
            (name.trim() == key).then(|| value.trim().to_string())
        })
}

#[cfg(test)]
pub(crate) mod test_jars {
    //! Jar fixtures shaped like the ones the real installers write.
    use std::io::Write;
    use std::path::Path;

    /// Write a zip at `path` holding `entries`.
    pub(crate) fn jar(path: &Path, entries: &[(&str, &str)]) {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).unwrap();
        }
        let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for (name, body) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }

    /// A manifest wrapped the way `java.util.jar.Manifest` writes it: lines
    /// of at most 72 bytes, continued on lines that start with one space.
    pub(crate) fn manifest(attrs: &[(&str, &str)]) -> String {
        let mut out = String::from("Manifest-Version: 1.0\r\n");
        for (name, value) in attrs {
            let line = format!("{name}: {value}");
            let bytes = line.as_bytes();
            out.push_str(&String::from_utf8_lossy(&bytes[..bytes.len().min(72)]));
            out.push_str("\r\n");
            for chunk in bytes[bytes.len().min(72)..].chunks(71) {
                out.push(' ');
                out.push_str(&String::from_utf8_lossy(chunk));
                out.push_str("\r\n");
            }
        }
        out.push_str("\r\n");
        out
    }

    /// Create an empty file (and its folders).
    pub(crate) fn touch(path: &Path) {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(path, b"x").unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::test_jars::{jar, manifest, touch};
    use super::*;
    use tempfile::tempdir;

    const MAIN_1710: &str = "cpw.mods.fml.relauncher.ServerLaunchWrapper";
    const MAIN_1122: &str = "net.minecraftforge.fml.relauncher.ServerLaunchWrapper";
    const MAIN_1165: &str = "net.minecraftforge.server.ServerMain";

    fn forge_jar(root: &Path, file: &str, main: &str, class_path: &str, extra: &[(&str, &str)]) {
        let mf = manifest(&[("Main-Class", main), ("Class-Path", class_path)]);
        let mut entries = vec![("META-INF/MANIFEST.MF", mf.as_str())];
        entries.extend_from_slice(extra);
        jar(&root.join(file), &entries);
    }

    fn args_dir(root: &Path, group: &str, dir: &str) {
        touch(&root.join(format!("{group}/{dir}/win_args.txt")));
        touch(&root.join(format!("{group}/{dir}/unix_args.txt")));
    }

    const FORGE: &str = "libraries/net/minecraftforge/forge";
    const NEOFORGE: &str = "libraries/net/neoforged/neoforge";

    /// The real 1.7.10 server layout: universal jar at the root, no Forge
    /// folder under libraries/, the vanilla jar on its Class-Path.
    fn legacy_1710(root: &Path) {
        forge_jar(
            root,
            "forge-1.7.10-10.13.4.1614-1.7.10-universal.jar",
            MAIN_1710,
            "libraries/net/minecraft/launchwrapper/1.12/launchwrapper-1.12.jar minecraft_server.1.7.10.jar",
            &[(
                "version.json",
                r#"{"id":"1.7.10-Forge10.13.4.1614-1.7.10","libraries":[{"name":"net.minecraftforge:forge:1.7.10-10.13.4.1614-1.7.10"},{"name":"net.minecraft:launchwrapper:1.12"}]}"#,
            )],
        );
        touch(&root.join("minecraft_server.1.7.10.jar"));
        touch(&root.join("libraries/net/minecraft/launchwrapper/1.12/launchwrapper-1.12.jar"));
    }

    #[test]
    fn legacy_1_7_10_runs_its_root_universal_jar() {
        let d = tempdir().unwrap();
        legacy_1710(d.path());
        for os in [ArgsOs::Windows, ArgsOs::Unix] {
            assert_eq!(
                resolve_forge_family(d.path(), ForgeFlavor::Forge, os),
                ForgeInstall::Found(LaunchEntry::RootJar {
                    file: "forge-1.7.10-10.13.4.1614-1.7.10-universal.jar".into()
                })
            );
        }
        let (mc, build) =
            root_jar_versions(d.path(), "forge-1.7.10-10.13.4.1614-1.7.10-universal.jar");
        assert_eq!(mc, Answer::Known("1.7.10".into()));
        assert_eq!(build.as_deref(), Some("10.13.4.1614"));
        assert!(
            root_jar_missing(d.path(), "forge-1.7.10-10.13.4.1614-1.7.10-universal.jar").is_empty()
        );
    }

    #[test]
    fn legacy_1_12_2_and_1_16_5_read_the_build_from_the_jar_name() {
        let d = tempdir().unwrap();
        forge_jar(
            d.path(),
            "forge-1.12.2-14.23.5.2860.jar",
            MAIN_1122,
            "minecraft_server.1.12.2.jar",
            &[],
        );
        touch(&d.path().join("minecraft_server.1.12.2.jar"));
        touch(&d.path().join("installer.jar"));
        assert_eq!(
            resolve_forge_family(d.path(), ForgeFlavor::Forge, ArgsOs::current()),
            ForgeInstall::Found(LaunchEntry::RootJar {
                file: "forge-1.12.2-14.23.5.2860.jar".into()
            })
        );
        assert_eq!(
            root_jar_versions(d.path(), "forge-1.12.2-14.23.5.2860.jar"),
            (Answer::Known("1.12.2".into()), Some("14.23.5.2860".into()))
        );

        let e = tempdir().unwrap();
        forge_jar(e.path(), "forge-1.16.5-36.2.39.jar", MAIN_1165, "", &[]);
        touch(&e.path().join("minecraft_server.1.16.5.jar"));
        touch(&e.path().join(format!(
            "{FORGE}/1.16.5-36.2.39/forge-1.16.5-36.2.39-server.jar"
        )));
        assert_eq!(
            resolve_forge_family(e.path(), ForgeFlavor::Forge, ArgsOs::current()),
            ForgeInstall::Found(LaunchEntry::RootJar {
                file: "forge-1.16.5-36.2.39.jar".into()
            })
        );
        assert_eq!(
            root_jar_versions(e.path(), "forge-1.16.5-36.2.39.jar"),
            (Answer::Known("1.16.5".into()), Some("36.2.39".into()))
        );
    }

    #[test]
    fn args_file_install_resolves_for_both_os() {
        let d = tempdir().unwrap();
        args_dir(d.path(), FORGE, "1.17.1-37.1.1");
        touch(&d.path().join("installer.jar"));
        for (os, file) in [
            (ArgsOs::Windows, "win_args.txt"),
            (ArgsOs::Unix, "unix_args.txt"),
        ] {
            assert_eq!(
                resolve_forge_family(d.path(), ForgeFlavor::Forge, os),
                ForgeInstall::Found(LaunchEntry::ArgsFile {
                    rel: format!("{FORGE}/1.17.1-37.1.1/{file}"),
                    version_dir: "1.17.1-37.1.1".into()
                })
            );
        }
    }

    #[test]
    fn run_bat_names_the_live_install_among_stale_ones() {
        let d = tempdir().unwrap();
        args_dir(d.path(), FORGE, "1.20.1-47.2.0");
        args_dir(d.path(), FORGE, "1.20.1-47.3.0");
        std::fs::write(
            d.path().join("run.bat"),
            "@echo off\r\nREM java @user_jvm_args.txt @libraries/net/minecraftforge/forge/1.20.1-47.2.0/win_args.txt %*\r\njava @user_jvm_args.txt @libraries/net/minecraftforge/forge/1.20.1-47.3.0/win_args.txt %*\r\npause\r\n",
        )
        .unwrap();
        let r = resolve_forge_family(d.path(), ForgeFlavor::Forge, ArgsOs::Unix);
        assert!(
            matches!(&r, ForgeInstall::Found(LaunchEntry::ArgsFile { version_dir, .. }) if version_dir == "1.20.1-47.3.0"),
            "{r:?}"
        );
    }

    #[test]
    fn run_sh_beats_the_alphabetical_trap() {
        // "1.21.9-…" sorts after "1.21.11-…"; the script names the live one.
        let d = tempdir().unwrap();
        args_dir(d.path(), FORGE, "1.21.9-58.1.0");
        args_dir(d.path(), FORGE, "1.21.11-61.1.0");
        std::fs::write(
            d.path().join("run.sh"),
            "#!/usr/bin/env sh\n# old: java @libraries/net/minecraftforge/forge/1.21.9-58.1.0/unix_args.txt\njava @user_jvm_args.txt @libraries/net/minecraftforge/forge/1.21.11-61.1.0/unix_args.txt \"$@\"\n",
        )
        .unwrap();
        let r = resolve_forge_family(d.path(), ForgeFlavor::Forge, ArgsOs::Windows);
        assert!(
            matches!(&r, ForgeInstall::Found(LaunchEntry::ArgsFile { version_dir, .. }) if version_dir == "1.21.11-61.1.0"),
            "{r:?}"
        );
    }

    #[test]
    fn several_installs_without_a_script_cannot_be_told_apart() {
        let d = tempdir().unwrap();
        args_dir(d.path(), FORGE, "1.21.9-58.1.0");
        args_dir(d.path(), FORGE, "1.21.11-61.1.0");
        assert_eq!(
            resolve_forge_family(d.path(), ForgeFlavor::Forge, ArgsOs::current()),
            ForgeInstall::Ambiguous {
                candidates: vec![
                    format!("{FORGE}/1.21.11-61.1.0"),
                    format!("{FORGE}/1.21.9-58.1.0"),
                ]
            }
        );
    }

    #[test]
    fn scripts_naming_different_installs_cannot_be_told_apart() {
        let d = tempdir().unwrap();
        args_dir(d.path(), FORGE, "1.20.1-47.2.0");
        args_dir(d.path(), FORGE, "1.20.1-47.3.0");
        std::fs::write(
            d.path().join("run.bat"),
            "java @libraries\\net\\minecraftforge\\forge\\1.20.1-47.2.0\\win_args.txt\r\n",
        )
        .unwrap();
        std::fs::write(
            d.path().join("run.sh"),
            "java \"@libraries/net/minecraftforge/forge/1.20.1-47.3.0/unix_args.txt\"\n",
        )
        .unwrap();
        assert!(matches!(
            resolve_forge_family(d.path(), ForgeFlavor::Forge, ArgsOs::current()),
            ForgeInstall::Ambiguous { .. }
        ));
    }

    #[test]
    fn a_script_naming_a_missing_install_is_no_marker() {
        let d = tempdir().unwrap();
        args_dir(d.path(), FORGE, "1.20.1-47.3.0");
        std::fs::write(
            d.path().join("run.sh"),
            "java @libraries/net/minecraftforge/forge/1.20.1-47.9.9/unix_args.txt\n",
        )
        .unwrap();
        assert!(matches!(
            resolve_forge_family(d.path(), ForgeFlavor::Forge, ArgsOs::current()),
            ForgeInstall::Found(LaunchEntry::ArgsFile { version_dir, .. }) if version_dir == "1.20.1-47.3.0"
        ));
    }

    #[test]
    fn a_legacy_jar_beside_a_newer_install_cannot_be_told_apart() {
        let d = tempdir().unwrap();
        forge_jar(d.path(), "forge-1.16.5-36.2.39.jar", MAIN_1165, "", &[]);
        args_dir(d.path(), FORGE, "1.18.2-40.2.0");
        assert_eq!(
            resolve_forge_family(d.path(), ForgeFlavor::Forge, ArgsOs::current()),
            ForgeInstall::Ambiguous {
                candidates: vec![
                    "forge-1.16.5-36.2.39.jar".to_string(),
                    format!("{FORGE}/1.18.2-40.2.0"),
                ]
            }
        );
    }

    #[test]
    fn shim_installer_vanilla_and_broken_jars_are_not_candidates() {
        let d = tempdir().unwrap();
        forge_jar(
            d.path(),
            "forge-1.12.2-14.23.5.2860.jar",
            MAIN_1122,
            "",
            &[],
        );
        let shim = manifest(&[("Main-Class", "net.minecraftforge.bootstrap.shim.Main")]);
        jar(
            &d.path().join("forge-1.21.11-61.1.0-shim.jar"),
            &[("META-INF/MANIFEST.MF", &shim)],
        );
        let installer = manifest(&[("Main-Class", "net.minecraftforge.installer.SimpleInstaller")]);
        jar(
            &d.path().join("installer.jar"),
            &[("META-INF/MANIFEST.MF", &installer)],
        );
        let vanilla = manifest(&[("Main-Class", "net.minecraft.server.MinecraftServer")]);
        jar(
            &d.path().join("minecraft_server.1.12.2.jar"),
            &[("META-INF/MANIFEST.MF", &vanilla)],
        );
        std::fs::write(d.path().join("broken.jar"), b"not a zip").unwrap();
        assert_eq!(
            resolve_forge_family(d.path(), ForgeFlavor::Forge, ArgsOs::current()),
            ForgeInstall::Found(LaunchEntry::RootJar {
                file: "forge-1.12.2-14.23.5.2860.jar".into()
            })
        );
    }

    #[test]
    fn neoforge_never_takes_a_root_jar_and_resolves_its_folder() {
        let d = tempdir().unwrap();
        args_dir(d.path(), NEOFORGE, "21.1.10");
        forge_jar(d.path(), "forge-1.16.5-36.2.39.jar", MAIN_1165, "", &[]);
        assert_eq!(
            resolve_forge_family(d.path(), ForgeFlavor::NeoForge, ArgsOs::Unix),
            ForgeInstall::Found(LaunchEntry::ArgsFile {
                rel: format!("{NEOFORGE}/21.1.10/unix_args.txt"),
                version_dir: "21.1.10".into()
            })
        );
    }

    #[test]
    fn nothing_installed_is_absent() {
        let d = tempdir().unwrap();
        touch(&d.path().join("server.properties"));
        assert_eq!(
            resolve_forge_family(d.path(), ForgeFlavor::Forge, ArgsOs::current()),
            ForgeInstall::Absent
        );
        assert_eq!(version_dirs(d.path(), ForgeFlavor::Forge), Ok(Vec::new()));
    }

    #[test]
    fn forge_folder_names_follow_the_465_rule() {
        assert_eq!(
            split_forge_raw("1.7.10-10.13.4.1614-1.7.10"),
            Some(("1.7.10".into(), "10.13.4.1614".into()))
        );
        assert_eq!(
            split_forge_raw("1.20.1-47.2.0"),
            Some(("1.20.1".into(), "47.2.0".into()))
        );
        // Branch builds parse to a non-version MC: nothing to say.
        assert_eq!(split_forge_raw("1.10.2-12.18.0.2007-1.10.0"), None);
        assert_eq!(split_forge_raw("1.7.10-10.13.3.1401-1710ls"), None);
    }

    #[test]
    fn a_renamed_jar_takes_the_mc_from_the_vanilla_jar_it_runs() {
        let d = tempdir().unwrap();
        forge_jar(
            d.path(),
            "start.jar",
            MAIN_1122,
            "minecraft_server.1.12.2.jar",
            &[],
        );
        touch(&d.path().join("minecraft_server.1.12.2.jar"));
        assert_eq!(
            root_jar_versions(d.path(), "start.jar"),
            (Answer::Known("1.12.2".into()), None)
        );
    }

    #[test]
    fn a_jar_name_and_its_vanilla_jar_that_disagree_cannot_be_told() {
        let d = tempdir().unwrap();
        forge_jar(
            d.path(),
            "forge-1.12.2-14.23.5.2860.jar",
            MAIN_1122,
            "minecraft_server.1.7.10.jar",
            &[],
        );
        assert_eq!(
            root_jar_versions(d.path(), "forge-1.12.2-14.23.5.2860.jar"),
            (Answer::CannotTell, Some("14.23.5.2860".into()))
        );
    }

    #[test]
    fn a_legacy_jar_reports_the_class_path_files_it_is_missing() {
        let d = tempdir().unwrap();
        legacy_1710(d.path());
        std::fs::remove_file(d.path().join("minecraft_server.1.7.10.jar")).unwrap();
        assert_eq!(
            root_jar_missing(d.path(), "forge-1.7.10-10.13.4.1614-1.7.10-universal.jar"),
            vec!["minecraft_server.1.7.10.jar".to_string()]
        );
    }

    fn bundled_launcher(root: &Path, file: &str, game: &str, loader: &str) {
        let props = format!("fabric-loader-version={loader}\ngame-version={game}");
        jar(&root.join(file), &[("install.properties", props.as_str())]);
    }

    #[test]
    fn fabric_26_bundled_launcher_names_both_versions() {
        let d = tempdir().unwrap();
        bundled_launcher(d.path(), "server.jar", "26.1", "0.19.5");
        touch(
            &d.path()
                .join("libraries/net/fabricmc/fabric-loader/0.19.5/fabric-loader-0.19.5.jar"),
        );
        assert_eq!(
            fabric_family_versions(d.path(), FabricKind::Fabric),
            FabricVersions {
                mc: Answer::Known("26.1".into()),
                loader: Answer::Known("0.19.5".into()),
            }
        );
    }

    fn installer_style(root: &Path, launch_jar: &str, loader_path: &str, mc: &str) {
        let class_path = format!(
            "libraries/net/fabricmc/sponge-mixin/0.17.4/sponge-mixin-0.17.4.jar {loader_path} libraries/net/fabricmc/intermediary/{mc}/intermediary-{mc}.jar libraries/org/ow2/asm/asm/9.9/asm-9.9.jar"
        );
        let mf = manifest(&[
            (
                "Main-Class",
                "net.fabricmc.loader.impl.launch.server.FabricServerLauncher",
            ),
            ("Class-Path", class_path.as_str()),
        ]);
        jar(
            &root.join(launch_jar),
            &[("META-INF/MANIFEST.MF", mf.as_str())],
        );
    }

    #[test]
    fn fabric_installer_style_reads_the_launch_jar_class_path() {
        let d = tempdir().unwrap();
        installer_style(
            d.path(),
            "fabric-server-launch.jar",
            "libraries/net/fabricmc/fabric-loader/0.16.10/fabric-loader-0.16.10.jar",
            "1.21.10",
        );
        // Stale folders from before the upgrade must not matter.
        touch(
            &d.path()
                .join("libraries/net/fabricmc/intermediary/1.21.9/x.jar"),
        );
        touch(
            &d.path()
                .join("libraries/net/fabricmc/fabric-loader/0.16.9/x.jar"),
        );
        assert_eq!(
            fabric_family_versions(d.path(), FabricKind::Fabric),
            FabricVersions {
                mc: Answer::Known("1.21.10".into()),
                loader: Answer::Known("0.16.10".into()),
            }
        );
    }

    #[test]
    fn fabric_installer_style_26_takes_the_mc_from_the_vanilla_jar_it_runs() {
        let d = tempdir().unwrap();
        let mf = manifest(&[(
            "Class-Path",
            "libraries/net/fabricmc/fabric-loader/0.19.5/fabric-loader-0.19.5.jar",
        )]);
        jar(
            &d.path().join("fabric-server-launch.jar"),
            &[("META-INF/MANIFEST.MF", mf.as_str())],
        );
        std::fs::write(
            d.path().join("fabric-server-launcher.properties"),
            "serverJar=vanilla-26.1.jar\n",
        )
        .unwrap();
        jar(
            &d.path().join("vanilla-26.1.jar"),
            &[("version.json", r#"{"id":"26.1"}"#)],
        );
        assert_eq!(
            fabric_family_versions(d.path(), FabricKind::Fabric),
            FabricVersions {
                mc: Answer::Known("26.1".into()),
                loader: Answer::Known("0.19.5".into()),
            }
        );
    }

    #[test]
    fn a_stale_bundled_launcher_disagreeing_with_the_live_install_cannot_be_told() {
        let d = tempdir().unwrap();
        bundled_launcher(
            d.path(),
            "fabric-server-mc.1.20.1-loader.0.15.0-launcher.1.0.1.jar",
            "1.20.1",
            "0.15.0",
        );
        installer_style(
            d.path(),
            "fabric-server-launch.jar",
            "libraries/net/fabricmc/fabric-loader/0.16.10/fabric-loader-0.16.10.jar",
            "1.21.1",
        );
        assert_eq!(
            fabric_family_versions(d.path(), FabricKind::Fabric),
            FabricVersions {
                mc: Answer::CannotTell,
                loader: Answer::CannotTell,
            }
        );
    }

    #[test]
    fn stale_version_folders_alone_cannot_be_told_apart() {
        let d = tempdir().unwrap();
        touch(&d.path().join("fabric-server-launch.jar"));
        touch(
            &d.path()
                .join("libraries/net/fabricmc/intermediary/1.21.9/x.jar"),
        );
        touch(
            &d.path()
                .join("libraries/net/fabricmc/intermediary/1.21.10/x.jar"),
        );
        touch(
            &d.path()
                .join("libraries/net/fabricmc/fabric-loader/0.16.9/x.jar"),
        );
        touch(
            &d.path()
                .join("libraries/net/fabricmc/fabric-loader/0.16.10/x.jar"),
        );
        assert_eq!(
            fabric_family_versions(d.path(), FabricKind::Fabric),
            FabricVersions {
                mc: Answer::CannotTell,
                loader: Answer::CannotTell,
            }
        );
    }

    #[test]
    fn a_single_version_folder_still_speaks_when_nothing_else_does() {
        let d = tempdir().unwrap();
        touch(
            &d.path()
                .join("libraries/net/fabricmc/intermediary/1.20.4/x.jar"),
        );
        touch(
            &d.path()
                .join("libraries/net/fabricmc/fabric-loader/0.16.5/x.jar"),
        );
        assert_eq!(
            fabric_family_versions(d.path(), FabricKind::Fabric),
            FabricVersions {
                mc: Answer::Known("1.20.4".into()),
                loader: Answer::Known("0.16.5".into()),
            }
        );
    }

    #[test]
    fn quilt_takes_the_mc_from_intermediary_not_hashed() {
        let d = tempdir().unwrap();
        // Quilt collects its Class-Path in a HashSet: order is not stable.
        let mf = manifest(&[
            ("Main-Class", "org.quiltmc.loader.impl.launch.server.QuiltServerLauncher"),
            (
                "Class-Path",
                "libraries/org/quiltmc/hashed/1.21.1+build.1/hashed-1.21.1+build.1.jar libraries/net/fabricmc/intermediary/1.21.1/intermediary-1.21.1.jar libraries/org/quiltmc/quilt-loader/0.30.1/quilt-loader-0.30.1.jar",
            ),
        ]);
        jar(
            &d.path().join("quilt-server-launch.jar"),
            &[("META-INF/MANIFEST.MF", mf.as_str())],
        );
        assert_eq!(
            fabric_family_versions(d.path(), FabricKind::Quilt),
            FabricVersions {
                mc: Answer::Known("1.21.1".into()),
                loader: Answer::Known("0.30.1".into()),
            }
        );
    }

    #[test]
    fn a_quilt_fat_launch_jar_without_class_path_says_nothing() {
        let d = tempdir().unwrap();
        let mf = manifest(&[(
            "Main-Class",
            "org.quiltmc.loader.impl.launch.server.QuiltServerLauncher",
        )]);
        jar(
            &d.path().join("quilt-server-launch.jar"),
            &[("META-INF/MANIFEST.MF", mf.as_str())],
        );
        assert_eq!(
            fabric_family_versions(d.path(), FabricKind::Quilt),
            FabricVersions {
                mc: Answer::NoEvidence,
                loader: Answer::NoEvidence,
            }
        );
    }

    #[test]
    fn manifest_continuation_lines_are_unwrapped() {
        let long = "libraries/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.jar libraries/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.jar";
        let mf = manifest(&[("Class-Path", long), ("Main-Class", "a.B")]);
        assert!(mf.lines().any(|l| l.starts_with(' ')), "fixture must wrap");
        assert_eq!(manifest_attr(&mf, "Class-Path").as_deref(), Some(long));
        assert_eq!(manifest_attr(&mf, "main-class").as_deref(), Some("a.B"));
    }
}
