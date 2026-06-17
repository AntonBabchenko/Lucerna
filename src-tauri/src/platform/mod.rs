//! OS-divergent primitives, isolated behind one module so a later macOS
//! spec slots in by adding `cfg` arms here rather than across the codebase.
//!
//! Boundary: subprocess spawning stays in `process::` (the documented
//! chokepoint — Windows `taskkill` is a subprocess and lives there). This
//! module owns the NON-subprocess OS calls (chmod, POSIX signals, Win32
//! window detection) plus the dispatch entry points the launcher calls.
//! Enforced by `tests/structural_platform_chokepoint.rs`.

pub mod gpu;

use std::path::{Path, PathBuf};

/// Standard on-disk locations where third-party launchers keep their
/// per-instance folders. Best-effort: returned paths may not exist; the
/// manual folder picker is the fallback when a path is wrong.
pub fn default_launcher_roots() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        let mut roots = Vec::new();
        if let Ok(appdata) = std::env::var("APPDATA") {
            let base = PathBuf::from(&appdata);
            roots.push(base.join("PrismLauncher").join("instances"));
            roots.push(base.join("MultiMC").join("instances"));
            roots.push(base.join("PolyMC").join("instances"));
            roots.push(base.join("ATLauncher").join("instances"));
            // Modern Modrinth App location; the legacy theseus path is kept
            // as a fallback for installs that never migrated.
            roots.push(base.join("ModrinthApp").join("profiles"));
            roots.push(base.join("com.modrinth.theseus").join("profiles"));
            roots.push(base.join(".minecraft"));
        }
        if let Ok(home) = std::env::var("USERPROFILE") {
            roots.push(
                PathBuf::from(&home)
                    .join("curseforge")
                    .join("minecraft")
                    .join("Instances"),
            );
        }
        return roots;
    }
    #[cfg(target_os = "linux")]
    {
        let mut roots = Vec::new();
        if let Ok(home) = std::env::var("HOME") {
            let base = PathBuf::from(&home);
            roots.push(base.join(".local/share/PrismLauncher/instances"));
            roots.push(base.join(".local/share/multimc/instances"));
            roots.push(base.join(".local/share/PolyMC/instances"));
            roots.push(
                base.join(".var/app/org.prismlauncher.PrismLauncher/data/PrismLauncher/instances"),
            );
            roots.push(base.join(".minecraft"));
        }
        return roots;
    }
    #[cfg(target_os = "macos")]
    {
        let mut roots = Vec::new();
        if let Ok(home) = std::env::var("HOME") {
            let base = PathBuf::from(&home);
            roots.push(base.join("Library/Application Support/PrismLauncher/instances"));
            roots.push(base.join("Library/Application Support/MultiMC/instances"));
            roots.push(base.join("Library/Application Support/minecraft"));
        }
        return roots;
    }
    #[allow(unreachable_code)]
    vec![]
}

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

/// The POSIX `kill` target for terminating Minecraft's process group. MC is
/// spawned with `process_group(0)`, so its pgid equals its pid; signalling
/// `-pid` reaps the JVM and any helper children it spawned. Returns `None`
/// for pids that don't fit a positive pid_t, and crucially excludes pid 1 —
/// `kill(-1, …)` is the POSIX "every process the caller can kill" broadcast,
/// never a real MC group.
#[cfg(unix)]
fn killpg_target(pid: u32) -> Option<i32> {
    positive_pid(pid).filter(|&p| p > 1).map(|p| -p)
}

/// Terminate the Minecraft process and its helper children. Best-effort —
/// failures are ignored because the launch exit-watcher fires `ProcessExited`
/// regardless of how the process died.
///
/// Windows kills the whole tree via the `process::` taskkill chokepoint (the
/// MC launcher spawns helper processes there). Unix signals Minecraft's
/// process group with `SIGTERM` — MC is spawned with `process_group(0)` so
/// its pgid equals its pid, and `-pgid` reaps the JVM together with any
/// helper children it spawned; the pgid is validated so a wrapped value can
/// never broadcast.
pub fn kill_process_tree(pid: u32) {
    #[cfg(target_os = "windows")]
    {
        crate::process::taskkill_tree(pid);
    }
    #[cfg(unix)]
    {
        if let Some(target) = killpg_target(pid) {
            // SAFETY: FFI to POSIX kill(2). `target` is a negative pgid derived
            // from a validated pid (> 1), so it addresses MC's process group,
            // not the -1 broadcast. SIGTERM is a libc constant; no memory is
            // shared. An error (ESRCH — group already gone) is ignored:
            // teardown is best-effort.
            unsafe {
                libc::kill(target, libc::SIGTERM);
            }
        }
    }
}

/// Total physical RAM in MB, or `None` when it can't be read. Used to
/// bound the OOM heap suggestion (never propose more than half of RAM).
/// Three cfg-gated impls; no new crate dependency.
pub fn total_system_ram_mb() -> Option<u64> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        // SAFETY: MEMORYSTATUSEX is a plain-old-data struct; we zero it,
        // set dwLength as the API requires, and pass a valid pointer.
        let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
        status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
        if ok == 0 {
            return None;
        }
        return Some(status.ullTotalPhys / (1024 * 1024));
    }
    #[cfg(target_os = "linux")]
    {
        let text = std::fs::read_to_string("/proc/meminfo").ok()?;
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("MemTotal:") {
                // "MemTotal:       16384256 kB"
                let kb: u64 = rest.trim().trim_end_matches("kB").trim().parse().ok()?;
                return Some(kb / 1024);
            }
        }
        return None;
    }
    #[cfg(target_os = "macos")]
    {
        // sysctl hw.memsize → total bytes via the libc sysctlbyname FFI.
        // `libc` is already a `cfg(unix)` dependency (used above by
        // kill_process_tree), and macOS is unix, so it resolves here.
        let mut size: u64 = 0;
        let mut len = std::mem::size_of::<u64>();
        let name = c"hw.memsize";
        // SAFETY: standard sysctlbyname usage; name is a valid C string,
        // out buffer + len are correctly sized for a u64.
        let rc = unsafe {
            libc::sysctlbyname(
                name.as_ptr(),
                &mut size as *mut u64 as *mut libc::c_void,
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if rc != 0 {
            return None;
        }
        return Some(size / (1024 * 1024));
    }
    #[allow(unreachable_code)]
    None
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

#[cfg(all(test, windows))]
mod modrinth_root_tests {
    use super::default_launcher_roots;

    #[test]
    fn includes_modern_modrinth_app_profiles_root() {
        let roots = default_launcher_roots();
        // `PathBuf::ends_with` matches whole path components, so this pins the
        // exact `ModrinthApp/profiles` tail rather than loose substrings.
        assert!(
            roots.iter().any(|p| p.ends_with("ModrinthApp/profiles")),
            "missing %APPDATA%/ModrinthApp/profiles; got: {roots:?}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kill_process_tree_unknown_pid_does_not_panic() {
        // Regression sentinel for the POSIX broadcast footgun. u32::MAX must
        // not reach a signal (rejected by positive_pid). pid 0 and pid 1 must
        // not broadcast: on unix killpg_target excludes them (kill(-1,…) would
        // hit every process the user can kill); on Windows taskkill harmlessly
        // fails on these. Each must be a safe no-op, never a panic.
        kill_process_tree(u32::MAX);
        kill_process_tree(0);
        kill_process_tree(1);
    }

    #[cfg(unix)]
    #[test]
    fn killpg_target_addresses_group_and_excludes_broadcast() {
        assert_eq!(killpg_target(1234), Some(-1234));
        assert_eq!(killpg_target(0), None);
        assert_eq!(killpg_target(1), None, "pid 1 → -1 would broadcast");
        assert_eq!(killpg_target(u32::MAX), None);
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

    #[test]
    fn total_system_ram_is_plausible_on_this_host() {
        // On any real CI/dev host this returns Some(>= 512 MB). We only
        // assert the lower bound + that the call doesn't panic; the exact
        // value is host-dependent.
        if let Some(mb) = super::total_system_ram_mb() {
            assert!(mb >= 512, "implausibly small RAM reading: {mb} MB");
        }
        // None is tolerated (unsupported/locked-down host) — callers treat
        // it as "unknown" and fall back to a conservative fixed bump.
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn wait_for_window_ready_is_immediate_noop_off_windows() {
        // Off Windows there is no window-detect yet (deliberate fallback);
        // it must return promptly rather than block the hide-to-tray task.
        wait_for_window_ready(123_456).await;
    }
}
