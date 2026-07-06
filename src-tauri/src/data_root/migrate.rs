use crate::error::{Error, Result};
use std::path::{Path, PathBuf};

/// Hard cap on recursion depth for `dir_size` / symlink scans. A directory
/// tree this deep is already pathological; the cap exists purely so a cyclic
/// junction/symlink (which `symlink_metadata` detection *should* catch first,
/// but belt-and-braces) can never blow the stack.
const MAX_SCAN_DEPTH: u32 = 256;

/// Total size in bytes of all files under `root` (recursive). Bounded by
/// `MAX_SCAN_DEPTH` so a cyclic link can never recurse without end.
pub fn dir_size(root: &Path) -> u64 {
    dir_size_depth(root, 0)
}

fn dir_size_depth(root: &Path, depth: u32) -> u64 {
    if depth >= MAX_SCAN_DEPTH {
        return 0;
    }
    let mut total = 0;
    if let Ok(entries) = std::fs::read_dir(root) {
        for e in entries.flatten() {
            let p = e.path();
            // Use symlink_metadata so a symlink is never followed (its target
            // could be outside the tree or form a cycle). A symlink to a
            // directory reports as a symlink here, not a dir, and contributes
            // its own (tiny) link size rather than its target's contents.
            match std::fs::symlink_metadata(&p) {
                Ok(m) if m.file_type().is_dir() => total += dir_size_depth(&p, depth + 1),
                Ok(m) => total += m.len(),
                Err(_) => {}
            }
        }
    }
    total
}

/// True if `dir` does not exist, or exists and contains no entries.
pub fn target_is_empty(dir: &Path) -> bool {
    match std::fs::read_dir(dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
        Ok(mut it) => it.next().is_none(),
        Err(_) => false,
    }
}

/// True if `dir` does not exist, or exists and contains no entries other than
/// the launcher-transient top-level names in `safe`. Used for the reset target
/// (the OS-default dir legitimately holds `data-location.json`, `logs`, and
/// `updates` even when it holds no user data).
pub fn empty_or_only_safe(dir: &Path, safe: &[&str]) -> bool {
    match std::fs::read_dir(dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
        Ok(it) => it.flatten().all(|e| {
            e.file_name()
                .to_str()
                .map(|n| safe.contains(&n))
                .unwrap_or(false)
        }),
        Err(_) => false,
    }
}

/// Exists and is writable (creates + removes a probe file).
pub fn is_available(dir: &Path) -> bool {
    if !dir.exists() {
        return false;
    }
    let probe = dir.join(".lucerna-write-probe");
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// Canonicalize `path` even when it does not fully exist: walk up to the
/// deepest ancestor that DOES exist, canonicalize that, then re-append the
/// non-existing remainder. This resolves case differences, `\\?\` verbatim
/// prefixes, and 8.3 short names on the existing portion — the part that
/// matters for a same/nested comparison against `current` (which always
/// exists). Falls back to the raw path if even the root cannot be canonicalized.
fn canonicalize_best_effort(path: &Path) -> PathBuf {
    let mut remainder: Vec<std::ffi::OsString> = Vec::new();
    let mut probe = path.to_path_buf();
    loop {
        if let Ok(c) = std::fs::canonicalize(&probe) {
            // Re-append the non-existing tail we peeled off.
            let mut resolved = c;
            for seg in remainder.iter().rev() {
                resolved.push(seg);
            }
            return resolved;
        }
        match probe.file_name() {
            Some(name) => {
                remainder.push(name.to_os_string());
                if !probe.pop() {
                    return path.to_path_buf();
                }
            }
            // Reached a root/prefix that still won't canonicalize.
            None => return path.to_path_buf(),
        }
    }
}

/// Case-fold a path to a comparable string. On Windows, filesystem paths are
/// case-insensitive, so lowercase before comparing; on Unix they are
/// case-sensitive, so compare verbatim.
fn compare_key(path: &Path) -> String {
    let s = path.to_string_lossy();
    #[cfg(windows)]
    {
        s.to_lowercase()
    }
    #[cfg(not(windows))]
    {
        s.into_owned()
    }
}

/// True when `target` is the same directory as `current`, or nested inside it.
///
/// Robust against case differences, `\\?\` verbatim prefixes, and 8.3 short
/// names because both paths are canonicalized (best-effort for the possibly
/// non-existing `target`) before comparison. This is the load-bearing guard
/// that prevents the delete loop from wiping both source and a freshly-copied
/// target when the two are the same location spelled differently.
pub fn is_same_or_nested(current: &Path, target: &Path) -> bool {
    let cur = canonicalize_best_effort(current);
    let tgt = canonicalize_best_effort(target);
    let cur_key = compare_key(&cur);
    let tgt_key = compare_key(&tgt);
    if cur_key == tgt_key {
        return true;
    }
    // Component-wise ancestor check on the case-folded canonical forms, so
    // `C:\Data` is an ancestor of `C:\Data\Sub` but NOT of `C:\DataOther`.
    let cur_norm = PathBuf::from(&cur_key);
    let tgt_norm = PathBuf::from(&tgt_key);
    tgt_norm.starts_with(&cur_norm)
}

/// Recursively scan `root` for any symbolic link or junction (reparse point).
/// Returns `Ok(true)` on the first one found. Junctions on Windows surface as
/// symlinks via `symlink_metadata().file_type().is_symlink()`. Bounded by
/// `MAX_SCAN_DEPTH`.
pub fn contains_reparse_point(root: &Path) -> Result<bool> {
    contains_reparse_point_depth(root, 0)
}

fn contains_reparse_point_depth(root: &Path, depth: u32) -> Result<bool> {
    if depth >= MAX_SCAN_DEPTH {
        return Ok(false);
    }
    let entries = match std::fs::read_dir(root) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(Error::io(root.display().to_string(), e)),
    };
    for entry in entries.flatten() {
        let p = entry.path();
        let meta =
            std::fs::symlink_metadata(&p).map_err(|e| Error::io(p.display().to_string(), e))?;
        let ft = meta.file_type();
        if ft.is_symlink() {
            return Ok(true);
        }
        if ft.is_dir() && contains_reparse_point_depth(&p, depth + 1)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Recursively copy `src` → `dst`, invoking `on_bytes(copied_so_far)` after each
/// file. `skip(name)` returning true omits that entry — but ONLY at the tree
/// root (depth 0), so the redirect file at the default location is left out of
/// the move while a nested user file that happens to share its name is copied.
pub fn copy_tree(
    src: &Path,
    dst: &Path,
    skip: &dyn Fn(&Path) -> bool,
    on_bytes: &mut dyn FnMut(u64),
    copied: &mut u64,
) -> Result<()> {
    copy_tree_depth(src, dst, skip, on_bytes, copied, 0)
}

fn copy_tree_depth(
    src: &Path,
    dst: &Path,
    skip: &dyn Fn(&Path) -> bool,
    on_bytes: &mut dyn FnMut(u64),
    copied: &mut u64,
    depth: u32,
) -> Result<()> {
    std::fs::create_dir_all(dst).map_err(|e| Error::io(dst.display().to_string(), e))?;
    let entries = std::fs::read_dir(src).map_err(|e| Error::io(src.display().to_string(), e))?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        // The skip predicate anchors to the root's direct children only.
        if depth == 0 && skip(Path::new(&name)) {
            continue;
        }
        let from = entry.path();
        let to = dst.join(&name);
        if from.is_dir() {
            copy_tree_depth(&from, &to, skip, on_bytes, copied, depth + 1)?;
        } else {
            let bytes =
                std::fs::copy(&from, &to).map_err(|e| Error::io(to.display().to_string(), e))?;
            *copied += bytes;
            on_bytes(*copied);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn size_and_empty_and_available() {
        let d = tempdir().unwrap();
        let root = d.path().join("root");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("a.txt"), b"hello").unwrap();
        std::fs::write(root.join("sub/b.txt"), b"world!").unwrap();
        assert_eq!(dir_size(&root), 11);
        assert!(!target_is_empty(&root));
        assert!(target_is_empty(&d.path().join("nope")));
        assert!(is_available(&root));
        assert!(!is_available(&d.path().join("nope")));
    }

    #[test]
    fn empty_or_only_safe_predicate() {
        let d = tempdir().unwrap();
        let root = d.path().join("root");
        std::fs::create_dir_all(&root).unwrap();
        let safe = ["data-location.json", "logs", "updates"];
        // Empty → ok.
        assert!(empty_or_only_safe(&root, &safe));
        // Only-safe entries → ok.
        std::fs::write(root.join("data-location.json"), b"{}").unwrap();
        std::fs::create_dir_all(root.join("logs")).unwrap();
        assert!(empty_or_only_safe(&root, &safe));
        // A user directory → not ok.
        std::fs::create_dir_all(root.join("instances")).unwrap();
        assert!(!empty_or_only_safe(&root, &safe));
        // Missing dir → ok (treated as empty).
        assert!(empty_or_only_safe(&d.path().join("nope"), &safe));
    }

    #[test]
    fn copy_tree_copies_and_skips_root_only_and_reports_progress() {
        let d = tempdir().unwrap();
        let src = d.path().join("src");
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::write(src.join("keep.txt"), b"12345").unwrap();
        std::fs::write(src.join("data-location.json"), b"{}").unwrap();
        std::fs::write(src.join("sub/x.txt"), b"ab").unwrap();
        // A nested file coincidentally named like the redirect must NOT be
        // skipped — the skip predicate is anchored to the root.
        std::fs::write(src.join("sub/data-location.json"), b"zz").unwrap();
        let dst = d.path().join("dst");
        let skip = |p: &Path| p == Path::new("data-location.json");
        let mut ticks = Vec::new();
        let mut copied = 0;
        copy_tree(&src, &dst, &skip, &mut |c| ticks.push(c), &mut copied).unwrap();
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

    #[test]
    fn same_path_is_same_or_nested() {
        let d = tempdir().unwrap();
        let a = d.path().join("data");
        std::fs::create_dir_all(&a).unwrap();
        assert!(is_same_or_nested(&a, &a));
    }

    #[test]
    fn nested_target_is_flagged() {
        let d = tempdir().unwrap();
        let cur = d.path().join("data");
        std::fs::create_dir_all(&cur).unwrap();
        // Target does not exist yet — best-effort canonicalization must still
        // resolve it as nested under the existing `cur`.
        let nested = cur.join("sub").join("deeper");
        assert!(is_same_or_nested(&cur, &nested));
    }

    #[test]
    fn sibling_target_is_not_nested() {
        let d = tempdir().unwrap();
        let cur = d.path().join("data");
        std::fs::create_dir_all(&cur).unwrap();
        let sibling = d.path().join("data-other");
        assert!(
            !is_same_or_nested(&cur, &sibling),
            "a sibling sharing a name prefix must not be treated as nested"
        );
    }

    // On Windows the same directory can be spelled with a different case or a
    // `\\?\` verbatim prefix. The canonical comparison must treat those as the
    // same location so the delete loop never wipes a freshly-copied target.
    #[cfg(windows)]
    #[test]
    fn case_differing_target_is_same() {
        let d = tempdir().unwrap();
        let cur = d.path().join("DataDir");
        std::fs::create_dir_all(&cur).unwrap();
        let upper = d.path().join("DATADIR");
        assert!(
            is_same_or_nested(&cur, &upper),
            "case-only difference must be treated as the same directory"
        );
    }

    #[cfg(windows)]
    #[test]
    fn verbatim_prefix_target_is_same() {
        let d = tempdir().unwrap();
        let cur = d.path().join("DataDir");
        std::fs::create_dir_all(&cur).unwrap();
        // Build a `\\?\`-prefixed spelling of the same path.
        let verbatim = PathBuf::from(format!(r"\\?\{}", cur.display()));
        assert!(
            is_same_or_nested(&cur, &verbatim),
            "verbatim-prefixed spelling must be treated as the same directory"
        );
    }

    #[cfg(windows)]
    #[test]
    fn case_differing_nested_target_is_flagged() {
        let d = tempdir().unwrap();
        let cur = d.path().join("DataDir");
        std::fs::create_dir_all(&cur).unwrap();
        let nested = d.path().join("datadir").join("instances");
        assert!(
            is_same_or_nested(&cur, &nested),
            "case-only difference on an ancestor must still flag nesting"
        );
    }

    #[test]
    fn no_reparse_point_in_plain_tree() {
        let d = tempdir().unwrap();
        let root = d.path().join("root");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("a.txt"), b"x").unwrap();
        assert!(!contains_reparse_point(&root).unwrap());
    }

    // A real symlink is detected. On Windows creating one may require
    // privilege, so this is Unix-only; the Windows junction path shares the
    // same `is_symlink()` detection.
    #[cfg(unix)]
    #[test]
    fn symlink_is_detected() {
        let d = tempdir().unwrap();
        let root = d.path().join("root");
        std::fs::create_dir_all(&root).unwrap();
        let target = d.path().join("elsewhere");
        std::fs::create_dir_all(&target).unwrap();
        std::os::unix::fs::symlink(&target, root.join("link")).unwrap();
        assert!(contains_reparse_point(&root).unwrap());
    }
}
