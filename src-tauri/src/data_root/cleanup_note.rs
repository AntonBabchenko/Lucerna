//! `pending-cleanup.json`: old `webview/` profile directories to remove once
//! nothing holds them — i.e. after the restart that follows a data-root move.
//!
//! The note lives beside the bootstrap redirect in the OS-default dir. It only
//! ever names browser-profile directories, and the sweep re-checks every entry
//! against guards before removing anything, so a stale or hand-edited note
//! costs disk space, never data.

use crate::data_root::transient::{CLEANUP_NOTE_FILE, WEBVIEW_DIR};
use crate::data_root::{migrate, walk};
use crate::error::{Error, Result};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Note {
    #[serde(default)]
    webview_dirs: Vec<PathBuf>,
}

pub fn note_path(default_dir: &Path) -> PathBuf {
    default_dir.join(CLEANUP_NOTE_FILE)
}

/// Why the sweep left an entry alone.
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Passed every guard: remove it.
    Remove,
    /// Already gone: drop the entry.
    Gone,
    /// Failed a guard: drop the entry and say why.
    Rejected(&'static str),
    /// Could not be inspected: keep the entry for the next start.
    Unclear,
}

enum NoteRead {
    Absent,
    Loaded(Note),
    Corrupt,
    Unreadable(std::io::Error),
}

fn read(path: &Path) -> NoteRead {
    match std::fs::read_to_string(path) {
        Ok(raw) => match serde_json::from_str::<Note>(&raw) {
            Ok(note) => NoteRead::Loaded(note),
            Err(_) => NoteRead::Corrupt,
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => NoteRead::Absent,
        Err(e) => NoteRead::Unreadable(e),
    }
}

/// Atomic write (tmp + rename), creating the parent if needed — the same
/// shape as `redirect::write`.
fn write(path: &Path, note: &Note) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent.display().to_string(), e))?;
    }
    let json =
        serde_json::to_string_pretty(note).map_err(|e| Error::io(path.display().to_string(), e))?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, json).map_err(|e| Error::io(tmp.display().to_string(), e))?;
    std::fs::rename(&tmp, path).map_err(|e| Error::io(path.display().to_string(), e))
}

/// Add `webview_dir` to the note (idempotent). An unreadable note is an
/// error; a corrupt one is replaced.
pub fn append(default_dir: &Path, webview_dir: &Path) -> Result<()> {
    let path = note_path(default_dir);
    let mut note = match read(&path) {
        NoteRead::Loaded(note) => note,
        NoteRead::Absent | NoteRead::Corrupt => Note::default(),
        NoteRead::Unreadable(e) => return Err(Error::io(path.display().to_string(), e)),
    };
    if !note.webview_dirs.iter().any(|dir| dir == webview_dir) {
        note.webview_dirs.push(webview_dir.to_path_buf());
    }
    write(&path, &note)
}

/// Guards for one noted directory. `effective_root` is the root this process runs from.
pub fn judge(entry: &Path, effective_root: &Path) -> Verdict {
    if !entry.is_absolute() {
        return Verdict::Rejected("not an absolute path");
    }
    if entry.file_name() != Some(OsStr::new(WEBVIEW_DIR)) {
        return Verdict::Rejected("not a webview directory");
    }
    match std::fs::symlink_metadata(entry) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => return Verdict::Rejected("not a directory"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Verdict::Gone,
        Err(_) => return Verdict::Unclear,
    }
    if migrate::is_same_path(entry, &effective_root.join(WEBVIEW_DIR)) {
        return Verdict::Rejected("the running launcher's own profile");
    }
    if migrate::is_same_or_nested(entry, effective_root) {
        return Verdict::Rejected("the running launcher's data root is inside it");
    }
    match walk::real_list_dir(entry) {
        Ok(children) if children.is_empty() || entry.join("EBWebView").is_dir() => Verdict::Remove,
        Ok(_) => Verdict::Rejected("does not look like a WebView2 profile"),
        Err(_) => Verdict::Unclear,
    }
}

/// Process the note. `remove` is injected (production: `std::fs::remove_dir_all`).
/// Returns one human-readable line per entry for the launcher log.
pub fn sweep(
    default_dir: &Path,
    effective_root: &Path,
    remove: &dyn Fn(&Path) -> std::io::Result<()>,
) -> Vec<String> {
    let path = note_path(default_dir);
    let note = match read(&path) {
        NoteRead::Absent => return Vec::new(),
        NoteRead::Loaded(note) => note,
        NoteRead::Unreadable(e) => {
            return vec![format!(
                "[cleanup] note unreadable, kept for the next start: {e}"
            )]
        }
        NoteRead::Corrupt => {
            let removed = std::fs::remove_file(&path);
            return vec![format!("[cleanup] corrupt note removed: {removed:?}")];
        }
    };

    let mut lines = Vec::new();
    let mut keep = Vec::new();
    for dir in note.webview_dirs {
        let shown = dir.display().to_string();
        match judge(&dir, effective_root) {
            Verdict::Gone => lines.push(format!("[cleanup] {shown}: already gone")),
            Verdict::Rejected(why) => lines.push(format!("[cleanup] {shown}: dropped — {why}")),
            Verdict::Unclear => {
                lines.push(format!(
                    "[cleanup] {shown}: could not be inspected, will retry"
                ));
                keep.push(dir);
            }
            Verdict::Remove => match remove(&dir) {
                Ok(()) => lines.push(format!("[cleanup] {shown}: removed")),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    lines.push(format!("[cleanup] {shown}: already gone"))
                }
                Err(e) => {
                    lines.push(format!("[cleanup] {shown}: still in use, will retry — {e}"));
                    keep.push(dir);
                }
            },
        }
    }

    let updated = if keep.is_empty() {
        std::fs::remove_file(&path).map_err(|e| Error::io(path.display().to_string(), e))
    } else {
        write(&path, &Note { webview_dirs: keep })
    };
    if let Err(e) = updated {
        lines.push(format!("[cleanup] the note could not be updated: {e}"));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn webview_in(root: &Path) -> PathBuf {
        let dir = root.join("webview");
        std::fs::create_dir_all(dir.join("EBWebView")).unwrap();
        std::fs::write(dir.join("EBWebView").join("Local State"), b"{}").unwrap();
        dir
    }

    fn read_note(default_dir: &Path) -> Vec<PathBuf> {
        let raw = std::fs::read_to_string(note_path(default_dir)).unwrap();
        serde_json::from_str::<Note>(&raw).unwrap().webview_dirs
    }

    #[test]
    fn append_creates_the_note_and_is_idempotent() {
        let d = tempdir().unwrap();
        let default_dir = d.path().join("default");
        let old = d.path().join("old").join("webview");
        append(&default_dir, &old).unwrap();
        append(&default_dir, &old).unwrap();
        append(&default_dir, &d.path().join("older").join("webview")).unwrap();
        assert_eq!(
            read_note(&default_dir),
            vec![old, d.path().join("older").join("webview")]
        );
        assert!(
            !default_dir.join("pending-cleanup.tmp").exists(),
            "the atomic-write temp must not be left behind"
        );
    }

    #[test]
    fn append_replaces_a_corrupt_note() {
        let d = tempdir().unwrap();
        std::fs::write(note_path(d.path()), b"{ not json").unwrap();
        let old = d.path().join("old").join("webview");
        append(d.path(), &old).unwrap();
        assert_eq!(read_note(d.path()), vec![old]);
    }

    #[test]
    fn a_real_old_profile_passes_the_guards() {
        let d = tempdir().unwrap();
        let old = webview_in(&d.path().join("old-root"));
        let effective = d.path().join("new-root");
        std::fs::create_dir_all(&effective).unwrap();
        assert_eq!(judge(&old, &effective), Verdict::Remove);
    }

    #[test]
    fn an_empty_webview_dir_passes_too() {
        let d = tempdir().unwrap();
        let old = d.path().join("old-root").join("webview");
        std::fs::create_dir_all(&old).unwrap();
        assert_eq!(judge(&old, &d.path().join("new-root")), Verdict::Remove);
    }

    #[test]
    fn the_guards_refuse_everything_that_is_not_an_old_profile() {
        let d = tempdir().unwrap();
        let effective = d.path().join("root");
        let own = webview_in(&effective);

        // The profile this process is using right now.
        assert!(matches!(judge(&own, &effective), Verdict::Rejected(_)));
        // A relative path.
        assert!(matches!(
            judge(Path::new("webview"), &effective),
            Verdict::Rejected(_)
        ));
        // Not named `webview`.
        let other = d.path().join("old").join("instances");
        std::fs::create_dir_all(other.join("EBWebView")).unwrap();
        assert!(matches!(judge(&other, &effective), Verdict::Rejected(_)));
        // Named `webview` but not shaped like a profile.
        let fake = d.path().join("docs").join("webview");
        std::fs::create_dir_all(&fake).unwrap();
        std::fs::write(fake.join("thesis.docx"), b"x").unwrap();
        assert!(matches!(judge(&fake, &effective), Verdict::Rejected(_)));
        // An ancestor of the effective root (contrived, but one check closes it).
        let nested_root = d.path().join("webview").join("data");
        std::fs::create_dir_all(nested_root.join("x")).unwrap();
        std::fs::create_dir_all(d.path().join("webview").join("EBWebView")).unwrap();
        assert!(matches!(
            judge(&d.path().join("webview"), &nested_root),
            Verdict::Rejected(_)
        ));
        // Already gone.
        assert_eq!(
            judge(&d.path().join("nowhere").join("webview"), &effective),
            Verdict::Gone
        );
    }

    #[test]
    fn sweep_removes_what_passes_keeps_what_fails_to_delete_and_drops_the_rest() {
        let d = tempdir().unwrap();
        let default_dir = d.path().join("default");
        let effective = d.path().join("new-root");
        std::fs::create_dir_all(&effective).unwrap();
        let removable = webview_in(&d.path().join("old-a"));
        let locked = webview_in(&d.path().join("old-b"));
        let gone = d.path().join("old-c").join("webview");
        let own = webview_in(&effective);
        for dir in [&removable, &locked, &gone, &own] {
            append(&default_dir, dir).unwrap();
        }

        let remove = |p: &Path| -> std::io::Result<()> {
            if p.starts_with(d.path().join("old-b")) {
                Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "scripted: still in use",
                ))
            } else {
                std::fs::remove_dir_all(p)
            }
        };
        let lines = sweep(&default_dir, &effective, &remove);

        assert!(!removable.exists());
        assert!(locked.exists());
        assert!(own.exists(), "the live profile is never touched");
        assert_eq!(
            read_note(&default_dir),
            vec![locked.clone()],
            "only the failed removal is retried"
        );
        assert_eq!(lines.len(), 4, "{lines:?}");

        // Second start: the lock is gone → the note disappears with its last entry.
        let lines = sweep(&default_dir, &effective, &|p| std::fs::remove_dir_all(p));
        assert!(!locked.exists());
        assert!(!note_path(&default_dir).exists(), "{lines:?}");
    }

    #[test]
    fn sweep_without_a_note_does_nothing_and_a_corrupt_note_is_removed() {
        let d = tempdir().unwrap();
        assert!(sweep(d.path(), d.path(), &|p| std::fs::remove_dir_all(p)).is_empty());

        std::fs::write(note_path(d.path()), b"{ not json").unwrap();
        let lines = sweep(d.path(), d.path(), &|p| std::fs::remove_dir_all(p));
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(!note_path(d.path()).exists());
    }
}
