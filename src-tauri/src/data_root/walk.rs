//! Strict, cancellable, rule-aware tree walks for the data-root move.
//!
//! "Strict" is the point: an unreadable directory ENTRY aborts the walk. The
//! walks this replaces iterated `read_dir(..).flatten()`, so an entry that
//! failed to enumerate was neither copied nor verified — and then deleted
//! with its parent. The directory listing and the file copy are injected so a
//! test can fail them at a chosen path.

use crate::data_root::transient::EntryRule;
use crate::error::Error;
use std::path::{Path, PathBuf};

/// A tree this deep is pathological; refusing beats recursing without end.
const MAX_DEPTH: u32 = 256;

#[derive(Debug)]
pub enum WalkStop {
    Cancelled,
    Failed(Error),
}

impl From<Error> for WalkStop {
    fn from(e: Error) -> Self {
        WalkStop::Failed(e)
    }
}

pub struct Walk<'a> {
    /// Receives the path RELATIVE to the tree root.
    pub rule: &'a dyn Fn(&Path) -> EntryRule,
    /// Children of a directory. Any entry error must fail the whole call.
    pub list_dir: &'a dyn Fn(&Path) -> std::io::Result<Vec<PathBuf>>,
    pub copy_file: &'a dyn Fn(&Path, &Path) -> std::io::Result<u64>,
    /// Polled before every file.
    pub cancelled: &'a dyn Fn() -> bool,
}

/// `read_dir` that fails when ANY entry fails, instead of skipping it.
pub fn real_list_dir(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    std::fs::read_dir(dir)?
        .map(|entry| entry.map(|e| e.path()))
        .collect()
}

pub fn real_copy_file(from: &Path, to: &Path) -> std::io::Result<u64> {
    std::fs::copy(from, to)
}

/// Copy `src` into `dst`, honouring `walk.rule`. `on_bytes(total_so_far)` is
/// called after every file.
pub fn copy_tree(
    _src: &Path,
    _dst: &Path,
    _walk: &Walk<'_>,
    _on_bytes: &mut dyn FnMut(u64),
    _copied: &mut u64,
) -> Result<(), WalkStop> {
    Ok(()) // RED stub — Task 10 replaces it
}

/// Every file under `src` that the rules copy must exist under `dst` — with an
/// identical length (`Normal`) or at all (`ExistenceOnly`). Walks the SOURCE,
/// so content already present in `dst` never matters.
pub fn verify_tree(_src: &Path, _dst: &Path, _walk: &Walk<'_>) -> Result<(), WalkStop> {
    Ok(()) // RED stub — Task 10 replaces it
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use tempfile::tempdir;

    fn normal(_: &Path) -> EntryRule {
        EntryRule::Normal
    }

    fn never() -> bool {
        false
    }

    fn real_walk<'a>(rule: &'a dyn Fn(&Path) -> EntryRule) -> Walk<'a> {
        Walk {
            rule,
            list_dir: &real_list_dir,
            copy_file: &real_copy_file,
            cancelled: &never,
        }
    }

    // Ported from migrate.rs (`copy_tree_copies_and_skips_root_only_and_reports_progress`).
    #[test]
    fn copy_tree_copies_and_skips_root_only_and_reports_progress() {
        let d = tempdir().unwrap();
        let src = d.path().join("src");
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::write(src.join("keep.txt"), b"hello").unwrap();
        std::fs::write(src.join("data-location.json"), b"{}").unwrap();
        std::fs::write(src.join("sub/x.txt"), b"hi").unwrap();
        std::fs::write(src.join("sub/data-location.json"), b"{}").unwrap();
        let dst = d.path().join("dst");

        let rule = |rel: &Path| {
            if rel == Path::new("data-location.json") {
                EntryRule::Skip
            } else {
                EntryRule::Normal
            }
        };
        let mut ticks = Vec::new();
        let mut copied = 0u64;
        copy_tree(
            &src,
            &dst,
            &real_walk(&rule),
            &mut |c| ticks.push(c),
            &mut copied,
        )
        .unwrap();

        assert!(dst.join("keep.txt").is_file());
        assert!(dst.join("sub/x.txt").is_file());
        assert!(
            !dst.join("data-location.json").exists(),
            "root redirect must be skipped"
        );
        assert!(
            dst.join("sub/data-location.json").is_file(),
            "nested same-named file must be copied, not skipped"
        );
        assert_eq!(copied, 9); // 5 + 2 + 2, root json skipped
        assert_eq!(ticks.last(), Some(&9));
    }

    // Ported from migrate.rs.
    #[test]
    fn verify_passes_despite_overwritten_and_extra_target_files() {
        let d = tempdir().unwrap();
        let src = d.path().join("src");
        std::fs::create_dir_all(src.join("logs")).unwrap();
        std::fs::write(src.join("logs/other.log"), b"NEW-LOG-CONTENT").unwrap();
        std::fs::write(src.join("a.txt"), b"hello").unwrap();
        let dst = d.path().join("dst");
        std::fs::create_dir_all(dst.join("logs")).unwrap();
        std::fs::write(dst.join("logs/other.log"), b"old").unwrap(); // different size
        std::fs::write(dst.join("stale-unrelated.tmp"), b"leftover").unwrap();

        let walk = real_walk(&normal);
        let mut copied = 0u64;
        copy_tree(&src, &dst, &walk, &mut |_| {}, &mut copied).unwrap();
        verify_tree(&src, &dst, &walk).expect("verify tolerates overwrite + extra target files");
        assert_eq!(
            std::fs::read(dst.join("logs/other.log")).unwrap(),
            b"NEW-LOG-CONTENT"
        );
    }

    // Ported from migrate.rs.
    #[test]
    fn verify_fails_when_a_source_file_is_missing_in_target() {
        let d = tempdir().unwrap();
        let src = d.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("a.txt"), b"hello").unwrap();
        let dst = d.path().join("dst");
        std::fs::create_dir_all(&dst).unwrap(); // copy never ran
        assert!(verify_tree(&src, &dst, &real_walk(&normal)).is_err());
    }

    #[test]
    fn verify_catches_a_length_mismatch_but_not_on_an_existence_only_file() {
        let d = tempdir().unwrap();
        let src = d.path().join("src");
        std::fs::create_dir_all(src.join("logs")).unwrap();
        std::fs::write(src.join("logs/lucerna.log"), b"line-1\n").unwrap();
        std::fs::write(src.join("data.bin"), b"PAYLOAD").unwrap();
        let dst = d.path().join("dst");

        let rule = |rel: &Path| {
            if rel == Path::new("logs").join("lucerna.log") {
                EntryRule::ExistenceOnly
            } else {
                EntryRule::Normal
            }
        };
        let walk = real_walk(&rule);
        let mut copied = 0u64;
        copy_tree(&src, &dst, &walk, &mut |_| {}, &mut copied).unwrap();

        // The launcher appended to its own log after the copy: must still verify.
        std::fs::write(src.join("logs/lucerna.log"), b"line-1\nline-2\n").unwrap();
        verify_tree(&src, &dst, &walk).expect("a grown live log must not fail verification");

        // Any other file that differs in length must fail.
        std::fs::write(dst.join("data.bin"), b"TRUNC").unwrap();
        match verify_tree(&src, &dst, &walk) {
            Err(WalkStop::Failed(e)) => assert!(e.to_string().contains("data.bin"), "{e}"),
            other => panic!("expected a verification failure, got {other:?}"),
        }

        // And an existence-only file that is MISSING must fail too.
        std::fs::write(dst.join("data.bin"), b"PAYLOAD").unwrap();
        std::fs::remove_file(dst.join("logs/lucerna.log")).unwrap();
        assert!(verify_tree(&src, &dst, &walk).is_err());
    }

    #[test]
    fn an_unreadable_directory_aborts_the_copy() {
        let d = tempdir().unwrap();
        let src = d.path().join("src");
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::write(src.join("sub/x.txt"), b"hi").unwrap();
        let dst = d.path().join("dst");

        let list_dir = |dir: &Path| {
            if dir.ends_with("sub") {
                Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "scripted",
                ))
            } else {
                real_list_dir(dir)
            }
        };
        let walk = Walk {
            rule: &normal,
            list_dir: &list_dir,
            copy_file: &real_copy_file,
            cancelled: &never,
        };
        let mut copied = 0u64;
        match copy_tree(&src, &dst, &walk, &mut |_| {}, &mut copied) {
            Err(WalkStop::Failed(e)) => assert!(e.to_string().contains("sub"), "{e}"),
            other => panic!("expected the copy to abort, got {other:?}"),
        }
        assert!(matches!(
            verify_tree(&src, &dst, &walk),
            Err(WalkStop::Failed(_))
        ));
    }

    #[test]
    fn cancel_stops_before_the_next_file() {
        let d = tempdir().unwrap();
        let src = d.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        for name in ["a", "b", "c"] {
            std::fs::write(src.join(name), b"x").unwrap();
        }
        let dst = d.path().join("dst");

        let done = Cell::new(0usize);
        let copy_file = |from: &Path, to: &Path| {
            done.set(done.get() + 1);
            std::fs::copy(from, to)
        };
        let cancelled = || done.get() >= 1;
        let walk = Walk {
            rule: &normal,
            list_dir: &real_list_dir,
            copy_file: &copy_file,
            cancelled: &cancelled,
        };
        let mut copied = 0u64;
        assert!(matches!(
            copy_tree(&src, &dst, &walk, &mut |_| {}, &mut copied),
            Err(WalkStop::Cancelled)
        ));
        assert_eq!(
            done.get(),
            1,
            "exactly one file was copied before the cancel"
        );
    }

    #[test]
    fn a_failed_file_copy_names_the_target_path() {
        let d = tempdir().unwrap();
        let src = d.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("big.jar"), b"x").unwrap();
        let dst = d.path().join("dst");
        let copy_file = |_: &Path, _: &Path| -> std::io::Result<u64> {
            Err(std::io::Error::new(std::io::ErrorKind::Other, "disk full"))
        };
        let walk = Walk {
            rule: &normal,
            list_dir: &real_list_dir,
            copy_file: &copy_file,
            cancelled: &never,
        };
        let mut copied = 0u64;
        match copy_tree(&src, &dst, &walk, &mut |_| {}, &mut copied) {
            Err(WalkStop::Failed(e)) => {
                let text = e.to_string();
                assert!(
                    text.contains("big.jar") && text.contains("disk full"),
                    "{text}"
                );
            }
            other => panic!("expected a copy failure, got {other:?}"),
        }
    }
}
