//! The client-side processor stage of a Forge/NeoForge install, as Forge's own
//! installer runs it (`PostProcessors.process`, spec 2026-10-16 §5.1).
//!
//! First, for every client processor, everything that can fail without Java is
//! checked: its jar and classpath are present (`prepare` fetched them, pinned,
//! with `install_profile.libraries`; a missing one is an error, as in Forge's
//! installer) and its arguments resolve. Only then, for each processor in turn:
//! - if it declares outputs and every one exists with its declared SHA-1, skip
//!   it; a mismatching output is deleted;
//! - otherwise run it, and check each declared output again: a mismatch deletes
//!   the file and fails the install.
//!
//! Processors without declared outputs always run when the stage runs. The
//! stage runs on a first install, and on a rebuild when a declared output is
//! stale or the patched client jar is missing (`ensure_processor_outputs`). A
//! processor that fails in the middle of a rebuild leaves the install for
//! another Repair, as with Forge's installer: its outputs have no old copy kept.

use crate::error::{Error, Result};
use crate::forge::installer::transitional::{
    substitute_args, DataEntry, InstallProfile, ProcessorEntry,
};
use crate::forge::patcher::{maven_coord_to_relative_path, ProcessorContext};
use crate::versions::version_json::Library;
use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};

/// Everything the stage reads, as plain paths: no `AppHandle`, so the test
/// harnesses can run it.
#[derive(Debug, Clone)]
pub struct StageEnv {
    pub libs_root: PathBuf,
    /// `{ROOT}`: `forge/cache/<mc>-<fv>`, not the installation root Forge's
    /// installer uses. Only NeoForge FML10's PROCESS_MINECRAFT_JAR writes under
    /// it (`--extract-libraries-to {ROOT}/libraries/`), as today.
    pub cache_dir: PathBuf,
    /// `{INSTALLER}`, the cached installer jar.
    pub installer_path: PathBuf,
    /// `{MINECRAFT_JAR}`, the vanilla client jar.
    pub minecraft_jar: PathBuf,
    pub java_bin: Option<PathBuf>,
}

/// The stage's one effect on the outside world.
pub trait StageIo {
    /// Run one processor (`crate::forge::patcher::run_processor` in production).
    fn run(
        &self,
        coord: &str,
        args: Vec<String>,
        ctx: &ProcessorContext,
    ) -> impl Future<Output = Result<()>> + Send;
}

/// One file a client processor declares, with its SHA-1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredOutput {
    pub processor: String,
    pub path: PathBuf,
    pub sha1: String,
}

/// Client-side processors, in install order (`sides` absent or naming "client").
pub fn client_processors(profile: &InstallProfile) -> impl Iterator<Item = &ProcessorEntry> {
    profile.processors.iter().filter(|p| {
        p.sides
            .as_ref()
            .is_none_or(|sides| sides.iter().any(|s| s == "client"))
    })
}

/// The library path a `[group:artifact:version[:classifier][@ext]]` reference
/// names under `libs_root`.
fn library_ref(raw: &str, libs_root: &Path) -> Option<PathBuf> {
    let coord = raw.strip_prefix('[')?.strip_suffix(']')?;
    maven_coord_to_relative_path(coord).map(|rel| libs_root.join(rel))
}

/// A `{KEY}` reference to the profile's client-side data value.
fn data_value<'a>(raw: &str, data: &'a HashMap<String, DataEntry>) -> Option<&'a str> {
    let key = raw.strip_prefix('{')?.strip_suffix('}')?;
    data.get(key).map(|e| e.client.as_str())
}

/// One processor's declared outputs. Keys are `{DATA}` or `[coord]`, resolving
/// to a library path. Values are `{DATA}` or `'hex'`, resolving to a SHA-1.
/// Anything else is a corrupted installer: every output in today's installers
/// is a library coordinate (spec §5.3.3). Sorted by path, for a stable order.
pub fn declared_outputs(
    p: &ProcessorEntry,
    data: &HashMap<String, DataEntry>,
    libs_root: &Path,
) -> std::result::Result<Vec<DeclaredOutput>, String> {
    let mut out = Vec::with_capacity(p.outputs.len());
    for (key, value) in &p.outputs {
        let key_raw = data_value(key, data).unwrap_or(key);
        let path = library_ref(key_raw, libs_root)
            .ok_or_else(|| format!("{}: output {key} is not a library coordinate", p.jar))?;
        let value_raw = data_value(value, data).unwrap_or(value);
        let sha1 = value_raw
            .strip_prefix('\'')
            .and_then(|s| s.strip_suffix('\''))
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("{}: output {key} has no SHA-1 ({value})", p.jar))?;
        out.push(DeclaredOutput {
            processor: p.jar.clone(),
            path,
            sha1: sha1.to_ascii_lowercase(),
        });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// Every output the install's client processors declare (spec §5.3).
pub fn client_declared_outputs(
    profile: &InstallProfile,
    libs_root: &Path,
) -> std::result::Result<Vec<DeclaredOutput>, String> {
    let mut all = Vec::new();
    for p in client_processors(profile) {
        all.extend(declared_outputs(p, &profile.data, libs_root)?);
    }
    Ok(all)
}

/// Whether every declared output exists with its SHA-1. A mismatching one is
/// deleted, as Forge's installer does, so the run rewrites it. A delete that
/// fails is an error: a stale file left in place would be "current" next time.
async fn outputs_current(outputs: &[DeclaredOutput]) -> Result<bool> {
    let mut all = true;
    for o in outputs {
        match crate::mods::txn::file_sha1(&o.path).await {
            Ok(Some(got)) if got == o.sha1 => {}
            Ok(Some(got)) => {
                crate::diag!(
                    "forge: {} output {} is stale (expected {}, found {got}), rebuilding",
                    o.processor,
                    o.path.display(),
                    o.sha1
                );
                match tokio::fs::remove_file(&o.path).await {
                    Ok(()) => {}
                    // Gone between the hash and the delete: nothing left to replace.
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => {
                        return Err(Error::ForgePatcherFailed {
                            processor: o.processor.clone(),
                            details: format!(
                                "could not delete the stale {}: {e}",
                                o.path.display()
                            ),
                        })
                    }
                }
                all = false;
            }
            Ok(None) => all = false,
            Err(e) => return Err(Error::io(o.path.display().to_string(), e)),
        }
    }
    Ok(all)
}

/// Whether every declared output exists with its SHA-1, without touching any
/// file. For `ensure_processor_outputs`' pre-check.
pub(crate) async fn outputs_all_match(outputs: &[DeclaredOutput]) -> Result<bool> {
    for o in outputs {
        match crate::mods::txn::file_sha1(&o.path).await {
            Ok(Some(got)) if got == o.sha1 => {}
            Ok(_) => return Ok(false),
            Err(e) => return Err(Error::io(o.path.display().to_string(), e)),
        }
    }
    Ok(true)
}

/// After a run: every declared output must exist with its SHA-1. A mismatching
/// file is deleted so the next attempt re-runs instead of trusting it.
async fn check_outputs_after_run(outputs: &[DeclaredOutput]) -> Result<()> {
    for o in outputs {
        match crate::mods::txn::file_sha1(&o.path).await {
            Ok(Some(got)) if got == o.sha1 => {}
            Ok(Some(got)) => {
                crate::diag!(
                    "forge: {} wrote {} with SHA-1 {got}, but the installer declares {}",
                    o.processor,
                    o.path.display(),
                    o.sha1
                );
                let removal = match tokio::fs::remove_file(&o.path).await {
                    Ok(()) => String::new(),
                    Err(e) => format!("; the file could not be deleted: {e}"),
                };
                return Err(Error::ForgeProcessorOutputMismatch {
                    processor: o.processor.clone(),
                    details: format!(
                        "{}: expected {}, got {got}{removal}",
                        o.path.display(),
                        o.sha1
                    ),
                });
            }
            Ok(None) => {
                return Err(Error::ForgePatcherFailed {
                    processor: o.processor.clone(),
                    details: format!("{} is missing after the run", o.path.display()),
                })
            }
            Err(e) => return Err(Error::io(o.path.display().to_string(), e)),
        }
    }
    Ok(())
}

/// A processor's jar or classpath entry must be a file before it runs, as in
/// Forge's installer ("Missing Jar for processor", "Missing Processor
/// Dependencies").
async fn require_file(path: &Path, processor: &str) -> Result<()> {
    match tokio::fs::metadata(path).await {
        Ok(m) if m.is_file() => Ok(()),
        Ok(_) => Err(Error::ForgePatcherFailed {
            processor: processor.to_string(),
            details: format!("{} is not a file", path.display()),
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(Error::ForgePatcherFailed {
            processor: processor.to_string(),
            details: format!("missing {}", path.display()),
        }),
        Err(e) => Err(Error::io(path.display().to_string(), e)),
    }
}

fn coord_path(coord: &str, libs_root: &Path, processor: &str) -> Result<PathBuf> {
    maven_coord_to_relative_path(coord)
        .map(|rel| libs_root.join(rel))
        .ok_or_else(|| Error::ForgePatcherFailed {
            processor: processor.to_string(),
            details: format!("cannot read the maven coordinate {coord:?}"),
        })
}

/// Run the client processors of `profile` (spec §5.1).
pub async fn run_client_processors(
    profile: &InstallProfile,
    installer_bytes: &[u8],
    env: &StageEnv,
    io: &impl StageIo,
) -> Result<()> {
    let text = |p: &Path| p.display().to_string();
    // Everything that can fail without Java, for every processor, before any
    // stale output is deleted (review 2026-10-17).
    let mut planned = Vec::new();
    for p in client_processors(profile) {
        let outputs = declared_outputs(p, &profile.data, &env.libs_root).map_err(|why| {
            Error::ForgeInstallerCorrupted {
                mc: profile.minecraft.clone(),
                fv: profile.version.clone(),
                details: why,
            }
        })?;
        let jar = coord_path(&p.jar, &env.libs_root, &p.jar)?;
        let mut classpath = vec![jar.clone()];
        for c in &p.classpath {
            classpath.push(coord_path(c, &env.libs_root, &p.jar)?);
        }
        for f in &classpath {
            require_file(f, &p.jar).await?;
        }
        let args = substitute_args(
            &p.args,
            &profile.data,
            "client",
            &env.libs_root,
            installer_bytes,
            &text(&env.installer_path),
            &text(&env.cache_dir),
            &text(&env.minecraft_jar),
        )
        .await?;
        let ctx = ProcessorContext {
            jar,
            classpath,
            cache_dir: env.cache_dir.clone(),
            java_bin: env.java_bin.clone(),
        };
        planned.push((p, outputs, args, ctx));
    }
    for (p, outputs, args, ctx) in planned {
        if !outputs.is_empty() && outputs_current(&outputs).await? {
            crate::diag!("forge: {} skipped, its declared outputs are current", p.jar);
            continue;
        }
        crate::diag!("forge: running {}", p.jar);
        io.run(&p.jar, args, &ctx).await?;
        check_outputs_after_run(&outputs).await?;
    }
    Ok(())
}

/// Production I/O: libraries through `versions::libraries::ensure_libraries`,
/// processors through `forge::patcher::run_processor`.
pub struct AppStageIo {
    pub app: tauri::AppHandle,
}

impl StageIo for AppStageIo {
    async fn run(&self, coord: &str, args: Vec<String>, ctx: &ProcessorContext) -> Result<()> {
        crate::forge::patcher::run_processor(coord, args, ctx).await
    }
}

/// One lock for every processor stage. The outputs are not per Forge version:
/// the MCP-versioned slim, extra and srg jars and the mappings are shared by
/// every Forge build of one Minecraft version, and on 1.21.1 by Forge and
/// NeoForge. Stages are rare, so one lock costs nothing (review 2026-10-17).
pub(crate) async fn stage_lock() -> tokio::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await
}

/// Everything a processor run needs before it starts, shared by first installs
/// (`modern`/`transitional`) and rebuilds (`ensure_processor_outputs`):
/// - the vanilla client jar (FART and jarsplitter read `{MINECRAFT_JAR}`);
/// - Lucerna's runtime for this Minecraft (processors never run on a Java the
///   user chose: their output goes into the shared libraries cache);
/// - the installer's `maven/` tree;
/// - every downloadable `install_profile.libraries` entry, SHA-1-pinned. The
///   processor jars are among them.
pub(crate) async fn prepare(
    profile: &InstallProfile,
    installer_bytes: &[u8],
    mc: &str,
    fv: &str,
    app: &tauri::AppHandle,
) -> Result<StageEnv> {
    let app_data = crate::paths::app_dir(app).map_err(|e| Error::io("<app_data>", e))?;
    let cache_dir = app_data
        .join("forge")
        .join("cache")
        .join(format!("{mc}-{fv}"));
    tokio::fs::create_dir_all(&cache_dir)
        .await
        .map_err(|e| Error::io(cache_dir.display().to_string(), e))?;
    let libs_root = crate::paths::libraries_dir(app).map_err(|e| Error::io("<libraries>", e))?;
    let installer_path = crate::forge::meta::installer_cache_path(&app_data, mc, fv);
    let minecraft_jar = app_data.join("versions").join(mc).join(format!("{mc}.jar"));

    let vanilla_details = crate::versions::install::ensure_version_json(mc, app).await?;
    let vanilla_major = crate::java::required_major(&vanilla_details);
    let component = vanilla_details
        .java_version
        .as_ref()
        .map(|jv| jv.component.clone())
        .unwrap_or_else(|| crate::jre::DEFAULT_LEGACY_COMPONENT.to_string());
    let client_download =
        vanilla_details
            .downloads
            .as_ref()
            .ok_or_else(|| Error::ForgeInstallerCorrupted {
                mc: mc.into(),
                fv: fv.into(),
                details: "vanilla version JSON has no downloads field".into(),
            })?;
    // Discarding the downloaded-vs-cached flag: the Forge install path builds no
    // install report.
    let _ = crate::versions::client::ensure_client(mc, &client_download.client, app).await?;
    crate::jre::ensure_jre(&component, app, |_, _, _| {})
        .await
        .map_err(|e| {
            crate::jre::unavailable(
                e,
                &component,
                vanilla_major,
                crate::error::JreStage::InstallerTools,
                app,
            )
        })?;
    let java_bin = crate::jre::java_executable_path(&component, app)?;

    super::transitional::extract_installer_maven_tree_pub(installer_bytes, &libs_root).await?;

    let downloadable: Vec<Library> = profile
        .libraries
        .iter()
        .filter(|l| {
            let from_top = l.url.as_deref().is_some_and(|s| !s.is_empty());
            let from_artifact = l
                .downloads
                .as_ref()
                .and_then(|d| d.artifact.as_ref())
                .is_some_and(|a| !a.url.is_empty());
            from_top || from_artifact
        })
        .cloned()
        .collect();
    crate::versions::libraries::ensure_libraries(
        &downloadable,
        crate::versions::install::current_os(),
        crate::versions::install::current_arch(),
        app,
    )
    .await?;

    Ok(StageEnv {
        libs_root,
        cache_dir,
        installer_path,
        minecraft_jar,
        java_bin: Some(java_bin),
    })
}

/// The patched client jar the binarypatcher writes (`data.PATCHED`), relative to
/// the libraries root. Some eras declare it as an output, others do not; verify
/// checks it either way (`verify::plan::forge_artifacts`).
pub(crate) fn patched_rel_path(profile: &InstallProfile) -> Option<String> {
    profile
        .data
        .get("PATCHED")
        .and_then(|e| e.client.strip_prefix('['))
        .and_then(|s| s.strip_suffix(']'))
        .and_then(maven_coord_to_relative_path)
}

/// Whether an installed version needs the stage again: a declared output is
/// missing or stale, or the patched client jar is not a file. Reads only.
async fn stage_needed(declared: &[DeclaredOutput], patched: Option<&Path>) -> Result<bool> {
    if !outputs_all_match(declared).await? {
        return Ok(true);
    }
    let Some(patched) = patched else {
        return Ok(false);
    };
    match tokio::fs::metadata(patched).await {
        Ok(m) => Ok(!m.is_file()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(e) => Err(Error::io(patched.display().to_string(), e)),
    }
}

/// Rebuilds in flight. A rebuild rewrites shared library files in place, so
/// `launch::start` refuses while one runs, and a rebuild refuses while a game
/// runs or starts.
static REBUILDS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// True while a rebuild rewrites installed Forge files (`launch::start` checks).
pub fn rebuild_in_progress() -> bool {
    REBUILDS.load(std::sync::atomic::Ordering::SeqCst) > 0
}

/// Marks a rebuild for as long as it lives; panic-safe.
struct Rebuilding;

impl Rebuilding {
    fn enter() -> Self {
        REBUILDS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Rebuilding
    }
}

impl Drop for Rebuilding {
    fn drop(&mut self) {
        REBUILDS.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// A game is running, or on its way: a rebuild would rewrite files it reads.
fn games_active() -> bool {
    crate::launch::is_any_running() || crate::launch::spawn::is_any_starting()
}

/// The launchwrapper era (Forge <= 1.12.2) has no processors.
const LAUNCHWRAPPER_MAIN_CLASS: &str = "net.minecraft.launchwrapper.Launch";

/// For an installed Forge/NeoForge version: if a declared processor output is
/// missing or stale, or the patched client jar is missing, run the client stage
/// again, as a reinstall with Forge's installer would (spec §5.3.2). Old Lucerna
/// installs were built by the removed Rust patchers, whose bytes did not match
/// on the versions measured, and this is their path to the official ones.
///
/// - A legacy profile returns before touching the installer, so offline repair
///   of legacy Forge works as before.
/// - Otherwise the installer is needed to know what is declared. It is cached;
///   `fetch_installer_bytes` re-fetches it if the cache is gone.
/// - A rebuild is refused while any game runs or starts: the outputs are
///   shared, and a game reads them. It marks itself (`rebuild_in_progress`)
///   before its last check, and `launch::start` claims its slot before
///   checking the mark, so one of the two always sees the other.
/// - `on_rebuild` fires once, before a rebuild, for progress.
pub async fn ensure_processor_outputs(
    flavor: crate::forge::ForgeFlavor,
    mc: &str,
    fv: &str,
    main_class: &str,
    app: &tauri::AppHandle,
    on_rebuild: impl FnOnce(),
) -> Result<()> {
    if main_class == LAUNCHWRAPPER_MAIN_CLASS {
        return Ok(());
    }
    let corrupted = |details: String| Error::ForgeInstallerCorrupted {
        mc: mc.into(),
        fv: fv.into(),
        details,
    };
    let bytes = crate::forge::meta::fetch_installer_bytes(flavor, mc, fv, app).await?;
    let value = super::read_install_profile(&bytes).map_err(corrupted)?;
    if super::detect_era(&value) == super::Era::Legacy {
        return Ok(());
    }
    let text = serde_json::to_string(&value).map_err(|e| corrupted(e.to_string()))?;
    let profile = super::transitional::parse_install_profile(&text)?;
    let libs_root = crate::paths::libraries_dir(app).map_err(|e| Error::io("<libraries>", e))?;
    let declared = client_declared_outputs(&profile, &libs_root).map_err(corrupted)?;
    let patched = patched_rel_path(&profile).map(|rel| libs_root.join(rel));
    if !stage_needed(&declared, patched.as_deref()).await? {
        return Ok(());
    }
    if games_active() {
        return Err(Error::InstanceBusy);
    }
    let env = prepare(&profile, &bytes, mc, fv, app).await?;
    let _stage = stage_lock().await;
    // Another task may have rebuilt while we waited for the lock.
    if !stage_needed(&declared, patched.as_deref()).await? {
        return Ok(());
    }
    let _rebuilding = Rebuilding::enter();
    if games_active() {
        return Err(Error::InstanceBusy);
    }
    on_rebuild();
    run_client_processors(&profile, &bytes, &env, &AppStageIo { app: app.clone() }).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forge::installer::transitional::parse_install_profile;
    use sha1::{Digest, Sha1};
    use std::sync::Mutex;

    fn sha(b: &[u8]) -> String {
        hex::encode(Sha1::digest(b))
    }

    /// Profile: P1 (client, declares OUT with sha("good")), P2 (no outputs),
    /// P3 (server only). Paths resolve under the temp libraries root.
    fn profile(out_sha: &str) -> InstallProfile {
        parse_install_profile(&format!(
            r#"{{"spec":1,"profile":"forge","version":"1.20.1-forge-x","minecraft":"1.20.1",
                "data":{{
                  "OUT":{{"client":"[g:out:1]","server":"[g:out:1]"}},
                  "OUT_SHA":{{"client":"'{out_sha}'","server":"'x'"}}
                }},
                "processors":[
                  {{"jar":"g:p1:1","classpath":["g:dep:1"],"args":["--o","{{OUT}}"],
                    "outputs":{{"{{OUT}}":"{{OUT_SHA}}"}}}},
                  {{"jar":"g:p2:1","args":[]}},
                  {{"jar":"g:p3:1","sides":["server"],"args":[]}}
                ]}}"#
        ))
        .unwrap()
    }

    /// Records calls; "running" P1 writes `writes` to OUT.
    struct Fake {
        out: PathBuf,
        writes: &'static [u8],
        ran: Mutex<Vec<String>>,
    }

    impl StageIo for Fake {
        async fn run(
            &self,
            coord: &str,
            _args: Vec<String>,
            _ctx: &ProcessorContext,
        ) -> Result<()> {
            self.ran.lock().unwrap().push(coord.to_string());
            if coord == "g:p1:1" {
                std::fs::create_dir_all(self.out.parent().unwrap()).unwrap();
                std::fs::write(&self.out, self.writes).unwrap();
            }
            Ok(())
        }
    }

    fn env(root: &Path) -> StageEnv {
        StageEnv {
            libs_root: root.join("libraries"),
            cache_dir: root.join("cache"),
            installer_path: root.join("installer.jar"),
            minecraft_jar: root.join("1.20.1.jar"),
            java_bin: None,
        }
    }

    /// The processor jars and classpath must exist, as Forge's installer
    /// requires ("Missing Jar for processor").
    fn place_jars(root: &Path) {
        for rel in ["g/p1/1/p1-1.jar", "g/dep/1/dep-1.jar", "g/p2/1/p2-1.jar"] {
            let p = root.join("libraries").join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"jar").unwrap();
        }
    }

    fn out_path(root: &Path) -> PathBuf {
        root.join("libraries/g/out/1/out-1.jar")
    }

    #[tokio::test]
    async fn runs_client_processors_in_order_and_skips_server_ones() {
        let td = tempfile::tempdir().unwrap();
        place_jars(td.path());
        let io = Fake {
            out: out_path(td.path()),
            writes: b"good",
            ran: Mutex::default(),
        };
        run_client_processors(&profile(&sha(b"good")), b"", &env(td.path()), &io)
            .await
            .unwrap();
        assert_eq!(*io.ran.lock().unwrap(), vec!["g:p1:1", "g:p2:1"]);
    }

    #[tokio::test]
    async fn a_processor_whose_outputs_are_current_is_skipped() {
        let td = tempfile::tempdir().unwrap();
        place_jars(td.path());
        std::fs::create_dir_all(out_path(td.path()).parent().unwrap()).unwrap();
        std::fs::write(out_path(td.path()), b"good").unwrap();
        let io = Fake {
            out: out_path(td.path()),
            writes: b"good",
            ran: Mutex::default(),
        };
        run_client_processors(&profile(&sha(b"good")), b"", &env(td.path()), &io)
            .await
            .unwrap();
        assert_eq!(
            *io.ran.lock().unwrap(),
            vec!["g:p2:1"],
            "P1 cache hit, P2 always runs"
        );
    }

    #[tokio::test]
    async fn a_stale_output_is_deleted_and_rebuilt() {
        let td = tempfile::tempdir().unwrap();
        place_jars(td.path());
        std::fs::create_dir_all(out_path(td.path()).parent().unwrap()).unwrap();
        std::fs::write(out_path(td.path()), b"rust-made").unwrap();
        let io = Fake {
            out: out_path(td.path()),
            writes: b"good",
            ran: Mutex::default(),
        };
        run_client_processors(&profile(&sha(b"good")), b"", &env(td.path()), &io)
            .await
            .unwrap();
        assert_eq!(std::fs::read(out_path(td.path())).unwrap(), b"good");
        assert_eq!(io.ran.lock().unwrap()[0], "g:p1:1");
    }

    #[tokio::test]
    async fn a_wrong_output_after_the_run_fails_and_is_deleted() {
        let td = tempfile::tempdir().unwrap();
        place_jars(td.path());
        let io = Fake {
            out: out_path(td.path()),
            writes: b"bad",
            ran: Mutex::default(),
        };
        let err = run_client_processors(&profile(&sha(b"good")), b"", &env(td.path()), &io)
            .await
            .unwrap_err();
        assert!(
            matches!(err, Error::ForgeProcessorOutputMismatch { ref processor, .. } if processor == "g:p1:1"),
            "{err:?}"
        );
        assert!(!out_path(td.path()).exists(), "the bad file is deleted");
        assert_eq!(
            *io.ran.lock().unwrap(),
            vec!["g:p1:1"],
            "the stage stops at the failure"
        );
    }

    #[tokio::test]
    async fn a_missing_output_after_the_run_fails() {
        let td = tempfile::tempdir().unwrap();
        place_jars(td.path());
        // P1 "runs" but writes nothing.
        let io = Fake {
            out: td.path().join("elsewhere.jar"),
            writes: b"good",
            ran: Mutex::default(),
        };
        let err = run_client_processors(&profile(&sha(b"good")), b"", &env(td.path()), &io)
            .await
            .unwrap_err();
        assert!(matches!(err, Error::ForgePatcherFailed { .. }), "{err:?}");
    }

    #[tokio::test]
    async fn a_missing_processor_jar_is_refused_before_running() {
        let td = tempfile::tempdir().unwrap(); // no jars placed
        let io = Fake {
            out: out_path(td.path()),
            writes: b"good",
            ran: Mutex::default(),
        };
        let err = run_client_processors(&profile(&sha(b"good")), b"", &env(td.path()), &io)
            .await
            .unwrap_err();
        assert!(matches!(err, Error::ForgePatcherFailed { .. }), "{err:?}");
        assert!(io.ran.lock().unwrap().is_empty());
    }

    // Review 2026-10-17: everything that can fail without Java is checked for
    // every processor before any stale output is deleted.
    #[tokio::test]
    async fn a_stale_output_stays_when_a_later_processor_cannot_run() {
        let td = tempfile::tempdir().unwrap();
        place_jars(td.path());
        std::fs::remove_file(td.path().join("libraries/g/p2/1/p2-1.jar")).unwrap();
        std::fs::create_dir_all(out_path(td.path()).parent().unwrap()).unwrap();
        std::fs::write(out_path(td.path()), b"rust-made").unwrap();
        let io = Fake {
            out: out_path(td.path()),
            writes: b"good",
            ran: Mutex::default(),
        };
        let err = run_client_processors(&profile(&sha(b"good")), b"", &env(td.path()), &io)
            .await
            .unwrap_err();
        assert!(matches!(err, Error::ForgePatcherFailed { .. }), "{err:?}");
        assert_eq!(std::fs::read(out_path(td.path())).unwrap(), b"rust-made");
        assert!(io.ran.lock().unwrap().is_empty(), "nothing ran");
    }

    #[tokio::test]
    async fn a_missing_patched_jar_needs_the_stage_even_when_outputs_match() {
        let td = tempfile::tempdir().unwrap();
        let patched = td.path().join("patched.jar");
        assert!(stage_needed(&[], Some(&patched)).await.unwrap());
        std::fs::write(&patched, b"jar").unwrap();
        assert!(!stage_needed(&[], Some(&patched)).await.unwrap());
        assert!(!stage_needed(&[], None).await.unwrap());
    }

    #[tokio::test]
    async fn a_stale_declared_output_needs_the_stage() {
        let td = tempfile::tempdir().unwrap();
        let out = DeclaredOutput {
            processor: "g:p1:1".into(),
            path: td.path().join("out.jar"),
            sha1: sha(b"good"),
        };
        std::fs::write(&out.path, b"rust-made").unwrap();
        assert!(stage_needed(std::slice::from_ref(&out), None)
            .await
            .unwrap());
        std::fs::write(&out.path, b"good").unwrap();
        assert!(!stage_needed(std::slice::from_ref(&out), None)
            .await
            .unwrap());
    }

    // `launch::start` refuses while this is up (spec §5.3.2).
    #[test]
    fn a_rebuild_is_visible_while_it_runs() {
        assert!(!rebuild_in_progress());
        let r = Rebuilding::enter();
        assert!(rebuild_in_progress());
        drop(r);
        assert!(!rebuild_in_progress());
    }

    #[test]
    fn the_patched_jar_path_comes_from_the_profile_data() {
        let mut p = profile("ab12");
        assert_eq!(patched_rel_path(&p), None);
        p.data.insert(
            "PATCHED".into(),
            DataEntry {
                client: "[net.minecraftforge:forge:1.20.1-47.4.10:client]".into(),
                server: "x".into(),
            },
        );
        assert_eq!(
            patched_rel_path(&p).as_deref(),
            Some("net/minecraftforge/forge/1.20.1-47.4.10/forge-1.20.1-47.4.10-client.jar")
        );
    }

    #[test]
    fn declared_outputs_resolve_under_the_libraries_root() {
        let p = profile("ab12");
        let got = declared_outputs(&p.processors[0], &p.data, Path::new("L")).unwrap();
        assert_eq!(
            got,
            vec![DeclaredOutput {
                processor: "g:p1:1".into(),
                path: Path::new("L").join("g/out/1/out-1.jar"),
                sha1: "ab12".into(),
            }]
        );
    }

    #[test]
    fn an_output_that_is_not_a_library_coordinate_is_corruption() {
        let mut p = profile("ab12");
        p.data.insert(
            "OUT".into(),
            DataEntry {
                client: "/data/x.lzma".into(),
                server: "/data/x.lzma".into(),
            },
        );
        assert!(declared_outputs(&p.processors[0], &p.data, Path::new("L")).is_err());
    }

    #[test]
    fn client_declared_outputs_collects_client_processors_only() {
        let mut p = profile("ab12");
        // A server-only processor that declares an output must not count.
        p.processors[2]
            .outputs
            .insert("{OUT}".into(), "{OUT_SHA}".into());
        let got = client_declared_outputs(&p, Path::new("L")).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].processor, "g:p1:1");
    }
}
