//! OS-divergent primitives, isolated behind one module so a later macOS
//! spec slots in by adding `cfg` arms here rather than across the codebase.
//!
//! Boundary: subprocess spawning stays in `process::` (the documented
//! chokepoint — Windows `taskkill` is a subprocess and lives there). This
//! module owns the NON-subprocess OS calls (chmod, POSIX signals, Win32
//! window detection) plus the dispatch entry points the launcher calls.
//! Enforced by `tests/structural_platform_chokepoint.rs`.

use std::path::Path;

/// True iff this platform supports in-app self-update (download + verify +
/// launch an installer). Windows-only today; Linux is check-and-notify and
/// macOS lands in a later spec.
pub fn supports_in_app_install() -> bool {
    cfg!(target_os = "windows")
}

/// Mark `path` executable. No-op on Windows (the `.exe`/`.dll` extensions
/// decide there); on Unix sets mode `0o755`. Propagates the IO error if the
/// metadata read or chmod fails — the caller turns it into a launch error.
#[cfg(unix)]
pub fn set_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms)
}

#[cfg(not(unix))]
pub fn set_executable(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Create a symlink at `link` pointing to `target` (a path relative to the
/// link's directory, as written in Mojang JRE manifests). Idempotent:
/// removes any existing entry first so re-install over an installed JRE
/// works. Unix-only — Windows JRE manifests never carry `link` entries, so
/// off Unix this returns `Unsupported` to fail loudly if one ever appears.
#[cfg(unix)]
pub fn symlink(target: &str, link: &Path) -> std::io::Result<()> {
    // Remove a stale entry so re-install is idempotent. Only "not found" is
    // benign — surface anything else (e.g. a directory in the way) so the
    // failure is diagnosable rather than masked by a later EEXIST.
    if let Err(e) = std::fs::remove_file(link) {
        if e.kind() != std::io::ErrorKind::NotFound {
            return Err(e);
        }
    }
    std::os::unix::fs::symlink(target, link)
}

#[cfg(not(unix))]
pub fn symlink(_target: &str, _link: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "symlink entries in a JRE manifest are not supported on this platform",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn set_executable_sets_owner_exec_bit_on_unix() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("java");
        std::fs::write(&f, b"#!/bin/sh\n").unwrap();
        // Start non-executable.
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();

        set_executable(&f).expect("set_executable ok");

        let mode = std::fs::metadata(&f).unwrap().permissions().mode();
        assert_ne!(mode & 0o111, 0, "expected an executable bit, got {mode:o}");
    }

    #[cfg(not(unix))]
    #[test]
    fn set_executable_is_noop_ok_on_non_unix() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("javaw.exe");
        std::fs::write(&f, b"x").unwrap();
        set_executable(&f).expect("no-op returns Ok on non-unix");
    }

    #[cfg(unix)]
    #[test]
    fn symlink_creates_link_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real.txt");
        std::fs::write(&target, b"hi").unwrap();
        let link = dir.path().join("alias.txt");

        symlink("real.txt", &link).expect("first symlink ok");
        assert_eq!(
            std::fs::read_link(&link).unwrap(),
            std::path::PathBuf::from("real.txt")
        );

        // Re-creating over an existing link must succeed (re-install case).
        symlink("real.txt", &link).expect("second symlink idempotent");
        assert_eq!(std::fs::read(&link).unwrap(), b"hi");
    }

    #[cfg(not(unix))]
    #[test]
    fn symlink_is_unsupported_off_unix() {
        let dir = tempfile::tempdir().unwrap();
        let err = symlink("target", &dir.path().join("alias")).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);
    }
}
