//! Process spawn + per-instance run state + exit watcher.
//!
//! State design: running processes are tracked in a keyed
//! `ProcessRegistry<ClientRun>` (id → pid + run data), so multiple instances
//! can run at once. Each running `Child` is owned by its exit-watcher task,
//! which calls `.wait().await` on it. `stop(id)` cannot share that `&mut Child`
//! with the watcher, so it kills by PID instead, via
//! `crate::platform::kill_process_tree`. Removal is pid-matched (`remove_if_pid`)
//! so a stop()+relaunch's stale watcher never evicts the fresh entry.

use crate::accounts::Account;
use crate::error::{Error, Result};
use crate::instances::schema::InstanceFile;
use crate::jre::java_executable_path;
use crate::launch::args::{build_argv, ArgvInput};
use crate::launch::natives::extract_natives;
use crate::launch::quick_play::QuickPlay;
use crate::paths::{
    assets_dir, instance_dir, instance_logs_dir, instance_natives_dir, libraries_dir,
    minecraft_dir, versions_dir,
};
use crate::process::registry::ProcessRegistry;
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
    pub instance_id: String,
    pub version_id: String,
    pub pid: u32,
}

#[derive(Debug, Clone, Serialize, Type, Event)]
pub struct ProcessExited {
    pub instance_id: String,
    pub version_id: String,
    /// Process exit code. `-1` when the process was terminated by a signal.
    pub code: i32,
    /// True when the exit was caused by the user pressing Stop.
    pub user_requested: bool,
    /// Absolute path to the launch log file for this run.
    pub log_path: String,
}

/// Per-running-instance data. `pid` lives in the registry entry; this carries
/// what the RAM guardrail and the account-conflict check need.
#[derive(Clone)]
struct ClientRun {
    max_heap_mb: u32,
    /// Account id this instance was launched with — lets `account_in_use`
    /// warn when the same account is about to be used by a second instance.
    account_id: String,
}

fn registry() -> &'static ProcessRegistry<ClientRun> {
    static REG: OnceLock<ProcessRegistry<ClientRun>> = OnceLock::new();
    REG.get_or_init(ProcessRegistry::new)
}

/// True iff `instance_id` currently has a running process.
pub fn is_running(instance_id: &str) -> bool {
    registry().is_running(instance_id)
}

/// True iff ANY instance is running (tray + app-exit teardown).
pub fn is_any_running() -> bool {
    registry().is_any_running()
}

/// `(instance_id, pid, max_heap_mb)` for every running instance.
pub fn running_snapshot() -> Vec<(String, u32, u32)> {
    registry()
        .snapshot()
        .into_iter()
        .map(|(id, pid, run)| (id, pid, run.max_heap_mb))
        .collect()
}

/// The instance_id of a currently-running instance launched with `account_id`,
/// if any, excluding `exclude_instance_id` (the candidate about to launch — it
/// must never be reported as conflicting with itself). Used to warn about the
/// same account being used by two running instances (online-session conflict on
/// real servers).
///
/// If multiple running instances share the account, which one is named is
/// arbitrary (HashMap order); the conflict statement is true regardless.
pub fn account_in_use(account_id: &str, exclude_instance_id: &str) -> Option<String> {
    registry()
        .snapshot()
        .into_iter()
        .find(|(id, _, run)| id != exclude_instance_id && run.account_id == account_id)
        .map(|(id, _, _)| id)
}

// ---------------------------------------------------------------------------
// User-initiated stop tracking
// ---------------------------------------------------------------------------
//
// `stop()` force-kills the process tree, which the OS reports as a non-zero
// exit code (Windows) or a signal-termination / `-1` (Unix) — indistinguishable
// from a crash at the exit-watcher. This set carries the "the user asked for
// this" intent from `stop()` through to the per-instance `ProcessExited` event.
// Keyed by instance id so one instance's Stop never mislabels another's exit.

fn stop_requested() -> &'static Mutex<std::collections::HashSet<String>> {
    static SET: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    SET.get_or_init(|| Mutex::new(std::collections::HashSet::new()))
}

/// Record that the user requested `instance_id` be stopped.
fn mark_stop_requested(instance_id: &str) {
    stop_requested()
        .lock()
        .expect("stop-requested set poisoned")
        .insert(instance_id.to_string());
}

/// Read and clear the stop request for `instance_id` — true iff a stop was
/// requested since it was last cleared.
fn take_stop_requested(instance_id: &str) -> bool {
    stop_requested()
        .lock()
        .expect("stop-requested set poisoned")
        .remove(instance_id)
}

// ---------------------------------------------------------------------------
// Playtime session tracking
// ---------------------------------------------------------------------------

struct SessionStart {
    instance_root: std::path::PathBuf,
    started_unix_ms: i64,
}

fn sessions() -> &'static Mutex<std::collections::HashMap<String, SessionStart>> {
    static SESSIONS: OnceLock<Mutex<std::collections::HashMap<String, SessionStart>>> =
        OnceLock::new();
    SESSIONS.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

fn note_session_start(instance_id: &str, instance_root: std::path::PathBuf) {
    let start = chrono::Utc::now().timestamp_millis();
    sessions()
        .lock()
        .expect("playtime session mutex poisoned")
        .insert(
            instance_id.to_string(),
            SessionStart {
                instance_root,
                started_unix_ms: start,
            },
        );
}

/// Close the in-flight playtime session and return the instance root plus the
/// session length in seconds, so the caller can journal the launch attempt
/// without duplicating the session bookkeeping. `None` when no session was
/// open (a stale watcher, or a start that never registered).
fn note_session_end(instance_id: &str) -> Option<(std::path::PathBuf, u64)> {
    let start = sessions()
        .lock()
        .expect("playtime session mutex poisoned")
        .remove(instance_id)?;
    let end = chrono::Utc::now().timestamp_millis();
    let seconds = ((end - start.started_unix_ms).max(0) as u64) / 1000;
    if let Err(e) = crate::playtime::record_session_at(&start.instance_root, seconds) {
        crate::diag!(
            "playtime: failed to record session at {}: {e}",
            start.instance_root.display()
        );
    }
    Some((start.instance_root, seconds))
}

/// Wall-clock start time (unix ms) of the in-flight playtime session for
/// `instance_id`, if it is running. Lets the UI show live elapsed playtime.
pub fn session_started_ms(instance_id: &str) -> Option<i64> {
    sessions()
        .lock()
        .expect("playtime session mutex poisoned")
        .get(instance_id)
        .map(|s| s.started_unix_ms)
}

// ---------------------------------------------------------------------------
// Tray hide / restore
// ---------------------------------------------------------------------------

/// Hide the launcher to tray on launch only when the user opted in AND this is
/// the FIRST running instance (no instance was running before this one).
fn should_hide_on_launch(opted_in: bool, was_any_running_before: bool) -> bool {
    opted_in && !was_any_running_before
}

/// If the user opted in, schedule a hide-to-tray for when the spawned
/// MC process has actually opened its window. We don't hide the
/// launcher synchronously on spawn because the JVM may take 5–15
/// seconds (Mojang splash, Forge mod scan, etc.) to render anything —
/// hiding the launcher first leaves the user staring at the desktop.
fn maybe_schedule_hide_to_tray(
    app: &tauri::AppHandle,
    instance_id: &str,
    pid: u32,
    was_any_running_before: bool,
) {
    let path = match crate::paths::app_file(app) {
        Ok(p) => p,
        Err(e) => {
            crate::diag!("tray: skipping hide — no app.json path: {e}");
            return;
        }
    };
    let settings = match crate::instances::store::read_app_json(&path) {
        Ok(s) => s,
        Err(e) => {
            crate::diag!("tray: skipping hide — read failed: {e}");
            return;
        }
    };
    let opted_in = settings.general.hide_to_tray_during_game;
    if !should_hide_on_launch(opted_in, was_any_running_before) {
        return;
    }

    let instance_id = instance_id.to_string();
    let app_clone = app.clone();
    tokio::spawn(async move {
        crate::platform::wait_for_window_ready(pid).await;
        // If MC exited during the wait (crash, fast-quit, manual
        // kill), there's nothing to hide *for* — skip popping a tray
        // icon that would immediately get removed by the exit-watcher
        // restore call.
        if !is_running(&instance_id) {
            return;
        }
        let app_for_hide = app_clone.clone();
        let res = app_clone.run_on_main_thread(move || {
            if let Err(e) = crate::tray::hide_to_tray(&app_for_hide) {
                crate::diag!("tray: hide failed — leaving window visible: {e}");
            }
        });
        if let Err(e) = res {
            crate::diag!("tray: run_on_main_thread failed: {e}");
        }
    });
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
    quick_play: Option<&QuickPlay>,
) -> Result<u32> {
    // Reserve THIS instance's id across the whole spawn. `claim_start` returns
    // None if the id is already running OR already starting — closing the TOCTOU
    // window where two rapid `start()` calls could both spawn a Minecraft for
    // the same instance. The claim's `Drop` releases the reservation if any step
    // before `insert` fails (via `?` early-return); `claim.commit()` releases it
    // once the live process is registered and the exit-watcher owns teardown.
    let claim = registry()
        .claim_start(&instance.id)
        .ok_or_else(|| Error::AlreadyRunning {
            instance_id: instance.id.clone(),
        })?;
    // Fresh run: clear any stale stop request for THIS instance.
    let _ = take_stop_requested(&instance.id);

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
    // MC id so we don't reference the orphaned synth-path jar. We know the MC
    // version from the instance, so use the unambiguous mc-aware parse (a
    // loader version may itself contain `-`, e.g. `0.24.0-beta.1`).
    let synth_loader = crate::versions::loaders::parse_synth_id_with_mc(
        effective_version_id,
        &instance.mc_version,
    )
    .map(|(loader, _lv)| loader);
    let client_jar_id = if synth_loader.is_some() {
        instance.mc_version.clone()
    } else {
        effective_version_id.to_string()
    };

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
    let client_jar: Option<PathBuf> = if needs_vanilla_client_jar(synth_loader, &details.main_class)
    {
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
        quick_play,
    })?;

    // Prepend custom JVM args: `-Xmx<heap>m` plus the sanitised
    // `extra_jvm_args`. Both go in BEFORE the manifest's JVM args, so a
    // manifest-supplied flag can override the user's setting if it needs to
    // (spec decision). The heap is clamped into a launchable range and the
    // extra args are sanitised here, at the point of use, so a stale or
    // hostile instance file cannot produce a JVM that refuses to start or an
    // unbounded/control-char arg blob.
    let heap_mb = crate::launch::args::clamp_heap_mb(
        instance.max_heap_mb,
        crate::platform::total_system_ram_mb(),
    );
    // Initial heap (-Xms, when set) + max heap (-Xmx). Built by a pure helper so
    // the clamp (Xms never exceeds Xmx, or the JVM refuses to start) is unit
    // tested. Both go BEFORE the manifest's JVM args so a manifest flag can
    // still override.
    let mut argv: Vec<String> = crate::launch::args::heap_args(instance.min_heap_mb, heap_mb);
    argv.extend(crate::launch::args::sanitize_jvm_args(
        &instance.extra_jvm_args,
    ));
    argv.extend(argv_from_manifest);

    // GPU preference: best-effort, never blocks launch.
    let gpu_pref = crate::paths::app_file(app)
        .ok()
        .and_then(|p| crate::instances::store::read_app_json(&p).ok())
        .map(|af| af.general.gpu_preference)
        .unwrap_or_default();
    // Windows: keep the registry entry for THIS javaw in sync with the setting.
    if let Err(e) = crate::platform::gpu::sync_for_exe(&java_path, gpu_pref) {
        crate::diag!("gpu: registry sync failed for {}: {e}", java_path.display());
    }
    // Linux: env vars on the child (empty elsewhere).
    let gpu_env = crate::platform::gpu::launch_env(gpu_pref);

    let log_path = logs_dir.join(unique_launch_log_name());
    let log_file = std::fs::File::create(&log_path)
        .map_err(|e| Error::io(log_path.display().to_string(), e))?;
    let log_file_err = log_file
        .try_clone()
        .map_err(|e| Error::io(log_path.display().to_string(), e))?;

    // resolved before spawn so no fallible `?` can fire after a live child exists (orphan-leak guard).
    // `instance_dir` resolves to `<app_data>/instances/<id>`, the instance root
    // expected by `crate::playtime::record_session_at`.
    let inst_root = instance_dir(app, &instance.id).map_err(|e| Error::io("<instance_dir>", e))?;

    let mut child = crate::process::spawn_minecraft(
        &java_path,
        &argv,
        &game_dir,
        &gpu_env,
        log_file,
        log_file_err,
    )?;
    let pid = child.id().ok_or_else(|| Error::JavaSpawn {
        details: "spawned but no PID available".into(),
    })?;

    let version_id_owned = effective_version_id.to_string();
    let log_path_owned = log_path.clone();

    // Snapshot liveness BEFORE this instance is registered so the tray hide
    // fires only for the first running instance.
    let was_any_running_before = registry().is_any_running();

    registry().insert(
        &instance.id,
        pid,
        ClientRun {
            max_heap_mb: heap_mb,
            account_id: account.id.clone(),
        },
    );
    // Registry now holds the live process; the exit-watcher owns teardown.
    claim.commit();

    let _ = ProcessSpawned {
        instance_id: instance.id.clone(),
        version_id: version_id_owned.clone(),
        pid,
    }
    .emit(app);

    note_session_start(&instance.id, inst_root);
    maybe_schedule_hide_to_tray(app, &instance.id, pid, was_any_running_before);

    let instance_id_for_retention = instance.id.to_string();
    let instance_id_for_event = instance.id.to_string();
    let app_clone = app.clone();
    tokio::spawn(async move {
        let exit_code = child
            .wait()
            .await
            .ok()
            .and_then(|st| st.code())
            .unwrap_or(-1);
        // ABA-safe: only clear/announce if this map entry is still OUR pid. A
        // stop()+relaunch could have inserted a NEW pid under this id before
        // this stale watcher woke; `remove_if_pid` refuses to evict it.
        if !registry().remove_if_pid(&instance_id_for_event, pid) {
            return;
        }
        // Consume the stop request: did this exit come from the user pressing
        // Stop for THIS instance?
        let user_requested = take_stop_requested(&instance_id_for_event);
        let log_path_for_journal = log_path_owned.to_string_lossy().into_owned();
        let _ = ProcessExited {
            instance_id: instance_id_for_event.clone(),
            version_id: version_id_owned,
            code: exit_code,
            user_requested,
            log_path: log_path_for_journal.clone(),
        }
        .emit(&app_clone);
        // Record session end. Fires for both clean (code 0) and crash
        // (non-zero / -1) exits — the spec requires we always persist
        // the duration regardless of exit reason.
        //
        // The same close-out feeds the instance journal: one row per FINISHED
        // launch, classified by the same (user_requested, exit_code) pair the
        // `ProcessExited` event above carries, so the history can never
        // disagree with the exit toast the user just saw.
        if let Some((inst_root, seconds)) = note_session_end(&instance_id_for_event) {
            crate::journal::record(
                &inst_root,
                crate::journal::JournalEvent::Launch {
                    outcome: crate::journal::launch_outcome(user_requested, exit_code),
                    exit_code: Some(exit_code),
                    duration_seconds: seconds as f64,
                    log_path: Some(log_path_for_journal),
                },
            );
        }
        // Opt-in old-log cleanup. Runs after the session's logs have
        // settled (MC rotates latest.log at START, so the full set is
        // complete at exit). Non-fatal: a cleanup failure must never
        // block process teardown or tray restore.
        if let Err(e) =
            crate::logs::retention::apply_from_settings(&app_clone, &instance_id_for_retention)
        {
            crate::diag!("log-retention: cleanup failed for {instance_id_for_retention}: {e}");
        }
        // Restore window from tray on ANY exit. Idempotent — no-op when the
        // window was never hidden (hide_to_tray_during_game was off).
        //
        // MUST run on the main (GUI) thread, exactly like the hide path
        // above. `restore_from_tray` removes the icon via
        // `remove_tray_by_id` (which calls `TrayIcon::close()`), and on
        // Windows that teardown (`Shell_NotifyIcon`) is a GUI-thread
        // operation — it has to run on the thread that created the icon.
        // Off-thread, the icon was left painted in the tray and every
        // subsequent launch stacked another orphan.
        let app_for_restore = app_clone.clone();
        let res = app_clone.run_on_main_thread(move || {
            if let Err(e) = crate::tray::restore_from_tray(&app_for_restore) {
                crate::diag!("tray: restore failed: {e}");
            }
        });
        if let Err(e) = res {
            crate::diag!("tray: run_on_main_thread (restore) failed: {e}");
        }
    });

    Ok(pid)
}

/// Kill the running MC process for `instance_id` if any. Idempotent: returns
/// Ok(()) if that instance is not running. Uses platform-native kill-by-PID so
/// it doesn't have to share `&mut Child` with the exit-watcher task.
pub fn stop(instance_id: &str) -> Result<()> {
    let Some(pid) = registry().pid_of(instance_id) else {
        return Ok(());
    };
    // Mark before the kill so the exit-watcher (which wakes on `.wait()`
    // returning) sees the intent and labels the exit as user-requested.
    mark_stop_requested(instance_id);
    crate::platform::kill_process_tree(pid);
    Ok(())
}

/// Force-kill every tracked client process (called on app exit so no Minecraft
/// is orphaned). Best-effort; clears the registry.
pub fn kill_all_running() {
    for (id, pid, _heap) in running_snapshot() {
        mark_stop_requested(&id);
        crate::platform::kill_process_tree(pid);
        // record playtime ourselves: we remove the registry entry below, so the
        // exit-watcher's remove_if_pid will no-op and skip note_session_end.
        // Same reason the journal row is written here: without it, quitting the
        // launcher with the game running would drop that launch from history.
        // No exit code is observable on this path — we killed the process and
        // dropped the registry entry before anything could read a status.
        if let Some((inst_root, seconds)) = note_session_end(&id) {
            crate::journal::record(
                &inst_root,
                crate::journal::JournalEvent::Launch {
                    outcome: crate::journal::LaunchOutcome::Stopped,
                    exit_code: None,
                    duration_seconds: seconds as f64,
                    log_path: None,
                },
            );
        }
        registry().remove(&id);
    }
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

/// Collision-safe launch-log filename. The stamp is `launch-<unix-seconds>`
/// (Slice 7's logs viewer sorts by mtime, so seconds granularity is fine for
/// ordering), but a crash-loop relaunch within the same wall-clock second
/// would otherwise reuse the exact filename and truncate the previous run's
/// log — destroying the crash evidence we need to diagnose the loop. A
/// per-process monotonic counter disambiguates same-second relaunches.
fn unique_launch_log_name() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("launch-{secs}-{n}.log")
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
    fn unique_launch_log_name_is_filename_safe_and_distinct() {
        let a = unique_launch_log_name();
        let b = unique_launch_log_name();
        assert!(!a.is_empty());
        assert!(a.ends_with(".log"));
        // Two calls in the same second must not collide (counter suffix).
        assert_ne!(a, b, "same-second relaunches must get distinct names");
        for ch in a.chars() {
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

    // The stop-requested set is keyed by instance id; this test owns a unique
    // id start-to-finish so it never races another test's key.
    #[test]
    fn stop_requested_mark_take_round_trip() {
        let id = "inst-stop-unit-test";
        assert!(!take_stop_requested(id), "unmarked reads false");
        mark_stop_requested(id);
        assert!(take_stop_requested(id), "marked reads true");
        assert!(!take_stop_requested(id), "take consumes the flag");
    }

    #[test]
    fn tray_hides_only_on_first_when_opted_in() {
        assert!(should_hide_on_launch(true, false));
        assert!(!should_hide_on_launch(true, true));
        assert!(!should_hide_on_launch(false, false));
    }

    // These two touch the process-wide registry/sessions singletons; each owns
    // a unique instance id and cleans up its own entry so it never interferes
    // with another test's view of global run state.

    #[test]
    fn account_in_use_finds_matching_running_instance() {
        let id = "inst-account-in-use-unit-test";
        let other = "inst-account-in-use-unit-test-OTHER";
        let account_id = "account-in-use-unit-test-xyz";
        assert_eq!(
            account_in_use(account_id, other),
            None,
            "no entry initially"
        );
        registry().insert(
            id,
            424_242,
            ClientRun {
                max_heap_mb: 2048,
                account_id: account_id.to_string(),
            },
        );
        // Excluding a DIFFERENT instance still reports the match.
        assert_eq!(account_in_use(account_id, other).as_deref(), Some(id));
        // Excluding the matching instance's OWN id must not report a self-conflict.
        assert_eq!(
            account_in_use(account_id, id),
            None,
            "an instance never conflicts with itself"
        );
        assert_eq!(
            account_in_use("some-other-account-unit-test", other),
            None,
            "a different account is not reported in use"
        );
        registry().remove(id);
        assert_eq!(
            account_in_use(account_id, other),
            None,
            "removal clears the entry"
        );
    }

    #[test]
    fn session_started_ms_reports_running_session_start() {
        let id = "inst-session-started-unit-test";
        assert_eq!(session_started_ms(id), None, "no session initially");
        note_session_start(
            id,
            std::path::PathBuf::from("/nonexistent/session-unit-test"),
        );
        let ms = session_started_ms(id).expect("running session has a start ms");
        assert!(ms > 0, "start ms is a real unix timestamp");
        // Remove directly (not via note_session_end) to avoid the bogus-root
        // playtime write; we only need to clear the global map entry.
        sessions()
            .lock()
            .expect("playtime session mutex poisoned")
            .remove(id);
        assert_eq!(session_started_ms(id), None, "removal clears the session");
    }
}
