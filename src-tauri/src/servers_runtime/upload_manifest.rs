//! Resume manifest for a server SFTP upload (`upload-progress.json`).
//!
//! Persists the upload *target*, the planned file set with sizes, a per-file
//! `done` marker (set only after the remote `close` succeeds), and the rel path
//! of the file that was in flight when an upload was last interrupted. On the
//! next upload to the SAME target we can skip already-done files and re-upload
//! only the remainder (Section B of the hosting-hardening spec).
//!
//! This module owns the data model + IO + the pure resume decision; the actual
//! SFTP transfer lives in `transfer.rs` and only calls into here.

use crate::error::{Error, Result};
use crate::servers_runtime::schema::UploadConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The identity of an upload target. Two uploads are "the same target" iff all
/// four fields match — a change to any of them invalidates a resume.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UploadTarget {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub remote_path: String,
}

/// One planned file in the manifest: its forward-slash relative path, its local
/// byte size at plan time, and whether its remote `close` has succeeded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestFile {
    pub rel: String,
    pub size: u64,
    #[serde(default)]
    pub done: bool,
}

/// The on-disk resume manifest. `in_flight` records the file being written when
/// the upload was last interrupted (always re-uploaded from scratch on resume).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UploadManifest {
    pub target: UploadTarget,
    pub files: Vec<ManifestFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub in_flight: Option<String>,
    pub started_unix_ms: f64,
}

/// `<base>/servers/<id>/upload-progress.json`.
pub fn manifest_path(base: &Path, server_id: &str) -> PathBuf {
    crate::paths::server_paths(base, server_id)
        .root
        .join("upload-progress.json")
}

/// Derive the target identity from an upload config (host-key is irrelevant to
/// target identity — re-trusting a key does not invalidate a resume).
pub fn target_of(cfg: &UploadConfig) -> UploadTarget {
    UploadTarget {
        host: cfg.host.clone(),
        port: cfg.port,
        user: cfg.user.clone(),
        remote_path: cfg.remote_path.clone(),
    }
}

/// Read a manifest. Absent or corrupt → `None` (a corrupt manifest must never
/// block a fresh upload; treat it as "no resume available").
pub fn read_manifest(path: &Path) -> Option<UploadManifest> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Atomically persist a manifest (tmp + rename, mirroring `store.rs`).
pub fn write_manifest(path: &Path, m: &UploadManifest) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::io(path.display().to_string(), "no parent dir"))?;
    std::fs::create_dir_all(parent).map_err(|e| Error::io(parent.display().to_string(), e))?;
    let tmp = path.with_extension("tmp");
    let json = serde_json::to_string_pretty(m)
        .map_err(|e| Error::io(path.display().to_string(), format!("serialize: {e}")))?;
    std::fs::write(&tmp, json).map_err(|e| Error::io(tmp.display().to_string(), e))?;
    std::fs::rename(&tmp, path).map_err(|e| Error::io(path.display().to_string(), e))
}

/// Invalidate (delete) a manifest. Idempotent: a missing file is success.
pub fn delete_manifest(path: &Path) {
    let _ = std::fs::remove_file(path);
}

/// Build a fresh manifest for `target` over the enumerated file set (all files
/// start `done: false`, `in_flight: None`).
pub fn seed_manifest(target: UploadTarget, files: &[(PathBuf, String)]) -> UploadManifest {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0);
    UploadManifest {
        target,
        files: files
            .iter()
            .map(|(local, rel)| ManifestFile {
                rel: rel.clone(),
                size: std::fs::metadata(local).map(|m| m.len()).unwrap_or(0),
                done: false,
            })
            .collect(),
        in_flight: None,
        started_unix_ms: now,
    }
}

/// Pure resume decision. Returns the rel-paths of the files that must be
/// (re)uploaded — every file in `current` NOT returned is safe to skip.
///
/// A file is uploaded when ANY of:
///   * it is the recorded `in_flight` file (always re-uploaded from scratch);
///   * it is absent from the manifest (added since the plan was made);
///   * its manifest entry is not `done`;
///   * its remote `stat` size is missing or != the local size (torn file).
/// Conversely, a file is skipped only when it is `done` AND its remote size
/// matches the local size AND it is not the in-flight file.
pub fn plan_resume(
    manifest: &UploadManifest,
    current: &[(PathBuf, String)],
    remote_sizes: &HashMap<String, u64>,
) -> Vec<String> {
    let by_rel: HashMap<&str, &ManifestFile> =
        manifest.files.iter().map(|f| (f.rel.as_str(), f)).collect();
    current
        .iter()
        .filter_map(|(local, rel)| {
            if manifest.in_flight.as_deref() == Some(rel.as_str()) {
                return Some(rel.clone()); // always re-upload the interrupted file
            }
            let Some(entry) = by_rel.get(rel.as_str()) else {
                return Some(rel.clone()); // new file, not in the manifest
            };
            if !entry.done {
                return Some(rel.clone());
            }
            // `done` — verify the remote really has the whole file.
            let local_size = std::fs::metadata(local)
                .map(|m| m.len())
                .unwrap_or(entry.size);
            match remote_sizes.get(rel.as_str()) {
                Some(&remote) if remote == local_size => None, // intact → skip
                _ => Some(rel.clone()),                        // missing/mismatch → re-upload
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn cfg() -> UploadConfig {
        UploadConfig {
            host: "h.example.com".into(),
            port: 2222,
            user: "mc".into(),
            remote_path: "/srv/mc".into(),
            known_host_fp: Some("aa".into()),
            last_upload: None,
        }
    }

    #[test]
    fn target_of_ignores_host_key() {
        let mut a = cfg();
        let mut b = cfg();
        a.known_host_fp = Some("xx".into());
        b.known_host_fp = None;
        assert_eq!(target_of(&a), target_of(&b));
    }

    #[test]
    fn target_of_differs_on_remote_path() {
        let mut b = cfg();
        b.remote_path = "/srv/other".into();
        assert_ne!(target_of(&cfg()), target_of(&b));
    }

    #[test]
    fn manifest_path_is_under_server_root() {
        let p = manifest_path(Path::new("/data"), "srv-1");
        assert_eq!(p, Path::new("/data/servers/srv-1/upload-progress.json"));
    }

    #[test]
    fn seed_then_write_then_read_roundtrips() {
        let d = tempdir().unwrap();
        let rt = d.path();
        std::fs::write(rt.join("a.jar"), b"1234567890").unwrap(); // 10 bytes
        std::fs::write(rt.join("b.txt"), b"abc").unwrap(); // 3 bytes
        let files = vec![
            (rt.join("a.jar"), "a.jar".to_string()),
            (rt.join("b.txt"), "b.txt".to_string()),
        ];
        let m = seed_manifest(target_of(&cfg()), &files);
        assert_eq!(m.files.len(), 2);
        assert_eq!(m.files.iter().find(|f| f.rel == "a.jar").unwrap().size, 10);
        assert!(m.files.iter().all(|f| !f.done));
        assert!(m.in_flight.is_none());

        let path = manifest_path(d.path(), "srv-1");
        write_manifest(&path, &m).unwrap();
        let back = read_manifest(&path).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn read_missing_or_corrupt_is_none() {
        let d = tempdir().unwrap();
        let path = d.path().join("nope.json");
        assert!(read_manifest(&path).is_none());
        std::fs::write(&path, "{ not json").unwrap();
        assert!(read_manifest(&path).is_none());
    }

    #[test]
    fn delete_manifest_is_idempotent() {
        let d = tempdir().unwrap();
        let path = d.path().join("upload-progress.json");
        delete_manifest(&path); // absent — must not panic
        std::fs::write(&path, "{}").unwrap();
        delete_manifest(&path);
        assert!(!path.exists());
    }

    fn target() -> UploadTarget {
        target_of(&cfg())
    }

    fn manifest_with(files: Vec<(&str, u64, bool)>, in_flight: Option<&str>) -> UploadManifest {
        UploadManifest {
            target: target(),
            files: files
                .into_iter()
                .map(|(rel, size, done)| ManifestFile {
                    rel: rel.to_string(),
                    size,
                    done,
                })
                .collect(),
            in_flight: in_flight.map(|s| s.to_string()),
            started_unix_ms: 1.0,
        }
    }

    fn current(files: &[(&str, u64)]) -> Vec<(PathBuf, String)> {
        files
            .iter()
            .map(|(rel, _)| (PathBuf::from(format!("/local/{rel}")), rel.to_string()))
            .collect()
    }

    #[test]
    fn plan_resume_skips_done_uploads_rest() {
        let m = manifest_with(
            vec![
                ("a.jar", 10, true),
                ("b.txt", 3, false),
                ("c.dat", 5, false),
            ],
            None,
        );
        let cur = current(&[("a.jar", 10), ("b.txt", 3), ("c.dat", 5)]);
        let remote = HashMap::from([("a.jar".to_string(), 10u64)]);
        let mut out = plan_resume(&m, &cur, &remote);
        out.sort();
        assert_eq!(out, vec!["b.txt".to_string(), "c.dat".to_string()]);
    }

    #[test]
    fn plan_resume_reuploads_in_flight_even_if_marked_done() {
        // The in-flight file must always be re-uploaded — even if a stale `done`
        // flag claims it finished (it was being written when interrupted).
        let m = manifest_with(vec![("a.jar", 10, true), ("b.txt", 3, true)], Some("b.txt"));
        let cur = current(&[("a.jar", 10), ("b.txt", 3)]);
        let remote = HashMap::from([("a.jar".to_string(), 10u64), ("b.txt".to_string(), 3u64)]);
        let out = plan_resume(&m, &cur, &remote);
        assert_eq!(out, vec!["b.txt".to_string()]);
    }

    #[test]
    fn plan_resume_reuploads_size_mismatch() {
        // Torn last file: remote size != local size → re-upload despite `done`.
        let m = manifest_with(vec![("a.jar", 10, true)], None);
        let cur = current(&[("a.jar", 10)]);
        let remote = HashMap::from([("a.jar".to_string(), 7u64)]); // partial
        let out = plan_resume(&m, &cur, &remote);
        assert_eq!(out, vec!["a.jar".to_string()]);
    }

    #[test]
    fn plan_resume_reuploads_when_remote_missing() {
        // Marked done but no remote stat (file gone / never closed) → re-upload.
        let m = manifest_with(vec![("a.jar", 10, true)], None);
        let cur = current(&[("a.jar", 10)]);
        let remote = HashMap::new();
        let out = plan_resume(&m, &cur, &remote);
        assert_eq!(out, vec!["a.jar".to_string()]);
    }

    #[test]
    fn plan_resume_includes_new_files_absent_from_manifest() {
        // A file added since the manifest was written must be uploaded.
        let m = manifest_with(vec![("a.jar", 10, true)], None);
        let cur = current(&[("a.jar", 10), ("new.txt", 2)]);
        let remote = HashMap::from([("a.jar".to_string(), 10u64)]);
        let mut out = plan_resume(&m, &cur, &remote);
        out.sort();
        assert_eq!(out, vec!["new.txt".to_string()]);
    }
}
