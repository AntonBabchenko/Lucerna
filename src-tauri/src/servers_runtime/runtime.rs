//! Долгоживущий серверный процесс: состояние, консоль-стрим, команды, стоп.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use tauri::AppHandle;
use tokio::io::{AsyncBufReadExt, BufReader};

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
pub(crate) fn build_launch_argv(
    loader: LoaderKind,
    runtime: &Path,
    heap_mb: u32,
) -> Result<Vec<String>> {
    let xmx = format!("-Xmx{heap_mb}m");
    match loader {
        LoaderKind::Vanilla | LoaderKind::Fabric | LoaderKind::Quilt => Ok(vec![
            xmx,
            "-jar".into(),
            "server.jar".into(),
            "nogui".into(),
        ]),
        LoaderKind::Forge | LoaderKind::NeoForge => {
            let args_rel =
                find_loader_args_file(runtime).ok_or_else(|| Error::ServerSpawnFailed {
                    details: "installer args file not found under libraries/".into(),
                })?;
            Ok(vec![
                xmx,
                "@user_jvm_args.txt".into(),
                format!("@{args_rel}"),
                "nogui".into(),
            ])
        }
    }
}

/// Relative path (from runtime/) of the installer-generated args file for the
/// current OS, e.g. `libraries/net/neoforged/neoforge/<v>/win_args.txt`.
fn find_loader_args_file(runtime: &Path) -> Option<String> {
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
    if is_running(server_id) {
        return Err(Error::ServerAlreadyRunning {
            id: server_id.to_string(),
        });
    }

    let base = crate::paths::app_dir(app).map_err(|e| Error::io("<app_dir>", e))?;
    let p = crate::paths::server_paths(&base, server_id);
    let file = crate::servers_runtime::store::read_server_json(&p.json)?;
    crate::servers_runtime::eula::require_accepted(file.eula_accepted)?;

    let component =
        crate::servers_runtime::create::resolve_server_java_component(&file.mc_version).await?;
    crate::jre::ensure_jre(&component, app, |_, _, _| {}).await?;
    let javaw = crate::jre::java_executable_path(&component, app)?;
    let java = console_java_path(&javaw);

    let argv = build_launch_argv(file.loader, &p.runtime, file.max_heap_mb)?;

    std::fs::create_dir_all(&p.logs).map_err(|e| Error::io(p.logs.display().to_string(), e))?;
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
    tokio::spawn(async move {
        let code = child.wait().await.ok().and_then(|s| s.code()).unwrap_or(-1);
        state()
            .lock()
            .expect("server state poisoned")
            .remove(&id_exit);
        let _ = ServerExited {
            server_id: id_exit,
            code,
        }
        .emit(&app_exit);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

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
        )
        .unwrap();
        assert_eq!(argv, vec!["-Xmx2048m", "-jar", "server.jar", "nogui"]);
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
}
