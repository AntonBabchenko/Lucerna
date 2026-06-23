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

/// How this particular run can replace itself with a newer build. Decided at
/// runtime, not just by compile target: a Linux build self-updates in-app only
/// when launched as an AppImage (one user-owned file we can swap in place); a
/// `.deb`/`.rpm` install lives in root-owned system paths managed by the
/// package manager, so it stays notify-only. macOS is notify-only for now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallKind {
    /// Windows: download the NSIS installer, run it, exit so it can replace
    /// the locked launcher binary.
    WindowsInstaller,
    /// Linux AppImage: download the new `.AppImage`, replace this file in
    /// place, relaunch. `path` is the running AppImage (`$APPIMAGE`).
    LinuxAppImage { path: PathBuf },
    /// No in-app install (`.deb`/`.rpm`, macOS, or a non-AppImage run). The
    /// UI links to the release page instead.
    NotifyOnly,
}

/// Determine this run's install mechanism. Pure aside from reading `$APPIMAGE`
/// + a single `stat` on Linux.
pub fn install_kind() -> InstallKind {
    #[cfg(target_os = "windows")]
    {
        return InstallKind::WindowsInstaller;
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(path) = running_appimage_path() {
            return InstallKind::LinuxAppImage { path };
        }
        return InstallKind::NotifyOnly;
    }
    #[allow(unreachable_code)]
    InstallKind::NotifyOnly
}

/// True iff this run can perform an in-app self-update (download + verify +
/// apply). Windows always; Linux only as an AppImage; otherwise notify-only.
pub fn supports_in_app_install() -> bool {
    !matches!(install_kind(), InstallKind::NotifyOnly)
}

/// The absolute path of the running AppImage, taken from the `APPIMAGE`
/// environment variable the AppImage runtime exports. `None` when not running
/// as an AppImage (`.deb`/`.rpm` install, dev build) — the prerequisite for
/// in-app self-update on Linux.
#[cfg(target_os = "linux")]
pub(crate) fn running_appimage_path() -> Option<PathBuf> {
    appimage_path_from(std::env::var_os("APPIMAGE"))
}

/// Pure validation of the `APPIMAGE` value: `Some(path)` iff it is an absolute
/// path to an existing file. Split out so it is testable without mutating the
/// process environment.
#[cfg(target_os = "linux")]
fn appimage_path_from(var: Option<std::ffi::OsString>) -> Option<PathBuf> {
    let p = PathBuf::from(var?);
    (p.is_absolute() && p.is_file()).then_some(p)
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

/// True iff a process with this PID currently exists. Reconciles the in-memory
/// running map against the persisted PID file after a launcher restart (Bug A
/// part 2). Pair with `process_image_matches` to defeat PID recycling.
pub fn process_alive(pid: u32) -> bool {
    if pid <= 1 || pid == u32::MAX {
        return false;
    }
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        // SAFETY: OpenProcess returns null on failure; we only close a non-null handle.
        let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if h.is_null() {
            return false;
        }
        unsafe { CloseHandle(h) };
        true
    }
    #[cfg(unix)]
    {
        // signal 0 probes existence without delivering a signal.
        // SAFETY: FFI to kill(2) with sig 0; no memory shared.
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }
}

/// Best-effort check that PID's executable image path contains `needle`
/// (case-insensitive), e.g. "java". Guards against PID recycling: a recycled
/// PID belonging to an unrelated program must not be treated as our server.
/// Platforms without a process-image source return `false` — the orphan fix
/// simply won't be offered there.
pub fn process_image_matches(pid: u32, needle: &str) -> bool {
    let needle = needle.to_ascii_lowercase();
    #[cfg(target_os = "windows")]
    {
        match query_image_path_windows(pid) {
            Some(p) => p.to_ascii_lowercase().contains(&needle),
            None => false,
        }
    }
    #[cfg(target_os = "linux")]
    {
        match std::fs::read_link(format!("/proc/{pid}/exe")) {
            Ok(p) => p.to_string_lossy().to_ascii_lowercase().contains(&needle),
            Err(_) => false,
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = (pid, needle);
        false
    }
}

#[cfg(target_os = "windows")]
fn query_image_path_windows(pid: u32) -> Option<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, MAX_PATH};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // SAFETY: standard OpenProcess/Query/Close handshake; buffer sized to MAX_PATH,
    // len passed by &mut, handle closed on every path.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut buf = [0u16; MAX_PATH as usize];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(h);
        if ok == 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..len as usize]))
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

/// Free disk space (MB) available to the caller on the filesystem holding
/// `path`, or `None` when it can't be read (locked-down host, missing path).
/// Used only for the *advisory* low-disk server diagnosis — never an auto-fix.
/// Three cfg-gated impls; no new crate dependency.
pub fn free_disk_mb(path: &Path) -> Option<u64> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
        // GetDiskFreeSpaceExW wants a directory path; pass the dir itself, or
        // its parent when `path` is a not-yet-created file. NUL-terminated UTF-16.
        let dir = if path.is_dir() {
            path
        } else {
            path.parent().unwrap_or(path)
        };
        let wide: Vec<u16> = dir
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let mut free_to_caller: u64 = 0;
        // SAFETY: standard Win32 call. `wide` is NUL-terminated; we pass a valid
        // out-pointer for the one figure we need and null for the two we don't.
        let ok = unsafe {
            GetDiskFreeSpaceExW(
                wide.as_ptr(),
                &mut free_to_caller,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return None;
        }
        return Some(free_to_caller / (1024 * 1024));
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let dir = if path.is_dir() {
            path
        } else {
            path.parent().unwrap_or(path)
        };
        let c_path = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
        // SAFETY: statvfs writes into a zeroed POD struct; c_path is a valid,
        // NUL-terminated C string. rc != 0 means the path could not be statted.
        let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
        let rc = unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) };
        if rc != 0 {
            return None;
        }
        // bavail = blocks available to a non-privileged process; frsize = fragment size.
        let bytes = (stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64);
        return Some(bytes / (1024 * 1024));
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
                crate::diag!("tray: OpenProcess failed for pid {pid} — hiding immediately");
                return;
            }
            // 0 = input idle reached, 0x102 = WAIT_TIMEOUT — both
            // fall through to hide. 0xFFFFFFFF = WAIT_FAILED.
            let result = WaitForInputIdle(handle, 30_000);
            if result == 0xFFFFFFFF {
                crate::diag!("tray: WaitForInputIdle failed for pid {pid}");
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
    fn process_alive_true_for_self_false_for_impossible() {
        let me = std::process::id();
        assert!(process_alive(me), "current process must be alive");
        assert!(!process_alive(0), "pid 0 is never a user process");
        assert!(!process_alive(u32::MAX), "u32::MAX cannot be a live pid");
    }

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

    #[test]
    fn free_disk_mb_is_plausible_for_temp_dir() {
        // On any real CI/dev host the temp dir's filesystem has >0 free MB.
        let dir = tempfile::tempdir().unwrap();
        let mb = super::free_disk_mb(dir.path());
        // None is tolerated (locked-down/unsupported host); when Some, it must be
        // a sane positive figure, not a wrapped/garbage value.
        if let Some(free) = mb {
            assert!(free > 0, "temp filesystem reports 0 free MB: {free}");
            assert!(
                free < 1_000_000_000,
                "implausibly large free reading: {free} MB"
            );
        }
    }

    #[test]
    fn free_disk_mb_none_for_nonexistent_path() {
        let p = std::path::Path::new("/this/path/does/not/exist/lucerna-zzz");
        assert_eq!(
            super::free_disk_mb(p),
            None,
            "nonexistent path must read None"
        );
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn wait_for_window_ready_is_immediate_noop_off_windows() {
        // Off Windows there is no window-detect yet (deliberate fallback);
        // it must return promptly rather than block the hide-to-tray task.
        wait_for_window_ready(123_456).await;
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn appimage_path_from_accepts_only_absolute_existing_file() {
        use super::appimage_path_from;
        // Unset → not running as an AppImage.
        assert_eq!(appimage_path_from(None), None);
        // Relative path → rejected (must be absolute).
        assert_eq!(
            appimage_path_from(Some("relative/Lucerna.AppImage".into())),
            None
        );
        // Absolute but missing → rejected.
        assert_eq!(
            appimage_path_from(Some("/nonexistent/zzz/Lucerna.AppImage".into())),
            None
        );
        // Absolute path to a real file → accepted.
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("Lucerna.AppImage");
        std::fs::write(&f, b"x").unwrap();
        assert_eq!(
            appimage_path_from(Some(f.clone().into_os_string())),
            Some(f)
        );
    }
}
