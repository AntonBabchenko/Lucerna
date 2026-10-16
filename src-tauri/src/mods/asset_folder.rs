//! What a resource-pack or shader folder holds, as the game sees it — and how
//! the assets registry is reconciled with it.
//!
//! The folder is the truth: the game loads what is there, not what
//! `installed-assets.json` says. The registry keeps only what the disk cannot
//! tell — a file's provenance — so a `.zip` with no row is adopted, a row with
//! no file is dropped, and an unpacked pack folder is read from disk on every
//! listing and never stored.

use std::path::{Path, PathBuf};

use tokio::fs;

use crate::error::Error;
use crate::mods::platform::{ContentKind, InstalledAsset};

/// One entry of a pack folder, classified WITHOUT following links
/// (`DirEntry::file_type`; on Windows a junction is a link too).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderEntry {
    pub name: String,
    pub ty: EntryType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryType {
    File,
    Dir,
    /// A link, or anything else that is neither a plain file nor a directory.
    Other,
}

/// The rows of one kind after matching them to the folder's entries.
#[derive(Debug, PartialEq)]
pub struct Matched {
    /// Rows that found their entry, spelled as on disk, in registry order.
    pub rows: Vec<InstalledAsset>,
    /// A row was dropped or respelled.
    pub changed: bool,
    /// Plain `.zip` files no row claimed — the caller adopts them.
    pub adopt: Vec<String>,
}

/// `.minecraft/<resourcepacks|shaderpacks>` — the folder the game reads.
pub fn pack_dir(instance_root: &Path, kind: ContentKind) -> PathBuf {
    let mc = instance_root.join(".minecraft");
    match kind {
        ContentKind::Shader => mc.join("shaderpacks"),
        // `assets::require_asset_kind` keeps every other kind away from the
        // asset commands: resource packs are the only one left.
        _ => mc.join("resourcepacks"),
    }
}

/// A file name the game reads as a zip pack: the game's own test,
/// `String.endsWith(".zip")` — case-sensitive, so `Pack.ZIP` is not one
/// (`datapacks::detect::has_zip_suffix`) — and something before it.
pub fn is_zip_name(name: &str) -> bool {
    name.len() > 4 && crate::datapacks::detect::has_zip_suffix(name)
}

/// The display name of a hand-placed zip: its file name without `.zip`.
pub fn zip_stem(name: &str) -> &str {
    if is_zip_name(name) {
        // `is_zip_name` saw the four ASCII bytes `.zip` at the end: a char boundary.
        &name[..name.len() - 4]
    } else {
        name
    }
}

/// Match rows to entries one-to-one, the mods way (`installed::reconcile`):
/// exact name first for every row, then ASCII case-insensitive (on Windows and
/// default macOS that is the same file). A directory never matches a file row
/// — a folder named `X.zip` is not the file `X.zip`. Pure.
pub fn match_rows(rows: Vec<InstalledAsset>, entries: &[FolderEntry]) -> Matched {
    // Entries that can hold a file row: anything but a directory. A link stays
    // in, so a row whose name is present is never dropped for its type.
    let holders: Vec<&FolderEntry> = entries.iter().filter(|e| e.ty != EntryType::Dir).collect();
    let mut claimed = vec![false; holders.len()];
    let mut matched: Vec<Option<usize>> = vec![None; rows.len()];
    // Exact names for every row first, so a case-folded match never takes the
    // file another row names exactly.
    for (i, r) in rows.iter().enumerate() {
        if let Some(j) = (0..holders.len()).find(|&j| !claimed[j] && holders[j].name == r.filename)
        {
            claimed[j] = true;
            matched[i] = Some(j);
        }
    }
    for (i, r) in rows.iter().enumerate() {
        if matched[i].is_some() {
            continue;
        }
        if let Some(j) = (0..holders.len())
            .find(|&j| !claimed[j] && holders[j].name.eq_ignore_ascii_case(&r.filename))
        {
            claimed[j] = true;
            matched[i] = Some(j);
        }
    }

    let mut changed = false;
    let mut kept = Vec::with_capacity(rows.len());
    for (mut r, at) in rows.into_iter().zip(matched) {
        let Some(j) = at else {
            changed = true; // its file is gone
            continue;
        };
        if r.filename != holders[j].name {
            // The spelling on disk, so a later removal names a file that exists.
            r.filename = holders[j].name.clone();
            changed = true;
        }
        kept.push(r);
    }
    let adopt = holders
        .iter()
        .zip(&claimed)
        .filter(|(e, taken)| !**taken && e.ty == EntryType::File && is_zip_name(&e.name))
        .map(|(e, _)| e.name.clone())
        .collect();
    Matched {
        rows: kept,
        changed,
        adopt,
    }
}

/// Every entry of `dir`. `Some(vec![])` when the folder does not exist (no
/// packs); `None` when it, or any entry, could not be read — the caller then
/// reconciles nothing: rows are never dropped against a partial listing.
pub async fn read_entries(dir: &Path) -> Option<Vec<FolderEntry>> {
    let mut rd = match fs::read_dir(dir).await {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return absent_or_unknown(dir).await;
        }
        Err(e) => {
            crate::diag!("assets: could not read {}: {e}", dir.display());
            return None;
        }
    };
    let mut out = Vec::new();
    loop {
        let entry = match rd.next_entry().await {
            Ok(Some(entry)) => entry,
            Ok(None) => break,
            Err(e) => {
                crate::diag!("assets: could not read an entry of {}: {e}", dir.display());
                return None;
            }
        };
        let ty = match entry.file_type().await {
            Ok(t) if t.is_file() => EntryType::File,
            Ok(t) if t.is_dir() => EntryType::Dir,
            Ok(_) => EntryType::Other,
            Err(e) => {
                crate::diag!("assets: could not stat {}: {e}", entry.path().display());
                return None;
            }
        };
        out.push(FolderEntry {
            name: entry.file_name().to_string_lossy().into_owned(),
            ty,
        });
    }
    Some(out)
}

/// `read_dir` said "not found": the folder is truly absent (no packs), or it is
/// a link — a shared resourcepacks folder linked in — whose target is gone or
/// unmounted. Only a confirmed absence is an empty folder; a dangling link is
/// "could not tell", and the caller then keeps every row.
async fn absent_or_unknown(dir: &Path) -> Option<Vec<FolderEntry>> {
    match fs::symlink_metadata(dir).await {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some(Vec::new()),
        Ok(_) => {
            crate::diag!(
                "assets: {} is there but cannot be listed (a link to nowhere?)",
                dir.display()
            );
            None
        }
        Err(e) => {
            crate::diag!(
                "assets: could not tell whether {} exists: {e}",
                dir.display()
            );
            None
        }
    }
}

/// Whether the zip at `path` is a pack of `kind` as the game reads one: a
/// resource pack holds `pack.mcmeta` at its root (Minecraft skips a zip
/// without it), a shader pack holds a `shaders/` folder (Iris and OptiFine
/// look one level down too). A zip that will not open is no pack; an I/O error
/// is "could not tell", returned as `Err`. Synchronous: run it off the
/// executor.
pub fn zip_is_pack(path: &Path, kind: ContentKind) -> std::io::Result<bool> {
    let file = std::io::BufReader::new(std::fs::File::open(path)?);
    let archive = match zip::ZipArchive::new(file) {
        Ok(archive) => archive,
        Err(zip::result::ZipError::Io(e)) => return Err(e),
        // Not a readable zip: the game cannot load it either.
        Err(_) => return Ok(false),
    };
    Ok(match kind {
        ContentKind::Shader => crate::zip_entry::names(&archive).any(|n| {
            let mut parts = n.split('/');
            let first = parts.next();
            first == Some("shaders") || parts.next() == Some("shaders")
        }),
        _ => archive.index_for_name("pack.mcmeta").is_some(),
    })
}

/// The unpacked pack folders among `entries`, sorted case-insensitively. A
/// directory counts exactly when the game would list it: a resource pack holds
/// a regular `pack.mcmeta` (Minecraft's folder-pack rule), a shader a
/// `shaders/` directory (Iris's and OptiFine's). A marker that cannot be
/// stat'ed is "could not tell": not listed this time, logged — display only,
/// nothing is written.
pub async fn folder_packs(dir: &Path, kind: ContentKind, entries: &[FolderEntry]) -> Vec<String> {
    let mut out = Vec::new();
    for e in entries.iter().filter(|e| e.ty == EntryType::Dir) {
        let (marker, wants_dir) = match kind {
            ContentKind::Shader => (dir.join(&e.name).join("shaders"), true),
            _ => (dir.join(&e.name).join("pack.mcmeta"), false),
        };
        match fs::metadata(&marker).await {
            Ok(m) if (wants_dir && m.is_dir()) || (!wants_dir && m.is_file()) => {
                out.push(e.name.clone())
            }
            Ok(_) => {
                // The marker is there but of the wrong type: not a pack.
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                // No marker: an ordinary folder, not a pack.
            }
            Err(err) => crate::diag!(
                "assets: could not tell whether {} is a pack: {err}",
                dir.join(&e.name).display()
            ),
        }
    }
    out.sort_by_key(|n| n.to_lowercase());
    out
}

/// What [`remove_entry`] found and removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Removed {
    /// Nothing was there: a ghost row's repair.
    Nothing,
    File,
    /// A folder pack, moved to the recycle bin. It never has a registry row.
    Folder,
}

/// Remove one entry of a pack folder by its real type: a file is deleted, a
/// folder pack goes to the OS recycle bin through `recycle` (it may be the
/// user's own work), a link is refused, and nothing there is success — a ghost
/// row's repair. Any failure is returned, so the caller keeps the row: the
/// folder is the truth, and a file that stayed would be listed again.
pub async fn remove_entry<F>(path: &Path, recycle: F) -> Result<Removed, Error>
where
    F: FnOnce(PathBuf) -> std::io::Result<()> + Send + 'static,
{
    let meta = match fs::symlink_metadata(path).await {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Removed::Nothing),
        Err(e) => return Err(io_err(path, e)),
    };
    if meta.file_type().is_symlink() {
        return Err(Error::AddonEntryIsLink {
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
        });
    }
    if meta.is_dir() {
        // Off the executor; a panicking move is a failed removal, never a
        // crashed command (`off_thread`).
        return crate::platform::recycle_bin::off_thread(path.to_path_buf(), recycle)
            .await
            .map(|()| Removed::Folder)
            .map_err(|e| io_err(path, e));
    }
    match fs::remove_file(path).await {
        Ok(()) => Ok(Removed::File),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Removed::Nothing),
        Err(e) => Err(io_err(path, e)),
    }
}

fn io_err(path: &Path, e: std::io::Error) -> Error {
    Error::ModsInstancePath {
        path: path.display().to_string(),
        details: e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str) -> InstalledAsset {
        crate::mods::assets::make_asset(
            ContentKind::ResourcePack,
            name,
            "aa",
            None,
            None,
            None,
            zip_stem(name),
            "2026-10-11T00:00:00+00:00".into(),
        )
    }
    fn file(name: &str) -> FolderEntry {
        FolderEntry {
            name: name.into(),
            ty: EntryType::File,
        }
    }
    fn dir(name: &str) -> FolderEntry {
        FolderEntry {
            name: name.into(),
            ty: EntryType::Dir,
        }
    }
    fn names(rows: &[InstalledAsset]) -> Vec<&str> {
        rows.iter().map(|r| r.filename.as_str()).collect()
    }

    #[test]
    fn a_row_with_its_file_stays_and_nothing_changes() {
        let m = match_rows(vec![row("A.zip")], &[file("A.zip")]);
        assert_eq!(names(&m.rows), ["A.zip"]);
        assert!(!m.changed);
        assert!(m.adopt.is_empty());
    }

    #[test]
    fn a_row_whose_file_is_gone_is_dropped() {
        let m = match_rows(vec![row("A.zip"), row("B.zip")], &[file("B.zip")]);
        assert_eq!(names(&m.rows), ["B.zip"]);
        assert!(m.changed);
    }

    #[test]
    fn a_row_takes_the_on_disk_spelling() {
        let m = match_rows(vec![row("faithful.zip")], &[file("Faithful.ZIP")]);
        assert_eq!(names(&m.rows), ["Faithful.ZIP"]);
        assert!(m.changed);
        assert!(
            m.adopt.is_empty(),
            "the respelled file is claimed, not adopted again"
        );
    }

    #[test]
    fn exact_names_win_before_case_folding() {
        // Linux: two files differing only in case. The row keeps its exact twin
        // and the other one is adopted — the game loads both.
        let m = match_rows(vec![row("a.zip")], &[file("A.zip"), file("a.zip")]);
        assert_eq!(names(&m.rows), ["a.zip"]);
        assert_eq!(m.adopt, ["A.zip"]);
    }

    #[test]
    fn an_unclaimed_zip_is_adopted_and_other_files_are_not() {
        let m = match_rows(
            vec![],
            &[
                file("New.zip"),
                file("BSL.zip.txt"),
                file("notes.md"),
                dir("Unpacked"),
            ],
        );
        assert!(m.rows.is_empty());
        assert_eq!(m.adopt, ["New.zip"]);
    }

    #[test]
    fn a_directory_never_holds_a_file_row() {
        let m = match_rows(vec![row("X.zip")], &[dir("X.zip")]);
        assert!(
            m.rows.is_empty(),
            "a folder named X.zip is not the file X.zip"
        );
        assert!(m.changed);
        assert!(m.adopt.is_empty(), "a directory is never adopted as a zip");
    }

    #[test]
    fn zip_names_follow_the_games_rule() {
        assert!(is_zip_name("Faithful.zip"));
        assert!(
            !is_zip_name("Faithful.ZIP"),
            "the game's test is case-sensitive"
        );
        assert!(!is_zip_name(".zip"));
        assert!(!is_zip_name("日本語"), "a multi-byte tail must not panic");
        assert_eq!(zip_stem("Пак.zip"), "Пак");
    }

    #[test]
    fn a_link_keeps_its_row_but_is_never_adopted() {
        let link = FolderEntry {
            name: "L.zip".into(),
            ty: EntryType::Other,
        };
        let kept = match_rows(vec![row("L.zip")], std::slice::from_ref(&link));
        assert_eq!(
            names(&kept.rows),
            ["L.zip"],
            "a present name is never dropped"
        );
        let fresh = match_rows(vec![], &[link]);
        assert!(fresh.adopt.is_empty());
    }

    // ── removal ─────────────────────────────────────────────────────────

    fn bin_ok(_: PathBuf) -> std::io::Result<()> {
        Ok(())
    }

    #[tokio::test]
    async fn removal_deletes_a_file() {
        let td = tempfile::tempdir().unwrap();
        let p = td.path().join("A.zip");
        std::fs::write(&p, b"a").unwrap();
        assert_eq!(remove_entry(&p, bin_ok).await.unwrap(), Removed::File);
        assert!(!p.exists());
    }

    #[tokio::test]
    async fn removal_of_nothing_is_ok() {
        let td = tempfile::tempdir().unwrap();
        assert_eq!(
            remove_entry(&td.path().join("ghost.zip"), bin_ok)
                .await
                .unwrap(),
            Removed::Nothing
        );
    }

    #[tokio::test]
    async fn a_folder_goes_to_the_recycle_bin() {
        let td = tempfile::tempdir().unwrap();
        let p = td.path().join("Dev Pack");
        std::fs::create_dir_all(&p).unwrap();
        let seen = std::sync::Arc::new(std::sync::Mutex::new(None));
        let s = seen.clone();
        let removed = remove_entry(&p, move |q| {
            *s.lock().unwrap() = Some(q);
            Ok(())
        })
        .await
        .unwrap();
        assert_eq!(removed, Removed::Folder);
        assert_eq!(seen.lock().unwrap().as_deref(), Some(p.as_path()));
        assert!(
            p.exists(),
            "the seam stood in for the bin: nothing was deleted"
        );
    }

    #[tokio::test]
    async fn a_failed_recycle_is_an_error() {
        let td = tempfile::tempdir().unwrap();
        let p = td.path().join("Dev Pack");
        std::fs::create_dir_all(&p).unwrap();
        let r = remove_entry(&p, |_| Err(std::io::Error::other("no bin here"))).await;
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn a_panicking_recycle_is_an_error_not_a_crash() {
        let td = tempfile::tempdir().unwrap();
        let p = td.path().join("Dev Pack");
        std::fs::create_dir_all(&p).unwrap();
        let r = remove_entry(&p, |_| panic!("CoCreateInstance failed")).await;
        assert!(r.is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_link_is_refused() {
        let td = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(td.path().join("real")).unwrap();
        let link = td.path().join("Linked");
        std::os::unix::fs::symlink(td.path().join("real"), &link).unwrap();
        let r = remove_entry(&link, bin_ok).await;
        assert!(matches!(r, Err(Error::AddonEntryIsLink { .. })));
        assert!(link.exists());
    }
}
