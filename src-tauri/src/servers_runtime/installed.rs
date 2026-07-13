//! Per-server installed-mods/plugins sidecar registry.
//!
//! Mirrors `crate::mods::installed` (the client per-instance registry) for a
//! server's jar directory (`runtime/mods/` for mod loaders, `runtime/plugins/`
//! for Paper/Purpur cores). Deltas from the client registry:
//!   * No `enabled` field on the record — activation is the on-disk `.jar` vs
//!     `.jar.disabled` suffix, resolved at list time and returned as
//!     `ServerInstalledEntry::enabled`.
//!   * No `requires` edge list — server orphan detection is out of scope.
//!   * Synchronous `std::fs`, matching the sibling `servers_runtime` modules
//!     (`plugins.rs`, `quarantine.rs`, `store.rs`).
//!
//! Keyed by sha1 (lowercased) so identity survives an enable/disable rename.
//! Fail-open: a missing/corrupt sidecar is treated as empty and rebuilt from
//! disk by `reconcile_on_list` — never a hard error.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

use crate::error::{Error, Result};
use crate::mods::platform::ModSource;

const FILE_VERSION: u32 = 1;
const SIDECAR: &str = ".lucerna-installed.json";

/// Process-lifetime write counter. Mirrors `mods::installed`: a unique per-write
/// temp name so concurrent `save()` calls for the same dir don't collide on the
/// tmp path and fail the rename.
static WRITE_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq)]
pub struct ServerInstalledRecord {
    pub filename: String,
    pub sha1: String,
    pub source: Option<ModSource>,
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    pub name: Option<String>,
    pub version_number: Option<String>,
    #[serde(default)]
    pub enrich_attempted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq)]
pub struct ServerInstalledEntry {
    pub record: ServerInstalledRecord,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Sidecar {
    #[serde(default = "default_version")]
    version: u32,
    #[serde(default)]
    records: Vec<ServerInstalledRecord>,
}

fn default_version() -> u32 {
    FILE_VERSION
}

fn sidecar_path(jar_dir: &Path) -> PathBuf {
    jar_dir.join(SIDECAR)
}

pub fn load(jar_dir: &Path) -> Vec<ServerInstalledRecord> {
    let Ok(bytes) = std::fs::read(sidecar_path(jar_dir)) else {
        return Vec::new();
    };
    serde_json::from_slice::<Sidecar>(&bytes)
        .map(|s| s.records)
        .unwrap_or_default()
}

pub fn save(jar_dir: &Path, records: &[ServerInstalledRecord]) -> Result<()> {
    std::fs::create_dir_all(jar_dir).map_err(|e| Error::io(jar_dir.display().to_string(), e))?;
    let final_path = sidecar_path(jar_dir);
    let seq = WRITE_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = final_path.with_extension(format!("json.tmp.{}.{seq}", std::process::id()));
    let sidecar = Sidecar {
        version: FILE_VERSION,
        records: records.to_vec(),
    };
    let bytes = serde_json::to_vec_pretty(&sidecar)
        .map_err(|e| Error::io(final_path.display().to_string(), e))?;
    std::fs::write(&tmp, &bytes).map_err(|e| Error::io(tmp.display().to_string(), e))?;
    std::fs::rename(&tmp, &final_path).map_err(|e| Error::io(final_path.display().to_string(), e))
}

pub fn sha1_of(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).map_err(|e| Error::io(path.display().to_string(), e))?;
    Ok(hex::encode(Sha1::digest(bytes)))
}

fn scan_dir(jar_dir: &Path) -> Result<Vec<(String, String, bool)>> {
    let mut on_disk: Vec<(String, String, bool)> = Vec::new();
    let Ok(rd) = std::fs::read_dir(jar_dir) else {
        return Ok(on_disk);
    };
    for entry in rd.flatten() {
        match entry.metadata() {
            Ok(m) if m.is_file() => {}
            _ => continue,
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let (enabled, base_name) = if let Some(stripped) = name.strip_suffix(".disabled") {
            if !stripped.ends_with(".jar") {
                continue;
            }
            (false, stripped.to_string())
        } else if name.ends_with(".jar") {
            (true, name.clone())
        } else {
            continue;
        };
        let bytes = std::fs::read(entry.path())
            .map_err(|e| Error::io(entry.path().display().to_string(), e))?;
        on_disk.push((base_name, hex::encode(Sha1::digest(&bytes)), enabled));
    }
    Ok(on_disk)
}

pub fn reconcile_on_list(jar_dir: &Path) -> Result<Vec<ServerInstalledEntry>> {
    let on_disk = scan_dir(jar_dir)?;
    let mut records = load(jar_dir);
    let mut changed = false;

    for r in records.iter_mut() {
        if let Some((disk_name, _, _)) = on_disk
            .iter()
            .find(|(_, sha, _)| sha.eq_ignore_ascii_case(&r.sha1))
        {
            if r.filename != *disk_name {
                r.filename = disk_name.clone();
                changed = true;
            }
        }
    }

    let disk_shas: HashSet<String> = on_disk
        .iter()
        .map(|(_, s, _)| s.to_ascii_lowercase())
        .collect();
    let before = records.len();
    records.retain(|r| disk_shas.contains(&r.sha1.to_ascii_lowercase()));
    if records.len() != before {
        changed = true;
    }

    let mut known: HashSet<String> = records
        .iter()
        .map(|r| r.sha1.to_ascii_lowercase())
        .collect();
    for (filename, sha, _enabled) in on_disk.iter() {
        let key = sha.to_ascii_lowercase();
        if !known.contains(&key) {
            records.push(ServerInstalledRecord {
                filename: filename.clone(),
                sha1: sha.clone(),
                source: None,
                project_id: None,
                version_id: None,
                name: None,
                version_number: None,
                enrich_attempted: false,
            });
            known.insert(key);
            changed = true;
        }
    }

    if changed {
        save(jar_dir, &records)?;
    }

    let by_sha: HashMap<String, ServerInstalledRecord> = records
        .into_iter()
        .map(|r| (r.sha1.to_ascii_lowercase(), r))
        .collect();
    let mut entries: Vec<ServerInstalledEntry> = on_disk
        .into_iter()
        .filter_map(|(_, sha, enabled)| {
            by_sha
                .get(&sha.to_ascii_lowercase())
                .cloned()
                .map(|record| ServerInstalledEntry { record, enabled })
        })
        .collect();
    entries.sort_by(|a, b| {
        a.record
            .filename
            .to_ascii_lowercase()
            .cmp(&b.record.filename.to_ascii_lowercase())
    });
    Ok(entries)
}

pub fn upsert(jar_dir: &Path, record: ServerInstalledRecord) -> Result<()> {
    let mut records = load(jar_dir);
    records.retain(|r| !r.sha1.eq_ignore_ascii_case(&record.sha1));
    records.push(record);
    save(jar_dir, &records)
}

pub fn remove(jar_dir: &Path, sha1: &str) -> Result<()> {
    let mut records = load(jar_dir);
    let before = records.len();
    records.retain(|r| !r.sha1.eq_ignore_ascii_case(sha1));
    if records.len() != before {
        save(jar_dir, &records)?;
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct ResolvedServerIdentity {
    pub source: ModSource,
    pub project_id: String,
    pub version_id: Option<String>,
    pub name: Option<String>,
    pub version_number: Option<String>,
}

pub fn apply_enrichment(
    jar_dir: &Path,
    resolved: &HashMap<String, ResolvedServerIdentity>,
    attempted: &HashSet<String>,
) -> Result<()> {
    let mut records = load(jar_dir);
    for r in records.iter_mut() {
        let key = r.sha1.to_ascii_lowercase();
        if let Some(id) = resolved.get(&key) {
            r.source = Some(id.source);
            r.project_id = Some(id.project_id.clone());
            r.version_id = id.version_id.clone();
            r.name = id.name.clone();
            r.version_number = id.version_number.clone();
        }
        if attempted.contains(&key) {
            r.enrich_attempted = true;
        }
    }
    save(jar_dir, &records)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(filename: &str, sha: &str) -> ServerInstalledRecord {
        ServerInstalledRecord {
            filename: filename.into(),
            sha1: sha.into(),
            source: Some(ModSource::Modrinth),
            project_id: Some("proj".into()),
            version_id: Some("ver".into()),
            name: Some("Nice Mod".into()),
            version_number: Some("1.2.3".into()),
            enrich_attempted: true,
        }
    }

    #[test]
    fn load_is_fail_open_on_missing_and_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(dir.path()).is_empty());
        std::fs::write(sidecar_path(dir.path()), b"{ not json").unwrap();
        assert!(load(dir.path()).is_empty());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let records = vec![rec("a.jar", "aa"), rec("b.jar", "bb")];
        save(dir.path(), &records).unwrap();
        assert_eq!(load(dir.path()), records);
    }

    fn write_jar(dir: &Path, name: &str, bytes: &[u8]) -> String {
        std::fs::write(dir.join(name), bytes).unwrap();
        hex::encode(Sha1::digest(bytes))
    }

    #[test]
    fn reconcile_synthesizes_untracked_and_derives_enabled_from_suffix() {
        let dir = tempfile::tempdir().unwrap();
        let sha_on = write_jar(dir.path(), "on.jar", b"AAAA");
        let sha_off = write_jar(dir.path(), "off.jar.disabled", b"BBBB");
        let entries = reconcile_on_list(dir.path()).unwrap();
        let on = entries.iter().find(|e| e.record.sha1 == sha_on).unwrap();
        let off = entries.iter().find(|e| e.record.sha1 == sha_off).unwrap();
        assert!(on.enabled && on.record.filename == "on.jar" && on.record.source.is_none());
        assert!(!off.enabled && off.record.filename == "off.jar");
    }

    #[test]
    fn reconcile_keeps_identity_across_disable_rename() {
        let dir = tempfile::tempdir().unwrap();
        let sha = write_jar(dir.path(), "m.jar", b"CCCC");
        upsert(
            dir.path(),
            ServerInstalledRecord {
                filename: "m.jar".into(),
                sha1: sha.clone(),
                source: Some(ModSource::Modrinth),
                project_id: Some("p".into()),
                version_id: Some("v".into()),
                name: None,
                version_number: None,
                enrich_attempted: false,
            },
        )
        .unwrap();
        std::fs::rename(dir.path().join("m.jar"), dir.path().join("m.jar.disabled")).unwrap();
        let entries = reconcile_on_list(dir.path()).unwrap();
        let e = entries.iter().find(|e| e.record.sha1 == sha).unwrap();
        assert!(!e.enabled);
        assert_eq!(e.record.project_id.as_deref(), Some("p"));
        assert_eq!(e.record.filename, "m.jar");
    }

    #[test]
    fn reconcile_drops_stale_records() {
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), &[rec("gone.jar", "deadbeef")]).unwrap();
        let entries = reconcile_on_list(dir.path()).unwrap();
        assert!(entries.is_empty());
        assert!(load(dir.path()).is_empty());
    }

    #[test]
    fn remove_deletes_by_sha() {
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), &[rec("a.jar", "aa"), rec("b.jar", "bb")]).unwrap();
        remove(dir.path(), "AA").unwrap();
        let shas: Vec<_> = load(dir.path()).into_iter().map(|r| r.sha1).collect();
        assert_eq!(shas, vec!["bb".to_string()]);
    }

    #[test]
    fn apply_enrichment_fills_identity_and_flag() {
        let dir = tempfile::tempdir().unwrap();
        let sha = write_jar(dir.path(), "u.jar", b"DDDD");
        reconcile_on_list(dir.path()).unwrap();
        let mut resolved = std::collections::HashMap::new();
        resolved.insert(
            sha.clone(),
            ResolvedServerIdentity {
                source: ModSource::Modrinth,
                project_id: "pid".into(),
                version_id: Some("vid".into()),
                name: Some("Cool".into()),
                version_number: Some("2.0".into()),
            },
        );
        let attempted: std::collections::HashSet<String> = [sha.clone()].into_iter().collect();
        apply_enrichment(dir.path(), &resolved, &attempted).unwrap();
        let r = load(dir.path())
            .into_iter()
            .find(|r| r.sha1 == sha)
            .unwrap();
        assert_eq!(r.source, Some(ModSource::Modrinth));
        assert_eq!(r.project_id.as_deref(), Some("pid"));
        assert!(r.enrich_attempted);
    }

    #[test]
    fn reconcile_is_noop_when_stable() {
        let dir = tempfile::tempdir().unwrap();
        write_jar(dir.path(), "stable.jar", b"STABLE");
        // First pass synthesizes + persists the sidecar.
        reconcile_on_list(dir.path()).unwrap();
        // Make the sidecar read-only: a 2nd pass that tried to persist would
        // fail the rename. A genuine no-op never touches it → Ok.
        let sc = sidecar_path(dir.path());
        let mut perms = std::fs::metadata(&sc).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&sc, perms).unwrap();
        assert!(reconcile_on_list(dir.path()).is_ok());
        // Restore writability so tempdir cleanup can remove it.
        let mut perms = std::fs::metadata(&sc).unwrap().permissions();
        perms.set_readonly(false);
        std::fs::set_permissions(&sc, perms).unwrap();
    }

    #[test]
    fn synthesis_dedups_identical_sha() {
        let dir = tempfile::tempdir().unwrap();
        let sha_a = write_jar(dir.path(), "a.jar", b"SAME");
        let sha_b = write_jar(dir.path(), "b.jar", b"SAME");
        assert_eq!(sha_a, sha_b);
        reconcile_on_list(dir.path()).unwrap();
        let matching: Vec<_> = load(dir.path())
            .into_iter()
            .filter(|r| r.sha1 == sha_a)
            .collect();
        assert_eq!(
            matching.len(),
            1,
            "identical-sha jars must dedup to a single record"
        );
    }

    #[test]
    fn scan_dir_skips_non_jar_disabled() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("notes.txt.disabled"), b"x").unwrap();
        let entries = reconcile_on_list(dir.path()).unwrap();
        assert!(entries.is_empty());
        assert!(load(dir.path()).is_empty());
    }

    #[test]
    fn upsert_replaces_same_sha() {
        let dir = tempfile::tempdir().unwrap();
        let mut r = rec("x.jar", "aa");
        r.name = Some("old".into());
        upsert(dir.path(), r).unwrap();
        let mut r2 = rec("x.jar", "aa");
        r2.name = Some("new".into());
        upsert(dir.path(), r2).unwrap();
        let records = load(dir.path());
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].name.as_deref(), Some("new"));
    }
}
