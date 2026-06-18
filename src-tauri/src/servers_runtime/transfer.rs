//! Server export + SFTP upload. ALL SSH/SFTP client construction lives in this
//! module (enforced by `tests/structural_no_raw_sftp.rs`): a user-initiated
//! outbound channel to the user's OWN server, sanctioned per docs/PRINCIPLES.md.

use crate::error::{Error, Result};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;
use zip::CompressionMethod;

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

/// SHA-256 hex fingerprint of a host public key's raw bytes.
pub(crate) fn host_key_fingerprint(public_key_bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(public_key_bytes))
}

/// Zip the server `runtime` directory (minus `logs/` and `installer.jar`) into
/// `dest`. Entry names are forward-slash relative paths, matching the set
/// produced by [`enumerate_upload_files`].
pub(crate) fn export_zip(runtime: &Path, dest: &Path) -> Result<()> {
    let file = std::fs::File::create(dest).map_err(|e| Error::io(dest.display().to_string(), e))?;
    let mut zw = zip::ZipWriter::new(std::io::BufWriter::new(file));
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (local, rel) in enumerate_upload_files(runtime)? {
        zw.start_file(&rel, opts)
            .map_err(|e| Error::io(rel.clone(), format!("zip start: {e}")))?;
        let bytes = std::fs::read(&local).map_err(|e| Error::io(local.display().to_string(), e))?;
        zw.write_all(&bytes)
            .map_err(|e| Error::io(rel.clone(), format!("zip write: {e}")))?;
    }
    zw.finish()
        .map_err(|e| Error::io(dest.display().to_string(), format!("zip finish: {e}")))?;
    Ok(())
}

/// TOFU decision: accept iff first use (`known` is None) or the fingerprint
/// matches the stored one. A changed key is rejected (caller surfaces
/// `SftpHostKeyMismatch`; the user may explicitly re-trust).
pub(crate) fn host_key_decision(known: Option<&str>, current: &str) -> bool {
    match known {
        None => true,
        Some(k) => k == current,
    }
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

    #[test]
    fn fingerprint_is_stable_sha256_hex() {
        let key = b"ssh-ed25519 AAAArealkeybytes";
        let fp = host_key_fingerprint(key);
        assert_eq!(fp.len(), 64);
        assert_eq!(fp, host_key_fingerprint(key));
        assert_ne!(fp, host_key_fingerprint(b"different"));
    }
    #[test]
    fn host_key_decision_tofu() {
        assert!(host_key_decision(None, "abc")); // first use → accept
        assert!(host_key_decision(Some("abc"), "abc")); // same → accept
        assert!(!host_key_decision(Some("abc"), "xyz")); // changed → reject
    }

    #[test]
    fn export_zip_excludes_logs_and_installer() {
        let d = tempdir().unwrap();
        let rt = d.path().join("runtime");
        std::fs::create_dir_all(rt.join("logs")).unwrap();
        std::fs::write(rt.join("server.jar"), b"j").unwrap();
        std::fs::write(rt.join("installer.jar"), b"i").unwrap();
        std::fs::write(rt.join("logs/x.log"), b"l").unwrap();
        let dest = d.path().join("export.zip");
        export_zip(&rt, &dest).unwrap();
        let f = std::fs::File::open(&dest).unwrap();
        let mut z = zip::ZipArchive::new(f).unwrap();
        let names: Vec<String> = (0..z.len())
            .map(|i| z.by_index(i).unwrap().name().to_string())
            .collect();
        assert!(names.iter().any(|n| n.ends_with("server.jar")));
        assert!(!names.iter().any(|n| n.contains("logs/")));
        assert!(!names.iter().any(|n| n.ends_with("installer.jar")));
    }
}
