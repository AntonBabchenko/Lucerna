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
//! Fail-open only for what is PROVEN: an absent sidecar, or one that read fine
//! but does not parse, is treated as empty and rebuilt from disk by
//! `reconcile_on_list`. A sidecar that exists but cannot be READ is an error:
//! every consumer feeds `load`'s result into a read-modify-write `save`, so
//! reading ignorance as "empty" would save away every record's identity
//! metadata (source / project_id / version_id / name / version_number).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::SystemTime;

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

/// Read the sidecar registry, discriminating "absent" from "could not read".
///
/// Absent (`NotFound`) is a fact — a dir that never had anything installed
/// holds no sidecar, and "no records" is the true answer. Any OTHER read
/// failure (permission, AV hold, sharing violation) is ignorance, and is an
/// error: the callers all read-modify-write, so treating it as empty would
/// persist the loss. A parse failure on successfully read bytes stays
/// fail-open — the file is provably corrupt, and `reconcile_on_list` rebuilds
/// it from disk (see the module doc).
pub fn load(jar_dir: &Path) -> Result<Vec<ServerInstalledRecord>> {
    let path = sidecar_path(jar_dir);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(Error::io(path.display().to_string(), e)),
    };
    Ok(serde_json::from_slice::<Sidecar>(&bytes)
        .map(|s| s.records)
        .unwrap_or_default())
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

/// SHA-1 of a jar, also seeding [`HASH_CACHE`] so the next `reconcile_on_list`
/// over the same unchanged file answers from memory instead of re-reading it.
/// The install / update paths call this right after writing a jar and the UI
/// re-lists immediately — without the seeding that jar is read twice.
///
/// The stat is taken BEFORE the read on purpose: if the file changes mid-read we
/// store the digest under the OLD `(mtime, size)`, and the next lookup stats
/// fresh metadata, misses, and re-hashes. Seeding is best-effort — a failed stat
/// just leaves the slow path in place, exactly as it behaves today.
///
/// Server analogue of `crate::mods::installed::seed_hash_cache`, placed inside
/// the hashing function so every call site is covered rather than each one
/// having to remember.
pub fn sha1_of(path: &Path) -> Result<String> {
    let stat = std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok().map(|mtime| (mtime, m.len())));
    let bytes = std::fs::read(path).map_err(|e| Error::io(path.display().to_string(), e))?;
    let sha = hex::encode(Sha1::digest(bytes));
    if let Some((mtime, size)) = stat {
        let mut cache = HASH_CACHE.lock().unwrap_or_else(|p| p.into_inner());
        cache.insert(path.to_path_buf(), (mtime, size, sha.clone()));
    }
    Ok(sha)
}

/// Process-lifetime SHA-1 cache keyed by path, re-using the stored digest when
/// a jar's (mtime, size) are unchanged — turning a full read+hash into a cheap
/// stat on repeat lists. Mirrors `crate::mods::installed`'s HASH_CACHE. The
/// cache lives for the process (dropped on restart, which is fine — a rescan
/// simply repopulates it).
static HASH_CACHE: LazyLock<Mutex<HashMap<PathBuf, (SystemTime, u64, String)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// SHA-1 of the file at `path`, re-using the cached digest when `(mtime, size)`
/// are unchanged since it was last hashed. Only reads+hashes on a miss. Sync
/// mirror of `crate::mods::installed::cached_sha1` (this module is `std::fs`).
fn cached_sha1(path: &Path, mtime: SystemTime, size: u64) -> Result<String> {
    {
        let cache = HASH_CACHE.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((m, s, sha)) = cache.get(path) {
            if *m == mtime && *s == size {
                return Ok(sha.clone());
            }
        }
    }
    let bytes = std::fs::read(path).map_err(|e| Error::io(path.display().to_string(), e))?;
    let sha = hex::encode(Sha1::digest(&bytes));
    {
        let mut cache = HASH_CACHE.lock().unwrap_or_else(|p| p.into_inner());
        cache.insert(path.to_path_buf(), (mtime, size, sha.clone()));
    }
    Ok(sha)
}

fn scan_dir(jar_dir: &Path) -> Result<Vec<(String, String, bool)>> {
    let mut on_disk: Vec<(String, String, bool)> = Vec::new();
    let rd = match std::fs::read_dir(jar_dir) {
        Ok(rd) => rd,
        // Absent is a fact: a server that never installed anything has no
        // jar dir, and "nothing installed" is the true answer. Any other
        // error is ignorance — `reconcile_on_list` SAVES the sidecar it
        // reconciles against this listing, so an unreadable dir read as
        // "empty" would drop every record from it.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(on_disk),
        Err(e) => return Err(Error::io(jar_dir.display().to_string(), e)),
    };
    for entry in rd.flatten() {
        let meta = match entry.metadata() {
            Ok(m) if m.is_file() => m,
            _ => continue,
        };
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
        let path = entry.path();
        let mtime = meta
            .modified()
            .map_err(|e| Error::io(path.display().to_string(), e))?;
        let sha = cached_sha1(&path, mtime, meta.len())?;
        on_disk.push((base_name, sha, enabled));
    }
    Ok(on_disk)
}

pub fn reconcile_on_list(jar_dir: &Path) -> Result<Vec<ServerInstalledEntry>> {
    let on_disk = scan_dir(jar_dir)?;
    let mut records = load(jar_dir)?;
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
    let mut records = load(jar_dir)?;
    records.retain(|r| !r.sha1.eq_ignore_ascii_case(&record.sha1));
    records.push(record);
    save(jar_dir, &records)
}

pub fn remove(jar_dir: &Path, sha1: &str) -> Result<()> {
    let mut records = load(jar_dir)?;
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
    let mut records = load(jar_dir)?;
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
        assert!(load(dir.path()).unwrap().is_empty());
        // Absent must stay absent: a read must not create the sidecar.
        assert!(!sidecar_path(dir.path()).exists());
        std::fs::write(sidecar_path(dir.path()), b"{ not json").unwrap();
        assert!(load(dir.path()).unwrap().is_empty());
    }

    #[test]
    fn load_unreadable_sidecar_is_an_error_not_empty() {
        let dir = tempfile::tempdir().unwrap();
        // A DIRECTORY at the sidecar path makes `fs::read` fail with something
        // other than NotFound on every platform — "unreadable", not "absent".
        std::fs::create_dir(sidecar_path(dir.path())).unwrap();
        assert!(load(dir.path()).is_err());
    }

    #[test]
    fn upsert_unreadable_sidecar_errors_instead_of_wiping() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(sidecar_path(dir.path())).unwrap();
        assert!(
            upsert(dir.path(), rec("x.jar", "aa")).is_err(),
            "upsert against an unreadable sidecar must error, not save a one-record file"
        );
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let records = vec![rec("a.jar", "aa"), rec("b.jar", "bb")];
        save(dir.path(), &records).unwrap();
        assert_eq!(load(dir.path()).unwrap(), records);
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
    fn reconcile_absent_dir_is_empty_but_unreadable_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        // Absent is a fact: a server that never installed anything has no
        // jar dir, and "nothing installed" is the true answer.
        let missing = dir.path().join("missing");
        assert!(reconcile_on_list(&missing).unwrap().is_empty());
        // Unreadable is ignorance: a FILE where the dir should be fails
        // read_dir with something other than NotFound on every platform.
        // Answering "empty" here would let the reconcile drop (and save
        // away) every sidecar record.
        let as_file = dir.path().join("mods");
        std::fs::write(&as_file, b"not a dir").unwrap();
        assert!(reconcile_on_list(&as_file).is_err());
    }

    #[test]
    fn reconcile_unreadable_sidecar_is_an_error_not_empty() {
        let dir = tempfile::tempdir().unwrap();
        // A DIRECTORY at the sidecar path makes `fs::read` fail with something
        // other than NotFound on every platform — "unreadable", not "absent".
        std::fs::create_dir(sidecar_path(dir.path())).unwrap();
        assert!(
            reconcile_on_list(dir.path()).is_err(),
            "an unreadable sidecar must surface as an error, not read as empty"
        );
    }

    #[test]
    fn reconcile_drops_stale_records() {
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), &[rec("gone.jar", "deadbeef")]).unwrap();
        let entries = reconcile_on_list(dir.path()).unwrap();
        assert!(entries.is_empty());
        assert!(load(dir.path()).unwrap().is_empty());
    }

    #[test]
    fn remove_deletes_by_sha() {
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), &[rec("a.jar", "aa"), rec("b.jar", "bb")]).unwrap();
        remove(dir.path(), "AA").unwrap();
        let shas: Vec<_> = load(dir.path())
            .unwrap()
            .into_iter()
            .map(|r| r.sha1)
            .collect();
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
            .unwrap()
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
            .unwrap()
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
        assert!(load(dir.path()).unwrap().is_empty());
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
        let records = load(dir.path()).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].name.as_deref(), Some("new"));
    }

    #[test]
    fn reconcile_rewrites_base_filename_on_content_preserving_rename() {
        let dir = tempfile::tempdir().unwrap();
        let sha = write_jar(dir.path(), "sodium-0.5.jar", b"SAMEBYTES");
        upsert(
            dir.path(),
            ServerInstalledRecord {
                filename: "sodium-0.5.jar".into(),
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
        // Rename to a DIFFERENT base name, identical bytes (same sha1).
        std::fs::rename(
            dir.path().join("sodium-0.5.jar"),
            dir.path().join("sodium.jar"),
        )
        .unwrap();
        let entries = reconcile_on_list(dir.path()).unwrap();
        let e = entries.iter().find(|e| e.record.sha1 == sha).unwrap();
        assert_eq!(e.record.filename, "sodium.jar"); // base filename rewritten by sha1 match
        assert_eq!(e.record.project_id.as_deref(), Some("p")); // identity preserved
        assert!(e.enabled);
    }

    #[test]
    fn sha1_of_seeds_the_hash_cache() {
        let dir = tempfile::tempdir().unwrap();
        let jar = dir.path().join("seeded.jar");
        std::fs::write(&jar, b"some bytes").unwrap();

        // Hashing through `sha1_of` must leave the digest in HASH_CACHE, so the
        // reconcile that follows an install answers from memory instead of
        // reading the jar a second time.
        let real = sha1_of(&jar).unwrap();

        let meta = std::fs::metadata(&jar).unwrap();
        let mtime = meta.modified().unwrap();
        {
            let mut cache = HASH_CACHE.lock().unwrap_or_else(|p| p.into_inner());
            let entry = cache
                .get_mut(&jar)
                .expect("sha1_of must have seeded the cache");
            assert_eq!(*entry, (mtime, meta.len(), real.clone()));
            // Poison it: a later lookup that re-read the bytes would recompute
            // `real`, so getting the poison back is what proves the cache was
            // hit — and therefore that `sha1_of` stored it under the right key.
            entry.2 = "poisoned".to_string();
        }

        let again = cached_sha1(&jar, mtime, meta.len()).unwrap();
        assert_eq!(
            again, "poisoned",
            "cached_sha1 re-read a file sha1_of should have cached"
        );
    }
}
