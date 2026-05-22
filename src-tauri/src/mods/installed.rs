//! Per-instance installed-mods registry.
//!
//! File: `{instance}/ftlauncher/installed-mods.json`. Schema v1.
//!
//! On every read, the registry is scanned against the actual contents
//! of `{instance}/.minecraft/mods/` so user-placed jars and renamed /
//! deleted files reconcile cleanly. Hand-editing the mods folder is a
//! supported workflow.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::SystemTime;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use tokio::fs;

use crate::error::Error;
use crate::mods::modpack::schema::{EnvSupport, ModpackUnresolvable};
use crate::mods::platform::{InstalledMod, ModSource};

const FILE_VERSION: u32 = 2;

/// Process-lifetime SHA-1 cache for files in `mods/`, keyed by path.
/// `reconcile()` re-uses the stored digest when a file's (mtime, size)
/// are unchanged, turning a full read+hash into a cheap `stat`.
static HASH_CACHE: LazyLock<Mutex<HashMap<PathBuf, (SystemTime, u64, String)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// SHA-1 of the file at `path`, re-using the cached digest when
/// `(mtime, size)` are unchanged since it was last hashed. `read_and_hash`
/// is only awaited on a miss. The lock is never held across the await.
async fn cached_sha1<F, Fut>(
    path: &Path,
    mtime: SystemTime,
    size: u64,
    read_and_hash: F,
) -> Result<String, Error>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<String, Error>>,
{
    {
        let cache = HASH_CACHE.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((m, s, sha)) = cache.get(path) {
            if *m == mtime && *s == size {
                return Ok(sha.clone());
            }
        }
    }
    let sha = read_and_hash().await?;
    {
        let mut cache = HASH_CACHE.lock().unwrap_or_else(|p| p.into_inner());
        cache.insert(path.to_path_buf(), (mtime, size, sha.clone()));
    }
    Ok(sha)
}

/// Snapshot of the mods the user selected at modpack-import time, kept
/// in `installed-mods.json` alongside the live entries so the launcher
/// can later diff "what's still here" vs "what was added/removed" without
/// re-parsing the original .mrpack/.zip. Pre-bundle-2 imports and
/// manually-created instances have `pack_origin = None` on disk.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq)]
pub struct PackOrigin {
    pub project_id: Option<String>,
    pub source: ModSource,
    pub project_name: String,
    pub version: String,
    pub files: Vec<PackOriginFile>,
    /// Mods the import could not auto-download (CurseForge distribution
    /// disabled / Modrinth non-CDN host). `#[serde(default)]` so
    /// registry files written before SF2 load with an empty list.
    #[serde(default)]
    pub missing_mods: Vec<ModpackUnresolvable>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq)]
pub struct PackOriginFile {
    pub sha1: String,
    pub name: String,
    pub filename: String,
    pub install_path: String,
    pub url: String,
    /// f64 not u64 — specta forbids BigInt-style exports. 2^53 bytes is
    /// far beyond any plausible mod jar size.
    pub size: f64,
    pub project_id: String,
    pub version_id: String,
    pub env_client: EnvSupport,
    pub source: ModSource,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct OnDisk {
    #[serde(default = "default_version")]
    version: u32,
    #[serde(default)]
    mods: Vec<InstalledMod>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pack_origin: Option<PackOrigin>,
}

fn default_version() -> u32 { FILE_VERSION }

pub fn registry_dir(instance_root: &Path) -> PathBuf {
    instance_root.join("ftlauncher")
}

pub fn registry_path(instance_root: &Path) -> PathBuf {
    registry_dir(instance_root).join("installed-mods.json")
}

pub fn mods_dir(instance_root: &Path) -> PathBuf {
    instance_root.join(".minecraft").join("mods")
}

/// Read the registry from disk and reconcile against the actual `mods/`
/// directory contents. Persists changes if reconciliation modified state.
pub async fn list(instance_root: &Path) -> Result<Vec<InstalledMod>, Error> {
    let mut state = read_or_empty(instance_root).await?;
    let changed = reconcile(instance_root, &mut state).await?;
    if changed {
        write(instance_root, &state).await?;
    }
    Ok(state.mods)
}

async fn read_or_empty(instance_root: &Path) -> Result<OnDisk, Error> {
    let path = registry_path(instance_root);
    if !fs::try_exists(&path).await.map_err(|e| io_err(&path, e))? {
        return Ok(OnDisk { version: FILE_VERSION, mods: vec![], pack_origin: None });
    }
    let bytes = fs::read(&path).await.map_err(|e| io_err(&path, e))?;
    // Corrupt JSON: treat as empty; reconcile will rebuild from disk.
    Ok(serde_json::from_slice::<OnDisk>(&bytes).unwrap_or(OnDisk { version: FILE_VERSION, mods: vec![], pack_origin: None }))
}

async fn write(instance_root: &Path, state: &OnDisk) -> Result<(), Error> {
    let dir = registry_dir(instance_root);
    fs::create_dir_all(&dir).await.map_err(|e| io_err(&dir, e))?;
    let final_path = registry_path(instance_root);
    let tmp = final_path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(state)
        .map_err(|e| Error::ModsDecode { platform: "installed-mods.json".into(), details: e.to_string() })?;
    fs::write(&tmp, &bytes).await.map_err(|e| io_err(&tmp, e))?;
    fs::rename(&tmp, &final_path).await.map_err(|e| io_err(&final_path, e))?;
    Ok(())
}

/// Sync `state.mods` against the contents of `mods/`. Returns true if
/// anything changed (caller persists).
async fn reconcile(instance_root: &Path, state: &mut OnDisk) -> Result<bool, Error> {
    let dir = mods_dir(instance_root);

    // (base_filename, sha1_lower, enabled) for every file on disk.
    // A missing mods/ directory is equivalent to an empty one: any stale
    // JSON entries should still be dropped.
    let mut on_disk: Vec<(String, String, bool)> = Vec::new();
    if fs::try_exists(&dir).await.map_err(|e| io_err(&dir, e))? {
        let mut rd = fs::read_dir(&dir).await.map_err(|e| io_err(&dir, e))?;
        while let Some(entry) = rd.next_entry().await.map_err(|e| io_err(&dir, e))? {
            let meta = entry.metadata().await.map_err(|e| io_err(&dir, e))?;
            if !meta.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let (enabled, base_name) = if let Some(stripped) = name.strip_suffix(".disabled") {
                (false, stripped.to_string())
            } else if name.ends_with(".jar") {
                (true, name.clone())
            } else {
                continue;
            };
            let path = entry.path();
            let size = meta.len();
            let mtime = meta.modified().map_err(|e| io_err(&path, e))?;
            let sha = cached_sha1(&path, mtime, size, || async {
                let bytes = fs::read(&path).await.map_err(|e| io_err(&path, e))?;
                Ok(hex::encode(Sha1::digest(&bytes)))
            })
            .await?;
            on_disk.push((base_name, sha, enabled));
        }
    }

    let mut changed = false;

    // 1. Update existing JSON entries by SHA, fixing filename / enabled drift.
    for m in state.mods.iter_mut() {
        if let Some((on_disk_name, _, on_disk_enabled)) =
            on_disk.iter().find(|(_, sha, _)| sha.eq_ignore_ascii_case(&m.sha1))
        {
            if m.filename != *on_disk_name {
                m.filename = on_disk_name.clone();
                changed = true;
            }
            if m.enabled != *on_disk_enabled {
                m.enabled = *on_disk_enabled;
                changed = true;
            }
        }
    }

    // 2. Drop JSON entries with no matching file on disk.
    let before = state.mods.len();
    let on_disk_shas: std::collections::HashSet<String> =
        on_disk.iter().map(|(_, s, _)| s.to_ascii_lowercase()).collect();
    state.mods.retain(|m| on_disk_shas.contains(&m.sha1.to_ascii_lowercase()));
    if state.mods.len() != before {
        changed = true;
    }

    // 3. Add synthesized entries for files on disk with no JSON record.
    let known_shas: std::collections::HashSet<String> =
        state.mods.iter().map(|m| m.sha1.to_ascii_lowercase()).collect();
    for (filename, sha, enabled) in on_disk.iter() {
        if !known_shas.contains(&sha.to_ascii_lowercase()) {
            state.mods.push(InstalledMod {
                filename: filename.clone(),
                sha1: sha.clone(),
                source: None,
                project_id: None,
                version_id: None,
                name: filename.clone(),
                version_number: None,
                installed_at: Utc::now().to_rfc3339(),
                enabled: *enabled,
            });
            changed = true;
        }
    }

    Ok(changed)
}

/// Append a new entry. Caller has already placed the file in `mods/`.
pub async fn add(instance_root: &Path, m: InstalledMod) -> Result<(), Error> {
    let mut state = read_or_empty(instance_root).await?;
    state.mods.retain(|x| !x.sha1.eq_ignore_ascii_case(&m.sha1));
    state.mods.push(m);
    write(instance_root, &state).await
}

/// Remove the entry with the given SHA-1.
pub async fn remove(instance_root: &Path, sha1: &str) -> Result<(), Error> {
    let mut state = read_or_empty(instance_root).await?;
    state.mods.retain(|x| !x.sha1.eq_ignore_ascii_case(sha1));
    write(instance_root, &state).await
}

/// Toggle `enabled` for the entry with the given SHA-1.
pub async fn set_enabled(instance_root: &Path, sha1: &str, enabled: bool) -> Result<(), Error> {
    let mut state = read_or_empty(instance_root).await?;
    if let Some(m) = state.mods.iter_mut().find(|x| x.sha1.eq_ignore_ascii_case(sha1)) {
        m.enabled = enabled;
    }
    write(instance_root, &state).await
}

/// Persist the modpack-origin snapshot for the instance. Read-modify-
/// write: preserves the existing `mods` list. Called once after a
/// successful import; the bundled file set is immutable thereafter.
pub async fn set_pack_origin(instance_root: &Path, origin: PackOrigin) -> Result<(), Error> {
    let mut state = read_or_empty(instance_root).await?;
    state.pack_origin = Some(origin);
    write(instance_root, &state).await
}

/// One-shot migration for `installed-mods.json`. Schema v1 imports
/// recorded non-`mods/` `pack_origin` entries for files that were never
/// installed (the v1 pipeline ignored `install_path`). Drop them and
/// bump the schema version so the migration runs exactly once. Returns
/// true if `state` was changed (the caller must then persist it).
fn migrate(state: &mut OnDisk) -> bool {
    if state.version >= FILE_VERSION {
        return false;
    }
    if let Some(origin) = state.pack_origin.as_mut() {
        origin.files.retain(|f| f.install_path.starts_with("mods/"));
    }
    state.version = FILE_VERSION;
    true
}

/// Read the modpack-origin snapshot if one was recorded at import time.
/// Runs the one-shot schema migration (writes back once for v1 files).
/// Returns `None` for manually-created instances and pre-bundle-2 imports.
pub async fn get_pack_origin(instance_root: &Path) -> Result<Option<PackOrigin>, Error> {
    let mut state = read_or_empty(instance_root).await?;
    if migrate(&mut state) {
        write(instance_root, &state).await?;
    }
    Ok(state.pack_origin)
}

fn io_err(path: &Path, e: std::io::Error) -> Error {
    Error::ModsInstancePath { path: path.display().to_string(), details: e.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn place_jar(dir: &Path, name: &str, body: &[u8]) -> String {
        fs::create_dir_all(dir).await.unwrap();
        fs::write(dir.join(name), body).await.unwrap();
        hex::encode(Sha1::digest(body))
    }

    #[tokio::test]
    async fn empty_instance_yields_empty_list() {
        let td = TempDir::new().unwrap();
        let mods = list(td.path()).await.unwrap();
        assert!(mods.is_empty());
    }

    #[tokio::test]
    async fn synthesizes_entry_for_manual_jar() {
        let td = TempDir::new().unwrap();
        let sha = place_jar(&mods_dir(td.path()), "manual.jar", b"abc").await;
        let mods = list(td.path()).await.unwrap();
        assert_eq!(mods.len(), 1);
        assert_eq!(mods[0].sha1, sha);
        assert_eq!(mods[0].filename, "manual.jar");
        assert!(mods[0].source.is_none());
        assert!(mods[0].enabled);
    }

    #[tokio::test]
    async fn disabled_suffix_marks_entry_disabled() {
        let td = TempDir::new().unwrap();
        place_jar(&mods_dir(td.path()), "foo.jar.disabled", b"xyz").await;
        let mods = list(td.path()).await.unwrap();
        assert_eq!(mods.len(), 1);
        assert_eq!(mods[0].filename, "foo.jar");
        assert!(!mods[0].enabled);
    }

    #[tokio::test]
    async fn drops_stale_json_entry_when_file_missing() {
        let td = TempDir::new().unwrap();
        let stale = InstalledMod {
            filename: "gone.jar".into(),
            sha1: "0000000000000000000000000000000000000000".into(),
            source: Some(ModSource::Modrinth),
            project_id: Some("zzz".into()),
            version_id: Some("yyy".into()),
            name: "Gone".into(),
            version_number: Some("1.0".into()),
            installed_at: Utc::now().to_rfc3339(),
            enabled: true,
        };
        add(td.path(), stale).await.unwrap();
        let mods = list(td.path()).await.unwrap();
        assert!(mods.is_empty());
    }

    #[tokio::test]
    async fn add_then_list_round_trips_metadata() {
        let td = TempDir::new().unwrap();
        let sha = place_jar(&mods_dir(td.path()), "jei.jar", b"jei-bytes").await;
        add(td.path(), InstalledMod {
            filename: "jei.jar".into(),
            sha1: sha.clone(),
            source: Some(ModSource::Modrinth),
            project_id: Some("u6dRKJwZ".into()),
            version_id: Some("ZG8XHvO0".into()),
            name: "Just Enough Items".into(),
            version_number: Some("15.2.0.27".into()),
            installed_at: Utc::now().to_rfc3339(),
            enabled: true,
        }).await.unwrap();
        let mods = list(td.path()).await.unwrap();
        assert_eq!(mods.len(), 1);
        assert_eq!(mods[0].name, "Just Enough Items");
        assert_eq!(mods[0].source, Some(ModSource::Modrinth));
    }

    #[tokio::test]
    async fn corrupt_json_rebuilds_from_disk() {
        let td = TempDir::new().unwrap();
        place_jar(&mods_dir(td.path()), "rebuilt.jar", b"data").await;
        let dir = registry_dir(td.path());
        fs::create_dir_all(&dir).await.unwrap();
        fs::write(registry_path(td.path()), b"this is not json").await.unwrap();
        let mods = list(td.path()).await.unwrap();
        assert_eq!(mods.len(), 1);
        assert_eq!(mods[0].filename, "rebuilt.jar");
    }

    fn sample_origin() -> PackOrigin {
        PackOrigin {
            project_id: Some("AANobbMI".into()),
            source: ModSource::Modrinth,
            project_name: "Simply Optimized".into(),
            version: "1.0.0".into(),
            files: vec![PackOriginFile {
                sha1: "a1b2c3".into(),
                name: "Sodium".into(),
                filename: "sodium.jar".into(),
                install_path: "mods/sodium.jar".into(),
                url: "https://cdn.modrinth.com/.../sodium.jar".into(),
                size: 1024.0,
                project_id: "AANobbMI".into(),
                version_id: "v1".into(),
                env_client: EnvSupport::Required,
                source: ModSource::Modrinth,
            }],
            missing_mods: vec![],
        }
    }

    #[tokio::test]
    async fn pack_origin_round_trips_through_disk() {
        let td = TempDir::new().unwrap();
        // Place a mod so `list()` reconciliation has something to look at.
        place_jar(&mods_dir(td.path()), "any.jar", b"any").await;
        // Force a write so the file exists on disk before set_pack_origin runs.
        let _ = list(td.path()).await.unwrap();
        let origin = sample_origin();
        set_pack_origin(td.path(), origin.clone()).await.unwrap();
        let got = get_pack_origin(td.path()).await.unwrap();
        assert_eq!(got, Some(origin));
    }

    #[tokio::test]
    async fn get_pack_origin_is_none_for_fresh_instance() {
        let td = TempDir::new().unwrap();
        let got = get_pack_origin(td.path()).await.unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn set_pack_origin_preserves_existing_mods() {
        let td = TempDir::new().unwrap();
        let sha = place_jar(&mods_dir(td.path()), "fixed.jar", b"abc").await;
        add(td.path(), InstalledMod {
            filename: "fixed.jar".into(),
            sha1: sha.clone(),
            source: Some(ModSource::Modrinth),
            project_id: Some("zzz".into()),
            version_id: Some("yyy".into()),
            name: "Pinned".into(),
            version_number: Some("1.0".into()),
            installed_at: Utc::now().to_rfc3339(),
            enabled: true,
        }).await.unwrap();
        set_pack_origin(td.path(), sample_origin()).await.unwrap();
        let mods = list(td.path()).await.unwrap();
        assert_eq!(mods.len(), 1);
        assert_eq!(mods[0].name, "Pinned");
        let origin = get_pack_origin(td.path()).await.unwrap();
        assert!(origin.is_some());
    }

    #[tokio::test]
    async fn loads_legacy_file_without_pack_origin_field() {
        // Files written before bundle 2 lack the pack_origin field
        // entirely. Default(None) + serde(default) makes them round-trip
        // cleanly without "missing field" errors.
        let td = TempDir::new().unwrap();
        let dir = registry_dir(td.path());
        fs::create_dir_all(&dir).await.unwrap();
        let legacy = br#"{"version":1,"mods":[]}"#;
        fs::write(registry_path(td.path()), legacy).await.unwrap();
        let origin = get_pack_origin(td.path()).await.unwrap();
        assert!(origin.is_none());
    }

    #[tokio::test]
    async fn migrate_drops_phantom_non_mods_entries_from_v1_pack() {
        let td = TempDir::new().unwrap();
        let mut mods_file = sample_origin().files[0].clone();
        mods_file.install_path = "mods/sodium.jar".into();
        let mut rp = sample_origin().files[0].clone();
        rp.install_path = "resourcepacks/RP.zip".into();
        rp.sha1 = "rp1".into();
        let v1 = OnDisk {
            version: 1,
            mods: vec![],
            pack_origin: Some(PackOrigin {
                project_id: None,
                source: ModSource::Modrinth,
                project_name: "P".into(),
                version: "1".into(),
                files: vec![mods_file, rp],
                missing_mods: vec![],
            }),
        };
        write(td.path(), &v1).await.unwrap();
        let origin = get_pack_origin(td.path()).await.unwrap().unwrap();
        assert_eq!(origin.files.len(), 1);
        assert!(origin.files[0].install_path.starts_with("mods/"));
        // version bumped on disk so the migration is one-shot.
        let raw = String::from_utf8(
            tokio::fs::read(registry_path(td.path())).await.unwrap(),
        )
        .unwrap();
        assert!(raw.contains("\"version\": 2"), "got {raw}");
    }

    #[tokio::test]
    async fn v2_pack_keeps_non_mods_entries() {
        let td = TempDir::new().unwrap();
        let mut rp = sample_origin().files[0].clone();
        rp.install_path = "resourcepacks/RP.zip".into();
        let v2 = OnDisk {
            version: 2,
            mods: vec![],
            pack_origin: Some(PackOrigin {
                project_id: None,
                source: ModSource::Modrinth,
                project_name: "P".into(),
                version: "1".into(),
                files: vec![rp],
                missing_mods: vec![],
            }),
        };
        write(td.path(), &v2).await.unwrap();
        let origin = get_pack_origin(td.path()).await.unwrap().unwrap();
        assert_eq!(origin.files.len(), 1);
        assert_eq!(origin.files[0].install_path, "resourcepacks/RP.zip");
    }

    #[tokio::test]
    async fn pack_origin_missing_mods_round_trip() {
        use crate::mods::modpack::schema::{ModpackUnresolvable, UnresolvableReason};
        let td = TempDir::new().unwrap();
        place_jar(&mods_dir(td.path()), "any.jar", b"any").await;
        let _ = list(td.path()).await.unwrap();
        let mut origin = sample_origin();
        origin.missing_mods = vec![ModpackUnresolvable {
            reason: UnresolvableReason::DistributionDisabled,
            mod_name: "Scape and Run: Parasites".into(),
            manual_action_url: "https://www.curseforge.com/projects/247571".into(),
            filename: "srparasites-1.12.2-2.7.1.jar".into(),
            size: 4096.0,
            sha1: Some("abc".into()),
            project_id: None,
        }];
        set_pack_origin(td.path(), origin.clone()).await.unwrap();
        let got = get_pack_origin(td.path()).await.unwrap();
        assert_eq!(got, Some(origin));
    }

    #[tokio::test]
    async fn legacy_pack_origin_loads_with_empty_missing_mods() {
        let td = TempDir::new().unwrap();
        let dir = registry_dir(td.path());
        fs::create_dir_all(&dir).await.unwrap();
        // A v2 file written before SF2 — pack_origin present, no missing_mods.
        let legacy = br#"{"version":2,"mods":[],"pack_origin":{"project_id":null,"source":"modrinth","project_name":"P","version":"1","files":[]}}"#;
        fs::write(registry_path(td.path()), legacy).await.unwrap();
        let origin = get_pack_origin(td.path()).await.unwrap().unwrap();
        assert!(origin.missing_mods.is_empty());
    }

    #[tokio::test]
    async fn missing_mod_without_project_id_loads_as_none() {
        // A missing_mods entry written before feature C has no
        // `project_id` field; `#[serde(default)]` must load it as None.
        let td = TempDir::new().unwrap();
        let dir = registry_dir(td.path());
        fs::create_dir_all(&dir).await.unwrap();
        let legacy = br#"{"version":2,"mods":[],"pack_origin":{"project_id":null,"source":"curseforge","project_name":"P","version":"1","files":[],"missing_mods":[{"reason":"distribution_disabled","mod_name":"SRP","manual_action_url":"https://x/1","filename":"srp.jar","size":1.0,"sha1":"aa"}]}}"#;
        fs::write(registry_path(td.path()), legacy).await.unwrap();
        let origin = get_pack_origin(td.path()).await.unwrap().unwrap();
        assert_eq!(origin.missing_mods.len(), 1);
        assert_eq!(origin.missing_mods[0].project_id, None);
    }

    #[tokio::test]
    async fn cached_sha1_hit_skips_recompute() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::time::{Duration, UNIX_EPOCH};
        let calls = AtomicUsize::new(0);
        // Synthetic, unique path — the cache key never needs a real file
        // because the read_and_hash closure is a stub.
        let path = Path::new("modlistcache-test-hashcache-hit.jar");
        let mtime = UNIX_EPOCH + Duration::from_secs(1000);
        let a = cached_sha1(path, mtime, 10, || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok("deadbeef".to_string())
        })
        .await
        .unwrap();
        let b = cached_sha1(path, mtime, 10, || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok("must-not-run".to_string())
        })
        .await
        .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(a, "deadbeef");
        assert_eq!(b, "deadbeef");
    }

    #[tokio::test]
    async fn cached_sha1_recomputes_when_size_changes() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::time::{Duration, UNIX_EPOCH};
        let calls = AtomicUsize::new(0);
        let path = Path::new("modlistcache-test-hashcache-size.jar");
        let mtime = UNIX_EPOCH + Duration::from_secs(2000);
        let _ = cached_sha1(path, mtime, 10, || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok("hash-v1".to_string())
        })
        .await
        .unwrap();
        let v2 = cached_sha1(path, mtime, 20, || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok("hash-v2".to_string())
        })
        .await
        .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(v2, "hash-v2");
    }

    #[tokio::test]
    async fn cached_sha1_recomputes_when_mtime_changes() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::time::{Duration, UNIX_EPOCH};
        let calls = AtomicUsize::new(0);
        let path = Path::new("modlistcache-test-hashcache-mtime.jar");
        let _ = cached_sha1(path, UNIX_EPOCH + Duration::from_secs(3000), 10, || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok("hash-old".to_string())
        })
        .await
        .unwrap();
        // Same path and size, newer mtime — must re-hash.
        let v2 = cached_sha1(path, UNIX_EPOCH + Duration::from_secs(3001), 10, || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok("hash-new".to_string())
        })
        .await
        .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(v2, "hash-new");
    }
}
