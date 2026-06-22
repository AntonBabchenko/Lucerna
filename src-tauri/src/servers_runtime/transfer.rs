//! Server export + SFTP upload. ALL SSH/SFTP client construction lives in this
//! module (enforced by `tests/structural_no_raw_sftp.rs`): a user-initiated
//! outbound channel to the user's OWN server, sanctioned per docs/PRINCIPLES.md.

use crate::error::{Error, Result};
use crate::servers_runtime::schema::UploadConfig;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use specta::Type;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::AppHandle;
use tauri_specta::Event;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use zip::write::SimpleFileOptions;
use zip::CompressionMethod;

/// How the SFTP upload authenticates (#28). Stored in an S4-owned sidecar
/// (`upload-auth.json`) next to the server, NOT in `server.json` (whose
/// `UploadConfig` is owned by another stream). The secret itself — the password,
/// or the passphrase of an encrypted key — lives in the OS keyring, never here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum UploadAuthMethod {
    /// Password authentication (the default; back-compat for existing configs).
    #[default]
    Password,
    /// OpenSSH private-key authentication (for key-only hosts).
    Key,
}

/// Persisted SFTP auth method + optional private-key path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, Default)]
pub struct UploadAuth {
    #[serde(default)]
    pub method: UploadAuthMethod,
    /// Absolute path to the OpenSSH private key (when `method == Key`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_key_path: Option<String>,
}

fn upload_auth_path(base: &Path, server_id: &str) -> PathBuf {
    crate::paths::server_paths(base, server_id)
        .root
        .join("upload-auth.json")
}

/// Read a server's SFTP auth method. Absent/invalid → password (back-compat).
pub fn read_upload_auth(base: &Path, server_id: &str) -> UploadAuth {
    std::fs::read_to_string(upload_auth_path(base, server_id))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Persist a server's SFTP auth method (creates the server root if needed).
pub fn write_upload_auth(base: &Path, server_id: &str, auth: &UploadAuth) -> Result<()> {
    let path = upload_auth_path(base, server_id);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| Error::io(dir.display().to_string(), e))?;
    }
    let json = serde_json::to_string_pretty(auth)
        .map_err(|e| Error::io(path.display().to_string(), format!("auth: {e}")))?;
    std::fs::write(&path, json).map_err(|e| Error::io(path.display().to_string(), e))
}

/// Host-key fingerprint surfaced to the user on first connect (#24). `trusted`
/// is true iff this exact fingerprint is already the stored TOFU value.
#[derive(Debug, Clone, Serialize, Type)]
pub struct HostKeyPreview {
    /// SHA-256 hex fingerprint of the server's host key.
    pub fingerprint: String,
    /// Whether this fingerprint is already trusted (matches the stored one).
    pub trusted: bool,
}

/// Progress for an in-flight SFTP server upload, emitted once per file as it is
/// written. `files_done` counts files completed including the current one.
#[derive(Debug, Clone, Serialize, Type, Event)]
pub struct ServerUploadProgress {
    pub server_id: String,
    pub current_file: String,
    pub files_done: u32,
    pub files_total: u32,
    /// f64 not u64 — specta forbids BigInt-style exports. 2^53 bytes (8 PiB) is
    /// far beyond any plausible server-runtime upload size.
    pub bytes_done: f64,
    /// f64 not u64 — specta forbids BigInt-style exports (see `bytes_done`).
    pub bytes_total: f64,
}

/// Files never uploaded or zipped, regardless of directory: the one-shot
/// `installer.jar` and ephemeral lock files Minecraft/loaders leave behind
/// (`session.lock` guards an open world; `.lock` is used by some loaders).
/// Uploading these races a live world or imports a stale lock onto the host.
/// Centralised so SFTP upload (`upload_server`) and ZIP export (`export_zip`)
/// — both routed through `enumerate_upload_files`/`walk` — stay in sync.
const EXCLUDED_FILE_NAMES: &[&str] = &["installer.jar", "session.lock", ".lock"];

/// Directories never descended into, regardless of depth.
const EXCLUDED_DIR_NAMES: &[&str] = &["logs"];

/// True iff a file with this exact name must be excluded from the set.
pub(crate) fn is_excluded_file(name: &str) -> bool {
    EXCLUDED_FILE_NAMES.contains(&name)
}

/// True iff a directory with this exact name must be skipped (not descended).
fn is_excluded_dir(name: &str) -> bool {
    EXCLUDED_DIR_NAMES.contains(&name)
}

/// Files to upload: recursively under `runtime`, EXCLUDING the `logs/` dir and
/// the one-shot `installer.jar`. Returns (local absolute path, remote relative
/// path with forward slashes).
pub(crate) fn enumerate_upload_files(runtime: &Path) -> Result<Vec<(PathBuf, String)>> {
    let mut out = Vec::new();
    walk(runtime, runtime, &mut out)?;
    Ok(out)
}

/// Sum of byte sizes of the enumerated upload set (missing/unreadable files
/// count as 0; the real `create`/`copy` later surfaces any true error).
pub(crate) fn upload_total_bytes(files: &[(PathBuf, String)]) -> u64 {
    files
        .iter()
        .map(|(local, _)| std::fs::metadata(local).map(|m| m.len()).unwrap_or(0))
        .sum()
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
            if is_excluded_dir(&name) {
                continue;
            }
            walk(root, &path, out)?;
        } else if !is_excluded_file(&name) {
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

/// Connect to the SFTP target and capture the server's host-key fingerprint at
/// key exchange — BEFORE any password/key is sent. Shared by the host-key
/// preview (#24) and the upload, so both observe the key the same way.
async fn connect_capture(
    cfg: &UploadConfig,
) -> Result<(russh::client::Handle<CaptureHostKey>, String)> {
    let captured_fp = Arc::new(std::sync::Mutex::new(None));
    let handler = CaptureHostKey {
        captured_fp: captured_fp.clone(),
    };
    let config = Arc::new(russh::client::Config::default());
    let session = russh::client::connect(config, (cfg.host.as_str(), cfg.port), handler)
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
    Ok((session, seen_fp))
}

/// Connect and return the server's host-key fingerprint for the user to verify
/// against their provider on first connect (#24). No password or key is sent —
/// the session is dropped immediately after key exchange. `trusted` reflects
/// whether this fingerprint already matches the stored TOFU value.
pub async fn preview_host_key(cfg: &UploadConfig) -> Result<HostKeyPreview> {
    let (_session, fingerprint) = connect_capture(cfg).await?;
    let trusted = cfg.known_host_fp.as_deref() == Some(fingerprint.as_str());
    Ok(HostKeyPreview {
        fingerprint,
        trusted,
    })
}

/// Authenticate the SSH session by the configured method (#28). `secret` is the
/// password (password auth) or the key passphrase (key auth; empty = none).
async fn authenticate(
    session: &mut russh::client::Handle<CaptureHostKey>,
    cfg: &UploadConfig,
    auth: &UploadAuth,
    secret: &str,
) -> Result<()> {
    let ok = match auth.method {
        UploadAuthMethod::Password => session
            .authenticate_password(&cfg.user, secret)
            .await
            .map_err(|e| Error::SftpAuthFailed {
                details: e.to_string(),
            })?
            .success(),
        UploadAuthMethod::Key => {
            let key_path =
                auth.private_key_path
                    .as_deref()
                    .ok_or_else(|| Error::SftpAuthFailed {
                        details: "key auth selected but no private-key path is set".to_string(),
                    })?;
            let passphrase = (!secret.is_empty()).then_some(secret);
            let key = russh::keys::load_secret_key(key_path, passphrase).map_err(|e| {
                Error::SftpAuthFailed {
                    details: format!("load private key: {e}"),
                }
            })?;
            // For RSA keys, negotiate the best hash the server advertises
            // (modern servers reject the legacy SHA-1 `ssh-rsa`); ignored for
            // ed25519/ecdsa keys.
            let hash_alg = session
                .best_supported_rsa_hash()
                .await
                .ok()
                .flatten()
                .flatten();
            session
                .authenticate_publickey(
                    &cfg.user,
                    russh::keys::PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg),
                )
                .await
                .map_err(|e| Error::SftpAuthFailed {
                    details: e.to_string(),
                })?
                .success()
        }
    };
    if !ok {
        return Err(Error::SftpAuthFailed {
            details: "server rejected the credentials".to_string(),
        });
    }
    Ok(())
}

/// Upload the server `runtime` directory to the configured SFTP target.
///
/// Security ordering is load-bearing: connect → capture host-key fingerprint →
/// TOFU decision → (only then) send the password → open SFTP → upload. On a
/// changed/unknown host key with `accept_new_host_key == false` we bail with
/// [`Error::SftpHostKeyMismatch`] **before** the password leaves the process.
///
/// A newly-trusted host key is persisted to `server.json` immediately (before
/// auth/transfer), so an interrupted or cancelled transfer doesn't re-prompt the
/// host-key dialog. `auth` selects password or key authentication; `secret` is
/// the password or key passphrase. The secret is never logged, emitted, or
/// stored by this function.
///
/// Files are streamed to the remote in fixed-size chunks rather than buffered in
/// memory, so a multi-GB world no longer risks OOM (#28). `cancel` is polled
/// before each file and between chunks; a flipped flag aborts with
/// [`Error::UploadCancelled`] (partially-written files are left on the host).
pub async fn upload_server(
    app: &AppHandle,
    server_id: &str,
    cfg: &UploadConfig,
    auth: &UploadAuth,
    secret: &str,
    accept_new_host_key: bool,
    cancel: &std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<()> {
    let base = crate::paths::app_dir(app).map_err(|e| Error::io("<app_dir>", e))?;
    let runtime = crate::paths::server_paths(&base, server_id).runtime;
    let files = enumerate_upload_files(&runtime)?;

    // --- connect + capture host-key fingerprint ---
    let (mut session, seen_fp) = connect_capture(cfg).await?;

    // --- TOFU decision BEFORE sending any credential ---
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

    // Persist a newly-trusted host key NOW (before auth/transfer), so an
    // interrupted/cancelled transfer doesn't re-prompt the host-key dialog.
    if let Some(fp) = new_fp {
        let p = crate::paths::server_paths(&base, server_id);
        if let Ok(mut f2) = crate::servers_runtime::store::read_server_json(&p.json) {
            if let Some(u) = f2.upload.as_mut() {
                u.known_host_fp = Some(fp);
                let _ = crate::servers_runtime::store::write_server_json(&p.json, &f2);
            }
        }
    }

    // --- authenticate (password or key) ---
    authenticate(&mut session, cfg, auth, secret).await?;

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

    // --- stream each file, creating remote dirs as needed ---
    let remote_root = cfg.remote_path.trim_end_matches('/');
    let total = files.len() as u32;
    let bytes_total = upload_total_bytes(&files);
    let mut bytes_done: u64 = 0;
    for (i, (local, rel)) in files.iter().enumerate() {
        ensure_remote_dirs(&sftp, remote_root, rel).await;

        let remote = format!("{remote_root}/{rel}");
        let mut local_file = tokio::fs::File::open(local)
            .await
            .map_err(|e| Error::io(local.display().to_string(), e))?;
        let mut remote_file =
            sftp.create(remote.clone())
                .await
                .map_err(|e| Error::SftpTransferFailed {
                    details: format!("{remote}: {e}"),
                })?;
        // Streamed copy in fixed-size chunks: constant memory regardless of file
        // size (#28), with cancellation polled before and between chunks.
        if cancel.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(Error::UploadCancelled);
        }
        let mut buf = vec![0u8; 256 * 1024];
        loop {
            if cancel.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(Error::UploadCancelled);
            }
            let n = local_file
                .read(&mut buf)
                .await
                .map_err(|e| Error::io(local.display().to_string(), e))?;
            if n == 0 {
                break;
            }
            remote_file
                .write_all(&buf[..n])
                .await
                .map_err(|e| Error::SftpTransferFailed {
                    details: format!("{remote}: {e}"),
                })?;
            bytes_done += n as u64;
        }
        remote_file
            .shutdown()
            .await
            .map_err(|e| Error::SftpTransferFailed {
                details: format!("{remote}: {e}"),
            })?;

        let _ = ServerUploadProgress {
            server_id: server_id.to_string(),
            current_file: rel.clone(),
            files_done: i as u32 + 1,
            files_total: total,
            bytes_done: bytes_done as f64,
            bytes_total: bytes_total as f64,
        }
        .emit(app);
    }

    Ok(())
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
    fn upload_total_bytes_sums_enumerated_files() {
        let d = tempdir().unwrap();
        let rt = d.path();
        std::fs::create_dir_all(rt.join("logs")).unwrap();
        std::fs::write(rt.join("a.jar"), b"1234567890").unwrap(); // 10
        std::fs::write(rt.join("b.txt"), b"abc").unwrap(); // 3
        std::fs::write(rt.join("installer.jar"), b"x").unwrap(); // excluded
        std::fs::write(rt.join("logs/s.log"), b"yyyy").unwrap(); // excluded
        let files = enumerate_upload_files(rt).unwrap();
        assert_eq!(upload_total_bytes(&files), 13);
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
    fn upload_auth_defaults_to_password() {
        let a = UploadAuth::default();
        assert_eq!(a.method, UploadAuthMethod::Password);
        assert!(a.private_key_path.is_none());
    }

    #[test]
    fn upload_auth_absent_sidecar_reads_as_password() {
        let base = tempdir().unwrap();
        let a = read_upload_auth(base.path(), "srv-1");
        assert_eq!(a.method, UploadAuthMethod::Password);
    }

    #[test]
    fn upload_auth_sidecar_roundtrips() {
        let base = tempdir().unwrap();
        let auth = UploadAuth {
            method: UploadAuthMethod::Key,
            private_key_path: Some("/home/me/.ssh/id_ed25519".into()),
        };
        write_upload_auth(base.path(), "srv-1", &auth).unwrap();
        assert_eq!(read_upload_auth(base.path(), "srv-1"), auth);
    }

    #[test]
    fn upload_auth_method_serializes_snake_case() {
        let json = serde_json::to_string(&UploadAuthMethod::Key).unwrap();
        assert_eq!(json, "\"key\"");
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

    #[test]
    fn enumerate_excludes_session_lock_and_ephemeral_locks() {
        let d = tempdir().unwrap();
        let rt = d.path();
        std::fs::create_dir_all(rt.join("world")).unwrap();
        std::fs::write(rt.join("server.jar"), b"j").unwrap();
        std::fs::write(rt.join("session.lock"), b"s").unwrap();
        std::fs::write(rt.join(".lock"), b"l").unwrap();
        std::fs::write(rt.join("world/session.lock"), b"ws").unwrap();
        let mut got: Vec<String> = enumerate_upload_files(rt)
            .unwrap()
            .into_iter()
            .map(|(_local, rel)| rel)
            .collect();
        got.sort();
        assert_eq!(got, vec!["server.jar".to_string()]);
        assert!(!got.iter().any(|r| r.ends_with("session.lock")));
        assert!(!got.iter().any(|r| r.ends_with(".lock")));
    }

    #[test]
    fn is_excluded_file_matches_lock_files() {
        assert!(is_excluded_file("installer.jar"));
        assert!(is_excluded_file("session.lock"));
        assert!(is_excluded_file(".lock"));
        assert!(!is_excluded_file("server.jar"));
        assert!(!is_excluded_file("server.lock")); // only exact names are excluded
        assert!(!is_excluded_file("my.session.lock")); // only exact names
    }

    #[test]
    fn export_zip_excludes_session_lock() {
        let d = tempdir().unwrap();
        let rt = d.path().join("runtime");
        std::fs::create_dir_all(rt.join("world")).unwrap();
        std::fs::write(rt.join("server.jar"), b"j").unwrap();
        std::fs::write(rt.join("session.lock"), b"s").unwrap();
        std::fs::write(rt.join(".lock"), b"l").unwrap();
        std::fs::write(rt.join("world/session.lock"), b"ws").unwrap();
        let dest = d.path().join("export.zip");
        export_zip(&rt, &dest).unwrap();
        let f = std::fs::File::open(&dest).unwrap();
        let mut z = zip::ZipArchive::new(f).unwrap();
        let names: Vec<String> = (0..z.len())
            .map(|i| z.by_index(i).unwrap().name().to_string())
            .collect();
        assert!(names.iter().any(|n| n.ends_with("server.jar")));
        assert!(!names.iter().any(|n| n.ends_with("session.lock")));
        assert!(!names.iter().any(|n| n.ends_with(".lock")));
    }
}
