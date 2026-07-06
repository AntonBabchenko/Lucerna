use crate::error::{Error, Result};
use std::path::Path;

/// Total size in bytes of all files under `root` (recursive).
pub fn dir_size(root: &Path) -> u64 {
    let mut total = 0;
    if let Ok(entries) = std::fs::read_dir(root) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                total += dir_size(&p);
            } else if let Ok(m) = e.metadata() {
                total += m.len();
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

/// Recursively copy `src` → `dst`, invoking `on_bytes(copied_so_far)` after each
/// file. `skip(relative_path)` returning true omits that entry (used to keep the
/// redirect file at the default location out of the move).
pub fn copy_tree(
    src: &Path,
    dst: &Path,
    skip: &dyn Fn(&Path) -> bool,
    on_bytes: &mut dyn FnMut(u64),
    copied: &mut u64,
) -> Result<()> {
    std::fs::create_dir_all(dst).map_err(|e| Error::io(dst.display().to_string(), e))?;
    let entries = std::fs::read_dir(src).map_err(|e| Error::io(src.display().to_string(), e))?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        if skip(Path::new(&name)) {
            continue;
        }
        let from = entry.path();
        let to = dst.join(&name);
        if from.is_dir() {
            copy_tree(&from, &to, skip, on_bytes, copied)?;
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
    fn copy_tree_copies_and_skips_and_reports_progress() {
        let d = tempdir().unwrap();
        let src = d.path().join("src");
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::write(src.join("keep.txt"), b"12345").unwrap();
        std::fs::write(src.join("data-location.json"), b"{}").unwrap();
        std::fs::write(src.join("sub/x.txt"), b"ab").unwrap();
        let dst = d.path().join("dst");
        let skip = |p: &Path| p == Path::new("data-location.json");
        let mut ticks = Vec::new();
        let mut copied = 0;
        copy_tree(&src, &dst, &skip, &mut |c| ticks.push(c), &mut copied).unwrap();
        assert!(dst.join("keep.txt").is_file());
        assert!(dst.join("sub/x.txt").is_file());
        assert!(
            !dst.join("data-location.json").exists(),
            "redirect must be skipped"
        );
        assert_eq!(copied, 7); // 5 + 2, json skipped
        assert_eq!(ticks.last(), Some(&7));
    }
}
