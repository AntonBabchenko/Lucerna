//! The files in a folder (the user's Downloads) worth checking for a pack
//! (spec §4.4): top level only, regular `.jar` / `.zip` files, and only those
//! whose length is one a pending entry has — so a folder of thousands of files
//! costs one listing and a few hashes. An unreadable folder is an error, never
//! an empty answer; one unreadable file is counted and tried again next time.
//! Unlike a drop (where size is only a hint), the folder scan keeps its size
//! filter: it is what keeps a folder of thousands of files cheap.

use std::collections::HashSet;
use std::path::Path;

use crate::mods::manual_downloads::place;

pub async fn candidates(
    folder: &Path,
    sizes: &HashSet<u64>,
) -> std::io::Result<(Vec<String>, u32)> {
    let mut entries = tokio::fs::read_dir(folder).await?;
    let mut found = Vec::new();
    let mut skipped = 0u32;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if place::ext_of(&path).is_none() {
            continue;
        }
        match entry.file_type().await {
            Ok(t) if t.is_file() => {}
            Ok(_) => continue,
            Err(_) => {
                skipped += 1;
                continue;
            }
        }
        match entry.metadata().await {
            Ok(m) if sizes.contains(&m.len()) => found.push(path.display().to_string()),
            Ok(_) => {}
            Err(_) => skipped += 1,
        }
    }
    Ok((found, skipped))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn only_top_level_files_of_a_waited_size_are_candidates() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.jar"), b"12345").unwrap();
        std::fs::write(dir.path().join("b.jar"), b"123").unwrap();
        std::fs::write(dir.path().join("c.txt"), b"12345").unwrap();
        std::fs::create_dir_all(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/d.jar"), b"12345").unwrap();
        let sizes: std::collections::HashSet<u64> = [5].into();
        let (found, skipped) = candidates(dir.path(), &sizes).await.unwrap();
        assert_eq!(found.len(), 1);
        assert!(found[0].ends_with("a.jar"));
        assert_eq!(skipped, 0);
        assert!(
            candidates(&dir.path().join("missing"), &sizes)
                .await
                .is_err(),
            "unreadable ≠ empty"
        );
    }
}
