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
        let named = std::fs::read_link(path)?;
        let target = match path.parent() {
            Some(parent) if named.is_relative() => parent.join(named),
            _ => named,
        };
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
/// `Ok(false)` — nothing of this kind is there; `Err` — could not tell,
/// including a link whose target is missing.
pub fn has_content(path: &Path, want: Expect) -> io::Result<bool> {
    match content_at(path, want)? {
        Content::Nothing => Ok(false),
        Content::Local | Content::Linked { .. } => Ok(true),
        Content::BrokenLink { target } => Err(broken_link_error(&target)),
    }
}

/// The error for a link whose target is missing. Deliberately NOT `NotFound`:
/// a caller that discriminates `NotFound` as "absent" must not read a broken
/// link that way.
pub fn broken_link_error(target: &Path) -> io::Error {
    io::Error::other(format!("links to a missing target: {}", display(target)))
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
        dunce::canonicalize(a).unwrap() == dunce::canonicalize(b).unwrap()
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
        assert_ne!(
            err.kind(),
            io::ErrorKind::NotFound,
            "must not read as absent"
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
