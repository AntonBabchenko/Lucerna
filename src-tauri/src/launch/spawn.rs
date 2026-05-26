//! Process spawn + single-instance state + exit watcher.
//!
//! State design: the running `Child` is owned by the exit-watcher
//! task that calls `.wait().await` on it. `stop()` cannot share that
//! `&mut Child` with the watcher, so it kills by PID instead, via
//! `crate::process::kill_process_tree`.

use crate::accounts::Account;
use crate::error::{Error, Result};
use crate::instances::schema::InstanceFile;
use crate::jre::java_executable_path;
use crate::launch::args::{build_argv, ArgvInput};
use crate::launch::natives::extract_natives;
use crate::paths::{
    assets_dir, instance_dir, instance_logs_dir, instance_natives_dir, libraries_dir,
    minecraft_dir, versions_dir,
};
use crate::versions::loaders::Loader;
use crate::versions::version_json::{parse, VersionDetails};
use serde::Serialize;
use specta::Type;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use tauri::AppHandle;
use tauri_specta::Event;

#[derive(Debug, Clone, Serialize, Type, Event)]
pub struct ProcessSpawned {
    pub version_id: String,
    pub pid: u32,
}

#[derive(Debug, Clone, Serialize, Type, Event)]
pub struct ProcessExited {
    pub version_id: String,
    /// Process exit code. `-1` when the process was terminated by a
    /// signal (no code available from the OS).
    pub code: i32,
    /// Absolute path to the launch log file for this run.
    pub log_path: String,
}

struct RunningState {
    pid: u32,
    log_path: PathBuf,
}

fn state() -> &'static Mutex<Option<RunningState>> {
    static STATE: OnceLock<Mutex<Option<RunningState>>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(None))
}

// ---------------------------------------------------------------------------
// Playtime session tracking
// ---------------------------------------------------------------------------

struct SessionStart {
    instance_root: std::path::PathBuf,
    started_unix_ms: i64,
}

fn session() -> &'static Mutex<Option<SessionStart>> {
    static SESSION: OnceLock<Mutex<Option<SessionStart>>> = OnceLock::new();
    SESSION.get_or_init(|| Mutex::new(None))
}

fn note_session_start(instance_root: std::path::PathBuf) {
    let start = chrono::Utc::now().timestamp_millis();
    *session().lock().expect("playtime session mutex poisoned") = Some(SessionStart {
        instance_root,
        started_unix_ms: start,
    });
}

// ---------------------------------------------------------------------------
// Tray hide / restore
// ---------------------------------------------------------------------------

/// If the user opted in, schedule a hide-to-tray for when the spawned
/// MC process has actually opened its window. We don't hide the
/// launcher synchronously on spawn because the JVM may take 5–15
/// seconds (Mojang splash, Forge mod scan, etc.) to render anything —
/// hiding the launcher first leaves the user staring at the desktop.
fn maybe_schedule_hide_to_tray(app: &tauri::AppHandle, pid: u32) {
    let path = match crate::paths::app_file(app) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("tray: skipping hide — no app.json path: {e}");
            return;
        }
    };
    let settings = match crate::instances::store::read_app_json(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("tray: skipping hide — read failed: {e}");
            return;
        }
    };
    if !settings.general.hide_to_tray_during_game {
        return;
    }

    let app_clone = app.clone();
    tokio::spawn(async move {
        wait_for_window_ready(pid).await;
        // If MC exited during the wait (crash, fast-quit, manual
        // kill), there's nothing to hide *for* — skip popping a tray
        // icon that would immediately get removed by the exit-watcher
        // restore call.
        if !is_running() {
            return;
        }
        let app_for_hide = app_clone.clone();
        let res = app_clone.run_on_main_thread(move || {
            if let Err(e) = crate::tray::hide_to_tray(&app_for_hide) {
                eprintln!("tray: hide failed — leaving window visible: {e}");
            }
        });
        if let Err(e) = res {
            eprintln!("tray: run_on_main_thread failed: {e}");
        }
    });
}

/// Block until the spawned process has set up its input message queue
/// (= top-level window created), or a 30-second cap elapses. Long
/// enough for heavy Forge modpacks; short enough that a never-opening
/// MC doesn't leave the launcher waiting forever.
#[cfg(windows)]
async fn wait_for_window_ready(pid: u32) {
    let _ = tokio::task::spawn_blocking(move || {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, WaitForInputIdle, PROCESS_QUERY_INFORMATION,
        };
        // SYNCHRONIZE access right (0x00100000) — required by
        // WaitForInputIdle per MSDN. Not re-exported from
        // Win32::System::Threading in windows-sys, so spelled out as
        // a literal to avoid pulling the Win32_Security feature for
        // a single constant.
        const SYNCHRONIZE: u32 = 0x0010_0000;
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_INFORMATION | SYNCHRONIZE, 0, pid);
            if handle.is_null() {
                eprintln!("tray: OpenProcess failed for pid {pid} — hiding immediately");
                return;
            }
            // 0 = input idle reached, 0x102 = WAIT_TIMEOUT — both
            // fall through to hide. 0xFFFFFFFF = WAIT_FAILED.
            let result = WaitForInputIdle(handle, 30_000);
            if result == 0xFFFFFFFF {
                eprintln!("tray: WaitForInputIdle failed for pid {pid}");
            }
            CloseHandle(handle);
        }
    })
    .await;
}

/// Non-Windows fallback. The launcher is currently Windows-only; when
/// Linux/macOS land (post-v0.5.0 backlog #13) this needs platform-
/// specific window-detection (X11 `_NET_CLIENT_LIST`, NSWorkspace,
/// etc.). Until then, hide-to-tray takes effect immediately on game
/// spawn — same as the pre-2026-05-26 behaviour.
#[cfg(not(windows))]
async fn wait_for_window_ready(_pid: u32) {}

fn note_session_end() {
    let Some(start) = session()
        .lock()
        .expect("playtime session mutex poisoned")
        .take()
    else {
        return;
    };
    let end = chrono::Utc::now().timestamp_millis();
    let delta_ms = (end - start.started_unix_ms).max(0) as u64;
    let seconds = delta_ms / 1000;
    if let Err(e) = crate::playtime::record_session_at(&start.instance_root, seconds) {
        eprintln!(
            "playtime: failed to record session at {}: {e}",
            start.instance_root.display()
        );
    }
}

/// True iff a Minecraft process is currently running.
pub fn is_running() -> bool {
    state()
        .lock()
        .expect("launch state mutex poisoned")
        .is_some()
}

/// Spawn Minecraft for `instance`. `effective_version_id` is what
/// `versions::install_version` was called with (e.g. `"1.20.4"` for
/// vanilla, `"fabric-loader-0.16.5-1.20.4"` for Fabric). Returns the
/// OS PID. Emits `ProcessSpawned` immediately and `ProcessExited` when
/// the child terminates.
pub async fn start(
    instance: &InstanceFile,
    effective_version_id: &str,
    account: &Account,
    app: &AppHandle,
) -> Result<u32> {
    {
        let guard = state().lock().expect("launch state mutex poisoned");
        if guard.is_some() {
            return Err(Error::AlreadyRunning);
        }
    }

    let versions = versions_dir(app).map_err(|e| Error::io("<versions_dir>", e))?;
    let libraries = libraries_dir(app).map_err(|e| Error::io("<libraries_dir>", e))?;
    let assets = assets_dir(app).map_err(|e| Error::io("<assets_dir>", e))?;
    let game_dir = minecraft_dir(app, &instance.id).map_err(|e| Error::io("<minecraft_dir>", e))?;
    let natives_dir = instance_natives_dir(app, &instance.id)
        .map_err(|e| Error::io("<instance_natives_dir>", e))?;
    let logs_dir =
        instance_logs_dir(app, &instance.id).map_err(|e| Error::io("<instance_logs_dir>", e))?;
    // Vanilla MC client.jar lives at `versions/<mc>/<mc>.jar` only — see
    // `versions::install` comment. For synth installs, resolve to the parent
    // MC id so we don't reference the orphaned synth-path jar.
    let synth = crate::versions::loaders::parse_synth_id(effective_version_id);
    let client_jar_id = synth
        .as_ref()
        .map(|(_loader, _lv, mc)| mc.clone())
        .unwrap_or_else(|| effective_version_id.to_string());

    // The version JSON is parsed here — before the `client_jar` decision
    // below — because that decision keys off `details.main_class`.
    let version_json_path = versions
        .join(effective_version_id)
        .join(format!("{effective_version_id}.json"));
    let version_json_str = tokio::fs::read_to_string(&version_json_path)
        .await
        .map_err(|e| Error::io(version_json_path.display().to_string(), e))?;
    let details: VersionDetails = parse(&version_json_str).map_err(|e| {
        Error::io(
            version_json_path.display().to_string(),
            format!("parse: {e}"),
        )
    })?;

    // Whether to append the vanilla MC client jar to the launch
    // classpath. Modern Forge / NeoForge ship a patched MC inside their
    // libraries (`libraries/net/minecraft/client/.../client-*-srg.jar`);
    // adding the vanilla jar on top duplicates the `net.minecraft.*`
    // bytecode and, on the modern Java module-path bootstrap, crashes
    // with a JPMS ResolutionException. Legacy-era Forge (≤1.12.2,
    // launchwrapper) ships no patched MC and runtime-patches the vanilla
    // jar — `needs_vanilla_client_jar` keys this off `details.main_class`.
    let client_jar: Option<PathBuf> = if needs_vanilla_client_jar(
        synth.as_ref().map(|(loader, _, _)| *loader),
        &details.main_class,
    ) {
        Some(
            versions
                .join(&client_jar_id)
                .join(format!("{client_jar_id}.jar")),
        )
    } else {
        None
    };

    let component = details
        .java_version
        .as_ref()
        .map(|jv| jv.component.as_str())
        .unwrap_or(crate::jre::DEFAULT_LEGACY_COMPONENT);
    let java_path = java_executable_path(component, app)?;

    let os = current_os();
    let arch = current_arch();

    extract_natives(&details.libraries, &libraries, &natives_dir, os, arch).await?;

    tokio::fs::create_dir_all(&game_dir)
        .await
        .map_err(|e| Error::io(game_dir.display().to_string(), e))?;
    tokio::fs::create_dir_all(&logs_dir)
        .await
        .map_err(|e| Error::io(logs_dir.display().to_string(), e))?;

    let argv_from_manifest = build_argv(&ArgvInput {
        details: &details,
        account,
        java_path: java_path.clone(),
        libraries_dir: libraries,
        assets_dir: assets,
        natives_dir,
        game_dir: game_dir.clone(),
        client_jar,
        os,
        arch,
    })?;

    // Prepend custom JVM args: `-Xmx<max_heap_mb>m` plus whitespace-
    // split `extra_jvm_args`. Both go in BEFORE the manifest's JVM
    // args, so a manifest-supplied flag can override the user's
    // setting if it needs to (spec decision).
    let mut argv: Vec<String> = vec![format!("-Xmx{}m", instance.max_heap_mb)];
    argv.extend(instance.extra_jvm_args.split_whitespace().map(String::from));
    argv.extend(argv_from_manifest);

    let log_path = logs_dir.join(format!("{}-launch.log", local_iso_stamp()));
    let log_file = std::fs::File::create(&log_path)
        .map_err(|e| Error::io(log_path.display().to_string(), e))?;
    let log_file_err = log_file
        .try_clone()
        .map_err(|e| Error::io(log_path.display().to_string(), e))?;

    let mut child =
        crate::process::spawn_minecraft(&java_path, &argv, &game_dir, log_file, log_file_err)?;
    let pid = child.id().ok_or_else(|| Error::JavaSpawn {
        details: "spawned but no PID available".into(),
    })?;

    let version_id_owned = effective_version_id.to_string();
    let log_path_owned = log_path.clone();

    {
        let mut guard = state().lock().expect("launch state mutex poisoned");
        *guard = Some(RunningState {
            pid,
            log_path: log_path_owned.clone(),
        });
    }

    let _ = ProcessSpawned {
        version_id: version_id_owned.clone(),
        pid,
    }
    .emit(app);

    // Record session start. `instance_dir` resolves to
    // `<app_data>/instances/<id>`, which is the instance root expected
    // by `crate::playtime::record_session_at`.
    let inst_root = instance_dir(app, &instance.id).map_err(|e| Error::io("<instance_dir>", e))?;
    note_session_start(inst_root);
    maybe_schedule_hide_to_tray(app, pid);

    let app_clone = app.clone();
    tokio::spawn(async move {
        let exit_code = child
            .wait()
            .await
            .ok()
            .and_then(|st| st.code())
            .unwrap_or(-1);
        let log_path_to_emit = {
            let mut guard = state().lock().expect("launch state mutex poisoned");
            let prev = guard.take();
            prev.map(|s| s.log_path).unwrap_or(log_path_owned)
        };
        let _ = ProcessExited {
            version_id: version_id_owned,
            code: exit_code,
            log_path: log_path_to_emit.to_string_lossy().into_owned(),
        }
        .emit(&app_clone);
        // Record session end. Fires for both clean (code 0) and crash
        // (non-zero / -1) exits — the spec requires we always persist
        // the duration regardless of exit reason.
        note_session_end();
        // Restore window from tray. Idempotent — no-op when the window
        // was never hidden (hide_to_tray_during_game was off).
        let _ = crate::tray::restore_from_tray(&app_clone);
    });

    Ok(pid)
}

/// Kill the running MC process if any. Idempotent: returns Ok(()) if
/// nothing is running. Uses platform-native kill-by-PID so it doesn't
/// have to share `&mut Child` with the exit-watcher task.
pub fn stop() -> Result<()> {
    let pid_opt = {
        let guard = state().lock().expect("launch state mutex poisoned");
        guard.as_ref().map(|s| s.pid)
    };
    let Some(pid) = pid_opt else {
        return Ok(());
    };
    crate::process::kill_process_tree(pid);
    Ok(())
}

/// `mainClass` of the legacy launchwrapper era (Minecraft ≤ 1.12.2).
/// Legacy-era Forge launches through launchwrapper and runtime-patches
/// the vanilla client jar, so the vanilla jar must be on the classpath.
const LAUNCHWRAPPER_MAIN_CLASS: &str = "net.minecraft.launchwrapper.Launch";

/// Whether the launch classpath needs the vanilla MC client jar.
///
/// Vanilla / Fabric / Quilt always need it — no patched MC; the loader
/// transforms bytecode at runtime. Forge / NeoForge need it ONLY in the
/// legacy launchwrapper era (MC ≤ 1.12.2): legacy Forge ships no patched
/// MC and runtime-patches the vanilla jar. Modern Forge / NeoForge ship
/// a patched MC inside their libraries — adding the vanilla jar there
/// duplicates `net.minecraft.*` bytecode and, on the modern Java
/// module-path bootstrap, throws a JPMS `ResolutionException`.
///
/// The version JSON's `mainClass` is the era signal: legacy Forge uses
/// `net.minecraft.launchwrapper.Launch`; modern Forge uses
/// `cpw.mods.modlauncher.Launcher` (1.13–1.16) or
/// `cpw.mods.bootstraplauncher.BootstrapLauncher` (1.17+); NeoForge uses
/// `BootstrapLauncher`.
fn needs_vanilla_client_jar(loader: Option<Loader>, main_class: &str) -> bool {
    match loader {
        Some(Loader::Forge) | Some(Loader::NeoForge) => main_class == LAUNCHWRAPPER_MAIN_CLASS,
        // Vanilla (None) / Fabric / Quilt.
        _ => true,
    }
}

fn current_os() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

fn current_arch() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "x86"
    }
}

/// Filename-safe stamp. v0.1.0 emits `launch-<unix-seconds>` rather
/// than a wall-clock ISO; pulling `chrono` for cosmetics felt overkill.
/// Slice 7's logs viewer sorts by mtime anyway.
fn local_iso_stamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("launch-{secs}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_os_is_known() {
        assert!(["windows", "macos", "linux"].contains(&current_os()));
    }

    #[test]
    fn current_arch_is_known() {
        assert!(["x64", "aarch64", "x86"].contains(&current_arch()));
    }

    #[test]
    fn local_iso_stamp_is_nonempty_and_filename_safe() {
        let s = local_iso_stamp();
        assert!(!s.is_empty());
        for ch in s.chars() {
            assert!(
                ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'),
                "stamp char {ch:?} not filename-safe",
            );
        }
    }

    #[test]
    fn needs_vanilla_jar_legacy_forge_true() {
        assert!(needs_vanilla_client_jar(
            Some(Loader::Forge),
            "net.minecraft.launchwrapper.Launch",
        ));
    }

    #[test]
    fn needs_vanilla_jar_modern_forge_bootstraplauncher_false() {
        assert!(!needs_vanilla_client_jar(
            Some(Loader::Forge),
            "cpw.mods.bootstraplauncher.BootstrapLauncher",
        ));
    }

    #[test]
    fn needs_vanilla_jar_modern_forge_modlauncher_false() {
        assert!(!needs_vanilla_client_jar(
            Some(Loader::Forge),
            "cpw.mods.modlauncher.Launcher",
        ));
    }

    #[test]
    fn needs_vanilla_jar_neoforge_false() {
        assert!(!needs_vanilla_client_jar(
            Some(Loader::NeoForge),
            "cpw.mods.bootstraplauncher.BootstrapLauncher",
        ));
    }

    #[test]
    fn needs_vanilla_jar_vanilla_true() {
        assert!(needs_vanilla_client_jar(
            None,
            "net.minecraft.client.main.Main"
        ));
    }

    #[test]
    fn needs_vanilla_jar_fabric_true() {
        assert!(needs_vanilla_client_jar(
            Some(Loader::Fabric),
            "net.fabricmc.loader.impl.launch.knot.KnotClient",
        ));
    }

    #[test]
    fn needs_vanilla_jar_quilt_true() {
        assert!(needs_vanilla_client_jar(
            Some(Loader::Quilt),
            "org.quiltmc.loader.impl.launch.knot.KnotClient",
        ));
    }

    // `is_running()` / `stop()` share process-wide state; behavioural
    // tests live in the manual e2e step. Pure helpers above are the
    // unit-test surface here.
}
