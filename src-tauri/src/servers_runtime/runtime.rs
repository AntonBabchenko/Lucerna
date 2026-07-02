//! Долгоживущий серверный процесс: состояние, консоль-стрим, команды, стоп.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use tauri::AppHandle;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use serde::Serialize;
use specta::Type;
use tauri_specta::Event;

use crate::error::{Error, Result};
use crate::instances::schema::LoaderKind;

/// One line of server console output (stdout or stderr), streamed to the UI.
#[derive(Debug, Clone, Serialize, Type, Event)]
pub struct ServerLogLine {
    pub server_id: String,
    pub line: String,
}

/// Emitted when a server process starts.
#[derive(Debug, Clone, Serialize, Type, Event)]
pub struct ServerSpawned {
    pub server_id: String,
    pub pid: u32,
}

/// Emitted when a server process exits. `code` is -1 if signal-terminated.
#[derive(Debug, Clone, Serialize, Type, Event)]
pub struct ServerExited {
    pub server_id: String,
    pub code: i32,
}

/// Live handle for a running server. `stdin` is shared so `send_command` can
/// write to it asynchronously without holding the map lock across an await.
pub(crate) struct RunningServer {
    pub pid: u32,
    pub stdin: Arc<tokio::sync::Mutex<tokio::process::ChildStdin>>,
}

fn state() -> &'static Mutex<HashMap<String, RunningServer>> {
    static S: OnceLock<Mutex<HashMap<String, RunningServer>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Server ids whose `start()` is in flight but has not yet registered a running
/// process. Guards the TOCTOU window between the `is_running` check and the
/// `RunningServer` insert: without it, two concurrent starts both pass the gate
/// and spawn two JVMs into one world (the ERROR_LOCK_VIOLATION world-region
/// lock). Lock order is always `state()` first, then `starting()`.
fn starting() -> &'static Mutex<HashSet<String>> {
    static S: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashSet::new()))
}

/// RAII claim on an in-flight start. Held from the top of `start()` until after
/// the running entry is inserted (or any early-return error); `Drop` releases
/// the id from `starting()`.
struct StartClaim {
    id: String,
}

impl Drop for StartClaim {
    fn drop(&mut self) {
        starting()
            .lock()
            .expect("server starting set poisoned")
            .remove(&self.id);
    }
}

/// Atomically claim the start slot for `id`. Returns `None` if the server is
/// already running OR already starting, so the caller maps that to
/// `ServerAlreadyRunning`. Takes both locks in the fixed order state→starting.
fn claim_start(id: &str) -> Option<StartClaim> {
    let running = state().lock().expect("server state poisoned");
    if running.contains_key(id) {
        return None;
    }
    let mut starting_guard = starting().lock().expect("server starting set poisoned");
    if !starting_guard.insert(id.to_string()) {
        return None;
    }
    Some(StartClaim { id: id.to_string() })
}

/// True iff a server with this id currently has a running process.
pub fn is_running(id: &str) -> bool {
    state()
        .lock()
        .expect("server state poisoned")
        .contains_key(id)
}

/// PID of the running server, if any.
pub fn running_pid(id: &str) -> Option<u32> {
    state()
        .lock()
        .expect("server state poisoned")
        .get(id)
        .map(|r| r.pid)
}

/// Snapshot of (server_id, pid) for every currently-tracked running server.
/// Collected under the lock and returned owned so callers never hold the lock
/// across a kill (avoids the exit-watcher deadlock).
pub fn running_ids_snapshot() -> Vec<(String, u32)> {
    state()
        .lock()
        .expect("server state poisoned")
        .iter()
        .map(|(id, rs)| (id.clone(), rs.pid))
        .collect()
}

/// Force-kill every tracked server process. Called synchronously from the
/// launcher's exit hook so server children are never orphaned (Bug A root).
/// Best-effort: the in-memory map is cleared so a repeat call is a no-op.
pub fn kill_all_running() {
    for (_id, pid) in running_ids_snapshot() {
        crate::platform::kill_process_tree(pid);
    }
    state().lock().expect("server state poisoned").clear();
}

/// Force-kill the JVM recorded in `pid_file` when it is still alive AND still
/// ours (image match defeats PID recycling), then clear the file regardless (a
/// stale or recycled record is removed too). Returns true iff a live owned
/// process was killed. Shared by the exit sweep, `stop`, and `server_delete`.
pub fn kill_owned_pid(pid_file: &Path) -> bool {
    let Some(pid) = crate::servers_runtime::pid::read_pid(pid_file) else {
        return false;
    };
    let ours =
        crate::platform::process_alive(pid) && crate::platform::process_image_matches(pid, "java");
    if ours {
        crate::platform::kill_process_tree(pid);
    }
    // Clear unconditionally: a killed-ours pid is now dead, a stale file points
    // at nothing, and a recycled pid was never our server.
    crate::servers_runtime::pid::clear_pid(pid_file);
    ours
}

/// Sweep every server's persisted `runtime/server.pid` under `servers_root`,
/// killing any leftover JVM that is still alive and ours and clearing each PID
/// file. Complements `kill_all_running` (this session's in-memory children
/// only): a server adopted after a launcher restart lives only on disk, so
/// without this sweep it would be re-orphaned on exit (re-triggering the
/// world-lock bug). Best-effort; a missing root is a no-op.
pub fn kill_persisted_orphans(servers_root: &Path) {
    let Ok(entries) = std::fs::read_dir(servers_root) else {
        return;
    };
    for entry in entries.flatten() {
        kill_owned_pid(&entry.path().join("runtime").join("server.pid"));
    }
}

/// Decide the running/pid a `server_list` row should show, reconciling the
/// in-memory map against a persisted PID. Pure: liveness/identity are passed in
/// so it is testable without real processes. The in-memory entry (this launcher
/// session owns it) always wins; otherwise a recorded PID is adopted only when
/// it is alive AND still ours (defeats PID recycling).
pub fn reconcile_running(
    in_memory: Option<u32>,
    recorded_pid: Option<u32>,
    alive_and_ours: bool,
) -> (bool, Option<u32>) {
    if let Some(pid) = in_memory {
        return (true, Some(pid));
    }
    match recorded_pid {
        Some(pid) if alive_and_ours => (true, Some(pid)),
        _ => (false, None),
    }
}

/// Console JVM: on Windows `jre::java_executable_path` returns `javaw.exe`
/// (no console → no stdout). Servers stream stdout, so swap to `java.exe`.
pub(crate) fn console_java_path(javaw: &Path) -> PathBuf {
    match javaw.file_name().and_then(|n| n.to_str()) {
        Some("javaw.exe") => javaw.with_file_name("java.exe"),
        _ => javaw.to_path_buf(),
    }
}

/// MC version JSON `java_version.component`, else Mojang's legacy default.
pub(crate) fn java_component_or_legacy(component: Option<&str>) -> String {
    component
        .unwrap_or(crate::jre::DEFAULT_LEGACY_COMPONENT)
        .to_string()
}

/// Build the JVM argv to launch an assembled server, per loader. Paths are
/// relative to `runtime/` (the spawn cwd). Forge/NeoForge use the installer-
/// generated `@argfile` mechanism; the args file lives under libraries/.
///
/// The user's `extra_jvm_args` blob is tokenized with the SAME sanitizer the
/// client launch uses (`crate::launch::args::sanitize_jvm_args`: drops
/// control-char tokens, caps total length) and spliced AFTER `-Xmx` and BEFORE
/// `-jar` / `@user_jvm_args.txt`, so a flag like `-XX:+UseG1GC` reaches the JVM
/// exactly as it does for a client instance. An empty/whitespace blob adds
/// nothing.
pub(crate) fn build_launch_argv(
    loader: LoaderKind,
    runtime: &Path,
    heap_mb: u32,
    extra_jvm_args: &str,
) -> Result<Vec<String>> {
    let xmx = format!("-Xmx{heap_mb}m");
    let extra = crate::launch::args::sanitize_jvm_args(extra_jvm_args);
    match loader {
        LoaderKind::Vanilla | LoaderKind::Fabric | LoaderKind::Quilt => {
            let mut argv = vec![xmx];
            argv.extend(extra);
            argv.extend(["-jar".into(), "server.jar".into(), "nogui".into()]);
            Ok(argv)
        }
        LoaderKind::Forge | LoaderKind::NeoForge => {
            let args_rel =
                find_loader_args_file(runtime).ok_or_else(|| Error::ServerSpawnFailed {
                    details: "installer args file not found under libraries/".into(),
                })?;
            let mut argv = vec![xmx];
            argv.extend(extra);
            argv.extend([
                "@user_jvm_args.txt".into(),
                format!("@{args_rel}"),
                "nogui".into(),
            ]);
            Ok(argv)
        }
    }
}

/// Relative path (from runtime/) of the installer-generated args file for the
/// current OS, e.g. `libraries/net/neoforged/neoforge/<v>/win_args.txt`.
pub(crate) fn find_loader_args_file(runtime: &Path) -> Option<String> {
    let name = if cfg!(windows) {
        "win_args.txt"
    } else {
        "unix_args.txt"
    };
    for base in [
        "libraries/net/neoforged/neoforge",
        "libraries/net/minecraftforge/forge",
    ] {
        if let Ok(rd) = std::fs::read_dir(runtime.join(base)) {
            for e in rd.flatten() {
                if e.path().join(name).exists() {
                    return Some(format!("{base}/{}/{name}", e.file_name().to_string_lossy()));
                }
            }
        }
    }
    None
}

/// Start an assembled server. Resolves the MC Java component, ensures the JRE,
/// builds per-loader argv, spawns with piped stdio, tees stdout+stderr to
/// `runtime/logs/server-latest.log` while emitting `ServerLogLine`, and
/// registers an exit watcher that emits `ServerExited` and clears state.
/// Returns the OS pid. Errors `ServerAlreadyRunning` if already up.
pub async fn start(app: &AppHandle, server_id: &str) -> Result<u32> {
    // Atomically claim the start slot (held until the running entry is inserted
    // below) so two concurrent starts can't both spawn a JVM into one world.
    let _claim = claim_start(server_id).ok_or_else(|| Error::ServerAlreadyRunning {
        id: server_id.to_string(),
    })?;

    let base = crate::paths::app_dir(app).map_err(|e| Error::io("<app_dir>", e))?;
    let p = crate::paths::server_paths(&base, server_id);
    let file = crate::servers_runtime::store::read_server_json(&p.json)?;
    crate::servers_runtime::eula::require_accepted(file.eula_accepted)?;

    // Prefer the Java component cached at create time so a server that ran once
    // starts offline. Fall back to resolving it over the network (pre-existing
    // servers) and lazy-persist it so the next launch is offline-capable too.
    let component = match &file.java_component {
        Some(c) => c.clone(),
        None => {
            let c = crate::servers_runtime::create::resolve_server_java_component(&file.mc_version)
                .await?;
            let mut upgraded = file.clone();
            upgraded.java_component = Some(c.clone());
            let _ = crate::servers_runtime::store::write_server_json(&p.json, &upgraded);
            c
        }
    };
    crate::jre::ensure_jre(&component, app, |_, _, _| {}).await?;
    let javaw = crate::jre::java_executable_path(&component, app)?;
    let java = console_java_path(&javaw);

    let argv = build_launch_argv(
        file.loader,
        &p.runtime,
        file.max_heap_mb,
        &file.extra_jvm_args,
    )?;

    std::fs::create_dir_all(&p.logs).map_err(|e| Error::io(p.logs.display().to_string(), e))?;
    // Preserve the previous session's log before truncating: rotate it to a
    // timestamped archive, then bound the archives with keep-N.
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let _ = crate::servers_runtime::serverlog::rotate_log(&p.logs, &stamp);
    // Honor the user's log-retention policy for server logs too (parity with
    // instance logs); default-on-error falls back to serverlog's safety floor.
    let retention = crate::paths::app_file(app)
        .ok()
        .and_then(|f| crate::instances::store::read_app_json(&f).ok())
        .map(|s| s.general.log_retention)
        .unwrap_or_default();
    crate::servers_runtime::serverlog::prune_logs(&p.logs, &retention);
    let log_path = p.logs.join("server-latest.log");
    let log_file = std::fs::File::create(&log_path)
        .map_err(|e| Error::io(log_path.display().to_string(), e))?;
    let log = Arc::new(Mutex::new(log_file));

    let mut child = crate::process::spawn_server(&java, &argv, &p.runtime)?;
    let pid = child.id().ok_or_else(|| Error::ServerSpawnFailed {
        details: "spawned but no pid".into(),
    })?;
    let stdin = child.stdin.take().ok_or_else(|| Error::ServerSpawnFailed {
        details: "no stdin handle".into(),
    })?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    state().lock().expect("server state poisoned").insert(
        server_id.to_string(),
        RunningServer {
            pid,
            stdin: Arc::new(tokio::sync::Mutex::new(stdin)),
        },
    );
    // Persist the PID so server_list can reconcile after a launcher restart
    // (Bug A part 2) and the diagnoser can offer "stop the leftover process".
    let _ = crate::servers_runtime::pid::write_pid(&p.pid, pid);
    // A successful (re)start makes any prior exit code stale — clear it so a
    // running server can't show a leftover "Crashed" status (#18).
    crate::servers_runtime::exit_state::clear(&p.runtime);
    let _ = ServerSpawned {
        server_id: server_id.to_string(),
        pid,
    }
    .emit(app);

    if let Some(out) = stdout {
        spawn_pump(out, app.clone(), server_id.to_string(), log.clone());
    }
    if let Some(err) = stderr {
        spawn_pump(err, app.clone(), server_id.to_string(), log.clone());
    }

    let app_exit = app.clone();
    let id_exit = server_id.to_string();
    let pid_path = p.pid.clone();
    let runtime_path = p.runtime.clone();
    let watched_pid = pid;
    tokio::spawn(async move {
        let code = child.wait().await.ok().and_then(|s| s.code()).unwrap_or(-1);
        // Only clear/announce the exit if this map entry is still OUR process:
        // a force-kill in stop() may have already removed us, and a restart may
        // have inserted a new pid under the same id — never evict that (ABA).
        let was_current = {
            let mut map = state().lock().expect("server state poisoned");
            if map.get(&id_exit).map(|rs| rs.pid) == Some(watched_pid) {
                map.remove(&id_exit);
                true
            } else {
                false
            }
        };
        if was_current {
            crate::servers_runtime::pid::clear_pid(&pid_path);
            // Record the exit code so server_list can show "Crashed" vs "Stopped" (#18).
            crate::servers_runtime::exit_state::write(&runtime_path, code);
            let _ = ServerExited {
                server_id: id_exit,
                code,
            }
            .emit(&app_exit);
        }
    });

    Ok(pid)
}

/// Spawn a task that reads `r` line-by-line, appends each line to `log`, and
/// emits a `ServerLogLine`. Works for both ChildStdout and ChildStderr.
fn spawn_pump<R>(r: R, app: AppHandle, id: String, log: Arc<Mutex<std::fs::File>>)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(r).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Ok(mut f) = log.lock() {
                let _ = writeln!(f, "{line}");
            }
            let _ = ServerLogLine {
                server_id: id.clone(),
                line,
            }
            .emit(&app);
        }
    });
}

/// Write `line` + newline to the running server's stdin (a console command,
/// e.g. `say hi`, `op Steve`, `stop`). The stdin handle is cloned out from
/// under the (std) map lock, the lock is released, THEN the async write
/// happens under the per-server tokio Mutex — never awaiting while holding
/// the map lock.
pub async fn send_command(server_id: &str, line: &str) -> Result<()> {
    let stdin = {
        let guard = state().lock().expect("server state poisoned");
        let rs = guard
            .get(server_id)
            .ok_or_else(|| Error::ServerNotRunning {
                id: server_id.to_string(),
            })?;
        rs.stdin.clone()
    };
    let payload = format!("{}\n", line.trim_end());
    let mut s = stdin.lock().await;
    s.write_all(payload.as_bytes())
        .await
        .map_err(|e| Error::ServerSpawnFailed {
            details: format!("stdin write: {e}"),
        })?;
    s.flush().await.map_err(|e| Error::ServerSpawnFailed {
        details: format!("stdin flush: {e}"),
    })?;
    Ok(())
}

/// Graceful-stop window before the force-kill fallback: 60s (300 × 200ms). A
/// large modded world can take far longer than the old 10s to flush its chunks
/// on shutdown, and force-killing mid-save can corrupt region files.
const GRACEFUL_STOP_POLLS: usize = 300;
const GRACEFUL_STOP_INTERVAL_MS: u64 = 200;

/// Graceful stop: send `stop` to the console, wait up to `GRACEFUL_STOP_POLLS *
/// GRACEFUL_STOP_INTERVAL_MS` for the exit watcher to clear state, then
/// force-kill the process tree as a fallback. When the server is not tracked
/// in-memory it may be a process adopted from before a launcher restart — there
/// is no stdin to talk to it, so fall back to force-killing its persisted PID
/// (alive-and-ours), which also frees the world lock the leftover JVM holds.
pub async fn stop(app: &AppHandle, server_id: &str) -> Result<()> {
    if !is_running(server_id) {
        return stop_persisted(app, server_id);
    }
    let _ = send_command(server_id, "stop").await;
    for _ in 0..GRACEFUL_STOP_POLLS {
        if !is_running(server_id) {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(GRACEFUL_STOP_INTERVAL_MS)).await;
    }
    // Timed out: force-kill, then clear our own in-memory state immediately so an
    // immediate restart()'s start() isn't rejected with ServerAlreadyRunning. The
    // exit watcher is pid-matched, so it will no-op on the now-gone entry; we
    // therefore also do its bookkeeping (clear pid, record the forced exit, emit).
    if let Some(pid) = running_pid(server_id) {
        crate::platform::kill_process_tree(pid);
        state()
            .lock()
            .expect("server state poisoned")
            .remove(server_id);
        if let Ok(base) = crate::paths::app_dir(app) {
            let p = crate::paths::server_paths(&base, server_id);
            crate::servers_runtime::pid::clear_pid(&p.pid);
            crate::servers_runtime::exit_state::write(&p.runtime, -1);
        }
        let _ = ServerExited {
            server_id: server_id.to_string(),
            code: -1,
        }
        .emit(app);
    }
    Ok(())
}

/// Stop a server that is NOT tracked in this session's map by force-killing its
/// persisted PID when it is still alive and still ours, then clearing the PID
/// file. Returns `ServerNotRunning` when there is no live owned process (a stale
/// PID file is cleared as a side effect).
fn stop_persisted(app: &AppHandle, server_id: &str) -> Result<()> {
    let base = crate::paths::app_dir(app).map_err(|e| Error::io("<app_dir>", e))?;
    let p = crate::paths::server_paths(&base, server_id);
    if kill_owned_pid(&p.pid) {
        Ok(())
    } else {
        Err(Error::ServerNotRunning {
            id: server_id.to_string(),
        })
    }
}

/// Read `server-port` from `runtime/server.properties` if present/parseable.
pub fn read_port(runtime: &Path) -> Option<u16> {
    let raw = std::fs::read_to_string(runtime.join("server.properties")).ok()?;
    crate::servers_runtime::properties::ServerProperties::parse(&raw)
        .get("server-port")
        .and_then(|v| v.parse().ok())
}

/// Restart = graceful stop (if running) then start.
pub async fn restart(app: &AppHandle, server_id: &str) -> Result<u32> {
    if is_running(server_id) {
        stop(app, server_id).await?;
    }
    start(app, server_id).await
}

/// True iff `name` is a single safe path component (no separators, no `..`,
/// no drive/root prefix). Rejecting anything that isn't exactly one
/// `Component::Normal` blocks `/`, `\`, `..`, AND Windows drive-relative names
/// like `C:evil.jar` (which `Path::join` would otherwise resolve OUTSIDE the
/// mods dir by discarding the base).
pub(crate) fn is_safe_mod_name(name: &str) -> bool {
    // This guard screens a name (from a directory listing or user input) before
    // it is joined under `mods/`, so it must reject every escape vector on
    // *every* host OS — not just the one we happen to be running on. `\` is a
    // path separator and `C:` a drive prefix on Windows, but both are legal
    // filename characters on Unix, so `std::path::Path` parsing alone would let
    // `a\b.jar` / `C:evil.jar` slip through on Unix. Screen those explicitly.
    if name.contains('\\') || name.contains(':') {
        return false;
    }
    // On the current platform, `Path::components` then catches `/`, `..`, `.`,
    // absolute paths, and empty: a safe name is exactly one Normal component.
    let mut comps = std::path::Path::new(name).components();
    matches!(comps.next(), Some(std::path::Component::Normal(_))) && comps.next().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn reconcile_prefers_in_memory_then_adopts_live_recorded() {
        // In-memory entry (this session owns it) always wins.
        assert_eq!(
            reconcile_running(Some(10), Some(99), false),
            (true, Some(10))
        );
        // No in-memory entry: adopt a recorded PID only if alive AND ours.
        assert_eq!(reconcile_running(None, Some(99), true), (true, Some(99)));
        assert_eq!(reconcile_running(None, Some(99), false), (false, None));
        assert_eq!(reconcile_running(None, None, false), (false, None));
    }

    #[test]
    fn claim_start_is_exclusive_and_releases_on_drop() {
        let id = "srv-claim-unit-test";
        let c = claim_start(id).expect("first claim succeeds");
        assert!(
            claim_start(id).is_none(),
            "a second concurrent claim must be rejected"
        );
        drop(c);
        let c2 = claim_start(id).expect("claim succeeds again after release");
        drop(c2);
    }

    #[test]
    fn kill_persisted_orphans_clears_dead_and_stale_pid_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("servers");
        // u32::MAX is treated as not-alive by process_alive, so this exercises
        // the stale-file cleanup path deterministically (no real process).
        let pid_file = root.join("srv-x/runtime/server.pid");
        crate::servers_runtime::pid::write_pid(&pid_file, u32::MAX).unwrap();
        assert!(pid_file.exists());
        kill_persisted_orphans(&root);
        assert!(
            !pid_file.exists(),
            "a dead/stale pid file should be cleared"
        );
        // A missing servers root is a no-op (must not panic).
        kill_persisted_orphans(&dir.path().join("does-not-exist"));
    }

    #[tokio::test]
    async fn send_command_errors_when_not_running() {
        let r = send_command("nope-xyz", "say hi").await;
        assert!(
            matches!(r, Err(Error::ServerNotRunning { .. })),
            "got: {r:?}"
        );
    }

    #[test]
    fn is_running_false_for_unknown() {
        assert!(!is_running("nope-xyz"));
        assert_eq!(running_pid("nope-xyz"), None);
    }

    #[test]
    fn launch_argv_vanilla_uses_jar_nogui() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("server.jar"), b"x").unwrap();
        let argv = build_launch_argv(
            crate::instances::schema::LoaderKind::Vanilla,
            dir.path(),
            2048,
            "",
        )
        .unwrap();
        assert_eq!(argv, vec!["-Xmx2048m", "-jar", "server.jar", "nogui"]);
    }

    #[test]
    fn launch_argv_vanilla_splices_extra_jvm_args_after_xmx_before_jar() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("server.jar"), b"x").unwrap();
        let argv = build_launch_argv(
            crate::instances::schema::LoaderKind::Vanilla,
            dir.path(),
            2048,
            "-XX:+UseG1GC -Xss512k",
        )
        .unwrap();
        assert_eq!(
            argv,
            vec![
                "-Xmx2048m",
                "-XX:+UseG1GC",
                "-Xss512k",
                "-jar",
                "server.jar",
                "nogui"
            ]
        );
    }

    #[test]
    fn launch_argv_forge_splices_extra_jvm_args_after_xmx_before_argfile() {
        let dir = tempfile::tempdir().unwrap();
        let af = dir.path().join("libraries/net/neoforged/neoforge/20.4.237");
        std::fs::create_dir_all(&af).unwrap();
        std::fs::write(af.join("win_args.txt"), b"@stuff\n").unwrap();
        std::fs::write(af.join("unix_args.txt"), b"@stuff\n").unwrap();
        let argv = build_launch_argv(
            crate::instances::schema::LoaderKind::NeoForge,
            dir.path(),
            3072,
            "-XX:+UseG1GC",
        )
        .unwrap();
        assert_eq!(argv.first().map(String::as_str), Some("-Xmx3072m"));
        // The extra flag comes right after -Xmx and before the argfile refs.
        assert_eq!(argv.get(1).map(String::as_str), Some("-XX:+UseG1GC"));
        assert_eq!(argv.get(2).map(String::as_str), Some("@user_jvm_args.txt"));
    }
    #[test]
    fn launch_argv_forge_uses_args_files() {
        let dir = tempfile::tempdir().unwrap();
        let af = dir.path().join("libraries/net/neoforged/neoforge/20.4.237");
        std::fs::create_dir_all(&af).unwrap();
        std::fs::write(dir.path().join("user_jvm_args.txt"), b"# jvm\n").unwrap();
        std::fs::write(af.join("win_args.txt"), b"@stuff\n").unwrap();
        std::fs::write(af.join("unix_args.txt"), b"@stuff\n").unwrap();
        let argv = build_launch_argv(
            crate::instances::schema::LoaderKind::NeoForge,
            dir.path(),
            3072,
            "",
        )
        .unwrap();
        assert_eq!(argv.first().map(String::as_str), Some("-Xmx3072m"));
        assert!(
            argv.iter().any(|a| a == "@user_jvm_args.txt"),
            "argv={argv:?}"
        );
        assert!(
            argv.iter()
                .any(|a| a.contains("args.txt") && a.starts_with('@') && a != "@user_jvm_args.txt"),
            "argv={argv:?}"
        );
        assert_eq!(argv.last().map(String::as_str), Some("nogui"));
    }
    #[test]
    fn launch_argv_forge_errors_without_args_file() {
        let dir = tempfile::tempdir().unwrap();
        let r = build_launch_argv(
            crate::instances::schema::LoaderKind::Forge,
            dir.path(),
            1024,
            "",
        );
        assert!(matches!(
            r,
            Err(crate::error::Error::ServerSpawnFailed { .. })
        ));
    }

    #[test]
    fn console_java_swaps_javaw_to_java_on_windows() {
        assert_eq!(
            console_java_path(Path::new("/x/bin/javaw.exe")),
            Path::new("/x/bin/java.exe")
        );
        assert_eq!(
            console_java_path(Path::new("/x/bin/java")),
            Path::new("/x/bin/java")
        );
    }

    #[test]
    fn java_component_defaults_to_legacy_when_absent() {
        assert_eq!(java_component_or_legacy(None), "jre-legacy");
        assert_eq!(
            java_component_or_legacy(Some("java-runtime-delta")),
            "java-runtime-delta"
        );
    }

    #[test]
    fn is_safe_mod_name_rejects_traversal() {
        assert!(is_safe_mod_name("cool-mod.jar"));
        assert!(is_safe_mod_name("a.jar.disabled"));
        assert!(!is_safe_mod_name("../evil.jar"));
        assert!(!is_safe_mod_name("a/b.jar"));
        assert!(!is_safe_mod_name("a\\b.jar"));
        assert!(!is_safe_mod_name(".."));
        assert!(!is_safe_mod_name(""));
        // Windows drive-relative names: Path::join would discard the base dir
        assert!(!is_safe_mod_name("C:evil.jar"));
        assert!(!is_safe_mod_name("D:payload.jar"));
        assert!(!is_safe_mod_name("/abs.jar"));
    }

    #[test]
    fn read_port_parses_server_properties() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("server.properties"), "server-port=25570\n").unwrap();
        assert_eq!(read_port(dir.path()), Some(25570));
        let empty = tempfile::tempdir().unwrap();
        assert_eq!(read_port(empty.path()), None);
    }
}
