//! Headless uninstall cleanup: `lucerna.exe --uninstall-cleanup [--list] [--lang <LCID>]`.
//!
//! Invoked by the NSIS uninstaller hook (`installer/hooks.nsh`) while the
//! binary still exists on disk. The uninstaller itself cannot know where the
//! relocatable data root lives, which OS-keyring entries belong to Lucerna, or
//! which legacy directories are safe to sweep — that knowledge stays here, in
//! the binary, as the single source of truth.
//!
//! Two modes:
//! - `--list`: print a localized (EN, or RU for LCID 1049) inventory block —
//!   one line per directory that would be deleted (label, path, size), plus a
//!   saved-sign-ins summary and an offline-data-root note when applicable.
//!   The NSIS hook embeds the block VERBATIM in its consent MessageBox — no
//!   NSIS-side parsing. `\r\n` separators, no trailing newline. Exit 0 when
//!   something exists to clean, 2 when nothing does (the hook skips its
//!   prompt).
//! - delete: remove keyring entries first (ids are read from files that are
//!   about to be deleted), then directories. Per-target `OK`/`FAIL` report on
//!   stdout (English — it goes to the uninstaller log, not the dialog). Exit
//!   0 = full success, 1 = any failure, 3 = success but the
//!   `data-location.json` pointer was deliberately kept because the configured
//!   data root is not reachable right now (unplugged drive) — the hook keeps
//!   pointer restoration eligible on 3 so that data is rediscoverable later.
//!
//! The deletion plan scans `<exe dir>\LucernaData` INDEPENDENTLY of the
//! redirect pointer: a broken or lost pointer must never hide data sitting
//! right next to the binary being uninstalled (this exact failure motivated
//! the portable-data-root feature).
//!
//! The arg/env entry point is Windows-gated (NSIS is the only caller); the
//! planning/execution core is platform-neutral and unit-tested on every OS.

use std::path::{Path, PathBuf};

use crate::data_root::migrate::{dir_size, is_same_or_nested};

/// Everything the entry point resolves from the environment; injected so the
/// core is testable with tempdirs.
pub struct CleanupInput {
    /// `%APPDATA%\<identifier>` — holds `data-location.json`, logs, updates,
    /// and the whole data root when neither a redirect nor a portable root
    /// exists.
    pub default_dir: PathBuf,
    /// `<exe dir>\LucernaData` — the portable data root, scanned regardless
    /// of what the redirect says.
    pub exe_side_root: Option<PathBuf>,
    /// `%LOCALAPPDATA%\<identifier>` — WebView2 profile data (older builds;
    /// portable roots keep it under `<root>\webview`, which is inside the
    /// root and needs no separate entry).
    pub webview_dir: Option<PathBuf>,
    /// Old default install dirs from pre-rename builds; swept only when their
    /// entire contents is an orphaned `uninstall.exe`.
    pub legacy_candidates: Vec<PathBuf>,
    /// The running helper's own path — any directory containing it is refused
    /// (a data root configured inside the install dir must not saw off the
    /// branch the uninstaller is standing on).
    pub current_exe: Option<PathBuf>,
}

/// What a planned directory is, for the human inventory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    GameData,
    Settings,
    BrowserCache,
    LegacyOrphan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedDir {
    pub kind: TargetKind,
    pub path: PathBuf,
}

/// Deletion plan: directories (primary data root first) plus the ids whose
/// keyring entries must go.
pub struct CleanupPlan {
    pub dirs: Vec<PlannedDir>,
    pub account_ids: Vec<String>,
    pub server_ids: Vec<String>,
    /// A configured custom root that exists in the redirect but is not
    /// reachable right now (e.g. unplugged USB drive). Its data is never
    /// deleted, and `execute` preserves `data-location.json` so the data
    /// stays discoverable after a reinstall.
    pub unreachable_root: Option<PathBuf>,
    /// The default app-data dir — `execute` needs it to know which planned
    /// dir holds the pointer file when `unreachable_root` is set.
    pub default_dir: PathBuf,
}

/// One executed target and what happened to it.
pub struct Report {
    pub label: String,
    pub outcome: Result<(), String>,
}

/// Inventory language. The LCID string comes from NSIS `$LANGUAGE`; the
/// installer ships English + Russian only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Ru,
}

impl Lang {
    pub fn from_lcid(lcid: &str) -> Self {
        if lcid.trim() == "1049" {
            Lang::Ru
        } else {
            Lang::En
        }
    }
}

/// Build the plan. Missing/corrupt inputs degrade to "less to delete", never
/// to an error: a broken redirect means the custom root cannot be resolved
/// (its data is left alone), a broken account file means no account ids.
pub fn build_plan(input: &CleanupInput) -> CleanupPlan {
    let redirect_file = input.default_dir.join("data-location.json");
    let configured_root = crate::data_root::redirect::read(&redirect_file)
        .ok()
        .flatten()
        .map(|r| r.path);
    let (custom_root, unreachable_root) = match configured_root {
        Some(p) if p.is_dir() => (Some(p), None),
        other => (None, other),
    };

    let default_kind = if dir_has_data_shape(&input.default_dir) {
        TargetKind::GameData
    } else {
        TargetKind::Settings
    };
    let mut dirs: Vec<PlannedDir> = Vec::new();
    if let Some(root) = custom_root {
        dirs.push(PlannedDir {
            kind: TargetKind::GameData,
            path: root,
        });
    }
    if let Some(exe_side) = &input.exe_side_root {
        // Deletion-worthiness gate: only a dir that recognizably belongs to
        // Lucerna (or is empty) may be planned. A foreign folder that merely
        // shares the LucernaData name must never be wiped by our consent.
        if exe_side_is_lucerna_like(exe_side) {
            dirs.push(PlannedDir {
                kind: TargetKind::GameData,
                path: exe_side.clone(),
            });
        }
    }
    dirs.push(PlannedDir {
        kind: default_kind,
        path: input.default_dir.clone(),
    });
    if let Some(webview) = &input.webview_dir {
        dirs.push(PlannedDir {
            kind: TargetKind::BrowserCache,
            path: webview.clone(),
        });
    }
    for legacy in &input.legacy_candidates {
        if legacy_is_sweepable(legacy) {
            dirs.push(PlannedDir {
                kind: TargetKind::LegacyOrphan,
                path: legacy.clone(),
            });
        }
    }

    let exe = input.current_exe.as_deref();
    let mut kept: Vec<PlannedDir> = Vec::new();
    for dir in dirs {
        if !dir.path.is_dir() || is_filesystem_root(&dir.path) {
            continue;
        }
        if exe.is_some_and(|e| is_same_or_nested(&dir.path, e)) {
            continue;
        }
        let duplicate = kept.iter().any(|k| {
            is_same_or_nested(&k.path, &dir.path) && is_same_or_nested(&dir.path, &k.path)
        });
        if !duplicate {
            kept.push(dir);
        }
    }

    // Credentials are enumerated from EVERY planned data dir, not just the
    // primary root: a de-pointered old root (the exe-side scan's whole reason
    // to exist) still names accounts whose tokens must go.
    let mut account_ids: Vec<String> = Vec::new();
    let mut server_ids: Vec<String> = Vec::new();
    for dir in &kept {
        account_ids.extend(account_ids_in(&dir.path));
        server_ids.extend(server_ids_in(&dir.path));
    }
    account_ids.sort();
    account_ids.dedup();
    server_ids.sort();
    server_ids.dedup();

    CleanupPlan {
        dirs: kept,
        account_ids,
        server_ids,
        unreachable_root,
        default_dir: input.default_dir.clone(),
    }
}

/// Delete everything in the plan: keyring entries first (their ids came from
/// files inside the dirs we are about to remove), then the directories. When
/// the configured root is unreachable, the default dir loses its contents but
/// keeps `data-location.json` — deleting the only pointer to data we cannot
/// reach would orphan that data forever.
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
        let preserve_pointer = plan.unreachable_root.is_some() && dir.path == plan.default_dir;
        if preserve_pointer {
            out.push(report(
                format!(
                    "dir {} (kept data-location.json: configured data root is not reachable)",
                    dir.path.display()
                ),
                remove_children_except(&dir.path, "data-location.json"),
            ));
        } else {
            out.push(report(
                format!("dir {}", dir.path.display()),
                remove_dir_tolerant(&dir.path),
            ));
        }
    }
    out
}

/// The `--list` output: the full localized inventory block the NSIS hook
/// embeds verbatim in the consent dialog. `None` when there is nothing to
/// clean (exit 2). Kept comfortably under NSIS's string budget: a handful of
/// lines, long paths middle-truncated.
pub fn inventory_block(plan: &CleanupPlan, lang: Lang) -> Option<String> {
    if plan.dirs.is_empty() {
        return None;
    }
    let mut lines: Vec<String> = Vec::new();
    for dir in &plan.dirs {
        let label = kind_label(dir.kind, lang);
        let size = format_size_lang(dir_size(&dir.path), lang);
        let path = truncate_middle(&dir.path.display().to_string(), 90);
        lines.push(format!("{label}: {path} ({size})"));
    }
    let accounts = plan.account_ids.len();
    let servers = plan.server_ids.len();
    if accounts + servers > 0 {
        lines.push(creds_line(accounts, servers, lang));
    }
    if let Some(offline) = &plan.unreachable_root {
        let path = truncate_middle(&offline.display().to_string(), 90);
        lines.push(match lang {
            Lang::En => {
                format!("Configured data location is offline and will NOT be deleted: {path}")
            }
            Lang::Ru => {
                format!("Настроенное хранилище сейчас недоступно и НЕ будет удалено: {path}")
            }
        });
    }
    Some(lines.join("\r\n"))
}

fn kind_label(kind: TargetKind, lang: Lang) -> &'static str {
    match (kind, lang) {
        (TargetKind::GameData, Lang::En) => "Game data (instances, worlds, mods)",
        (TargetKind::GameData, Lang::Ru) => "Игровые данные (инстансы, миры, моды)",
        (TargetKind::Settings, Lang::En) => "Launcher settings and logs",
        (TargetKind::Settings, Lang::Ru) => "Настройки и логи лаунчера",
        (TargetKind::BrowserCache, Lang::En) => "Embedded browser cache",
        (TargetKind::BrowserCache, Lang::Ru) => "Кеш встроенного браузера",
        (TargetKind::LegacyOrphan, Lang::En) => "Leftover uninstaller from an older version",
        (TargetKind::LegacyOrphan, Lang::Ru) => "Остатки установщика старой версии",
    }
}

fn creds_line(accounts: usize, servers: usize, lang: Lang) -> String {
    let mut parts: Vec<String> = Vec::new();
    match lang {
        Lang::En => {
            if accounts > 0 {
                parts.push(format!("{accounts} account(s)"));
            }
            if servers > 0 {
                parts.push(format!("{servers} server password(s)"));
            }
            format!(
                "Saved sign-ins in Windows Credential Manager: {}",
                parts.join(", ")
            )
        }
        Lang::Ru => {
            if accounts > 0 {
                parts.push(format!("аккаунтов — {accounts}"));
            }
            if servers > 0 {
                parts.push(format!("паролей серверов — {servers}"));
            }
            format!(
                "Сохранённые входы в диспетчере учётных данных Windows: {}",
                parts.join(", ")
            )
        }
    }
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

/// Localized size string. RU swaps units and the decimal separator; the EN
/// form is the canonical one `format_size` produces ("13.8 GB", "250 MB").
pub fn format_size_lang(bytes: u64, lang: Lang) -> String {
    let en = format_size(bytes);
    match lang {
        Lang::En => en,
        Lang::Ru => en.replace('.', ",").replace("GB", "ГБ").replace("MB", "МБ"),
    }
}

/// Middle-truncate to `max` characters (char-counted, display string) so a
/// pathological path cannot blow the NSIS string budget.
fn truncate_middle(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        return s.to_string();
    }
    let keep = max.saturating_sub(1) / 2;
    let head: String = chars[..keep].iter().collect();
    let tail: String = chars[chars.len() - keep..].iter().collect();
    format!("{head}…{tail}")
}

/// Does this directory look like a (current or former) data root, as opposed
/// to a bare settings/scratch dir? Looser than the startup resolver's
/// `looks_like_data_root` on purpose — this only picks the inventory LABEL.
/// (`.lucerna-migrated` has never been written by any code in this
/// repository's history, but it is OBSERVED on a real user's migrated root —
/// most likely a pre-merge build of the relocation feature left it. Matching
/// it costs nothing and classifies such disks correctly.)
fn dir_has_data_shape(dir: &Path) -> bool {
    dir.join("app.json").is_file()
        || dir.join("instances").is_dir()
        || dir.join(".lucerna-migrated").exists()
}

/// Top-level names a Lucerna data root can legitimately contain. Used to
/// decide whether an exe-adjacent `LucernaData` dir is OURS to delete.
const LUCERNA_ROOT_ENTRIES: [&str; 18] = [
    "app.json",
    "account.json",
    "data-location.json",
    ".lucerna-migrated",
    "instances",
    "versions",
    "libraries",
    "assets",
    "jres",
    "logs",
    "updates",
    "mod-cache",
    "mods-cache",
    "servers",
    "skins",
    "capes",
    "forge",
    "webview",
];

/// The exe-adjacent dir qualifies for deletion when it is empty or contains
/// at least one recognizably-Lucerna top-level entry. A dir with NONE of
/// these is somebody else's folder that just happens to be called
/// LucernaData, and consent to delete "game data" does not extend to it.
fn exe_side_is_lucerna_like(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    let mut any_entry = false;
    for entry in entries.flatten() {
        any_entry = true;
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        if LUCERNA_ROOT_ENTRIES
            .iter()
            .any(|known| known.eq_ignore_ascii_case(&name))
        {
            return true;
        }
    }
    !any_entry
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

/// True when `p` resolves to a filesystem root (`D:\`, `/`, a bare UNC
/// share). A drive root can legitimately pass every softer check (absolute,
/// empty, is_dir) yet must never be fed to `remove_dir_all` — a redirect
/// pointing at "the whole drive" would otherwise erase the entire volume.
fn is_filesystem_root(p: &Path) -> bool {
    std::fs::canonicalize(p)
        .unwrap_or_else(|_| p.to_path_buf())
        .parent()
        .is_none()
}

fn remove_dir_tolerant(dir: &Path) -> crate::error::Result<()> {
    if is_filesystem_root(dir) {
        // Second layer of the guard in `build_plan`: even a hand-constructed
        // plan must never remove a whole volume.
        return Err(crate::error::Error::io(
            dir.display().to_string(),
            "refusing to remove a filesystem root".to_string(),
        ));
    }
    match std::fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(crate::error::Error::io(dir.display().to_string(), e)),
    }
}

/// Remove every child of `dir` except the top-level entry named `keep`. The
/// dir itself stays (it must keep holding the preserved file).
fn remove_children_except(dir: &Path, keep: &str) -> crate::error::Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(crate::error::Error::io(dir.display().to_string(), e)),
    };
    for entry in entries.flatten() {
        if entry
            .file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case(keep)
        {
            continue;
        }
        let path = entry.path();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let removed = if is_dir {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        match removed {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(crate::error::Error::io(path.display().to_string(), e)),
        }
    }
    Ok(())
}

fn report<E: std::fmt::Display>(label: String, outcome: Result<(), E>) -> Report {
    Report {
        label,
        outcome: outcome.map_err(|e| e.to_string()),
    }
}

// ----- Windows entry point (the NSIS hook is the only caller) -----

/// Handle `--uninstall-cleanup [--list] [--lang <LCID>]`. Returns
/// `Some(exit_code)` when the flag is present (the caller exits with it),
/// `None` to launch normally.
#[cfg(windows)]
pub fn maybe_run_from_args(identifier: &str) -> Option<i32> {
    let args: Vec<String> = std::env::args().collect();
    if !args.iter().any(|a| a == "--uninstall-cleanup") {
        return None;
    }
    let list_only = args.iter().any(|a| a == "--list");
    let lang = args
        .iter()
        .position(|a| a == "--lang")
        .and_then(|i| args.get(i + 1))
        .map(|v| Lang::from_lcid(v))
        .unwrap_or(Lang::En);
    Some(run(identifier, list_only, lang))
}

#[cfg(windows)]
fn run(identifier: &str, list_only: bool, lang: Lang) -> i32 {
    use std::io::Write;
    let Some(roaming) = std::env::var_os("APPDATA").map(PathBuf::from) else {
        eprintln!("APPDATA is not set");
        return 1;
    };
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let current_exe = std::env::current_exe().ok();
    let exe_side_root = current_exe
        .as_deref()
        .and_then(Path::parent)
        .map(|d| d.join("LucernaData"));
    let input = CleanupInput {
        default_dir: roaming.join(identifier),
        exe_side_root,
        webview_dir: local.as_ref().map(|l| l.join(identifier)),
        legacy_candidates: local
            .map(|l| vec![l.join("Lucerna"), l.join("lucerna")])
            .unwrap_or_default(),
        current_exe,
    };
    let plan = build_plan(&input);

    if list_only {
        let Some(block) = inventory_block(&plan, lang) else {
            return 2;
        };
        print!("{block}");
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
    } else if plan.unreachable_root.is_some() {
        3
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
            exe_side_root: None,
            webview_dir: None,
            legacy_candidates: Vec::new(),
            current_exe: None,
        }
    }

    fn paths(plan: &CleanupPlan) -> Vec<PathBuf> {
        plan.dirs.iter().map(|d| d.path.clone()).collect()
    }

    #[test]
    fn custom_root_from_redirect_is_primary() {
        let t = tempdir().unwrap();
        let default_dir = t.path().join("default");
        let custom = t.path().join("LucernaData");
        std::fs::create_dir_all(&custom).unwrap();
        write_redirect(&default_dir, &custom);

        let plan = build_plan(&input(&default_dir));
        assert_eq!(plan.dirs[0].path, custom);
        assert_eq!(plan.dirs[0].kind, TargetKind::GameData);
        assert!(paths(&plan).contains(&default_dir));
        assert!(plan.unreachable_root.is_none());
    }

    #[test]
    fn exe_side_root_is_planned_without_any_redirect() {
        // The whole point of the exe-side scan: a lost/broken pointer must not
        // hide data sitting next to the binary being uninstalled.
        let t = tempdir().unwrap();
        let default_dir = t.path().join("default");
        std::fs::create_dir_all(&default_dir).unwrap();
        let exe_side = t.path().join("install/LucernaData");
        std::fs::create_dir_all(&exe_side).unwrap();

        let mut i = input(&default_dir);
        i.exe_side_root = Some(exe_side.clone());
        i.current_exe = Some(t.path().join("install/lucerna.exe"));
        let plan = build_plan(&i);
        assert_eq!(plan.dirs[0].path, exe_side);
        assert_eq!(plan.dirs[0].kind, TargetKind::GameData);
    }

    #[test]
    fn foreign_dir_named_lucernadata_is_not_planned() {
        // Consent to delete "game data" does not extend to somebody else's
        // folder that merely shares the name.
        let t = tempdir().unwrap();
        let default_dir = t.path().join("default");
        std::fs::create_dir_all(&default_dir).unwrap();
        let foreign = t.path().join("install/LucernaData");
        std::fs::create_dir_all(&foreign).unwrap();
        std::fs::write(foreign.join("vacation-photos.zip"), b"zip").unwrap();

        let mut i = input(&default_dir);
        i.exe_side_root = Some(foreign.clone());
        assert!(!paths(&build_plan(&i)).contains(&foreign));

        // One recognizable Lucerna entry qualifies the dir again.
        std::fs::create_dir_all(foreign.join("instances")).unwrap();
        assert!(paths(&build_plan(&i)).contains(&foreign));
    }

    #[test]
    fn exe_side_root_dedupes_against_equal_redirect_target() {
        let t = tempdir().unwrap();
        let default_dir = t.path().join("default");
        let root = t.path().join("install/LucernaData");
        std::fs::create_dir_all(&root).unwrap();
        write_redirect(&default_dir, &root);

        let mut i = input(&default_dir);
        i.exe_side_root = Some(root.clone());
        let plan = build_plan(&i);
        assert_eq!(
            paths(&plan).iter().filter(|p| **p == root).count(),
            1,
            "same dir must be planned once"
        );
    }

    #[test]
    fn corrupt_redirect_falls_back_to_default_only() {
        let t = tempdir().unwrap();
        let default_dir = t.path().join("default");
        std::fs::create_dir_all(&default_dir).unwrap();
        std::fs::write(default_dir.join("data-location.json"), "{ not json").unwrap();

        let plan = build_plan(&input(&default_dir));
        assert_eq!(paths(&plan), vec![default_dir]);
        assert!(plan.unreachable_root.is_none());
    }

    #[test]
    fn redirect_to_missing_dir_is_reported_unreachable_not_planned() {
        let t = tempdir().unwrap();
        let default_dir = t.path().join("default");
        let gone = t.path().join("unplugged-usb");
        write_redirect(&default_dir, &gone);

        let plan = build_plan(&input(&default_dir));
        assert_eq!(paths(&plan), vec![default_dir]);
        assert_eq!(plan.unreachable_root, Some(gone));
    }

    #[test]
    fn filesystem_root_is_never_planned() {
        let t = tempdir().unwrap();
        let default_dir = t.path().join("default");
        std::fs::create_dir_all(&default_dir).unwrap();
        // "/" on Unix, "\" (current-drive root) on Windows — a real, existing
        // root directory either way.
        let root = PathBuf::from(std::path::MAIN_SEPARATOR.to_string());
        assert!(root.is_dir(), "test premise: root path exists");

        let mut i = input(&default_dir);
        i.webview_dir = Some(root.clone());
        i.exe_side_root = Some(root);
        let plan = build_plan(&i);
        assert_eq!(paths(&plan), vec![default_dir]);
    }

    #[test]
    fn remove_dir_tolerant_refuses_filesystem_root() {
        let root = PathBuf::from(std::path::MAIN_SEPARATOR.to_string());
        let outcome = remove_dir_tolerant(&root);
        assert!(outcome.is_err(), "must refuse to remove a filesystem root");
    }

    #[test]
    fn credential_ids_are_unioned_across_all_planned_roots() {
        // Stale creds of a de-pointered root must still be cleaned: ids come
        // from every planned dir, deduped.
        let t = tempdir().unwrap();
        let default_dir = t.path().join("default");
        std::fs::create_dir_all(default_dir.join("servers/srv-shared")).unwrap();
        std::fs::write(
            default_dir.join("account.json"),
            r#"{"version":3,"accounts":[
                {"id":"ms-new","kind":"microsoft","name":"N","uuid":"u1","expires_at":1.0}
            ],"active_id":"ms-new"}"#,
        )
        .unwrap();
        let exe_side = t.path().join("install/LucernaData");
        std::fs::create_dir_all(exe_side.join("servers/srv-shared")).unwrap();
        std::fs::create_dir_all(exe_side.join("servers/srv-old")).unwrap();
        std::fs::write(
            exe_side.join("account.json"),
            r#"{"version":3,"accounts":[
                {"id":"ms-old","kind":"microsoft","name":"O","uuid":"u2","expires_at":1.0},
                {"id":"ms-new","kind":"microsoft","name":"N","uuid":"u1","expires_at":1.0}
            ],"active_id":"ms-old"}"#,
        )
        .unwrap();

        let mut i = input(&default_dir);
        i.exe_side_root = Some(exe_side);
        let plan = build_plan(&i);
        assert_eq!(plan.account_ids, vec!["ms-new", "ms-old"]);
        assert_eq!(plan.server_ids, vec!["srv-old", "srv-shared"]);
    }

    #[test]
    fn default_dir_kind_follows_data_shape() {
        let t = tempdir().unwrap();
        let bare = t.path().join("bare");
        std::fs::create_dir_all(&bare).unwrap();
        assert_eq!(build_plan(&input(&bare)).dirs[0].kind, TargetKind::Settings);

        let dataful = t.path().join("dataful");
        std::fs::create_dir_all(dataful.join("instances")).unwrap();
        assert_eq!(
            build_plan(&input(&dataful)).dirs[0].kind,
            TargetKind::GameData
        );
    }

    #[test]
    fn legacy_dir_swept_only_when_just_an_orphan_uninstaller() {
        let t = tempdir().unwrap();
        let default_dir = t.path().join("default");
        std::fs::create_dir_all(&default_dir).unwrap();

        let orphan = t.path().join("orphan");
        std::fs::create_dir_all(&orphan).unwrap();
        std::fs::write(orphan.join("Uninstall.exe"), b"MZ").unwrap();
        let live = t.path().join("live");
        std::fs::create_dir_all(&live).unwrap();
        std::fs::write(live.join("uninstall.exe"), b"MZ").unwrap();
        std::fs::write(live.join("lucerna.exe"), b"MZ").unwrap();

        let mut i = input(&default_dir);
        i.legacy_candidates = vec![orphan.clone(), live.clone()];
        let plan = build_plan(&i);
        let planned = paths(&plan);
        assert!(planned.contains(&orphan));
        assert!(!planned.contains(&live));
        assert_eq!(
            plan.dirs.iter().find(|d| d.path == orphan).map(|d| d.kind),
            Some(TargetKind::LegacyOrphan)
        );
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
    fn custom_root_containing_the_running_exe_is_refused() {
        let t = tempdir().unwrap();
        let default_dir = t.path().join("default");
        let custom = t.path().join("install-dir/LucernaData");
        std::fs::create_dir_all(custom.join("nested")).unwrap();
        let exe = custom.join("nested/lucerna.exe");
        std::fs::write(&exe, b"MZ").unwrap();
        write_redirect(&default_dir, &custom);

        let mut i = input(&default_dir);
        i.current_exe = Some(exe);
        let plan = build_plan(&i);
        assert!(!paths(&plan).contains(&custom));
        assert!(paths(&plan).contains(&default_dir));
    }

    #[test]
    fn case_variant_duplicate_is_deduped_where_the_fs_is_case_insensitive() {
        let t = tempdir().unwrap();
        let default_dir = t.path().join("Default");
        std::fs::create_dir_all(&default_dir).unwrap();

        let mut i = input(&default_dir);
        i.webview_dir = Some(t.path().join("DEFAULT"));
        assert_eq!(paths(&build_plan(&i)), vec![default_dir]);
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
            dirs: vec![
                PlannedDir {
                    kind: TargetKind::GameData,
                    path: existing.clone(),
                },
                PlannedDir {
                    kind: TargetKind::Settings,
                    path: t.path().join("already-gone"),
                },
            ],
            account_ids: vec!["uc-acc".into()],
            server_ids: vec!["uc-srv".into()],
            unreachable_root: None,
            default_dir: t.path().join("default"),
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
    fn unreachable_root_preserves_the_pointer_file() {
        let _guard = crate::test_env_lock();
        let t = tempdir().unwrap();
        let default_dir = t.path().join("default");
        let gone = t.path().join("unplugged-usb");
        write_redirect(&default_dir, &gone);
        std::fs::create_dir_all(default_dir.join("logs")).unwrap();
        std::fs::write(default_dir.join("logs/lucerna.log"), b"log").unwrap();
        std::fs::write(default_dir.join("app.json"), b"{}").unwrap();

        let plan = build_plan(&input(&default_dir));
        assert_eq!(plan.unreachable_root, Some(gone));
        let reports = execute(&plan);

        assert!(reports.iter().all(|r| r.outcome.is_ok()));
        assert!(
            default_dir.join("data-location.json").is_file(),
            "the pointer to unreachable data must survive"
        );
        assert!(!default_dir.join("logs").exists());
        assert!(!default_dir.join("app.json").exists());
    }

    #[test]
    fn inventory_lists_every_target_with_localized_labels() {
        let t = tempdir().unwrap();
        let default_dir = t.path().join("default");
        std::fs::create_dir_all(&default_dir).unwrap();
        std::fs::write(default_dir.join("blob.bin"), vec![0u8; 3 * 1024 * 1024]).unwrap();
        let exe_side = t.path().join("install/LucernaData");
        std::fs::create_dir_all(exe_side.join("instances")).unwrap();
        std::fs::write(
            exe_side.join("account.json"),
            r#"{"version":3,"accounts":[
                {"id":"ms-1","kind":"microsoft","name":"A","uuid":"u","expires_at":1.0}
            ],"active_id":"ms-1"}"#,
        )
        .unwrap();

        let mut i = input(&default_dir);
        i.exe_side_root = Some(exe_side.clone());
        let plan = build_plan(&i);

        let en = inventory_block(&plan, Lang::En).unwrap();
        assert!(en.contains("Game data (instances, worlds, mods)"));
        assert!(en.contains(&exe_side.display().to_string()));
        assert!(en.contains("Launcher settings and logs"));
        assert!(en.contains("3 MB"));
        assert!(en.contains("Saved sign-ins in Windows Credential Manager: 1 account(s)"));
        assert!(en.contains("\r\n"));

        let ru = inventory_block(&plan, Lang::Ru).unwrap();
        assert!(ru.contains("Игровые данные (инстансы, миры, моды)"));
        assert!(ru.contains("Настройки и логи лаунчера"));
        assert!(ru.contains("3 МБ"));
        assert!(ru.contains("аккаунтов — 1"));
    }

    #[test]
    fn inventory_names_the_offline_configured_root() {
        let t = tempdir().unwrap();
        let default_dir = t.path().join("default");
        let gone = t.path().join("unplugged-usb");
        write_redirect(&default_dir, &gone);

        let plan = build_plan(&input(&default_dir));
        let ru = inventory_block(&plan, Lang::Ru).unwrap();
        assert!(ru.contains("НЕ будет удалено"));
        assert!(ru.contains(&gone.display().to_string()));
        let en = inventory_block(&plan, Lang::En).unwrap();
        assert!(en.contains("will NOT be deleted"));
    }

    #[test]
    fn inventory_none_when_nothing_exists() {
        let t = tempdir().unwrap();
        let plan = build_plan(&input(&t.path().join("never-created")));
        assert!(inventory_block(&plan, Lang::En).is_none());
    }

    #[test]
    fn long_paths_are_middle_truncated_in_inventory() {
        assert_eq!(truncate_middle("short", 90), "short");
        let long = "x".repeat(200);
        let cut = truncate_middle(&long, 90);
        assert!(cut.chars().count() <= 90);
        assert!(cut.contains('…'));
    }

    #[test]
    fn format_size_units_and_localization() {
        assert_eq!(format_size(0), "0 MB");
        assert_eq!(format_size(200 * 1024), "1 MB");
        assert_eq!(format_size(250 * 1024 * 1024), "250 MB");
        assert_eq!(format_size(14_800_000_000), "13.8 GB");
        assert_eq!(format_size_lang(14_800_000_000, Lang::Ru), "13,8 ГБ");
        assert_eq!(format_size_lang(250 * 1024 * 1024, Lang::Ru), "250 МБ");
    }

    #[test]
    fn lang_from_lcid() {
        assert_eq!(Lang::from_lcid("1049"), Lang::Ru);
        assert_eq!(Lang::from_lcid("1033"), Lang::En);
        assert_eq!(Lang::from_lcid("garbage"), Lang::En);
    }
}
