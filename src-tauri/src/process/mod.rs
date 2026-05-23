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

    eprintln!(
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
/// stdin is closed.
pub fn spawn_minecraft(
    java_path: &Path,
    argv: &[String],
    game_dir: &Path,
    log_stdout: std::fs::File,
    log_stderr: std::fs::File,
) -> Result<tokio::process::Child> {
    let mut cmd = tokio::process::Command::new(java_path);
    cmd.args(argv)
        .current_dir(game_dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(log_stdout))
        .stderr(std::process::Stdio::from(log_stderr));
    cmd.spawn().map_err(|e| Error::JavaSpawn {
        details: format!("spawn {}: {e}", java_path.display()),
    })
}

/// Terminate `pid` and its child processes. Best-effort: if the kill
/// command fails (e.g. the PID is already gone) the error is ignored —
/// the launch exit-watcher fires `ProcessExited` regardless of cause.
#[cfg(target_os = "windows")]
pub fn kill_process_tree(pid: u32) {
    // /F = force, /T = kill child processes too (MC spawns helpers).
    let _ = std::process::Command::new("taskkill")
        .args(["/F", "/T", "/PID", &pid.to_string()])
        .status();
}

/// POSIX path — the launcher targets Windows; this is included only so
/// non-Windows builds compile (e.g. `cargo test` on Linux CI runners).
/// Deliberately a no-op: shelling out to `kill <pid>` on POSIX is
/// unsafe when `pid` is large enough to wrap to a negative pid_t
/// (POSIX `kill -1` targets every process the user can kill — caught
/// by the Ubuntu CI runner taking SIGTERM from a unit test). The real
/// launcher never reaches this path; non-Windows callers are stubs.
#[cfg(not(target_os = "windows"))]
pub fn kill_process_tree(_pid: u32) {}

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
            out,
            err,
        );
        assert!(matches!(r, Err(Error::JavaSpawn { .. })), "got: {r:?}");
    }

    #[test]
    fn kill_process_tree_unknown_pid_does_not_panic() {
        // The Windows taskkill path is no-op for unknown PIDs; the
        // POSIX path is intentionally a no-op for ALL pids (see the
        // `#[cfg(not(target_os = "windows"))]` body). u32::MAX was the
        // value that, before this stub, made the CI runner take
        // SIGTERM — POSIX `kill 4294967295` wraps to `kill -1`
        // ("kill all processes the user can kill"). Test pinned at
        // u32::MAX as a regression sentinel.
        kill_process_tree(u32::MAX);
    }
}
