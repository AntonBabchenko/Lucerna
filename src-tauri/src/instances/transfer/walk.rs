//! The strict walk a transfer and a clone's mod-data copy share (spec §A5):
//! links and special files inside a tree are counted and never followed, and
//! an entry that cannot be read is an error — never a silent skip.

use std::io;
use std::path::{Path, PathBuf};

use super::catalog::Scope;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalkedFile {
    /// `/`-separated, relative to the game dir.
    pub rel: String,
    pub len: u64,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Walked {
    pub files: Vec<WalkedFile>,
    /// Links and special files inside the tree: not followed, not copied.
    pub skipped: u32,
}

impl Walked {
    pub fn bytes(&self) -> u64 {
        self.files.iter().map(|f| f.len).sum()
    }
}

/// Walk `<game_dir>/<rel>` — which the caller found to be a folder, real or
/// reached through a link at the root — by `scope`.
pub fn walk_root(game_dir: &Path, rel: &str, scope: Scope) -> io::Result<Walked> {
    let _ = (game_dir, rel, scope);
    todo!("red-first stub: the walk lands in the next commit")
}

/// Join a `/`-separated relative path segment by segment.
pub fn join_rel(base: &Path, rel: &str) -> PathBuf {
    rel.split('/')
        .filter(|seg| !seg.is_empty())
        .fold(base.to_path_buf(), |path, seg| path.join(seg))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content_root::test_links::link_dir;

    fn write(base: &Path, rel: &str, bytes: &[u8]) {
        let p = join_rel(base, rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }

    fn rels(w: &Walked) -> Vec<String> {
        let mut r: Vec<String> = w.files.iter().map(|f| f.rel.clone()).collect();
        r.sort();
        r
    }

    #[test]
    fn whole_tree_lists_nested_files_with_slash_paths() {
        let td = tempfile::tempdir().unwrap();
        write(td.path(), "schematics/a.litematic", b"aa");
        write(td.path(), "schematics/sub/b.nbt", b"b");
        let w = walk_root(td.path(), "schematics", Scope::WholeTree).unwrap();
        assert_eq!(rels(&w), ["schematics/a.litematic", "schematics/sub/b.nbt"]);
        assert_eq!(w.bytes(), 3);
    }

    #[test]
    fn child_folders_takes_only_matching_folders() {
        let td = tempfile::tempdir().unwrap();
        write(
            td.path(),
            "xaero/minimap/Multiplayer_mc.example.net/w.txt",
            b"w",
        );
        write(td.path(), "xaero/minimap/Realms_42/r.txt", b"r");
        write(td.path(), "xaero/minimap/New World/sp.txt", b"s");
        write(td.path(), "xaero/minimap/config.txt", b"c");
        let w = walk_root(
            td.path(),
            "xaero/minimap",
            Scope::ChildFolders(crate::instances::transfer::catalog::MULTIPLAYER),
        )
        .unwrap();
        assert_eq!(
            rels(&w),
            [
                "xaero/minimap/Multiplayer_mc.example.net/w.txt",
                "xaero/minimap/Realms_42/r.txt"
            ]
        );
    }

    #[test]
    fn top_level_files_skip_subfolders() {
        let td = tempfile::tempdir().unwrap();
        write(td.path(), "replay_recordings/a.mcpr", b"a");
        write(td.path(), "replay_recordings/recording/tmp.mcpr", b"t");
        let w = walk_root(td.path(), "replay_recordings", Scope::TopLevelFiles).unwrap();
        assert_eq!(rels(&w), ["replay_recordings/a.mcpr"]);
    }

    #[test]
    fn an_inner_link_is_counted_and_not_followed() {
        let td = tempfile::tempdir().unwrap();
        let outside = td.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), b"s").unwrap();
        write(td.path(), "schematics/a.nbt", b"a");
        link_dir(&outside, &td.path().join("schematics/linked"));
        let w = walk_root(td.path(), "schematics", Scope::WholeTree).unwrap();
        assert_eq!(rels(&w), ["schematics/a.nbt"]);
        assert_eq!(w.skipped, 1);
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_inner_folder_is_an_error_not_a_skip() {
        let td = tempfile::tempdir().unwrap();
        write(td.path(), "schematics/locked/a.nbt", b"a");
        let locked = td.path().join("schematics/locked");
        crate::platform::set_mode_for_test(&locked, 0o000).unwrap();
        let got = walk_root(td.path(), "schematics", Scope::WholeTree);
        crate::platform::set_mode_for_test(&locked, 0o755).unwrap();
        assert!(got.is_err(), "{got:?}");
    }
}
