//! Server export + SFTP upload. ALL SSH/SFTP client construction lives in this
//! module (enforced by `tests/structural_no_raw_sftp.rs`): a user-initiated
//! outbound channel to the user's OWN server, sanctioned per docs/PRINCIPLES.md.

use crate::error::{Error, Result};
use std::path::{Path, PathBuf};

/// Files to upload: recursively under `runtime`, EXCLUDING the `logs/` dir and
/// the one-shot `installer.jar`. Returns (local absolute path, remote relative
/// path with forward slashes).
pub(crate) fn enumerate_upload_files(runtime: &Path) -> Result<Vec<(PathBuf, String)>> {
    let mut out = Vec::new();
    walk(runtime, runtime, &mut out)?;
    Ok(out)
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<(PathBuf, String)>) -> Result<()> {
    let rd = std::fs::read_dir(dir).map_err(|e| Error::io(dir.display().to_string(), e))?;
    for entry in rd.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if name == "logs" {
                continue;
            }
            walk(root, &path, out)?;
        } else if name != "installer.jar" {
            let rel = path
                .strip_prefix(root)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            out.push((path, rel));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn enumerate_excludes_logs_and_installer() {
        let d = tempdir().unwrap();
        let rt = d.path();
        std::fs::create_dir_all(rt.join("mods")).unwrap();
        std::fs::create_dir_all(rt.join("logs")).unwrap();
        std::fs::write(rt.join("server.jar"), b"j").unwrap();
        std::fs::write(rt.join("installer.jar"), b"i").unwrap();
        std::fs::write(rt.join("mods/a.jar"), b"a").unwrap();
        std::fs::write(rt.join("logs/server-latest.log"), b"l").unwrap();
        let mut got: Vec<String> = enumerate_upload_files(rt)
            .unwrap()
            .into_iter()
            .map(|(_local, rel)| rel)
            .collect();
        got.sort();
        assert_eq!(
            got,
            vec!["mods/a.jar".to_string(), "server.jar".to_string()]
        );
    }

    #[test]
    fn enumerate_missing_dir_errors_or_empty() {
        // A non-existent runtime dir: read_dir fails → Err (acceptable; caller
        // never calls this on a missing dir in practice).
        let d = tempdir().unwrap();
        let r = enumerate_upload_files(&d.path().join("nope"));
        assert!(r.is_err());
    }
}
