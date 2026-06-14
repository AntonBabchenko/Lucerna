//! Launcher-import pipeline: copy selected content from a read-only
//! foreign instance into a fresh Lucerna instance, recover mod identities,
//! and finalize (with rollback on a mandatory-phase failure). The source
//! is opened read-only — nothing under it is ever written.

use std::path::Path;

use crate::error::{Error, Result};
use crate::instances::import::model::ContentCategory;
use crate::mods::modpack::path_safety::is_safe_relative_path;

/// Progress callback for a copy: (files_done, files_total).
type CopyProgress<'a> = dyn FnMut(u32, u32) + 'a;

/// Copy one content category from `src_mc/<cat>` to `dst_mc/<cat>`,
/// recursively. The source is opened read-only — this never writes under
/// `src_mc`. Symlinked entries that would escape the category dir are
/// skipped. Returns the number of files copied.
pub fn copy_category(
    src_mc: &Path,
    dst_mc: &Path,
    cat: ContentCategory,
    progress: &mut CopyProgress<'_>,
) -> Result<u32> {
    let rel = cat.rel_path();
    let src = src_mc.join(rel);
    let dst = dst_mc.join(rel);

    if cat.is_file() {
        if !src.is_file() {
            return Ok(0);
        }
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent).map_err(|e| io(&dst, e))?;
        }
        std::fs::copy(&src, &dst).map_err(|e| io(&dst, e))?;
        progress(1, 1);
        return Ok(1);
    }

    if !src.is_dir() {
        return Ok(0);
    }
    let files = collect_files(&src)?;
    let total = files.len() as u32;
    let mut done = 0u32;
    for relpath in &files {
        // Defense in depth: the relative path under the category must be safe.
        let rel_str = relpath.to_string_lossy().replace('\\', "/");
        if !is_safe_relative_path(&rel_str) {
            continue;
        }
        let from = src.join(relpath);
        let to = dst.join(relpath);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(|e| io(&to, e))?;
        }
        std::fs::copy(&from, &to).map_err(|e| io(&to, e))?;
        done += 1;
        progress(done, total);
    }
    Ok(done)
}

/// Relative file paths under `root`, not following symlinked dirs.
fn collect_files(root: &Path) -> Result<Vec<std::path::PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).map_err(|e| io(&d, e))?.flatten() {
            let ft = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };
            let p = entry.path();
            if ft.is_symlink() {
                continue; // never follow links out of the tree
            } else if ft.is_dir() {
                stack.push(p);
            } else if ft.is_file() {
                if let Ok(rel) = p.strip_prefix(root) {
                    out.push(rel.to_path_buf());
                }
            }
        }
    }
    Ok(out)
}

fn io(path: &Path, e: std::io::Error) -> Error {
    Error::io(path.display().to_string(), e)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::import::model::ContentCategory;

    fn write(p: &std::path::Path, body: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    #[test]
    fn copies_a_category_dir_recursively() {
        let tmp = tempfile::tempdir().unwrap();
        let src_mc = tmp.path().join("src/.minecraft");
        write(&src_mc.join("mods/a.jar"), "AAA");
        write(&src_mc.join("mods/sub/b.jar"), "BBB");
        let dst_mc = tmp.path().join("dst/.minecraft");

        let report =
            copy_category(&src_mc, &dst_mc, ContentCategory::Mods, &mut |_, _| {}).unwrap();

        assert_eq!(report, 2);
        assert_eq!(
            std::fs::read_to_string(dst_mc.join("mods/a.jar")).unwrap(),
            "AAA"
        );
        assert_eq!(
            std::fs::read_to_string(dst_mc.join("mods/sub/b.jar")).unwrap(),
            "BBB"
        );
    }

    #[test]
    fn copies_options_txt_single_file() {
        let tmp = tempfile::tempdir().unwrap();
        let src_mc = tmp.path().join("src/.minecraft");
        write(&src_mc.join("options.txt"), "version:3465");
        let dst_mc = tmp.path().join("dst/.minecraft");
        copy_category(
            &src_mc,
            &dst_mc,
            ContentCategory::OptionsTxt,
            &mut |_, _| {},
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(dst_mc.join("options.txt")).unwrap(),
            "version:3465"
        );
    }

    #[test]
    fn leaves_source_byte_identical() {
        let tmp = tempfile::tempdir().unwrap();
        let src_mc = tmp.path().join("src/.minecraft");
        write(&src_mc.join("mods/a.jar"), "AAA");
        let before = std::fs::read(src_mc.join("mods/a.jar")).unwrap();
        let dst_mc = tmp.path().join("dst/.minecraft");
        copy_category(&src_mc, &dst_mc, ContentCategory::Mods, &mut |_, _| {}).unwrap();
        let after = std::fs::read(src_mc.join("mods/a.jar")).unwrap();
        assert_eq!(before, after);
        assert!(
            src_mc.join("mods/a.jar").exists(),
            "source file must survive"
        );
    }
}
