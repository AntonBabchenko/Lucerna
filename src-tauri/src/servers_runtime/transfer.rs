//! Server export + SFTP upload. ALL SSH/SFTP client construction lives in this
//! module (enforced by `tests/structural_no_raw_sftp.rs`): a user-initiated
//! outbound channel to the user's OWN server, sanctioned per docs/PRINCIPLES.md.

use crate::error::{Error, Result};
use crate::servers_runtime::schema::UploadConfig;
use serde::Serialize;
use sha2::{Digest, Sha256};
use specta::Type;
use std::io::Write;
use std::path::{Path, PathBuf};
use tauri::AppHandle;
use tauri_specta::Event;
use tokio::io::AsyncWriteExt;
use zip::write::SimpleFileOptions;
use zip::CompressionMethod;

/// Progress for an in-flight SFTP server upload, emitted once per file as it is
/// written. `files_done` counts files completed including the current one.
#[derive(Debug, Clone, Serialize, Type, Event)]
pub struct ServerUploadProgress {
    pub server_id: String,
    pub current_file: String,
    pub files_done: u32,
    pub files_total: u32,
}

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
        let ft = entry
            .file_type()
            .map_err(|e| Error::io(path.display().to_string(), e))?;
        if ft.is_symlink() {
            continue; // don't follow symlinks (cycle/escape safety)
        }
        if ft.is_dir() {
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

/// SSH client handler. Captures the server's host-key fingerprint at key
/// exchange so the caller can make the TOFU decision *after* the transport is
/// up but *before* any password is sent. `check_server_key` accepts the
/// transport unconditionally (returns `Ok(true)`) — the trust decision is the
/// caller's, not the transport's; rejecting here would tear the connection
/// down before we could surface a useful host-key-mismatch error.
struct CaptureHostKey {
    captured_fp: std::sync::Arc<std::sync::Mutex<Option<String>>>,
}

impl russh::client::Handler for CaptureHostKey {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::ssh_key::PublicKey,
    ) -> std::result::Result<bool, Self::Error> {
        // SSH wire-format encoding of the public key → stable SHA-256 hex
        // fingerprint. `to_bytes` only fails on an encoding bug, not on
        // attacker input; treat a failure as "no fingerprint" so the caller's
        // TOFU check rejects rather than silently trusting.
        let fp = server_public_key
            .to_bytes()
            .ok()
            .map(|bytes| host_key_fingerprint(&bytes));
        if let Ok(mut slot) = self.captured_fp.lock() {
            *slot = fp;
        }
        Ok(true)
    }
}

/// Upload the server `runtime` directory to the configured SFTP target.
///
/// Security ordering is load-bearing: connect → capture host-key fingerprint →
/// TOFU decision → (only then) send the password → open SFTP → upload. On a
/// changed/unknown host key with `accept_new_host_key == false` we bail with
/// [`Error::SftpHostKeyMismatch`] **before** the password leaves the process.
///
/// Returns `Some(fingerprint)` when the host key is being trusted for the first
/// time or re-trusted (so the caller can persist it in `UploadConfig`), or
/// `None` when the already-known fingerprint matched. The password is never
/// logged, emitted, or stored by this function.
pub async fn upload_server(
    app: &AppHandle,
    server_id: &str,
    cfg: &UploadConfig,
    password: &str,
    accept_new_host_key: bool,
) -> Result<Option<String>> {
    let base = crate::paths::app_dir(app).map_err(|e| Error::io("<app_dir>", e))?;
    let runtime = crate::paths::server_paths(&base, server_id).runtime;
    let files = enumerate_upload_files(&runtime)?;

    // --- connect + capture host-key fingerprint ---
    let captured_fp = std::sync::Arc::new(std::sync::Mutex::new(None));
    let handler = CaptureHostKey {
        captured_fp: captured_fp.clone(),
    };
    let config = std::sync::Arc::new(russh::client::Config::default());
    let mut session = russh::client::connect(config, (cfg.host.as_str(), cfg.port), handler)
        .await
        .map_err(|e| Error::SftpConnectFailed {
            details: e.to_string(),
        })?;

    let seen_fp = captured_fp
        .lock()
        .ok()
        .and_then(|slot| slot.clone())
        .ok_or_else(|| Error::SftpConnectFailed {
            details: "server presented no usable host key".to_string(),
        })?;

    // --- TOFU decision BEFORE sending the password ---
    if !host_key_decision(cfg.known_host_fp.as_deref(), &seen_fp) && !accept_new_host_key {
        return Err(Error::SftpHostKeyMismatch {
            expected: cfg.known_host_fp.clone().unwrap_or_default(),
            got: seen_fp,
        });
    }
    let new_fp = if cfg.known_host_fp.as_deref() != Some(seen_fp.as_str()) {
        Some(seen_fp)
    } else {
        None
    };

    // --- authenticate (password) ---
    let auth = session
        .authenticate_password(&cfg.user, password)
        .await
        .map_err(|e| Error::SftpAuthFailed {
            details: e.to_string(),
        })?;
    if !auth.success() {
        return Err(Error::SftpAuthFailed {
            details: "password rejected by server".to_string(),
        });
    }

    // --- open the SFTP subsystem ---
    let channel = session
        .channel_open_session()
        .await
        .map_err(|e| Error::SftpTransferFailed {
            details: e.to_string(),
        })?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|e| Error::SftpTransferFailed {
            details: e.to_string(),
        })?;
    let sftp = russh_sftp::client::SftpSession::new(channel.into_stream())
        .await
        .map_err(|e| Error::SftpTransferFailed {
            details: e.to_string(),
        })?;

    // --- upload each file, creating remote dirs as needed ---
    let remote_root = cfg.remote_path.trim_end_matches('/');
    let total = files.len() as u32;
    for (i, (local, rel)) in files.iter().enumerate() {
        ensure_remote_dirs(&sftp, remote_root, rel).await;

        let data = std::fs::read(local).map_err(|e| Error::io(local.display().to_string(), e))?;
        let remote = format!("{remote_root}/{rel}");
        let mut file =
            sftp.create(remote.clone())
                .await
                .map_err(|e| Error::SftpTransferFailed {
                    details: format!("{remote}: {e}"),
                })?;
        file.write_all(&data)
            .await
            .map_err(|e| Error::SftpTransferFailed {
                details: format!("{remote}: {e}"),
            })?;
        file.shutdown()
            .await
            .map_err(|e| Error::SftpTransferFailed {
                details: format!("{remote}: {e}"),
            })?;

        let _ = ServerUploadProgress {
            server_id: server_id.to_string(),
            current_file: rel.clone(),
            files_done: i as u32 + 1,
            files_total: total,
        }
        .emit(app);
    }

    Ok(new_fp)
}

/// Best-effort `mkdir -p` of the parent directories for `rel` under
/// `remote_root`. SFTP has no atomic recursive mkdir, so we create each segment
/// in turn and ignore errors: an already-existing directory reports `Failure`
/// on most servers, and a directory we truly cannot create surfaces later as a
/// real error when the file `create` fails.
async fn ensure_remote_dirs(sftp: &russh_sftp::client::SftpSession, remote_root: &str, rel: &str) {
    let mut segments: Vec<&str> = rel.split('/').collect();
    segments.pop(); // drop the file name; keep only directory components
    let mut acc = remote_root.to_string();
    for seg in segments {
        if seg.is_empty() {
            continue;
        }
        acc.push('/');
        acc.push_str(seg);
        let _ = sftp.create_dir(acc.clone()).await;
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

    /// Regular files still enumerate correctly and symlinks inside the runtime
    /// directory are not descended or included.
    ///
    /// On platforms that support symlink creation (all major OS) we create a
    /// real symlink and assert it is absent from the output. On any platform
    /// where symlink creation fails (e.g. Windows without the SeCreateSymbolicLink
    /// privilege) we fall back to asserting that the regular file still appears,
    /// confirming the core enumeration path is unaffected by the symlink-skip branch.
    #[test]
    fn enumerate_skips_symlinks() {
        let d = tempdir().unwrap();
        let rt = d.path();
        std::fs::write(rt.join("real.jar"), b"r").unwrap();

        // Attempt to create a symlink file; ignore if the OS/privilege denies it.
        #[cfg(unix)]
        let symlink_created =
            std::os::unix::fs::symlink(rt.join("real.jar"), rt.join("link.jar")).is_ok();
        #[cfg(windows)]
        let symlink_created =
            std::os::windows::fs::symlink_file(rt.join("real.jar"), rt.join("link.jar")).is_ok();
        #[cfg(not(any(unix, windows)))]
        let symlink_created = false;

        let got: Vec<String> = enumerate_upload_files(rt)
            .unwrap()
            .into_iter()
            .map(|(_local, rel)| rel)
            .collect();

        // The real file must always appear.
        assert!(
            got.iter().any(|r| r.ends_with("real.jar")),
            "real file missing from output"
        );

        // If the symlink was created, it must NOT appear in the output.
        if symlink_created {
            assert!(
                !got.iter().any(|r| r.ends_with("link.jar")),
                "symlink should not appear in upload enumeration"
            );
        }
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
