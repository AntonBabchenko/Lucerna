//! OS-divergent primitives, isolated behind one module so a later macOS
//! spec slots in by adding `cfg` arms here rather than across the codebase.
//!
//! Boundary: subprocess spawning stays in `process::` (the documented
//! chokepoint — Windows `taskkill` is a subprocess and lives there). This
//! module owns the NON-subprocess OS calls (chmod, POSIX signals, Win32
//! window detection) plus the dispatch entry points the launcher calls.
//! Enforced by `tests/structural_platform_chokepoint.rs`.

use std::path::Path;

/// True iff this platform supports in-app self-update (download + verify +
/// launch an installer). Windows-only today; Linux is check-and-notify and
/// macOS lands in a later spec.
pub fn supports_in_app_install() -> bool {
    cfg!(target_os = "windows")
}

/// Mark `path` executable. No-op on Windows (the `.exe`/`.dll` extensions
/// decide there); on Unix sets mode `0o755`. Propagates the IO error if the
/// metadata read or chmod fails — the caller turns it into a launch error.
#[cfg(unix)]
pub fn set_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms)
}

#[cfg(not(unix))]
pub fn set_executable(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Create a symlink at `link` pointing to `target` (a path relative to the
/// link's directory, as written in Mojang JRE manifests). Idempotent:
/// removes any existing entry first so re-install over an installed JRE
/// works. Unix-only — Windows JRE manifests never carry `link` entries, so
/// off Unix this returns `Unsupported` to fail loudly if one ever appears.
#[cfg(unix)]
pub fn symlink(target: &str, link: &Path) -> std::io::Result<()> {
    // Remove a stale entry so re-install is idempotent. Only "not found" is
    // benign — surface anything else (e.g. a directory in the way) so the
    // failure is diagnosable rather than masked by a later EEXIST.
    if let Err(e) = std::fs::remove_file(link) {
        if e.kind() != std::io::ErrorKind::NotFound {
            return Err(e);
        }
    }
    std::os::unix::fs::symlink(target, link)
}

#[cfg(not(unix))]
pub fn symlink(_target: &str, _link: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "symlink entries in a JRE manifest are not supported on this platform",
    ))
}

/// Convert an OS pid (`u32`) to a positive POSIX `pid_t`, or `None` if it
/// does not fit. This is the footgun guard: a zero or too-large pid would
/// otherwise wrap to `<= 0`, and POSIX `kill` treats those as group/broadcast
/// targets (`kill -1` = every process the caller can kill).
#[cfg(unix)]
fn positive_pid(pid: u32) -> Option<i32> {
    let p = i32::try_from(pid).ok()?;
    (p > 0).then_some(p)
}

/// Terminate the Minecraft process. Best-effort — failures are ignored
/// because the launch exit-watcher fires `ProcessExited` regardless of how
/// the process died.
///
/// Windows kills the whole tree via the `process::` taskkill chokepoint (the
/// MC launcher spawns helper processes there). Unix sends `SIGTERM` to the
/// single JVM pid (the Linux/macOS client is one process); the pid is
/// validated first so a wrapped pid can never broadcast.
pub fn kill_process_tree(pid: u32) {
    #[cfg(target_os = "windows")]
    {
        crate::process::taskkill_tree(pid);
    }
    #[cfg(unix)]
    {
        if let Some(p) = positive_pid(pid) {
            // SAFETY: FFI to POSIX kill(2). `p` is a validated positive
            // pid_t and `SIGTERM` is a libc constant; no memory is shared.
            // An error (e.g. ESRCH — pid already gone) is intentionally
            // ignored: teardown is best-effort.
            unsafe {
                libc::kill(p, libc::SIGTERM);
            }
        }
    }
}

/// Block until the spawned process has created its top-level window (input
/// message queue ready), or a 30-second cap elapses. Used to delay
/// hide-to-tray until Minecraft is actually on screen.
///
/// Windows uses Win32 `WaitForInputIdle`. Other platforms are a deliberate
/// immediate-return no-op for now — Linux/macOS window detection (X11
/// `_NET_CLIENT_LIST`, NSWorkspace, and the Wayland gap) is a later spec; the
/// launcher hides immediately on spawn there, as it did pre-2026-05-26.
#[cfg(windows)]
pub async fn wait_for_window_ready(pid: u32) {
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

#[cfg(not(windows))]
pub async fn wait_for_window_ready(_pid: u32) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kill_process_tree_unknown_pid_does_not_panic() {
        // Regression sentinel: u32::MAX must NOT reach a POSIX broadcast.
        // On Unix it is rejected by `positive_pid` (does not fit i32 > 0);
        // on Windows taskkill is a harmless no-op for an unknown PID.
        kill_process_tree(u32::MAX);
        kill_process_tree(0);
    }

    #[cfg(unix)]
    #[test]
    fn positive_pid_guards_the_footgun() {
        assert_eq!(positive_pid(1234), Some(1234));
        assert_eq!(positive_pid(0), None, "0 would target the caller's group");
        assert_eq!(positive_pid(u32::MAX), None, "wraps past i32::MAX → reject");
    }

    #[cfg(unix)]
    #[test]
    fn set_executable_sets_owner_exec_bit_on_unix() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("java");
        std::fs::write(&f, b"#!/bin/sh\n").unwrap();
        // Start non-executable.
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();

        set_executable(&f).expect("set_executable ok");

        let mode = std::fs::metadata(&f).unwrap().permissions().mode();
        assert_ne!(mode & 0o111, 0, "expected an executable bit, got {mode:o}");
    }

    #[cfg(not(unix))]
    #[test]
    fn set_executable_is_noop_ok_on_non_unix() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("javaw.exe");
        std::fs::write(&f, b"x").unwrap();
        set_executable(&f).expect("no-op returns Ok on non-unix");
    }

    #[cfg(unix)]
    #[test]
    fn symlink_creates_link_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real.txt");
        std::fs::write(&target, b"hi").unwrap();
        let link = dir.path().join("alias.txt");

        symlink("real.txt", &link).expect("first symlink ok");
        assert_eq!(
            std::fs::read_link(&link).unwrap(),
            std::path::PathBuf::from("real.txt")
        );

        // Re-creating over an existing link must succeed (re-install case).
        symlink("real.txt", &link).expect("second symlink idempotent");
        assert_eq!(std::fs::read(&link).unwrap(), b"hi");
    }

    #[cfg(not(unix))]
    #[test]
    fn symlink_is_unsupported_off_unix() {
        let dir = tempfile::tempdir().unwrap();
        let err = symlink("target", &dir.path().join("alias")).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn wait_for_window_ready_is_immediate_noop_off_windows() {
        // Off Windows there is no window-detect yet (deliberate fallback);
        // it must return promptly rather than block the hide-to-tray task.
        wait_for_window_ready(123_456).await;
    }
}
