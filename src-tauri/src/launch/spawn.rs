//! Process spawn + single-instance state + exit watcher.
//!
//! State design: the running `Child` is owned by the exit-watcher
//! task that calls `.wait().await` on it. `stop()` cannot share that
//! `&mut Child` with the watcher, so we kill by PID instead — on
//! Windows via `taskkill /F /T /PID <pid>` which also terminates the
//! child's child processes (MC sometimes spawns helpers).

use crate::accounts::Account;
use crate::error::{Error, Result};
use crate::instances::schema::InstanceFile;
use crate::jre::java_executable_path;
use crate::launch::args::{build_argv, ArgvInput};
use crate::launch::natives::extract_natives;
use crate::paths::{
    assets_dir, instance_logs_dir, instance_natives_dir, libraries_dir, minecraft_dir,
    versions_dir,
};
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
    let game_dir = minecraft_dir(app, &instance.id)
        .map_err(|e| Error::io("<minecraft_dir>", e))?;
    let natives_dir = instance_natives_dir(app, &instance.id)
        .map_err(|e| Error::io("<instance_natives_dir>", e))?;
    let logs_dir = instance_logs_dir(app, &instance.id)
        .map_err(|e| Error::io("<instance_logs_dir>", e))?;
    // Vanilla MC client.jar lives at `versions/<mc>/<mc>.jar` only — see
    // `versions::install` comment. For synth installs, resolve to the parent
    // MC id so we don't reference the orphaned synth-path jar.
    //
    // Forge / NeoForge ship a patched MC inside their libraries (under
    // `libraries/net/minecraft/client/.../client-*-srg.jar`) which the
    // loader's own discovery (MinecraftLocator etc.) picks up. Adding the
    // vanilla jar to the classpath on top duplicates the `net.minecraft.*`
    // bytecode and, on modern Java module-path bootstraps
    // (cpw.mods.bootstraplauncher.BootstrapLauncher / ForgeBootstrap),
    // crashes with a JPMS ResolutionException:
    //   Module minecraft contains package net.minecraft.obfuscate,
    //   module _1._20._4 exports package net.minecraft.obfuscate to minecraft
    // Vanilla / Fabric / Quilt do need the vanilla jar on the classpath
    // (no patched MC; loader transforms bytecode at runtime).
    let synth = crate::versions::loaders::parse_synth_id(effective_version_id);
    let client_jar_id = synth
        .as_ref()
        .map(|(_loader, _lv, mc)| mc.clone())
        .unwrap_or_else(|| effective_version_id.to_string());
    let client_jar: Option<PathBuf> = match synth.as_ref().map(|(loader, _, _)| loader) {
        Some(crate::versions::loaders::Loader::Forge)
        | Some(crate::versions::loaders::Loader::NeoForge) => None,
        _ => Some(
            versions
                .join(&client_jar_id)
                .join(format!("{client_jar_id}.jar")),
        ),
    };
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
    let mut argv: Vec<String> =
        vec![format!("-Xmx{}m", instance.max_heap_mb)];
    argv.extend(
        instance
            .extra_jvm_args
            .split_whitespace()
            .map(String::from),
    );
    argv.extend(argv_from_manifest);

    let log_path = logs_dir.join(format!("{}-launch.log", local_iso_stamp()));
    let log_file = std::fs::File::create(&log_path)
        .map_err(|e| Error::io(log_path.display().to_string(), e))?;
    let log_file_err = log_file
        .try_clone()
        .map_err(|e| Error::io(log_path.display().to_string(), e))?;

    let mut cmd = tokio::process::Command::new(&java_path);
    cmd.args(&argv)
        .current_dir(&game_dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(log_file))
        .stderr(std::process::Stdio::from(log_file_err));

    let mut child = cmd.spawn().map_err(|e| Error::JavaSpawn {
        details: format!("spawn {}: {e}", java_path.display()),
    })?;
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
    kill_pid(pid);
    Ok(())
}

#[cfg(target_os = "windows")]
fn kill_pid(pid: u32) {
    // /F = force, /T = kill child processes too (MC sometimes spawns
    // helpers). Best-effort; if taskkill itself fails (e.g. PID
    // already gone) we ignore the error — the exit watcher will fire
    // ProcessExited regardless of cause.
    let _ = std::process::Command::new("taskkill")
        .args(["/F", "/T", "/PID", &pid.to_string()])
        .status();
}

#[cfg(not(target_os = "windows"))]
fn kill_pid(pid: u32) {
    // POSIX path is post-v0.1.0 — included so the cfg-gate doesn't
    // leave non-Windows builds with an unreachable function.
    let _ = std::process::Command::new("kill")
        .arg(pid.to_string())
        .status();
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

    // `is_running()` / `stop()` share process-wide state; behavioural
    // tests live in the manual e2e step. Pure helpers above are the
    // unit-test surface here.
}
