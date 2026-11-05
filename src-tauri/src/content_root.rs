//! What sits at a content root — `mods/`, `config/`, `saves/`, `resourcepacks/`,
//! `shaderpacks/`, `options.txt` — inspected WITHOUT following a link at the
//! root itself.
//!
//! The rule every import, clone, export and server→instance copy follows: a
//! link (symlink, or a Windows junction) at a category root IS the instance's
//! content — the game loads through it — so it is copied as real files and
//! every preview says where it points. A link whose target is missing, or a
//! root that cannot be read, is "could not tell", never "absent": the preview
//! shows it and a copy refuses. Links INSIDE a category tree are a different
//! matter and stay skipped by the walks.

use std::io;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// What is at a path, the path itself not followed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootKind {
    Absent,
    Dir,
    File,
    /// A symbolic link or junction. `target` is what it names (a relative
    /// target is joined to the link's parent).
    Linked {
        target: PathBuf,
        resolves_to: Resolved,
    },
    /// A fifo, socket or device.
    Other,
}

/// What the link at `link` names, as a path: `named` is what `read_link`
/// returned, and a relative answer is relative to the link's parent.
pub fn link_target(link: &Path, named: PathBuf) -> PathBuf {
    match link.parent() {
        Some(parent) if named.is_relative() => parent.join(named),
        _ => named,
    }
}

/// What a link resolves to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolved {
    Dir,
    File,
    Missing,
    Other,
}

/// What is at `path`, without following a link at `path` itself. `Absent`
/// only for `NotFound`; any other failure to state the path — or a link's
/// target — is `Err`, "could not tell".
pub fn inspect(path: &Path) -> io::Result<RootKind> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(RootKind::Absent),
        Err(e) => return Err(e),
    };
    let ft = meta.file_type();
    // True for a Windows junction too: its mount-point tag is a name
    // surrogate, which std reports as a symlink (data_root/migrate.rs relies
    // on the same fact).
    if ft.is_symlink() {
        let target = link_target(path, std::fs::read_link(path)?);
        let resolves_to = match std::fs::metadata(path) {
            Ok(m) if m.is_dir() => Resolved::Dir,
            Ok(m) if m.is_file() => Resolved::File,
            Ok(_) => Resolved::Other,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Resolved::Missing,
            Err(e) => return Err(e),
        };
        return Ok(RootKind::Linked {
            target,
            resolves_to,
        });
    }
    Ok(if ft.is_dir() {
        RootKind::Dir
    } else if ft.is_file() {
        RootKind::File
    } else {
        RootKind::Other
    })
}

/// The kind a category's root must have: `options.txt` is a file, the rest
/// are folders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expect {
    Dir,
    File,
}

/// What a category root holds, judged by the category's kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Content {
    /// Absent, the wrong kind (a file named `mods`), or a fifo/socket/device.
    Nothing,
    Local,
    Linked {
        target: PathBuf,
    },
    BrokenLink {
        target: PathBuf,
    },
}

pub fn content_at(path: &Path, want: Expect) -> io::Result<Content> {
    let wanted = |dir: bool, file: bool| match want {
        Expect::Dir => dir,
        Expect::File => file,
    };
    Ok(match inspect(path)? {
        RootKind::Dir if wanted(true, false) => Content::Local,
        RootKind::File if wanted(false, true) => Content::Local,
        RootKind::Linked {
            target,
            resolves_to: Resolved::Missing,
        } => Content::BrokenLink { target },
        RootKind::Linked {
            target,
            resolves_to,
        } if wanted(resolves_to == Resolved::Dir, resolves_to == Resolved::File) => {
            Content::Linked { target }
        }
        // Absent, a fifo/socket/device, or the wrong kind for this category.
        RootKind::Absent
        | RootKind::Other
        | RootKind::Dir
        | RootKind::File
        | RootKind::Linked { .. } => Content::Nothing,
    })
}

/// For a copy: `Ok(true)` — content to copy, real or through a link;
/// `Ok(false)` — nothing of this kind is there; `Err` — could not tell:
/// [`Error::LinkTargetMissing`] for a link whose target is missing, or
/// [`Error::Io`] naming `path` when it cannot be stated. The error is built
/// here, where the path and the kind are known, so a caller only needs `?`.
pub fn has_content(path: &Path, want: Expect) -> Result<bool> {
    match content_at(path, want).map_err(|e| Error::io(path.display().to_string(), e))? {
        Content::Nothing => Ok(false),
        Content::Local | Content::Linked { .. } => Ok(true),
        Content::BrokenLink { target } => Err(link_target_missing(path, &target, want)),
    }
}

/// The refusal for the link at `link` whose `target` is missing. Its own
/// variant, never an I/O error or `NotFound`: a caller that reads `NotFound`
/// as "absent" must not read a broken link that way, and the user is told
/// what the link is and where it points, not that something failed.
pub fn link_target_missing(link: &Path, target: &Path, want: Expect) -> Error {
    Error::LinkTargetMissing {
        path: display(link),
        target: display(target),
        is_file: want == Expect::File,
    }
}

/// A target path as the user should read it: `\\?\C:\x` → `C:\x`,
/// `\\?\UNC\srv\sh` → `\\srv\sh` (a no-op off Windows). Display only — the
/// result is never handed back to the file system — so the verbatim prefix
/// goes even where `dunce::simplified` keeps it (it leaves UNC paths alone).
pub fn display(target: &Path) -> String {
    let shown = target.display().to_string();
    #[cfg(windows)]
    {
        if let Some(rest) = shown.strip_prefix(r"\\?\UNC\") {
            return format!(r"\\{rest}");
        }
        if let Some(rest) = shown.strip_prefix(r"\\?\") {
            return rest.to_string();
        }
    }
    shown
}

/// Real links for tests: a Unix symlink, or a Windows directory junction,
/// which needs no privilege (unlike a Windows symlink).
#[cfg(test)]
pub(crate) mod test_links {
    use std::path::{Path, PathBuf};

    /// Link `link` → `target` (a directory).
    pub fn link_dir(target: &Path, link: &Path) {
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, link).unwrap();
        #[cfg(windows)]
        junction::create(target, link).unwrap();
    }

    /// A path whose `symlink_metadata` fails with an error OTHER than
    /// `NotFound` — "could not tell": a path under a regular file on Unix
    /// (`ENOTDIR`), a name holding `<` on Windows (`ERROR_INVALID_NAME`).
    pub fn unstattable(base: &Path) -> PathBuf {
        #[cfg(unix)]
        {
            let file = base.join("not-a-dir");
            std::fs::write(&file, b"x").unwrap();
            file.join("x")
        }
        #[cfg(windows)]
        {
            base.join("bad<name")
        }
    }

    /// Both sides canonicalized: temp paths differ by 8.3 short names on
    /// Windows and by `/private` on macOS.
    pub fn same_dir(a: &Path, b: &Path) -> bool {
        crate::pathsafe::ResolvedPath::resolve(a).unwrap()
            == crate::pathsafe::ResolvedPath::resolve(b).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::test_links::{link_dir, same_dir, unstattable};
    use super::*;

    #[test]
    fn content_root_absent_dir_and_file() {
        let td = tempfile::tempdir().unwrap();
        let file = td.path().join("options.txt");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(inspect(&td.path().join("nope")).unwrap(), RootKind::Absent);
        assert_eq!(inspect(td.path()).unwrap(), RootKind::Dir);
        assert_eq!(inspect(&file).unwrap(), RootKind::File);
    }

    #[test]
    fn content_root_linked_dir_resolves_to_dir() {
        let td = tempfile::tempdir().unwrap();
        let pool = td.path().join("pool");
        std::fs::create_dir(&pool).unwrap();
        let link = td.path().join("resourcepacks");
        link_dir(&pool, &link);
        match inspect(&link).unwrap() {
            RootKind::Linked {
                target,
                resolves_to,
            } => {
                assert_eq!(resolves_to, Resolved::Dir);
                assert!(
                    same_dir(&target, &pool),
                    "target {target:?} is not the pool"
                );
            }
            other => panic!("a link must read as Linked, got {other:?}"),
        }
    }

    #[test]
    fn content_root_broken_link_resolves_to_missing() {
        let td = tempfile::tempdir().unwrap();
        let pool = td.path().join("pool");
        std::fs::create_dir(&pool).unwrap();
        let link = td.path().join("resourcepacks");
        link_dir(&pool, &link);
        std::fs::remove_dir(&pool).unwrap();
        let got = inspect(&link).unwrap();
        assert!(
            matches!(
                got,
                RootKind::Linked {
                    resolves_to: Resolved::Missing,
                    ..
                }
            ),
            "a link to a missing folder is a broken link, not absent: {got:?}"
        );
    }

    #[test]
    fn content_root_unstattable_path_is_err_not_absent() {
        let td = tempfile::tempdir().unwrap();
        let got = inspect(&unstattable(td.path()));
        assert!(got.is_err(), "could not tell must not read as {got:?}");
    }

    #[test]
    fn content_root_has_content_by_kind() {
        let td = tempfile::tempdir().unwrap();
        let dir = td.path().join("mods");
        std::fs::create_dir(&dir).unwrap();
        let file = td.path().join("options.txt");
        std::fs::write(&file, b"x").unwrap();
        assert!(has_content(&dir, Expect::Dir).unwrap());
        assert!(
            !has_content(&dir, Expect::File).unwrap(),
            "a folder is not options.txt"
        );
        assert!(has_content(&file, Expect::File).unwrap());
        assert!(
            !has_content(&file, Expect::Dir).unwrap(),
            "a file is not a mods folder"
        );
        assert!(!has_content(&td.path().join("nope"), Expect::Dir).unwrap());

        let pool = td.path().join("pool");
        std::fs::create_dir(&pool).unwrap();
        let link = td.path().join("saves");
        link_dir(&pool, &link);
        assert!(has_content(&link, Expect::Dir).unwrap());
        std::fs::remove_dir(&pool).unwrap();
        let err = has_content(&link, Expect::Dir).expect_err("a broken link is refused");
        let Error::LinkTargetMissing {
            path,
            target,
            is_file,
        } = err
        else {
            panic!("a broken link is its own refusal, not an I/O error: {err:?}");
        };
        assert_eq!(path, display(&link), "names the link");
        // Not compared whole: a junction may store the temp dir's long form
        // where the test holds the 8.3 one, and the pool is gone, so it cannot
        // be canonicalized either.
        assert!(
            Path::new(&target).ends_with("pool") && !target.starts_with(r"\\?\"),
            "names where it points, as the user reads it: {target}"
        );
        assert!(!is_file, "a folder root");
    }

    #[test]
    fn content_root_has_content_unstattable_is_io_naming_the_path() {
        let td = tempfile::tempdir().unwrap();
        let bad = unstattable(td.path());
        let err = has_content(&bad, Expect::Dir).expect_err("could not tell");
        assert!(
            matches!(&err, Error::Io { path, .. } if *path == bad.display().to_string()),
            "an unstattable root stays an I/O error on its path: {err:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn content_root_broken_link_file_root_is_a_file() {
        let td = tempfile::tempdir().unwrap();
        let real = td.path().join("shared-options.txt");
        let link = td.path().join("options.txt");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let err = has_content(&link, Expect::File).expect_err("a broken link is refused");
        assert!(
            matches!(err, Error::LinkTargetMissing { is_file: true, .. }),
            "options.txt is worded as a file: {err:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn content_root_self_loop_symlink_is_err() {
        let td = tempfile::tempdir().unwrap();
        let link = td.path().join("mods");
        std::os::unix::fs::symlink(&link, &link).unwrap();
        assert!(inspect(&link).is_err(), "ELOOP is could-not-tell");
    }

    #[cfg(unix)]
    #[test]
    fn content_root_symlinked_file_resolves_to_file() {
        let td = tempfile::tempdir().unwrap();
        let real = td.path().join("shared-options.txt");
        std::fs::write(&real, b"x").unwrap();
        let link = td.path().join("options.txt");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(matches!(
            inspect(&link).unwrap(),
            RootKind::Linked {
                resolves_to: Resolved::File,
                ..
            }
        ));
    }

    #[cfg(windows)]
    #[test]
    fn content_root_display_strips_the_verbatim_prefix() {
        assert_eq!(display(Path::new(r"\\?\C:\Games\pool")), r"C:\Games\pool");
        assert_eq!(
            display(Path::new(r"\\?\UNC\srv\share\pool")),
            r"\\srv\share\pool"
        );
    }
}
