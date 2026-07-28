//! Headless uninstall cleanup: `lucerna.exe --uninstall-cleanup [--list]`.
//!
//! Invoked by the NSIS uninstaller hook (`installer/hooks.nsh`) while the
//! binary still exists on disk. The uninstaller itself cannot know where the
//! relocatable data root lives, which OS-keyring entries belong to Lucerna, or
//! which legacy directories are safe to sweep — that knowledge stays here, in
//! the binary, as the single source of truth.
//!
//! Two modes:
//! - `--list`: print `SIZE|PRIMARY_PATH` (one line, no trailing newline; `|`
//!   is invalid in Windows paths so the split is unambiguous). Exit 0 when
//!   something exists to clean, 2 when nothing does (the hook skips its
//!   prompt), 1 on environment errors.
//! - delete: remove keyring entries first (ids are read from files that are
//!   about to be deleted), then directories. Per-target `OK`/`FAIL` report on
//!   stdout; exit 0 all-ok / 1 any-failure.
//!
//! The arg/env entry point is Windows-gated (NSIS is the only caller); the
//! planning/execution core is platform-neutral and unit-tested on every OS.

use std::path::{Path, PathBuf};

/// Everything the entry point resolves from the environment; injected so the
/// core is testable with tempdirs.
pub struct CleanupInput {
    /// `%APPDATA%\<identifier>` — holds `data-location.json`, logs, updates,
    /// and the whole data root when no redirect is configured.
    pub default_dir: PathBuf,
    /// `%LOCALAPPDATA%\<identifier>` — WebView2 profile data.
    pub webview_dir: Option<PathBuf>,
    /// Old default install dirs from pre-rename builds; swept only when their
    /// entire contents is an orphaned `uninstall.exe`.
    pub legacy_candidates: Vec<PathBuf>,
    /// The running helper's own path — any directory containing it is refused
    /// (a data root configured inside the install dir must not saw off the
    /// branch the uninstaller is standing on).
    pub current_exe: Option<PathBuf>,
}

/// Deletion plan: directories (primary data root first) plus the ids whose
/// keyring entries must go.
pub struct CleanupPlan {
    pub dirs: Vec<PathBuf>,
    pub account_ids: Vec<String>,
    pub server_ids: Vec<String>,
}

/// One executed target and what happened to it.
pub struct Report {
    pub label: String,
    pub outcome: Result<(), String>,
}

/// Build the plan. Missing/corrupt inputs degrade to "less to delete", never
/// to an error: a broken redirect means the custom root cannot be resolved
/// (its data is left alone), a broken account file means no account ids.
pub fn build_plan(input: &CleanupInput) -> CleanupPlan {
    let redirect_file = input.default_dir.join("data-location.json");
    let custom_root = crate::data_root::redirect::read(&redirect_file)
        .ok()
        .flatten()
        .map(|r| r.path)
        .filter(|p| p.is_dir());
    let enumeration_root = custom_root
        .clone()
        .unwrap_or_else(|| input.default_dir.clone());

    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(root) = custom_root {
        dirs.push(root);
    }
    dirs.push(input.default_dir.clone());
    if let Some(webview) = &input.webview_dir {
        dirs.push(webview.clone());
    }
    for legacy in &input.legacy_candidates {
        if legacy_is_sweepable(legacy) {
            dirs.push(legacy.clone());
        }
    }

    let exe_key = input.current_exe.as_deref().map(canonical_key);
    let mut seen: Vec<String> = Vec::new();
    dirs.retain(|d| {
        if !d.is_dir() {
            return false;
        }
        let key = canonical_key(d);
        if let Some(exe) = &exe_key {
            let prefix = format!("{key}{}", std::path::MAIN_SEPARATOR);
            if *exe == key || exe.starts_with(&prefix) {
                return false;
            }
        }
        if seen.contains(&key) {
            return false;
        }
        seen.push(key);
        true
    });

    CleanupPlan {
        dirs,
        account_ids: account_ids_in(&enumeration_root),
        server_ids: server_ids_in(&enumeration_root),
    }
}

/// Delete everything in the plan: keyring entries first (their ids came from
/// files inside the dirs we are about to remove), then the directories.
pub fn execute(plan: &CleanupPlan) -> Vec<Report> {
    use crate::accounts::keychain;
    let mut out = Vec::new();
    for id in &plan.account_ids {
        out.push(report(
            format!("keyring microsoft-refresh {id}"),
            keychain::delete(&keychain::refresh_token_key(id)),
        ));
        out.push(report(
            format!("keyring mc-access {id}"),
            keychain::delete(&keychain::mc_access_key(id)),
        ));
    }
    for id in &plan.server_ids {
        out.push(report(
            format!("keyring sftp-password {id}"),
            keychain::delete(&keychain::sftp_password_key(id)),
        ));
    }
    out.push(report(
        "keyring curseforge-api-key".to_string(),
        crate::mods::curseforge::keyring::clear(),
    ));
    for dir in &plan.dirs {
        out.push(report(
            format!("dir {}", dir.display()),
            remove_dir_tolerant(dir),
        ));
    }
    out
}

/// The `--list` output: `SIZE|PRIMARY_PATH`. Callers must ensure the plan is
/// non-empty (exit 2 otherwise).
pub fn list_line(plan: &CleanupPlan) -> Option<String> {
    let primary = plan.dirs.first()?;
    let total: u64 = plan.dirs.iter().map(|d| dir_size(d)).sum();
    Some(format!("{}|{}", format_size(total), primary.display()))
}

/// Human size for the consent prompt. Coarse on purpose: the number's job is
/// "is this worth keeping", not accounting.
pub fn format_size(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * MB;
    if bytes == 0 {
        return "0 MB".to_string();
    }
    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} GB", b / GB)
    } else {
        format!("{:.0} MB", (b / MB).max(1.0))
    }
}

/// A legacy dir may be swept only when every entry is a plain `uninstall.exe`
/// (or the dir is empty) — anything else means it is not the orphan litter we
/// think it is, including a live default-location install.
fn legacy_is_sweepable(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let is_uninstaller = entry.file_type().map(|t| t.is_file()).unwrap_or(false)
            && entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case("uninstall.exe");
        if !is_uninstaller {
            return false;
        }
    }
    true
}

fn account_ids_in(root: &Path) -> Vec<String> {
    crate::accounts::store::read_account_file(&root.join("account.json"))
        .map(|f| f.accounts.into_iter().map(|a| a.id).collect())
        .unwrap_or_default()
}

fn server_ids_in(root: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(root.join("servers")) else {
        return Vec::new();
    };
    let mut ids: Vec<String> = entries
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    ids.sort();
    ids
}

/// Case-folded canonical path string for dedup and the self-exe guard.
/// Canonicalization failure falls back to the literal path — dedup then still
/// works for byte-identical inputs, and the guard stays conservative.
fn canonical_key(p: &Path) -> String {
    std::fs::canonicalize(p)
        .unwrap_or_else(|_| p.to_path_buf())
        .to_string_lossy()
        .to_lowercase()
}

fn remove_dir_tolerant(dir: &Path) -> crate::error::Result<()> {
    match std::fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(crate::error::Error::io(dir.display().to_string(), e)),
    }
}

/// Best-effort recursive size. Unreadable entries count as 0; directory
/// symlinks are not followed (no cycles, no double counting).
fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut total = 0u64;
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            total += dir_size(&entry.path());
        } else if let Ok(md) = entry.metadata() {
            total += md.len();
        }
    }
    total
}

fn report<E: std::fmt::Display>(label: String, outcome: Result<(), E>) -> Report {
    Report {
        label,
        outcome: outcome.map_err(|e| e.to_string()),
    }
}

// ----- Windows entry point (the NSIS hook is the only caller) -----

/// Handle `--uninstall-cleanup [--list]`. Returns `Some(exit_code)` when the
/// flag is present (the caller exits with it), `None` to launch normally.
#[cfg(windows)]
pub fn maybe_run_from_args(identifier: &str) -> Option<i32> {
    let args: Vec<String> = std::env::args().collect();
    if !args.iter().any(|a| a == "--uninstall-cleanup") {
        return None;
    }
    let list_only = args.iter().any(|a| a == "--list");
    Some(run(identifier, list_only))
}

#[cfg(windows)]
fn run(identifier: &str, list_only: bool) -> i32 {
    use std::io::Write;
    let Some(roaming) = std::env::var_os("APPDATA").map(PathBuf::from) else {
        eprintln!("APPDATA is not set");
        return 1;
    };
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let input = CleanupInput {
        default_dir: roaming.join(identifier),
        webview_dir: local.as_ref().map(|l| l.join(identifier)),
        legacy_candidates: local
            .map(|l| vec![l.join("Lucerna"), l.join("lucerna")])
            .unwrap_or_default(),
        current_exe: std::env::current_exe().ok(),
    };
    let plan = build_plan(&input);

    if list_only {
        let Some(line) = list_line(&plan) else {
            return 2;
        };
        print!("{line}");
        let _ = std::io::stdout().flush();
        return 0;
    }

    let mut failed = false;
    for r in execute(&plan) {
        match &r.outcome {
            Ok(()) => println!("OK   {}", r.label),
            Err(e) => {
                failed = true;
                println!("FAIL {}: {e}", r.label);
            }
        }
    }
    let _ = std::io::stdout().flush();
    if failed {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_redirect(default_dir: &Path, target: &Path) {
        std::fs::create_dir_all(default_dir).unwrap();
        crate::data_root::redirect::write(
            &default_dir.join("data-location.json"),
            &crate::data_root::redirect::Redirect {
                path: target.to_path_buf(),
            },
        )
        .unwrap();
    }

    fn input(default_dir: &Path) -> CleanupInput {
        CleanupInput {
            default_dir: default_dir.to_path_buf(),
            webview_dir: None,
            legacy_candidates: Vec::new(),
            current_exe: None,
        }
    }

    #[test]
    fn custom_root_from_redirect_is_primary() {
        let t = tempdir().unwrap();
        let default_dir = t.path().join("default");
        let custom = t.path().join("LucernaData");
        std::fs::create_dir_all(&custom).unwrap();
        write_redirect(&default_dir, &custom);

        let plan = build_plan(&input(&default_dir));
        assert_eq!(plan.dirs[0], custom);
        assert!(plan.dirs.contains(&default_dir));
    }

    #[test]
    fn corrupt_redirect_falls_back_to_default_only() {
        let t = tempdir().unwrap();
        let default_dir = t.path().join("default");
        std::fs::create_dir_all(&default_dir).unwrap();
        std::fs::write(default_dir.join("data-location.json"), "{ not json").unwrap();

        let plan = build_plan(&input(&default_dir));
        assert_eq!(plan.dirs, vec![default_dir]);
    }

    #[test]
    fn redirect_to_missing_dir_is_ignored() {
        let t = tempdir().unwrap();
        let default_dir = t.path().join("default");
        write_redirect(&default_dir, &t.path().join("unplugged-usb"));

        let plan = build_plan(&input(&default_dir));
        assert_eq!(plan.dirs, vec![default_dir]);
    }

    #[test]
    fn account_and_server_ids_come_from_the_effective_root() {
        let t = tempdir().unwrap();
        let default_dir = t.path().join("default");
        let custom = t.path().join("data");
        std::fs::create_dir_all(custom.join("servers/srv-b")).unwrap();
        std::fs::create_dir_all(custom.join("servers/srv-a")).unwrap();
        std::fs::write(custom.join("servers/stray.txt"), b"x").unwrap();
        std::fs::write(
            custom.join("account.json"),
            r#"{"version":3,"accounts":[
                {"id":"ms-1","kind":"microsoft","name":"A","uuid":"u1","expires_at":1.0},
                {"id":"of-2","kind":"offline","name":"B","uuid":"u2","expires_at":null}
            ],"active_id":"ms-1"}"#,
        )
        .unwrap();
        write_redirect(&default_dir, &custom);

        let plan = build_plan(&input(&default_dir));
        assert_eq!(plan.account_ids, vec!["ms-1", "of-2"]);
        assert_eq!(plan.server_ids, vec!["srv-a", "srv-b"]);
    }

    #[test]
    fn missing_or_corrupt_account_file_yields_no_ids() {
        let t = tempdir().unwrap();
        let default_dir = t.path().join("default");
        std::fs::create_dir_all(&default_dir).unwrap();
        assert!(build_plan(&input(&default_dir)).account_ids.is_empty());

        std::fs::write(default_dir.join("account.json"), "garbage").unwrap();
        assert!(build_plan(&input(&default_dir)).account_ids.is_empty());
    }

    #[test]
    fn legacy_dir_swept_only_when_just_an_orphan_uninstaller() {
        let t = tempdir().unwrap();
        let default_dir = t.path().join("default");
        std::fs::create_dir_all(&default_dir).unwrap();

        let orphan = t.path().join("orphan");
        std::fs::create_dir_all(&orphan).unwrap();
        std::fs::write(orphan.join("Uninstall.exe"), b"MZ").unwrap();
        let empty = t.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        let live = t.path().join("live");
        std::fs::create_dir_all(&live).unwrap();
        std::fs::write(live.join("uninstall.exe"), b"MZ").unwrap();
        std::fs::write(live.join("lucerna.exe"), b"MZ").unwrap();

        let mut i = input(&default_dir);
        i.legacy_candidates = vec![orphan.clone(), empty.clone(), live.clone()];
        let plan = build_plan(&i);
        assert!(plan.dirs.contains(&orphan));
        assert!(plan.dirs.contains(&empty));
        assert!(!plan.dirs.contains(&live));
    }

    #[test]
    fn dir_containing_the_running_exe_is_refused() {
        let t = tempdir().unwrap();
        let default_dir = t.path().join("default");
        std::fs::create_dir_all(&default_dir).unwrap();
        let exe = default_dir.join("lucerna.exe");
        std::fs::write(&exe, b"MZ").unwrap();

        let mut i = input(&default_dir);
        i.current_exe = Some(exe);
        assert!(build_plan(&i).dirs.is_empty());
    }

    #[test]
    fn duplicate_dirs_are_deduped() {
        let t = tempdir().unwrap();
        let default_dir = t.path().join("default");
        std::fs::create_dir_all(&default_dir).unwrap();

        let mut i = input(&default_dir);
        i.webview_dir = Some(default_dir.clone());
        i.legacy_candidates = vec![default_dir.clone()];
        assert_eq!(build_plan(&i).dirs, vec![default_dir]);
    }

    #[test]
    fn execute_removes_dirs_keyring_entries_and_tolerates_missing() {
        use crate::accounts::keychain;
        let _guard = crate::test_env_lock();
        let t = tempdir().unwrap();
        let existing = t.path().join("data");
        std::fs::create_dir_all(existing.join("nested")).unwrap();
        std::fs::write(existing.join("nested/file.bin"), b"xx").unwrap();

        keychain::store(&keychain::refresh_token_key("uc-acc"), "r").unwrap();
        keychain::store(&keychain::mc_access_key("uc-acc"), "a").unwrap();
        keychain::store(&keychain::sftp_password_key("uc-srv"), "p").unwrap();
        crate::mods::curseforge::keyring::set("cfkey").unwrap();

        let plan = CleanupPlan {
            dirs: vec![existing.clone(), t.path().join("already-gone")],
            account_ids: vec!["uc-acc".into()],
            server_ids: vec!["uc-srv".into()],
        };
        let reports = execute(&plan);

        assert!(reports.iter().all(|r| r.outcome.is_ok()));
        assert!(!existing.exists());
        assert_eq!(
            keychain::retrieve(&keychain::refresh_token_key("uc-acc")).unwrap(),
            None
        );
        assert_eq!(
            keychain::retrieve(&keychain::mc_access_key("uc-acc")).unwrap(),
            None
        );
        assert_eq!(
            keychain::retrieve(&keychain::sftp_password_key("uc-srv")).unwrap(),
            None
        );
        assert_eq!(crate::mods::curseforge::keyring::get().unwrap(), None);
    }

    #[test]
    fn list_line_is_size_pipe_primary_path() {
        let t = tempdir().unwrap();
        let default_dir = t.path().join("default");
        std::fs::create_dir_all(&default_dir).unwrap();
        std::fs::write(default_dir.join("blob.bin"), vec![0u8; 3 * 1024 * 1024]).unwrap();

        let plan = build_plan(&input(&default_dir));
        let line = list_line(&plan).unwrap();
        let (size, path) = line.split_once('|').unwrap();
        assert_eq!(size, "3 MB");
        assert_eq!(path, default_dir.display().to_string());
    }

    #[test]
    fn list_line_none_when_nothing_exists() {
        let t = tempdir().unwrap();
        let plan = build_plan(&input(&t.path().join("never-created")));
        assert!(list_line(&plan).is_none());
    }

    #[test]
    fn format_size_units() {
        assert_eq!(format_size(0), "0 MB");
        assert_eq!(format_size(200 * 1024), "1 MB");
        assert_eq!(format_size(250 * 1024 * 1024), "250 MB");
        assert_eq!(format_size(14_800_000_000), "13.8 GB");
    }
}
