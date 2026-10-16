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
    let base = join_rel(game_dir, rel);
    let mut out = Walked::default();
    match scope {
        Scope::WholeTree => walk_tree(&base, rel, &mut out)?,
        Scope::ChildFolders(prefixes) => {
            for (name, kind) in list(&base)? {
                if !prefixes.iter().any(|p| name.starts_with(p)) {
                    continue;
                }
                match kind {
                    Kind::Dir => walk_tree(&base.join(&name), &format!("{rel}/{name}"), &mut out)?,
                    Kind::File(_) => {}
                    Kind::Skipped => out.skipped += 1,
                }
            }
        }
        Scope::TopLevelFiles => {
            for (name, kind) in list(&base)? {
                match kind {
                    Kind::File(len) => out.files.push(WalkedFile {
                        rel: format!("{rel}/{name}"),
                        len,
                    }),
                    Kind::Dir => {}
                    Kind::Skipped => out.skipped += 1,
                }
            }
        }
    }
    Ok(out)
}

enum Kind {
    Dir,
    File(u64),
    Skipped,
}

/// `dir`'s entries, each classified without following a link, sorted by
/// name. A name that is not valid Unicode is an error: the transaction records
/// paths as strings, and a lossy name would name another file.
fn list(dir: &Path) -> io::Result<Vec<(String, Kind)>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|e| at(dir, e))? {
        let entry = entry.map_err(|e| at(dir, e))?;
        let path = entry.path();
        let name = portable_name(&path, entry.file_name())?;
        let meta = std::fs::symlink_metadata(&path).map_err(|e| at(&path, e))?;
        let ft = meta.file_type();
        // `is_symlink` is true for a Windows junction too.
        let kind = if ft.is_symlink() {
            Kind::Skipped
        } else if ft.is_dir() {
            Kind::Dir
        } else if ft.is_file() {
            Kind::File(meta.len())
        } else {
            Kind::Skipped
        };
        out.push((name, kind));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// `name` as the string the transaction records. Not valid Unicode: a lossy
/// name would name another file. A backslash: a folder separator on Windows,
/// so the file would land elsewhere there (and the transaction refuses such a
/// path mid-commit; said here, before anything is staged).
fn portable_name(path: &Path, name: std::ffi::OsString) -> io::Result<String> {
    let name = name.into_string().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{}: the name is not valid Unicode", path.display()),
        )
    })?;
    if name.contains('\\') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{}: the name has a backslash, which Windows reads as a folder separator",
                path.display()
            ),
        ));
    }
    Ok(name)
}

fn walk_tree(dir: &Path, rel: &str, out: &mut Walked) -> io::Result<()> {
    let mut stack = vec![(dir.to_path_buf(), rel.to_string())];
    while let Some((d, r)) = stack.pop() {
        for (name, kind) in list(&d)? {
            match kind {
                Kind::Dir => stack.push((d.join(&name), format!("{r}/{name}"))),
                Kind::File(len) => out.files.push(WalkedFile {
                    rel: format!("{r}/{name}"),
                    len,
                }),
                Kind::Skipped => out.skipped += 1,
            }
        }
    }
    Ok(())
}

fn at(path: &Path, e: io::Error) -> io::Error {
    io::Error::new(e.kind(), format!("{}: {e}", path.display()))
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

    /// A backslash is a folder separator on Windows: such a name would land
    /// somewhere else there, and no transaction can record it. Said at the
    /// walk, not deep in the commit.
    #[test]
    fn a_backslash_name_is_refused_on_every_system() {
        let err = portable_name(Path::new("x/a\\b.nbt"), "a\\b.nbt".into()).expect_err("refused");
        assert!(err.to_string().contains("backslash"), "{err}");
        assert_eq!(
            portable_name(Path::new("x/a.nbt"), "a.nbt".into()).unwrap(),
            "a.nbt"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_name_with_a_backslash_is_an_error() {
        let td = tempfile::tempdir().unwrap();
        write(td.path(), "schematics/a\\b.nbt", b"a");
        let got = walk_root(td.path(), "schematics", Scope::WholeTree);
        let err = got.expect_err("refused");
        assert!(err.to_string().contains("backslash"), "{err}");
    }
}
