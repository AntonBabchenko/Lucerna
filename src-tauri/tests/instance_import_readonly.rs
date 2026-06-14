//! Read-only-source invariant: importing copies content out of a source
//! tree and never mutates it. Exercises the pure pipeline pieces (the
//! Tauri-handle path is covered by the manual GUI gate).

use std::path::Path;

fn snapshot(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                let rel = p
                    .strip_prefix(dir)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((rel, std::fs::read(&p).unwrap()));
            }
        }
    }
    out.sort();
    out
}

#[test]
fn import_copy_leaves_source_untouched_and_copies_content() {
    use lucerna_lib::instances::import::model::ContentCategory;
    use lucerna_lib::instances::import::pipeline::copy_category;

    let tmp = tempfile::tempdir().unwrap();
    let src_mc = tmp.path().join("source/.minecraft");
    std::fs::create_dir_all(src_mc.join("mods")).unwrap();
    std::fs::write(src_mc.join("mods/a.jar"), b"AAA").unwrap();
    std::fs::write(src_mc.join("mods/b.jar"), b"BBB").unwrap();

    let before = snapshot(&src_mc);
    let dst_mc = tmp.path().join("dest/.minecraft");
    let n = copy_category(&src_mc, &dst_mc, ContentCategory::Mods, &mut |_, _| {}).unwrap();

    assert_eq!(n, 2);
    assert_eq!(std::fs::read(dst_mc.join("mods/a.jar")).unwrap(), b"AAA");
    // Source is byte-identical after the import copy.
    assert_eq!(before, snapshot(&src_mc));
}
