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
                Err(_) => {
                    // An entry we cannot stat contributes 0. This is an
                    // ESTIMATE: it feeds the storage-panel display and the
                    // relocation progress denominator, never a free-space
                    // gate, so under-counting costs a progress bar that
                    // finishes early — not a wrong decision.
                }
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
/// the launcher-owned top-level names in `safe` (the OS-default dir
/// legitimately holds them even when it holds no user data). An entry that
/// cannot be read is NOT "safe": could-not-tell resolves to "not empty".
pub fn empty_or_only_safe(dir: &Path, safe: &[&str]) -> bool {
    match blocking_entries(dir, safe) {
        Ok(blocking) => blocking.is_empty(),
        Err(_) => false,
    }
}

/// Top-level names in `dir` that are not in `safe`, sorted. A missing `dir`
/// has none. Used to tell the user WHAT makes a reset impossible.
pub fn blocking_entries(dir: &Path, safe: &[&str]) -> Result<Vec<String>> {
    let children = match crate::data_root::walk::real_list_dir(dir) {
        Ok(children) => children,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(Error::io(dir.display().to_string(), e)),
    };
    let mut blocking: Vec<String> = children
        .iter()
        .filter_map(|child| child.file_name())
        .filter(|name| !safe.iter().any(|s| *name == *s))
        .map(|name| name.to_string_lossy().into_owned())
        .collect();
    blocking.sort();
    Ok(blocking)
}

/// A CUSTOM move target must be empty — except for a `webview/` left by an
/// earlier move away from it, which the sweep has not removed yet.
pub fn target_is_empty_or_transient(dir: &Path) -> bool {
    empty_or_only_safe(dir, &[crate::data_root::transient::WEBVIEW_DIR])
}

/// Strict, skip-aware size of `root` in bytes. Unlike [`dir_size`] (a display
/// estimate that counts an unreadable entry as 0) any error aborts: the move
/// itself would abort on the same entry, so failing early is the honest answer.
/// `skip_top` filters TOP-LEVEL names only.
pub fn dir_size_strict(root: &Path, skip_top: &dyn Fn(&std::ffi::OsStr) -> bool) -> Result<u64> {
    size_strict(root, skip_top, 0)
}

fn size_strict(dir: &Path, skip_top: &dyn Fn(&std::ffi::OsStr) -> bool, depth: u32) -> Result<u64> {
    let mut total = 0u64;
    for (child, meta) in strict_children(dir, skip_top, depth)? {
        total += if meta.is_dir() {
            size_strict(&child, skip_top, depth + 1)?
        } else {
            meta.len()
        };
    }
    Ok(total)
}

/// Strict, skip-aware sibling of [`contains_reparse_point`]: an entry that
/// cannot be inspected, and a tree deeper than the cap, are ERRORS — not "no
/// links". (`contains_reparse_point` keeps its lenient semantics for world
/// migration.)
pub fn contains_link_strict(
    root: &Path,
    skip_top: &dyn Fn(&std::ffi::OsStr) -> bool,
) -> Result<bool> {
    link_strict(root, skip_top, 0)
}

fn link_strict(
    dir: &Path,
    skip_top: &dyn Fn(&std::ffi::OsStr) -> bool,
    depth: u32,
) -> Result<bool> {
    for (child, meta) in strict_children(dir, skip_top, depth)? {
        if meta.file_type().is_symlink()
            || (meta.is_dir() && link_strict(&child, skip_top, depth + 1)?)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn strict_children(
    dir: &Path,
    skip_top: &dyn Fn(&std::ffi::OsStr) -> bool,
    depth: u32,
) -> Result<Vec<(PathBuf, std::fs::Metadata)>> {
    if depth >= MAX_SCAN_DEPTH {
        return Err(Error::data_move_failed(format!(
            "{} is nested more than {MAX_SCAN_DEPTH} directories deep",
            dir.display()
        )));
    }
    let mut out = Vec::new();
    for child in crate::data_root::walk::real_list_dir(dir)
        .map_err(|e| Error::io(dir.display().to_string(), e))?
    {
        if depth == 0 && child.file_name().is_some_and(|name| skip_top(name)) {
            continue;
        }
        let meta = std::fs::symlink_metadata(&child)
            .map_err(|e| Error::io(child.display().to_string(), e))?;
        out.push((child, meta));
    }
    Ok(out)
}

/// What probing a folder told us. A `bool` cannot say "present but not
/// writable", and `Path::exists()` answers `false` for ANY stat failure — so
/// the copy used to tell the owner of a read-only folder to "reconnect" it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Availability {
    Available,
    /// `NotFound` — and only `NotFound`.
    Missing,
    /// Something is there, and it is not a directory.
    NotADirectory,
    /// The directory exists; the write probe failed. Rendered error.
    NotWritable(String),
    /// The stat failed with anything but `NotFound`: could not tell.
    Unknown(String),
}

const WRITE_PROBE_FILE: &str = ".lucerna-write-probe";

/// Probe `dir`: does it exist, is it a directory, can we write into it?
pub fn probe(dir: &Path) -> Availability {
    // RED STUB (push 1): the old two-valued answer.
    if !dir.exists() {
        return Availability::Missing;
    }
    let probe = dir.join(WRITE_PROBE_FILE);
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            // Best-effort: a leftover probe file changes nothing about the
            // answer, which is already known.
            let _ = std::fs::remove_file(&probe);
            Availability::Available
        }
        Err(_) => Availability::Missing,
    }
}

/// Exists and is writable. For callers that only need yes / no.
pub fn is_available(dir: &Path) -> bool {
    matches!(probe(dir), Availability::Available)
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

/// True when `a` and `b` are the same directory once canonicalized
/// (best-effort) and case-folded per platform — the equality half of
/// [`is_same_or_nested`], exposed for checks that must NOT treat nesting as a
/// conflict. Adopt uses it: pointing at a root the current root lives inside
/// is exactly the doubled-path recovery, so nesting must stay legal there.
pub fn is_same_path(a: &Path, b: &Path) -> bool {
    compare_key(&canonicalize_best_effort(a)) == compare_key(&canonicalize_best_effort(b))
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn a_missing_folder_is_missing() {
        let d = tempdir().unwrap();
        assert_eq!(probe(&d.path().join("nope")), Availability::Missing);
    }

    #[test]
    fn a_writable_folder_is_available_and_the_probe_file_is_gone() {
        let d = tempdir().unwrap();
        assert_eq!(probe(d.path()), Availability::Available);
        assert!(!d.path().join(WRITE_PROBE_FILE).exists());
    }

    #[test]
    fn a_file_where_a_folder_is_expected_is_not_a_directory() {
        let d = tempdir().unwrap();
        let f = d.path().join("LucernaData");
        std::fs::write(&f, b"not a folder").unwrap();
        assert_eq!(probe(&f), Availability::NotADirectory);
    }

    /// Unix only: a read-only bit on a Windows directory does not stop file
    /// creation inside it (the Windows case is a live check, spec §10.3).
    /// Permissions are saved and restored — never `set_readonly(false)`, which
    /// would widen the mode — and restored BEFORE the assertion so the tempdir
    /// cleans up even when it fails. `set_readonly` rather than a mode literal:
    /// the mode-bit extension trait is confined to `platform::` by
    /// `structural_platform_chokepoint`.
    #[cfg(unix)]
    #[test]
    fn a_read_only_folder_is_not_writable_not_missing() {
        let d = tempdir().unwrap();
        let ro = d.path().join("ro");
        std::fs::create_dir(&ro).unwrap();
        let original = std::fs::metadata(&ro).unwrap().permissions();
        let mut read_only = original.clone();
        read_only.set_readonly(true);
        std::fs::set_permissions(&ro, read_only).unwrap();

        let answer = probe(&ro);

        std::fs::set_permissions(&ro, original).unwrap();
        // root ignores mode bits; CI runners are not root, a dev container might be.
        if answer != Availability::Available {
            assert!(matches!(answer, Availability::NotWritable(_)), "{answer:?}");
        }
    }

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
    fn is_same_path_matches_same_dir_and_rejects_nested_and_sibling() {
        let d = tempdir().unwrap();
        let a = d.path().join("data");
        std::fs::create_dir_all(&a).unwrap();
        assert!(is_same_path(&a, &a));
        assert!(
            !is_same_path(&a, &a.join("sub")),
            "nested is not SAME — adopt must allow it"
        );
        assert!(!is_same_path(&a, &d.path().join("data-other")));
    }

    #[cfg(windows)]
    #[test]
    fn is_same_path_is_case_insensitive_on_windows() {
        let d = tempdir().unwrap();
        let a = d.path().join("DataDir");
        std::fs::create_dir_all(&a).unwrap();
        assert!(is_same_path(&a, &d.path().join("DATADIR")));
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
    #[test]
    fn strict_size_and_link_scans_skip_top_level_names_only() {
        let d = tempdir().unwrap();
        let root = d.path().join("root");
        std::fs::create_dir_all(root.join("webview")).unwrap();
        std::fs::create_dir_all(root.join("instances/webview")).unwrap();
        std::fs::write(root.join("webview/cache.bin"), vec![0u8; 100]).unwrap();
        std::fs::write(root.join("instances/webview/keep.bin"), vec![0u8; 7]).unwrap();
        std::fs::write(root.join("a.txt"), b"hello").unwrap();

        let skip = |name: &std::ffi::OsStr| name == "webview";
        assert_eq!(dir_size_strict(&root, &skip).unwrap(), 12);
        assert_eq!(dir_size_strict(&root, &|_| false).unwrap(), 112);
        assert!(!contains_link_strict(&root, &skip).unwrap());
    }

    #[test]
    fn strict_scans_fail_on_a_missing_root_instead_of_answering_zero() {
        let d = tempdir().unwrap();
        let gone = d.path().join("nope");
        assert!(dir_size_strict(&gone, &|_| false).is_err());
        assert!(contains_link_strict(&gone, &|_| false).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_inside_a_skipped_top_level_dir_is_not_the_moves_business() {
        let d = tempdir().unwrap();
        let root = d.path().join("root");
        std::fs::create_dir_all(root.join("webview")).unwrap();
        std::fs::create_dir_all(root.join("jres")).unwrap();
        std::os::unix::fs::symlink("/tmp", root.join("webview/link")).unwrap();
        let skip = |name: &std::ffi::OsStr| name == "webview";
        assert!(!contains_link_strict(&root, &skip).unwrap());
        std::os::unix::fs::symlink("/tmp", root.join("jres/link")).unwrap();
        assert!(contains_link_strict(&root, &skip).unwrap());
    }

    #[test]
    fn blocking_entries_names_what_stands_in_the_way_of_a_reset() {
        let d = tempdir().unwrap();
        let root = d.path().join("default");
        let safe = crate::data_root::transient::SAFE_OVERLAP;
        assert_eq!(
            blocking_entries(&root, &safe).unwrap(),
            Vec::<String>::new()
        );

        std::fs::create_dir_all(root.join("logs")).unwrap();
        std::fs::create_dir_all(root.join("webview")).unwrap();
        std::fs::write(root.join("data-location.json"), b"{}").unwrap();
        std::fs::write(root.join("pending-cleanup.json"), b"{}").unwrap();
        assert!(empty_or_only_safe(&root, &safe));

        std::fs::create_dir_all(root.join("libraries")).unwrap();
        std::fs::write(root.join("account.json"), b"{}").unwrap();
        assert_eq!(
            blocking_entries(&root, &safe).unwrap(),
            vec!["account.json".to_string(), "libraries".to_string()]
        );
        assert!(!empty_or_only_safe(&root, &safe));
    }

    #[test]
    fn a_custom_target_may_hold_only_an_old_webview() {
        let d = tempdir().unwrap();
        let target = d.path().join("LucernaData");
        assert!(target_is_empty_or_transient(&target), "missing = empty");
        std::fs::create_dir_all(target.join("webview")).unwrap();
        assert!(target_is_empty_or_transient(&target));
        std::fs::create_dir_all(target.join("logs")).unwrap();
        assert!(
            !target_is_empty_or_transient(&target),
            "logs is safe for a RESET target only"
        );
    }
}
