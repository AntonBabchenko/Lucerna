//! The throwaway root of a recovery session.
//!
//! When the configured data folder cannot be used, the launcher still starts —
//! but it must not write anything of the user's into the OS-default folder:
//! that is where a later "Reset to default" moves the real data, and a seeded
//! `app.json` there is exactly what makes that reset refuse. So the session
//! runs on a directory of its own under the app's CACHE dir, which is thrown
//! away at exit. Whatever any code path writes — known to anyone or not — can
//! then never land in a folder the user's data is resolved from.
//!
//! Not the OS temp dir: on Linux that is the shared `/tmp`, where a
//! predictable name invites pre-creation and symlink games, and on Windows
//! `%TEMP%` can hold tens of thousands of entries a sweep would have to list.
//! The uninstaller already plans the app's cache dir.
//!
//! Every function returns its log lines instead of logging: the create step
//! runs before `diag::init`, and until then `diag!` goes to stderr, which the
//! Windows GUI build discards.

use std::path::{Path, PathBuf};

/// `<app cache dir>/recovery/<pid>` — or `<default dir>/recovery/<pid>` when
/// the cache dir cannot be resolved (`paths::recovery_parent`).
const PARENT_DIR: &str = crate::data_root::transient::RECOVERY_DIR;

/// A session dir that was ACTUALLY created by this process. There is no other
/// way to obtain one, and the exit cleanup accepts nothing else — so "creation
/// failed, and the cleanup removed the default folder instead" cannot be written.
#[derive(Debug)]
pub struct RecoveryDir {
    path: PathBuf,
}

impl RecoveryDir {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// The directory that holds every session dir.
pub fn parent_in(cache_dir: &Path) -> PathBuf {
    cache_dir.join(PARENT_DIR)
}

/// Where this process's session dir is — whether or not it could be created.
/// When creation fails the session KEEPS this path as its root: every write
/// under it then fails loudly, and nothing falls through to the default folder.
pub fn session_path(parent: &Path, pid: u32) -> PathBuf {
    parent.join(pid.to_string())
}

/// Create this process's session dir, exclusively. A stale dir with the same
/// pid (pid reuse after a kill) is removed first — never reused with its content.
pub fn create(parent: &Path, pid: u32) -> (Option<RecoveryDir>, Vec<String>) {
    let path = session_path(parent, pid);
    let failed = |what: &str, e: std::io::Error| {
        (
            None,
            vec![format!(
                "[recovery] session dir could not be created ({what}: {e}); every write under {} will fail",
                path.display()
            )],
        )
    };
    if let Err(e) = std::fs::create_dir_all(parent) {
        return failed("parent", e);
    }
    match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return failed("stat", e),
        // Same pid as a session that was killed: never reuse its content.
        Ok(_) => {
            if let Err(e) = remove_no_follow(&path) {
                return failed("stale dir with this pid", e);
            }
        }
    }
    // EXCLUSIVE: `create_dir` fails if anything appeared in between.
    match crate::platform::create_private_dir(&path) {
        Ok(()) => (
            Some(RecoveryDir { path: path.clone() }),
            vec![format!("[recovery] session dir: {}", path.display())],
        ),
        Err(e) => failed("create", e),
    }
}

/// Remove every session dir under `parent` except `keep`. Runs on EVERY start:
/// a killed session's next start is usually a normal one. No liveness probe —
/// the single-instance mutex is shared by dev and installed builds and is
/// taken before `setup`, so two sessions cannot coexist; the one overlap is a
/// restart's parent still exiting, and its dir is being thrown away anyway.
pub fn sweep(
    parent: &Path,
    keep: Option<&Path>,
    remove: &dyn Fn(&Path) -> std::io::Result<()>,
) -> Vec<String> {
    let children = match std::fs::read_dir(parent) {
        Ok(children) => children,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            return vec![format!(
                "[recovery] stale session dirs not swept, {} is unreadable: {e}",
                parent.display()
            )]
        }
    };
    let mut lines = Vec::new();
    for child in children {
        let path = match child {
            Ok(child) => child.path(),
            Err(e) => {
                lines.push(format!(
                    "[recovery] an entry of {} is unreadable: {e}",
                    parent.display()
                ));
                continue;
            }
        };
        if keep.is_some_and(|keep| keep == path) {
            continue;
        }
        lines.push(match remove(&path) {
            Ok(()) => format!("[recovery] stale session dir removed: {}", path.display()),
            Err(e) => format!(
                "[recovery] stale session dir NOT removed ({e}), the next start retries: {}",
                path.display()
            ),
        });
    }
    lines
}

/// Exit cleanup. Takes a `RecoveryDir` and nothing else, on purpose.
pub fn remove_at_exit(dir: &RecoveryDir, remove: &dyn Fn(&Path) -> std::io::Result<()>) -> String {
    match remove(dir.path()) {
        Ok(()) => format!("[recovery] session dir removed: {}", dir.path().display()),
        Err(e) => format!(
            "[recovery] session dir NOT removed ({e}), the next start sweeps it: {}",
            dir.path().display()
        ),
    }
}

/// Is `target` the recovery parent or anything inside it? A session dir can
/// become root-shaped (a preference write creates `app.json`), and it is
/// deleted at exit — it must never be adopted as a data folder.
pub fn is_inside_parent(parent: &Path, target: &Path) -> bool {
    crate::data_root::migrate::is_same_or_nested(parent, target)
}

/// Remove `path` without ever following a link: a link (symlink, or a Windows
/// junction, which reports as a link and needs `remove_dir`) is unlinked; a
/// real directory is removed with its content.
pub fn remove_no_follow(path: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return std::fs::remove_file(path).or_else(|_| std::fs::remove_dir(path));
    }
    if meta.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn create_makes_a_dir_named_after_the_pid() {
        let d = tempdir().unwrap();
        let parent = parent_in(d.path());
        let (dir, lines) = create(&parent, 4242);
        let dir = dir.expect("created");
        assert_eq!(dir.path(), session_path(&parent, 4242));
        assert!(dir.path().is_dir());
        assert_eq!(lines.len(), 1, "{lines:?}");
    }

    #[test]
    fn create_replaces_a_stale_dir_with_the_same_pid_instead_of_reusing_it() {
        let d = tempdir().unwrap();
        let parent = parent_in(d.path());
        let stale = session_path(&parent, 7);
        std::fs::create_dir_all(stale.join("instances")).unwrap();
        std::fs::write(stale.join("app.json"), b"{}").unwrap();

        let (dir, _) = create(&parent, 7);

        let dir = dir.expect("created");
        assert!(
            !dir.path().join("app.json").exists(),
            "the old content is gone"
        );
        assert!(!dir.path().join("instances").exists());
    }

    #[test]
    fn a_failed_create_yields_no_dir_and_says_why() {
        let d = tempdir().unwrap();
        // The parent is a FILE: nothing can be created under it.
        let parent = d.path().join("recovery");
        std::fs::write(&parent, b"in the way").unwrap();

        let (dir, lines) = create(&parent, 1);

        assert!(dir.is_none());
        assert!(
            lines.iter().any(|l| l.contains("could not be created")),
            "{lines:?}"
        );
    }

    #[test]
    fn sweep_removes_every_child_but_the_current_session() {
        let d = tempdir().unwrap();
        let parent = parent_in(d.path());
        for name in ["11", "22", "not-a-pid"] {
            std::fs::create_dir_all(parent.join(name).join("mods-cache")).unwrap();
        }
        let mine = session_path(&parent, 22);

        let lines = sweep(&parent, Some(&mine), &remove_no_follow);

        assert!(!parent.join("11").exists());
        assert!(!parent.join("not-a-pid").exists());
        assert!(mine.is_dir(), "the running session's dir is kept");
        assert_eq!(lines.len(), 2, "{lines:?}");
    }

    #[test]
    fn sweep_reports_a_failed_removal_and_keeps_going() {
        let d = tempdir().unwrap();
        let parent = parent_in(d.path());
        std::fs::create_dir_all(parent.join("1")).unwrap();
        std::fs::create_dir_all(parent.join("2")).unwrap();
        let refuse_one = |p: &Path| {
            if p.ends_with("1") {
                Err(std::io::Error::other("scripted: in use"))
            } else {
                remove_no_follow(p)
            }
        };

        let lines = sweep(&parent, None, &refuse_one);

        assert!(parent.join("1").exists());
        assert!(!parent.join("2").exists());
        assert!(
            lines
                .iter()
                .any(|l| l.contains("scripted: in use") && l.contains("NOT removed")),
            "{lines:?}"
        );
    }

    #[test]
    fn sweep_of_a_missing_parent_is_silent() {
        let d = tempdir().unwrap();
        assert!(sweep(&parent_in(d.path()), None, &remove_no_follow).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn sweep_unlinks_a_link_without_following_it() {
        let d = tempdir().unwrap();
        let parent = parent_in(d.path());
        std::fs::create_dir_all(&parent).unwrap();
        let precious = d.path().join("precious");
        std::fs::create_dir_all(&precious).unwrap();
        std::fs::write(precious.join("world.dat"), b"data").unwrap();
        std::os::unix::fs::symlink(&precious, parent.join("99")).unwrap();

        sweep(&parent, None, &remove_no_follow);

        assert!(
            std::fs::symlink_metadata(parent.join("99")).is_err(),
            "the link is gone"
        );
        assert!(
            precious.join("world.dat").exists(),
            "what it pointed at is untouched"
        );
    }

    #[test]
    fn a_failed_create_leaves_nothing_for_the_exit_cleanup() {
        // The type is the guarantee: without a `RecoveryDir` there is nothing
        // `remove_at_exit` can be called with. This pins the other half — a
        // successful create is what the cleanup removes, and only that.
        let d = tempdir().unwrap();
        let parent = parent_in(d.path());
        std::fs::write(d.path().join("bystander.txt"), b"keep").unwrap();
        let (dir, _) = create(&parent, 5);
        let dir = dir.expect("created");

        let line = remove_at_exit(&dir, &remove_no_follow);

        assert!(!dir.path().exists());
        assert!(line.contains("removed"), "{line}");
        assert!(d.path().join("bystander.txt").exists());
        assert!(parent.is_dir(), "only the session dir goes, not the parent");
    }

    #[test]
    fn a_target_inside_the_recovery_parent_is_recognised() {
        let d = tempdir().unwrap();
        let parent = parent_in(d.path());
        std::fs::create_dir_all(parent.join("1")).unwrap();
        assert!(is_inside_parent(&parent, &parent.join("1")));
        assert!(is_inside_parent(&parent, &parent));
        assert!(!is_inside_parent(&parent, d.path()));
        assert!(!is_inside_parent(&parent, &d.path().join("elsewhere")));
    }
}
