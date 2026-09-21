//! `pending-cleanup.json`: old `webview/` profile directories to remove once
//! nothing holds them — i.e. after the restart that follows a data-root move.
//!
//! The note lives beside the bootstrap redirect in the OS-default dir. It only
//! ever names browser-profile directories, and the sweep re-checks every entry
//! against guards before removing anything, so a stale or hand-edited note
//! costs disk space, never data.

use crate::data_root::transient::{CLEANUP_NOTE_FILE, WEBVIEW_DIR};
use crate::error::{Error, Result};
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

/// Add `webview_dir` to the note (idempotent). An unreadable note is an
/// error; a corrupt one is replaced.
pub fn append(_default_dir: &Path, _webview_dir: &Path) -> Result<()> {
    // RED stub — Task 14 replaces it
    Err(Error::io("<pending-cleanup>", "stub"))
}

/// Guards for one noted directory. `effective_root` is the root this process runs from.
pub fn judge(_entry: &Path, _effective_root: &Path) -> Verdict {
    Verdict::Gone // RED stub — Task 14 replaces it (Gone, so the "passes" tests are red too)
}

/// Process the note. `remove` is injected (production: `std::fs::remove_dir_all`).
/// Returns one human-readable line per entry for the launcher log.
pub fn sweep(
    _default_dir: &Path,
    _effective_root: &Path,
    _remove: &dyn Fn(&Path) -> std::io::Result<()>,
) -> Vec<String> {
    Vec::new() // RED stub — Task 14 replaces it
}

// `WEBVIEW_DIR` is used by the final `judge`; referenced here so push 1 compiles warning-free.
const _: &str = WEBVIEW_DIR;

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
