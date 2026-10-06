//! Reusable helpers that download a modpack version archive to the OS temp
//! dir and return the local path. Extracted from `modpack_fetch_to_temp` so
//! the `ModpackSource` adapters can delegate to them without duplicating the
//! HTTP + temp-write logic. These helpers are the canonical archive-staging
//! logic used by the Modrinth, CurseForge, FTB and ATLauncher adapters.
//!
//! ## Who removes a staged file
//!
//! A staged path goes to the webview, which hands it back across several
//! commands (`modpack_inspect` → `modpack_import`, `modpack_compute_update` →
//! `modpack_apply_update`), so the file must outlive the call that wrote it.
//! This module owns it from then on:
//!
//! - **Consumed.** `modpack_import` and `modpack_apply_update` hold a
//!   [`ConsumeGuard`] for their whole run, so the file goes when the operation
//!   ends, however it ends. No UI path reads the path again after that: every
//!   retry stages a new file.
//! - **Abandoned.** A file no operation consumes (a dialog closed, Back to
//!   another version, a preview that failed, a launcher closed or killed
//!   midway) is removed by [`sweep_stale`] once it is [`STALE_AFTER`] old: at
//!   startup, and each time a new file is staged.
//!
//! Only a name this module writes (`<uuid>.<ext>`) directly inside
//! [`staging_dir`] is ever removed. A pack the user picked or dropped reaches
//! the same commands by its own path, and is never touched.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use tauri::Manager;

use crate::error::Error;

/// What a staged file holds, which fixes its extension. The extension plays
/// no part in recognising a staged file later (see [`is_staged_name`]), so a
/// new kind is cleaned up like the others with nothing else to update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StagedKind {
    /// A Modrinth `.mrpack` archive.
    Mrpack,
    /// A CurseForge modpack `.zip`.
    CurseforgeZip,
    /// An FTB version, resolved to a serialised `ModpackSummary`.
    FtbSummary,
    /// An ATLauncher version, resolved to a serialised `ModpackSummary`.
    AtlauncherSummary,
}

impl StagedKind {
    fn extension(self) -> &'static str {
        match self {
            StagedKind::Mrpack => "mrpack",
            StagedKind::CurseforgeZip => "zip",
            StagedKind::FtbSummary => "ftbpack.json",
            StagedKind::AtlauncherSummary => "atlpack.json",
        }
    }
}

/// How old a staged file must be before [`sweep_stale`] removes it. A file
/// still in use waits for the user to confirm a dialog, or for the operations
/// queued ahead of its import: hours at the very most. A day leaves every
/// live operation alone, including one in another Lucerna process sharing
/// this folder (a build with its own identifier, the user's other session),
/// and bounds what an abandoned one leaves behind.
const STALE_AFTER: Duration = Duration::from_secs(24 * 60 * 60);

/// `<temp>/lucerna/modpack`, where every staged file lives: one spelling for
/// the writer, the guard and the startup sweep.
pub(crate) fn staging_dir(app: &tauri::AppHandle) -> Result<PathBuf, Error> {
    let temp = app.path().temp_dir().map_err(|e| Error::Io {
        path: "<temp>".into(),
        details: e.to_string(),
    })?;
    Ok(temp.join("lucerna").join("modpack"))
}

/// Download a Modrinth `.mrpack` version archive to a staged file and return
/// its path. See [`fetch_modrinth_mrpack`] for how the archive is found.
pub(crate) async fn download_modrinth_mrpack(
    app: &tauri::AppHandle,
    base: &str,
    project_id: &str,
    version_id: &str,
) -> Result<String, Error> {
    let bytes = fetch_modrinth_mrpack(base, project_id, version_id).await?;
    write_to_temp(app, &bytes, StagedKind::Mrpack).await
}

/// Download a Modrinth `.mrpack` version archive and return its bytes. Calls
/// `{base}/v2/project/{project_id}/version/{version_id}`, locates the primary
/// `.mrpack` entry and downloads it. Stages nothing: a caller that consumes
/// the archive itself has no use for a copy on disk.
pub(crate) async fn fetch_modrinth_mrpack(
    base: &str,
    project_id: &str,
    version_id: &str,
) -> Result<Vec<u8>, Error> {
    let url = format!("{base}/v2/project/{project_id}/version/{version_id}");
    let resp = crate::network::request::get(
        &url,
        &[("user-agent", "AntonBabchenko/Lucerna")],
        "modpacks",
    )
    .await
    .map_err(|e| Error::mods_network(url.clone(), e))?;
    if !(200..300).contains(&resp.status) {
        return Err(Error::ModsNetwork {
            url,
            details: format!("HTTP {}", resp.status),
        });
    }

    #[derive(serde::Deserialize)]
    struct V {
        files: Vec<F>,
    }
    #[derive(serde::Deserialize)]
    struct F {
        url: String,
        filename: String,
        primary: bool,
    }

    let v: V = serde_json::from_slice(&resp.body).map_err(|e| Error::ModsDecode {
        platform: "modrinth".into(),
        details: e.to_string(),
    })?;
    let f = v
        .files
        .iter()
        .find(|f| f.primary)
        .or_else(|| v.files.iter().find(|f| f.filename.ends_with(".mrpack")))
        .ok_or(Error::ModpackManifestInvalid {
            format: "modrinth".into(),
            details: "no primary .mrpack file on version".into(),
        })?;
    crate::network::get_bytes(&f.url, "modpacks")
        .await
        .map_err(|e| Error::mods_network(f.url.clone(), e))
}

/// Download a CurseForge modpack `.zip` to a staged file and return its path.
/// Calls `cf_api::resolve_file_download` to get the CDN URL, then downloads
/// the bytes and writes them to `<temp>/lucerna/modpack/<uuid>.zip`.
pub(crate) async fn download_curseforge_zip(
    app: &tauri::AppHandle,
    base: &str,
    key: Option<&str>,
    project_id: &str,
    version_id: &str,
) -> Result<String, Error> {
    let dl = crate::mods::modpack::cf_api::resolve_file_download(base, key, project_id, version_id)
        .await?;
    let bytes = crate::network::get_bytes(&dl, "modpacks")
        .await
        .map_err(|e| Error::mods_network(dl.clone(), e))?;
    write_to_temp(app, &bytes, StagedKind::CurseforgeZip).await
}

/// Write `bytes` to a new staged file, `<temp>/lucerna/modpack/<uuid>.<ext>`,
/// and return its path. Shared by the download helpers above and the FTB and
/// ATLauncher summaries. Clears day-old staged files on the way.
pub(crate) async fn write_to_temp(
    app: &tauri::AppHandle,
    bytes: &[u8],
    kind: StagedKind,
) -> Result<String, Error> {
    let dir = staging_dir(app)?;
    let dest = write_staged(&dir, bytes, kind, SystemTime::now()).await?;
    Ok(dest.to_string_lossy().to_string())
}

/// [`write_to_temp`] below the `AppHandle`: sweep `dir`, then write the new
/// file into it.
async fn write_staged(
    dir: &Path,
    bytes: &[u8],
    kind: StagedKind,
    now: SystemTime,
) -> Result<PathBuf, Error> {
    tokio::fs::create_dir_all(dir)
        .await
        .map_err(|e| Error::Io {
            path: dir.display().to_string(),
            details: e.to_string(),
        })?;
    for line in sweep_stale(dir, now) {
        crate::diag!("{line}");
    }
    let dest = dir.join(format!("{}.{}", uuid::Uuid::new_v4(), kind.extension()));
    let written = tokio::fs::write(&dest, bytes).await;
    keep_or_discard(written, &dest, |line| crate::diag!("{line}"))
}

/// Settle a staged write. A write that failed may still have left part of the
/// file behind, and nothing will ever hand its path out, so it is removed
/// here; what the caller gets back is the write's own error. A leftover the
/// removal could not clear goes to `report_leftover` (a sweep retries it a day
/// later).
fn keep_or_discard(
    written: std::io::Result<()>,
    dest: &Path,
    report_leftover: impl FnOnce(String),
) -> Result<PathBuf, Error> {
    let failure = match written {
        Ok(()) => return Ok(dest.to_path_buf()),
        Err(e) => e,
    };
    if let Err(line) = remove_staged(dest) {
        report_leftover(line);
    }
    Err(Error::Io {
        path: dest.display().to_string(),
        details: failure.to_string(),
    })
}

/// Is `name` one this module writes: a uuid spelled exactly as
/// `Uuid::new_v4()` displays it, a dot, and an extension? Every file
/// [`write_to_temp`] writes has that shape and nothing else writes into the
/// staging folder, so the name alone tells ours from anything else, whatever
/// its kind.
fn is_staged_name(name: &str) -> bool {
    name.split_once('.').is_some_and(|(stem, extension)| {
        !extension.is_empty() && uuid::Uuid::parse_str(stem).is_ok_and(|id| id.to_string() == stem)
    })
}

/// Is `path` a staged file directly inside `dir`? Compared as given, without
/// canonicalising: a command receives back the very string [`write_to_temp`]
/// returned, and anything else (another folder, a `..` component, another
/// spelling of the same folder) counts as not staged, and is kept.
fn is_staged_in(dir: &Path, path: &Path) -> bool {
    path.parent() == Some(dir)
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(is_staged_name)
}

/// Remove one staged file: `Ok(true)` when this call removed it, `Ok(false)`
/// when it was already gone (another sweep, another Lucerna process), else
/// what stopped it, worded for the log.
fn remove_staged(path: &Path) -> Result<bool, String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!(
            "modpack staging: could not remove {}: {e}",
            path.display()
        )),
    }
}

/// Remove the staged files in `dir` that are [`STALE_AFTER`] old, and return
/// the lines to log: one per file that could not be examined or removed, and
/// one counting what this sweep removed. A missing `dir` means nothing was
/// ever staged. Only a regular file with a staged name is a candidate, judged
/// without following a link; one whose age cannot be read is kept.
pub(crate) fn sweep_stale(dir: &Path, now: SystemTime) -> Vec<String> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            return vec![format!(
                "modpack staging: cannot list {} to clear old files: {e}",
                dir.display()
            )]
        }
    };
    let mut lines = Vec::new();
    let mut removed = 0usize;
    for entry in entries {
        match sweep_entry(entry, dir, now) {
            Swept::Removed => removed += 1,
            Swept::Skipped => {}
            Swept::Failed(line) => lines.push(line),
        }
    }
    if removed > 0 {
        lines.push(format!(
            "modpack staging: removed {removed} staged file(s) older than a day from {}",
            dir.display()
        ));
    }
    lines
}

/// What [`sweep_stale`] did with one entry of the folder.
#[derive(Debug)]
enum Swept {
    /// A stale staged file, removed by this sweep.
    Removed,
    /// Not ours, not stale, or already gone (another sweep or another Lucerna
    /// process got there first): nothing to count or report.
    Skipped,
    /// Could not be examined or removed; the line says why.
    Failed(String),
}

fn sweep_entry(entry: std::io::Result<std::fs::DirEntry>, dir: &Path, now: SystemTime) -> Swept {
    let entry = match entry {
        Ok(entry) => entry,
        Err(e) => {
            return Swept::Failed(format!(
                "modpack staging: cannot read {}: {e}",
                dir.display()
            ))
        }
    };
    if !entry.file_name().to_str().is_some_and(is_staged_name) {
        return Swept::Skipped;
    }
    let path = entry.path();
    match is_stale(&entry, now) {
        Ok(true) => {}
        Ok(false) => return Swept::Skipped,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Swept::Skipped,
        Err(e) => {
            return Swept::Failed(format!(
                "modpack staging: cannot examine {}: {e}",
                path.display()
            ))
        }
    }
    match remove_staged(&path) {
        Ok(true) => Swept::Removed,
        Ok(false) => Swept::Skipped,
        Err(line) => Swept::Failed(line),
    }
}

/// A regular file last written [`STALE_AFTER`] or more before `now`. A link or
/// a directory is never stale, since this module writes neither; nor is a file
/// written "after" `now` (a clock set back).
fn is_stale(entry: &std::fs::DirEntry, now: SystemTime) -> std::io::Result<bool> {
    if !entry.file_type()?.is_file() {
        return Ok(false);
    }
    let modified = entry.metadata()?.modified()?;
    Ok(now
        .duration_since(modified)
        .is_ok_and(|age| age >= STALE_AFTER))
}

/// Removes a staged file when dropped. `modpack_import` and
/// `modpack_apply_update` bind one for their whole run, so the file they
/// consume goes when the operation ends, however it ends; every retry in the
/// UI stages a new one.
///
/// The path comes from the webview, so the removal is confined: a path that
/// is not a staged file directly inside [`staging_dir`] (a pack the user
/// picked or dropped, or anything else) is never touched.
#[must_use = "the file is removed when the guard drops: bind it to a named local, not `_`"]
pub(crate) struct ConsumeGuard {
    staged: Option<PathBuf>,
}

impl ConsumeGuard {
    pub(crate) fn new(app: &tauri::AppHandle, path: &str) -> Self {
        match staging_dir(app) {
            Ok(dir) => Self::within(&dir, Path::new(path)),
            // Without the folder nothing can be shown to be staged: keep it.
            Err(e) => {
                crate::diag!("modpack staging: {path} is kept, the staging folder is unknown: {e}");
                Self { staged: None }
            }
        }
    }

    fn within(dir: &Path, path: &Path) -> Self {
        Self {
            staged: is_staged_in(dir, path).then(|| path.to_path_buf()),
        }
    }
}

impl Drop for ConsumeGuard {
    fn drop(&mut self) {
        if let Some(path) = self.staged.take() {
            if let Err(line) = remove_staged(&path) {
                crate::diag!("{line}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    /// A uuid spelled the way `Uuid::new_v4()` displays one.
    const ID: &str = "0b0a6a37-5f4e-4a59-9d0e-6c1a2f0e8b11";

    /// Every kind, for the tests that stage one of each. Production code keeps
    /// no such list: a staged file is recognised by its name alone.
    const KINDS: [StagedKind; 4] = [
        StagedKind::Mrpack,
        StagedKind::CurseforgeZip,
        StagedKind::FtbSummary,
        StagedKind::AtlauncherSummary,
    ];

    /// A staged file of `kind` in `dir`, as `write_staged` would name it.
    fn staged_file(dir: &Path, kind: StagedKind) -> PathBuf {
        let path = dir.join(format!("{}.{}", uuid::Uuid::new_v4(), kind.extension()));
        fs::write(&path, b"archive").unwrap();
        path
    }

    /// Set `path`'s last write to `age` before `now`.
    fn back_date(path: &Path, now: SystemTime, age: Duration) {
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(now - age)
            .unwrap();
    }

    /// A non-empty directory: `remove_file` on it fails with something other
    /// than `NotFound` on Windows, Linux and macOS alike.
    fn blocked_path(dir: &Path) -> PathBuf {
        let path = dir.join(format!("{}.zip", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        fs::write(path.join("inner"), b"x").unwrap();
        path
    }

    #[test]
    fn a_staged_name_is_an_exact_uuid_with_an_extension() {
        for kind in KINDS {
            let name = format!("{ID}.{}", kind.extension());
            assert!(is_staged_name(&name), "{name}");
        }
        // Any extension: a kind added later is recognised with nothing to update.
        for name in [
            format!("{ID}.technicpack.json"),
            format!("{ID}.mrpack.part"),
        ] {
            assert!(is_staged_name(&name), "{name}");
        }
        for name in [
            "pack.mrpack".to_string(),
            format!("{}.zip", ID.to_uppercase()),
            format!("{}.zip", ID.replace('-', "")),
            format!("{{{ID}}}.zip"),
            format!("{ID}mrpack"),
            format!("x{ID}.zip"),
            format!(".{ID}.zip"),
            format!("{ID}."),
            ID.to_string(),
        ] {
            assert!(!is_staged_name(&name), "{name}");
        }
    }

    #[test]
    fn a_sweep_removes_only_staged_files_a_day_old() {
        let td = tempdir().unwrap();
        let now = SystemTime::now();
        let old_archive = staged_file(td.path(), StagedKind::Mrpack);
        let old_summary = staged_file(td.path(), StagedKind::FtbSummary);
        let in_use = staged_file(td.path(), StagedKind::CurseforgeZip);
        let foreign = td.path().join("notes.zip");
        fs::write(&foreign, b"not ours").unwrap();
        // Spelled out rather than read from `STALE_AFTER`: a day is the
        // decision this pins, so changing it must change this test too.
        let a_day = Duration::from_secs(24 * 60 * 60);
        let one_second = Duration::from_secs(1);
        for path in [&old_archive, &old_summary, &foreign] {
            back_date(path, now, a_day + one_second);
        }
        back_date(&in_use, now, a_day - one_second);

        let lines = sweep_stale(td.path(), now);

        assert!(!old_archive.exists(), "a day-old archive goes");
        assert!(!old_summary.exists(), "a day-old summary goes");
        assert!(
            in_use.exists(),
            "a second short of a day, it may still be in use"
        );
        assert!(foreign.exists(), "a name this module never writes stays");
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].contains("removed 2"), "{lines:?}");
    }

    #[test]
    fn a_sweep_never_removes_a_directory_or_a_name_it_did_not_write() {
        let td = tempdir().unwrap();
        let dir_with_a_staged_name = blocked_path(td.path());
        let foreign = td.path().join("notes.zip");
        fs::write(&foreign, b"not ours").unwrap();
        // Two days on, as far as this sweep can tell.
        let later = SystemTime::now() + 2 * STALE_AFTER;

        let lines = sweep_stale(td.path(), later);

        assert!(dir_with_a_staged_name.exists());
        assert!(foreign.exists());
        assert!(lines.is_empty(), "{lines:?}");
    }

    #[test]
    fn a_file_written_after_now_is_never_stale() {
        let td = tempdir().unwrap();
        let file = staged_file(td.path(), StagedKind::Mrpack);
        // The clock was set back two days after the file was written.
        let earlier = SystemTime::now() - 2 * STALE_AFTER;

        let lines = sweep_stale(td.path(), earlier);

        assert!(file.exists());
        assert!(lines.is_empty(), "{lines:?}");
    }

    #[test]
    fn an_entry_gone_before_its_turn_is_skipped_silently() {
        let td = tempdir().unwrap();
        let file = staged_file(td.path(), StagedKind::Mrpack);
        let entry = fs::read_dir(td.path()).unwrap().next().unwrap();
        // Another sweep removes it between the listing and this one's look.
        fs::remove_file(&file).unwrap();
        let later = SystemTime::now() + 2 * STALE_AFTER;

        let swept = sweep_entry(entry, td.path(), later);

        assert!(matches!(swept, Swept::Skipped), "{swept:?}");
    }

    #[cfg(unix)]
    #[test]
    fn a_link_is_never_swept_and_the_guard_removes_only_the_link() {
        let td = tempdir().unwrap();
        let staging = td.path().join("modpack");
        fs::create_dir(&staging).unwrap();
        let target = td.path().join("elsewhere.bin");
        fs::write(&target, b"not ours").unwrap();
        let link = staging.join(format!("{}.mrpack", uuid::Uuid::new_v4()));
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let later = SystemTime::now() + 2 * STALE_AFTER;

        let lines = sweep_stale(&staging, later);

        assert!(
            fs::symlink_metadata(&link).is_ok(),
            "a link is never something this module wrote"
        );
        assert!(lines.is_empty(), "{lines:?}");

        drop(ConsumeGuard::within(&staging, &link));

        assert!(
            fs::symlink_metadata(&link).is_err(),
            "the guard removes the link"
        );
        assert!(target.exists(), "and never what it points at");
    }

    #[test]
    fn a_sweep_of_a_folder_never_created_says_nothing() {
        let td = tempdir().unwrap();

        let lines = sweep_stale(&td.path().join("modpack"), SystemTime::now());

        assert!(lines.is_empty(), "{lines:?}");
    }

    #[test]
    fn removing_a_staged_file_tells_absent_from_failed() {
        let td = tempdir().unwrap();
        let file = staged_file(td.path(), StagedKind::Mrpack);
        let blocked = blocked_path(td.path());

        assert_eq!(remove_staged(&file), Ok(true));
        assert_eq!(
            remove_staged(&file),
            Ok(false),
            "already gone is no failure"
        );
        let failure = remove_staged(&blocked).unwrap_err();
        assert!(
            failure.contains(&blocked.display().to_string()),
            "{failure}"
        );
        assert!(blocked.exists());
    }

    #[test]
    fn a_consumed_staged_file_goes_when_the_guard_drops() {
        let td = tempdir().unwrap();
        let file = staged_file(td.path(), StagedKind::Mrpack);

        let guard = ConsumeGuard::within(td.path(), &file);
        assert!(file.exists(), "nothing goes while the operation runs");
        drop(guard);

        assert!(!file.exists());
    }

    #[test]
    fn the_guard_never_touches_a_path_that_is_not_staged() {
        let td = tempdir().unwrap();
        let staging = td.path().join("modpack");
        fs::create_dir(&staging).unwrap();
        let picked = td.path().join("My Pack.mrpack");
        fs::write(&picked, b"the user's own pack").unwrap();
        let lookalike = staged_file(td.path(), StagedKind::Mrpack);
        let foreign = staging.join("notes.mrpack");
        fs::write(&foreign, b"not ours").unwrap();
        // A staged-looking file next to the staging folder, reached through it.
        let escaped = staged_file(td.path(), StagedKind::CurseforgeZip);
        let escape = staging.join("..").join(escaped.file_name().unwrap());

        for path in [&picked, &lookalike, &foreign, &escape] {
            drop(ConsumeGuard::within(&staging, path));
        }

        assert!(picked.exists(), "a pack the user picked");
        assert!(
            lookalike.exists(),
            "a staged-looking name in another folder"
        );
        assert!(foreign.exists(), "a name this module never writes");
        assert!(
            escaped.exists(),
            "a path that leaves the folder through `..`"
        );
    }

    #[test]
    fn a_failed_write_leaves_nothing_behind_and_returns_its_own_error() {
        let td = tempdir().unwrap();
        let partial = staged_file(td.path(), StagedKind::Mrpack);
        let mut leftover = None;

        let result = keep_or_discard(Err(std::io::Error::other("disk full")), &partial, |line| {
            leftover = Some(line)
        });

        assert!(!partial.exists(), "the partial file goes");
        assert_eq!(leftover, None);
        match result {
            Err(Error::Io { details, .. }) => assert!(details.contains("disk full"), "{details}"),
            other => panic!("expected the write's own error, got {other:?}"),
        }
    }

    #[test]
    fn a_partial_file_that_cannot_be_removed_is_reported() {
        let td = tempdir().unwrap();
        let blocked = blocked_path(td.path());
        let mut leftover = None;

        let result = keep_or_discard(Err(std::io::Error::other("disk full")), &blocked, |line| {
            leftover = Some(line)
        });

        assert!(result.is_err());
        let line = leftover.expect("the leftover is reported");
        assert!(line.contains(&blocked.display().to_string()), "{line}");
    }

    #[test]
    fn a_completed_write_is_kept() {
        let td = tempdir().unwrap();
        let file = staged_file(td.path(), StagedKind::Mrpack);

        let result = keep_or_discard(Ok(()), &file, |line| panic!("nothing to report: {line}"));

        assert_eq!(result.unwrap(), file);
        assert!(file.exists());
    }

    #[tokio::test]
    async fn staging_writes_a_new_file_and_clears_day_old_ones() {
        let td = tempdir().unwrap();
        let dir = td.path().join("modpack");
        fs::create_dir(&dir).unwrap();
        let now = SystemTime::now();
        let old = staged_file(&dir, StagedKind::CurseforgeZip);
        back_date(&old, now, STALE_AFTER + Duration::from_secs(1));

        let new = write_staged(&dir, b"pack bytes", StagedKind::Mrpack, now)
            .await
            .unwrap();

        assert!(
            !old.exists(),
            "a day-old file goes when the next one is staged"
        );
        assert_eq!(new.parent(), Some(dir.as_path()));
        assert!(new.to_string_lossy().ends_with(".mrpack"));
        assert_eq!(fs::read(&new).unwrap(), b"pack bytes");
    }

    #[tokio::test]
    async fn every_kind_staged_is_released_by_the_guard() {
        let td = tempdir().unwrap();
        for kind in KINDS {
            let staged = write_staged(td.path(), b"x", kind, SystemTime::now())
                .await
                .unwrap();
            // The webview hands back the string `write_to_temp` returned.
            let echoed = staged.to_string_lossy().to_string();

            drop(ConsumeGuard::within(td.path(), Path::new(&echoed)));

            assert!(!staged.exists(), "{kind:?}");
        }
    }

    #[tokio::test]
    async fn fetching_a_modrinth_version_returns_its_primary_archive() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        let body = format!(
            r#"{{"files":[
                {{"url":"{0}/files/extra.zip","filename":"extra.zip","primary":false}},
                {{"url":"{0}/files/pack.mrpack","filename":"pack.mrpack","primary":true}}]}}"#,
            server.uri()
        );
        Mock::given(method("GET"))
            .and(path("/v2/project/proj/version/ver"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/files/pack.mrpack"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"mrpack bytes".to_vec()))
            .mount(&server)
            .await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);

        let bytes = fetch_modrinth_mrpack(&server.uri(), "proj", "ver")
            .await
            .unwrap();

        assert_eq!(bytes, b"mrpack bytes");
    }
}
