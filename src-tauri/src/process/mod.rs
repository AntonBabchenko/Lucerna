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

/// Запустить Forge/NeoForge installer: `java -jar <installer> --installServer`
/// в `work_dir`, дождаться, смапить spawn-failure/non-zero в
/// `Error::ServerInstallerFailed`. Installer генерирует серверную раскладку
/// (`libraries/`, run-скрипты / `@args`) прямо в `work_dir`.
pub async fn install_server(
    java_bin: &Path,
    installer_jar: &Path,
    work_dir: &Path,
    loader_label: &str,
) -> Result<()> {
    let mut cmd = tokio::process::Command::new(java_bin);
    cmd.arg("-jar")
        .arg(installer_jar)
        .arg("--installServer")
        .current_dir(work_dir);

    crate::diag!(
        "process: install_server ({loader_label}) {} in {}",
        installer_jar.display(),
        work_dir.display()
    );

    let output = cmd
        .output()
        .await
        .map_err(|e| Error::ServerInstallerFailed {
            loader: loader_label.to_string(),
            details: format!("spawn java: {e}"),
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::ServerInstallerFailed {
            loader: loader_label.to_string(),
            details: format!("installer exit {}: {}", output.status, stderr.trim()),
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

/// Spawn a long-lived server JVM. stdin is PIPED (console command channel),
/// stdout+stderr are PIPED (the caller spawns reader tasks to tee them to a
/// log file and emit line events). Unlike `spawn_minecraft`, stdin is NOT
/// closed. Caller must use the CONSOLE java (`java`, not `javaw`) so stdout
/// is actually produced on Windows.
pub fn spawn_server(
    java_bin: &Path,
    argv: &[String],
    work_dir: &Path,
) -> Result<tokio::process::Child> {
    let mut cmd = tokio::process::Command::new(java_bin);
    cmd.args(argv)
        .current_dir(work_dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(unix)]
    cmd.process_group(0);
    // CREATE_NO_WINDOW: the server uses the CONSOLE `java.exe` (so stdout is
    // produced) with all three stdio handles redirected to pipes. Launched from
    // the GUI (windows-subsystem) launcher without this flag, java.exe allocates
    // a console at startup — both a stray flashing window AND a known trigger for
    // STATUS_DLL_INIT_FAILED (0xC0000142): the process dies during console/DLL
    // init before emitting any output. Suppressing the console removes that init
    // step; piped stdout still works.
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.spawn().map_err(|e| Error::ServerSpawnFailed {
        details: format!("spawn {}: {e}", java_bin.display()),
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

/// Relaunch the just-replaced AppImage at `appimage` AFTER this process exits.
/// A detached `sh` polls until our PID disappears, then `exec`s the new file.
/// Waiting matters: the single-instance guard (tauri-plugin-single-instance)
/// would otherwise make the freshly launched instance focus the still-running
/// old one and exit, instead of starting the new build.
///
/// Injection: the new-AppImage path is passed via the `LUCERNA_NEW_APPIMAGE`
/// environment variable and expanded as a quoted `exec "$LUCERNA_NEW_APPIMAGE"`,
/// so a path with spaces or shell metacharacters cannot break out. The only
/// value interpolated into the script text is our own PID — a decimal `u32`
/// with no metacharacters, and not attacker-influenced.
///
/// The child is detached (`process_group(0)`, stdio nulled) so it survives our
/// imminent exit, which reparents it to init. Linux-only — in-app self-update
/// on other Unix targets is not supported.
#[cfg(target_os = "linux")]
pub fn spawn_appimage_relaunch(appimage: &Path) -> Result<()> {
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    let pid = std::process::id();
    let script = format!(
        "while kill -0 {pid} 2>/dev/null; do sleep 0.2; done; exec \"$LUCERNA_NEW_APPIMAGE\""
    );
    std::process::Command::new("sh")
        .arg("-c")
        .arg(&script)
        .env("LUCERNA_NEW_APPIMAGE", appimage)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        // Drop the Child: it is detached (own process group), and our process
        // exits right after — wait() would never run, and the shell reparents
        // to init rather than becoming our zombie.
        .map(|_child| ())
        .map_err(|e| Error::UpdateInstallFailed {
            details: format!("relaunch spawn {}: {e}", appimage.display()),
        })
}

#[cfg(not(target_os = "linux"))]
pub fn spawn_appimage_relaunch(appimage: &Path) -> Result<()> {
    Err(Error::UpdateInstallFailed {
        details: format!("AppImage relaunch is Linux-only ({})", appimage.display()),
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

/// Parse private (RFC1918) IPv4 addresses out of `ipconfig` output. `ipconfig`
/// labels are localized, so we extract IPv4 tokens and filter by range — not by
/// label. Drops loopback (127.) and link-local (169.254.). De-duped, order-
/// preserved. Pure (testable); the OS call lives in `local_ipv4_addresses`.
pub fn parse_lan_ipv4(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tok in text.split(|c: char| !(c.is_ascii_digit() || c == '.')) {
        let parts: Vec<&str> = tok.split('.').collect();
        if parts.len() != 4 {
            continue;
        }
        let octets: Option<Vec<u8>> = parts.iter().map(|p| p.parse::<u8>().ok()).collect();
        let Some(o) = octets else { continue };
        let is_private = o[0] == 10
            || (o[0] == 172 && (16..=31).contains(&o[1]))
            || (o[0] == 192 && o[1] == 168);
        let is_loopback = o[0] == 127;
        let is_link_local = o[0] == 169 && o[1] == 254;
        if is_private && !is_loopback && !is_link_local {
            let s = format!("{}.{}.{}.{}", o[0], o[1], o[2], o[3]);
            if !out.contains(&s) {
                out.push(s);
            }
        }
    }
    out
}

/// Host's private LAN IPv4 addresses. Windows: parse `ipconfig` (via the
/// process chokepoint). Other OSes: empty (the launcher targets Windows).
#[cfg(target_os = "windows")]
pub fn local_ipv4_addresses() -> Vec<String> {
    let out = std::process::Command::new("ipconfig").output();
    match out {
        Ok(o) => parse_lan_ipv4(&String::from_utf8_lossy(&o.stdout)),
        Err(_) => Vec::new(),
    }
}

#[cfg(not(target_os = "windows"))]
pub fn local_ipv4_addresses() -> Vec<String> {
    Vec::new()
}

/// True iff `netsh ... show rule name="<name>"` output contains the (ASCII) rule
/// name — present only when the rule exists. Locale-robust: the localized
/// "no rules match" text never contains our name. Pure (testable).
pub fn show_rule_indicates_present(output: &str, rule_name: &str) -> bool {
    output.contains(rule_name)
}

/// Windows: does an inbound firewall rule named `rule_name` exist?
#[cfg(target_os = "windows")]
pub fn firewall_rule_present(rule_name: &str) -> bool {
    let out = std::process::Command::new("netsh")
        .args([
            "advfirewall",
            "firewall",
            "show",
            "rule",
            &format!("name={rule_name}"),
        ])
        .output();
    match out {
        Ok(o) => show_rule_indicates_present(&String::from_utf8_lossy(&o.stdout), rule_name),
        Err(_) => false,
    }
}

#[cfg(not(target_os = "windows"))]
pub fn firewall_rule_present(_rule_name: &str) -> bool {
    false
}

/// Windows: add an inbound TCP allow rule for `port`, ELEVATED via a UAC prompt
/// (`powershell Start-Process netsh … -Verb RunAs`). Best-effort: returns Ok once
/// the elevation request is launched (we can't observe the elevated result).
#[cfg(target_os = "windows")]
pub fn firewall_add_rule_elevated(rule_name: &str, port: u16) -> crate::error::Result<()> {
    // Build the netsh arg-list as a PowerShell array; Start-Process -Verb RunAs
    // triggers UAC. Quote the rule name (it contains spaces).
    let args = format!(
        "'advfirewall','firewall','add','rule','name=\"{rule_name}\"','dir=in','action=allow','protocol=TCP','localport={port}'"
    );
    let cmd = format!(
        "Start-Process -FilePath netsh -ArgumentList {args} -Verb RunAs -WindowStyle Hidden"
    );
    std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &cmd])
        .spawn()
        .map(|_| ())
        .map_err(|e| crate::error::Error::io("<firewall>", format!("elevation spawn: {e}")))
}

#[cfg(not(target_os = "windows"))]
pub fn firewall_add_rule_elevated(_rule_name: &str, _port: u16) -> crate::error::Result<()> {
    Err(crate::error::Error::io("<firewall>", "Windows-only"))
}

/// Windows: remove the inbound allow rule named `rule_name`, ELEVATED via a UAC
/// prompt. Best-effort (we can't observe the elevated result). Mirrors
/// `firewall_add_rule_elevated` so deleting a server can clean up the rule it
/// added instead of leaving a stale open-port rule behind.
#[cfg(target_os = "windows")]
pub fn firewall_remove_rule_elevated(rule_name: &str) -> crate::error::Result<()> {
    let args =
        format!("'advfirewall','firewall','delete','rule','name=\"{rule_name}\"','protocol=TCP'");
    let cmd = format!(
        "Start-Process -FilePath netsh -ArgumentList {args} -Verb RunAs -WindowStyle Hidden"
    );
    std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &cmd])
        .spawn()
        .map(|_| ())
        .map_err(|e| crate::error::Error::io("<firewall>", format!("elevation spawn: {e}")))
}

#[cfg(not(target_os = "windows"))]
pub fn firewall_remove_rule_elevated(_rule_name: &str) -> crate::error::Result<()> {
    Err(crate::error::Error::io("<firewall>", "Windows-only"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::tempdir;

    #[tokio::test]
    async fn install_server_missing_java_errors() {
        let dir = tempdir().unwrap();
        let r = install_server(
            Path::new("nonexistent-java-xyz"),
            Path::new("installer.jar"),
            dir.path(),
            "forge",
        )
        .await;
        assert!(
            matches!(r, Err(Error::ServerInstallerFailed { .. })),
            "got: {r:?}"
        );
    }

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

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn spawn_appimage_relaunch_is_linux_only_off_linux() {
        // The relaunch helper exists on every target so callers compile, but
        // only Linux spawns; elsewhere it is a typed error. (On Linux it spawns
        // a real detached `sh`, so that path is left to manual verification
        // rather than a side-effecting unit test.)
        let r = spawn_appimage_relaunch(Path::new("/x/Lucerna.AppImage"));
        assert!(
            matches!(r, Err(Error::UpdateInstallFailed { .. })),
            "got {r:?}"
        );
    }

    #[test]
    fn spawn_server_missing_binary_errors() {
        let dir = tempdir().unwrap();
        let r = spawn_server(
            Path::new("nonexistent-java-xyz"),
            &["-version".to_string()],
            dir.path(),
        );
        assert!(
            matches!(r, Err(Error::ServerSpawnFailed { .. })),
            "got: {r:?}"
        );
    }

    #[test]
    fn parse_lan_ipv4_keeps_private_drops_loopback_and_linklocal() {
        // ipconfig output is localized, so we parse IPv4 tokens, not labels.
        let sample = "
Windows IP Configuration
   IPv4 Address. . . . . . . . . . . : 192.168.1.42
   Subnet Mask . . . . . . . . . . . : 255.255.255.0
Ethernet adapter Ethernet:
   IPv4 Address. . . . . . . . . . . : 10.0.0.5
   Autoconfiguration IPv4 Address. . : 169.254.13.7
   IPv4 Address. . . . . . . . . . . : 127.0.0.1
   Default Gateway . . . . . . . . . : 192.168.1.1
";
        let got = super::parse_lan_ipv4(sample);
        assert!(got.contains(&"192.168.1.42".to_string()));
        assert!(got.contains(&"10.0.0.5".to_string()));
        assert!(!got.iter().any(|a| a == "169.254.13.7"), "drop link-local");
        assert!(!got.iter().any(|a| a == "127.0.0.1"), "drop loopback");
        // Gateway is also private (192.168.1.1) — acceptable to include; the
        // important negatives are loopback + link-local. De-duped.
    }

    #[test]
    fn parse_lan_ipv4_dedupes_and_empty_on_none() {
        assert!(super::parse_lan_ipv4("no addresses here").is_empty());
        let dup = "10.1.2.3 ... 10.1.2.3";
        assert_eq!(super::parse_lan_ipv4(dup), vec!["10.1.2.3".to_string()]);
    }

    #[test]
    fn show_rule_present_detects_name_only_when_listed() {
        let name = "Lucerna Minecraft Server (TCP 25565)";
        assert!(super::show_rule_indicates_present(
            &format!("Rule Name:  {name}\nEnabled: Yes\n"),
            name
        ));
        // localized "no rules" text without our name → absent
        assert!(!super::show_rule_indicates_present(
            "No rules match the specified criteria.",
            name
        ));
        assert!(!super::show_rule_indicates_present(
            "Правила, соответствующие условиям, отсутствуют.",
            name
        ));
    }
}
