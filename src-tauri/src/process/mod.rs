//! Single chokepoint for spawning OS subprocesses. Every `Command` the
//! launcher constructs lives in this module, so the set of processes it
//! can run is enumerable from one place and documented in
//! `docs/PRINCIPLES.md` Appendix A. Enforced by the structural test
//! `tests/structural_no_raw_spawn.rs`.

use crate::error::{Error, Result};
use std::path::{Path, PathBuf};

/// Run a Java processor: `java -cp <classpath> <main_class> <args...>`,
/// wait for it, and map a spawn failure or non-zero exit to
/// `Error::ForgePatcherFailed`. All five install-time Java processors
/// (SpecialSource, FART, ART, binarypatcher, installertools) route
/// through here — see `docs/PRINCIPLES.md` Appendix A.
pub async fn run_java_processor(
    java_bin: &Path,
    classpath: &[PathBuf],
    main_class: &str,
    args: &[String],
    processor_label: &str,
) -> Result<()> {
    let sep = if cfg!(windows) { ";" } else { ":" };
    let cp: String = classpath
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(sep);

    let mut cmd = tokio::process::Command::new(java_bin);
    cmd.arg("-cp").arg(&cp);
    cmd.arg(main_class);
    cmd.args(args);

    crate::diag!(
        "process: spawning {} ({processor_label}) with {} classpath entries",
        java_bin.display(),
        classpath.len(),
    );

    let output = cmd.output().await.map_err(|e| Error::ForgePatcherFailed {
        processor: processor_label.to_string(),
        details: format!("spawn java: {e}"),
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(Error::ForgePatcherFailed {
            processor: processor_label.to_string(),
            details: format!(
                "java exit {}: stderr={} stdout={}",
                output.status,
                stderr.trim(),
                stdout.trim()
            ),
        });
    }
    Ok(())
}

/// Spawn the Minecraft client (`javaw`/`java`). Returns the `Child` so
/// the caller owns lifecycle and the exit watcher. stdout+stderr are
/// redirected to the launch-log file handles supplied by the caller;
/// stdin is closed. `extra_env` adds environment variables injected on
/// top of the inherited environment (e.g. GPU offload vars on Linux such
/// as `DRI_PRIME=1`); pass `&[]` on Windows/macOS where no override is
/// needed.
pub fn spawn_minecraft(
    java_path: &Path,
    argv: &[String],
    game_dir: &Path,
    extra_env: &[(String, String)],
    log_stdout: std::fs::File,
    log_stderr: std::fs::File,
) -> Result<tokio::process::Child> {
    let mut cmd = tokio::process::Command::new(java_path);
    cmd.args(argv)
        .current_dir(game_dir)
        .envs(extra_env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(log_stdout))
        .stderr(std::process::Stdio::from(log_stderr));
    // Put Minecraft in its own process group (pgid == child pid) so the
    // exit/stop path can signal the whole group and reap helper children,
    // not just the JVM. Unix-only; `process_group` is a no-op concept on
    // Windows (taskkill /T handles the tree there).
    #[cfg(unix)]
    cmd.process_group(0);
    cmd.spawn().map_err(|e| Error::JavaSpawn {
        details: format!("spawn {}: {e}", java_path.display()),
    })
}

/// Launch the downloaded NSIS installer and return immediately. The
/// caller exits the app right after so the installer can replace the
/// locked launcher binary. The wizard runs visibly (transparency; and
/// SmartScreen warns on the unsigned binary regardless). Windows-only —
/// the launcher targets Windows; other targets return a typed error so
/// non-Windows builds still compile.
#[cfg(target_os = "windows")]
pub fn spawn_installer(installer: &Path) -> Result<()> {
    std::process::Command::new(installer)
        .spawn()
        .map(|_child| ())
        .map_err(|e| Error::UpdateInstallFailed {
            details: format!("spawn installer {}: {e}", installer.display()),
        })
}

#[cfg(not(target_os = "windows"))]
pub fn spawn_installer(installer: &Path) -> Result<()> {
    Err(Error::UpdateInstallFailed {
        details: format!("installer launch is Windows-only ({})", installer.display()),
    })
}

/// Terminate `pid` and its child processes via `taskkill`. Best-effort: a
/// failure (e.g. the PID is already gone) is ignored. Windows-only — this is
/// the subprocess used by `platform::kill_process_tree` on Windows; the Unix
/// signal path lives in `platform::` (no subprocess).
#[cfg(target_os = "windows")]
pub fn taskkill_tree(pid: u32) {
    // /F = force, /T = kill child processes too (MC spawns helpers).
    let _ = std::process::Command::new("taskkill")
        .args(["/F", "/T", "/PID", &pid.to_string()])
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn run_java_processor_missing_binary_errors() {
        let r = run_java_processor(
            Path::new("nonexistent-java-binary-xyz"),
            &[],
            "Main",
            &[],
            "test-proc",
        )
        .await;
        match r {
            Err(Error::ForgePatcherFailed { processor, .. }) => {
                assert_eq!(processor, "test-proc");
            }
            other => panic!("expected ForgePatcherFailed, got {other:?}"),
        }
    }

    #[test]
    fn spawn_minecraft_missing_binary_errors() {
        let dir = tempdir().unwrap();
        let out = std::fs::File::create(dir.path().join("o.log")).unwrap();
        let err = std::fs::File::create(dir.path().join("e.log")).unwrap();
        let r = spawn_minecraft(
            Path::new("nonexistent-javaw-binary-xyz"),
            &[],
            dir.path(),
            &[], // extra_env
            out,
            err,
        );
        assert!(matches!(r, Err(Error::JavaSpawn { .. })), "got: {r:?}");
    }

    #[test]
    fn spawn_minecraft_accepts_extra_env() {
        let dir = tempdir().unwrap();
        let out = std::fs::File::create(dir.path().join("o.log")).unwrap();
        let err = std::fs::File::create(dir.path().join("e.log")).unwrap();
        let r = spawn_minecraft(
            Path::new("nonexistent-javaw-binary-xyz"),
            &[],
            dir.path(),
            &[("DRI_PRIME".to_string(), "1".to_string())],
            out,
            err,
        );
        assert!(matches!(r, Err(Error::JavaSpawn { .. })), "got: {r:?}");
    }

    #[test]
    fn spawn_installer_missing_binary_errors() {
        // On Windows the missing path fails to spawn; on non-Windows the
        // function is a typed "Windows-only" error. Either way: Err.
        let r = spawn_installer(Path::new("nonexistent-installer-xyz.exe"));
        assert!(
            matches!(r, Err(Error::UpdateInstallFailed { .. })),
            "got {r:?}"
        );
    }
}
