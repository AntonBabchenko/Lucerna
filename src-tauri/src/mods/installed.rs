//! Per-instance installed-mods registry.
//!
//! File: `{instance}/lucerna/installed-mods.json`. Schema v1.
//!
//! On every read, the registry is scanned against the actual contents
//! of `{instance}/.minecraft/mods/` so user-placed jars and renamed /
//! deleted files reconcile cleanly. Hand-editing the mods folder is a
//! supported workflow.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::SystemTime;

/// Monotonic counter giving every `write()` a unique temp-file name.
/// Several `list()` calls can run concurrently for the same instance (the
/// Installed view fires `modsListInstalled` + `modsPackOriginSummary` +
/// `mods_dependency_graph` together, and a first-open schema migration makes
/// each of them write). A shared fixed `*.json.tmp` name made those writes
/// race on the same path — the first rename won, the rest failed with
/// "cannot find the file" (os error 2). A per-write unique name removes the
/// collision; the final atomic rename still serializes the visible result.
static WRITE_SEQ: AtomicU64 = AtomicU64::new(0);

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use tokio::fs;

use crate::error::Error;
use crate::mods::modpack::schema::{EnvSupport, ModpackUnresolvable};
use crate::mods::platform::{InstalledMod, ModSource};

const FILE_VERSION: u32 = 4;

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
    /// User-chosen substitutes for `missing_mods` entries (installed from
    /// another source when the pack's CurseForge file is distribution
    /// disabled). `#[serde(default)]` so registry files written before this
    /// feature load with an empty list.
    #[serde(default)]
    pub resolved_missing: Vec<ResolvedMissing>,
}

/// A user-chosen substitute that closes a `missing_mods` entry the pack
/// author blocked from auto-download. Kept on `PackOrigin` as a resolution
/// overlay — separate from the frozen import snapshot and from the
/// parse-time `ModpackUnresolvable` (which every manifest parser builds).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq)]
pub struct ResolvedMissing {
    /// Expected jar filename of the closed entry (from the blocked entry).
    pub filename: String,
    /// Display name of the closed entry — disambiguates same-filename entries.
    /// Copied from the blocked `ModpackUnresolvable.mod_name` at record time so
    /// it matches the entry the overlay closes.
    pub mod_name: String,
    /// SHA-1 (lowercased) of the installed substitute jar. The entry reverts
    /// to unresolved if this sha1 leaves the registry (self-healing).
    pub sha1: String,
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

fn default_version() -> u32 {
    FILE_VERSION
}

pub fn registry_dir(instance_root: &Path) -> PathBuf {
    instance_root.join("lucerna")
}

pub fn registry_path(instance_root: &Path) -> PathBuf {
    registry_dir(instance_root).join("installed-mods.json")
}

pub fn mods_dir(instance_root: &Path) -> PathBuf {
    instance_root.join(".minecraft").join("mods")
}

/// Read the registry from disk and reconcile against the actual `mods/`
/// directory contents. Runs the one-shot schema migration before
/// reconciling so callers see the post-migration `mods` slice — without
/// this, `InstalledModsView.refresh()` would see pre-migration
/// `enrich_attempted` values from its `modsListInstalled` call and the
/// later `modsPackOriginSummary` call (which also runs migrate) would
/// only flip the on-disk values, leaving the in-memory backfill check
/// stale until the next refresh. Persists changes if migration or
/// reconciliation modified state.
pub async fn list(instance_root: &Path) -> Result<Vec<InstalledMod>, Error> {
    let mut state = read_or_empty(instance_root).await?;
    let migrated = migrate(&mut state);
    let reconciled = reconcile(instance_root, &mut state).await?;
    if migrated || reconciled {
        write(instance_root, &state).await?;
    }
    Ok(state.mods)
}

async fn read_or_empty(instance_root: &Path) -> Result<OnDisk, Error> {
    let path = registry_path(instance_root);
    if !fs::try_exists(&path).await.map_err(|e| io_err(&path, e))? {
        return Ok(OnDisk {
            version: FILE_VERSION,
            mods: vec![],
            pack_origin: None,
        });
    }
    let bytes = fs::read(&path).await.map_err(|e| io_err(&path, e))?;
    // Corrupt JSON: treat as empty; reconcile will rebuild from disk.
    Ok(serde_json::from_slice::<OnDisk>(&bytes).unwrap_or(OnDisk {
        version: FILE_VERSION,
        mods: vec![],
        pack_origin: None,
    }))
}

async fn write(instance_root: &Path, state: &OnDisk) -> Result<(), Error> {
    let dir = registry_dir(instance_root);
    fs::create_dir_all(&dir)
        .await
        .map_err(|e| io_err(&dir, e))?;
    let final_path = registry_path(instance_root);
    // Unique per-write temp name so concurrent writers don't collide on the
    // same tmp path and fail the rename (see WRITE_SEQ).
    let seq = WRITE_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = final_path.with_extension(format!("json.tmp.{}.{seq}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(state).map_err(|e| Error::ModsDecode {
        platform: "installed-mods.json".into(),
        details: e.to_string(),
    })?;
    fs::write(&tmp, &bytes).await.map_err(|e| io_err(&tmp, e))?;
    fs::rename(&tmp, &final_path)
        .await
        .map_err(|e| io_err(&final_path, e))?;
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
        if let Some((on_disk_name, _, on_disk_enabled)) = on_disk
            .iter()
            .find(|(_, sha, _)| sha.eq_ignore_ascii_case(&m.sha1))
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
    let on_disk_shas: std::collections::HashSet<String> = on_disk
        .iter()
        .map(|(_, s, _)| s.to_ascii_lowercase())
        .collect();
    state
        .mods
        .retain(|m| on_disk_shas.contains(&m.sha1.to_ascii_lowercase()));
    if state.mods.len() != before {
        changed = true;
    }

    // 3. Add synthesized entries for files on disk with no JSON record.
    let known_shas: std::collections::HashSet<String> = state
        .mods
        .iter()
        .map(|m| m.sha1.to_ascii_lowercase())
        .collect();
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
                enrich_attempted: false,
                requires: Vec::new(),
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

/// Overwrite the `requires` edge list for the entry with the given SHA-1.
/// No-op if the SHA-1 is unknown. Read-modify-write.
pub async fn set_requires(
    instance_root: &Path,
    sha1: &str,
    requires: Vec<String>,
) -> Result<(), Error> {
    let mut state = read_or_empty(instance_root).await?;
    if let Some(m) = state
        .mods
        .iter_mut()
        .find(|x| x.sha1.eq_ignore_ascii_case(sha1))
    {
        m.requires = requires;
    }
    write(instance_root, &state).await
}

/// Toggle `enabled` for the entry with the given SHA-1.
pub async fn set_enabled(instance_root: &Path, sha1: &str, enabled: bool) -> Result<(), Error> {
    let mut state = read_or_empty(instance_root).await?;
    if let Some(m) = state
        .mods
        .iter_mut()
        .find(|x| x.sha1.eq_ignore_ascii_case(sha1))
    {
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

/// Apply an enrichment pass to the registry. `resolved` maps an
/// installed mod's SHA-1 (lowercased) to the platform identity
/// recovered for it; `attempted` is the SHA-1 of every mod the pass
/// tried, resolved or not. Every attempted mod gets
/// `enrich_attempted = true`; a resolved mod additionally gets its
/// `source`/`project_id`/`version_id` filled in. Read-modify-write;
/// SHA-1 matching is case-insensitive. Called by `enrich::enrich_instance`.
pub async fn apply_enrichment(
    instance_root: &Path,
    resolved: &HashMap<String, crate::mods::platform::VersionRef>,
    attempted: &HashSet<String>,
) -> Result<(), Error> {
    let mut state = read_or_empty(instance_root).await?;
    for m in state.mods.iter_mut() {
        let key = m.sha1.to_ascii_lowercase();
        if attempted.contains(&key) {
            m.enrich_attempted = true;
        }
        if let Some(id) = resolved.get(&key) {
            m.source = Some(id.source);
            m.project_id = Some(id.project_id.clone());
            m.version_id = Some(id.version_id.clone());
        }
    }
    write(instance_root, &state).await
}

/// Reset `enrich_attempted = false` on every mod still missing a
/// platform identity (`source = None`). Idempotent. Used by the
/// CurseForge key-set hook to retry the backfill on mods that had
/// been Modrinth-only-attempted under a keyless install — once a CF
/// key is configured, those mods become re-queryable. Resolved mods
/// (`source.is_some()`) are not touched. A no-op on instances whose
/// `source = None` mods are already `enrich_attempted = false`.
pub async fn reset_enrichment_attempts_for_unresolved(instance_root: &Path) -> Result<(), Error> {
    let mut state = read_or_empty(instance_root).await?;
    let mut changed = false;
    for m in state.mods.iter_mut() {
        if m.source.is_none() && m.enrich_attempted {
            m.enrich_attempted = false;
            changed = true;
        }
    }
    if changed {
        write(instance_root, &state).await?;
    }
    Ok(())
}

/// One-shot migration for `installed-mods.json`. Each step is gated on
/// the source version, so a v1 file migrates straight to the current
/// version in one pass. Returns true if `state` was changed (the caller
/// must then persist it).
///
/// - v1 → v2: drop non-`mods/` `pack_origin` entries that the v1
///   pipeline recorded for files it never installed.
/// - v2 → v3: reset `enrich_attempted = false` on every mod still
///   `source = None`. Earlier code keyed CurseForge fingerprint-match
///   lookups off `exactMatches[].id` (the modId), not `file.fileFingerprint`,
///   silently dropping every CF match — so pack-bundled mods that CF
///   could have identified got permanently flagged "attempted but
///   unresolved". Resetting the flag puts them back in scope so the
///   fixed code can identify them on the next pass.
fn migrate(state: &mut OnDisk) -> bool {
    if state.version >= FILE_VERSION {
        return false;
    }
    if state.version < 2 {
        if let Some(origin) = state.pack_origin.as_mut() {
            origin.files.retain(|f| f.install_path.starts_with("mods/"));
        }
    }
    if state.version < 3 {
        for m in state.mods.iter_mut() {
            if m.source.is_none() && m.enrich_attempted {
                m.enrich_attempted = false;
            }
        }
    }
    if state.version < 4 {
        // v3 → v4: `requires` is added with `#[serde(default)]`; no field
        // backfill is possible (we never recorded edges before v4), so the
        // empty default is correct. Bumping the version stamps the upgrade.
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
    Error::ModsInstancePath {
        path: path.display().to_string(),
        details: e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn pack_origin_without_resolved_missing_field_loads_as_empty() {
        // A sidecar written before this feature has no `resolved_missing` key.
        let json = r#"{"project_id":null,"source":"modrinth","project_name":"P","version":"1","files":[],"missing_mods":[]}"#;
        let o: PackOrigin = serde_json::from_str(json).expect("legacy PackOrigin must load");
        assert!(o.resolved_missing.is_empty());
    }

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
            enrich_attempted: false,
            requires: Vec::new(),
        };
        add(td.path(), stale).await.unwrap();
        let mods = list(td.path()).await.unwrap();
        assert!(mods.is_empty());
    }

    #[tokio::test]
    async fn add_then_list_round_trips_metadata() {
        let td = TempDir::new().unwrap();
        let sha = place_jar(&mods_dir(td.path()), "jei.jar", b"jei-bytes").await;
        add(
            td.path(),
            InstalledMod {
                filename: "jei.jar".into(),
                sha1: sha.clone(),
                source: Some(ModSource::Modrinth),
                project_id: Some("u6dRKJwZ".into()),
                version_id: Some("ZG8XHvO0".into()),
                name: "Just Enough Items".into(),
                version_number: Some("15.2.0.27".into()),
                installed_at: Utc::now().to_rfc3339(),
                enabled: true,
                enrich_attempted: false,
                requires: Vec::new(),
            },
        )
        .await
        .unwrap();
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
        fs::write(registry_path(td.path()), b"this is not json")
            .await
            .unwrap();
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
            resolved_missing: Vec::new(),
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
        add(
            td.path(),
            InstalledMod {
                filename: "fixed.jar".into(),
                sha1: sha.clone(),
                source: Some(ModSource::Modrinth),
                project_id: Some("zzz".into()),
                version_id: Some("yyy".into()),
                name: "Pinned".into(),
                version_number: Some("1.0".into()),
                installed_at: Utc::now().to_rfc3339(),
                enabled: true,
                enrich_attempted: false,
                requires: Vec::new(),
            },
        )
        .await
        .unwrap();
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
    async fn installed_mod_without_enrich_attempted_loads_as_false() {
        // Registry files written before this feature have no
        // `enrich_attempted` field on their mod entries. `#[serde(default)]`
        // must load them as `false` rather than failing with "missing field".
        let td = TempDir::new().unwrap();
        let dir = registry_dir(td.path());
        fs::create_dir_all(&dir).await.unwrap();
        let legacy = br#"{"version":2,"mods":[{"filename":"a.jar","sha1":"aa","source":null,"project_id":null,"version_id":null,"name":"A","version_number":null,"installed_at":"2026-01-01T00:00:00Z","enabled":true}]}"#;
        fs::write(registry_path(td.path()), legacy).await.unwrap();
        let state = read_or_empty(td.path()).await.unwrap();
        assert_eq!(state.mods.len(), 1);
        assert!(!state.mods[0].enrich_attempted);
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
                resolved_missing: Vec::new(),
            }),
        };
        write(td.path(), &v1).await.unwrap();
        let origin = get_pack_origin(td.path()).await.unwrap().unwrap();
        assert_eq!(origin.files.len(), 1);
        assert!(origin.files[0].install_path.starts_with("mods/"));
        // version bumped on disk so the migration is one-shot.
        let raw =
            String::from_utf8(tokio::fs::read(registry_path(td.path())).await.unwrap()).unwrap();
        // Migration bumps a v1 file straight to the current
        // FILE_VERSION (4) in one pass.
        assert!(raw.contains("\"version\": 4"), "got {raw}");
    }

    #[tokio::test]
    async fn list_runs_migration_so_callers_see_post_migration_values() {
        // The Installed-view backfill issues `modsListInstalled` BEFORE
        // `modsPackOriginSummary`. If only `get_pack_origin` triggered
        // migrate(), the list returned to the frontend would still carry
        // stale `enrich_attempted=true` values and the backfill check
        // would skip the affected mods until the next refresh. Pin the
        // single-refresh behaviour: `list()` returns post-migration mods.
        let td = TempDir::new().unwrap();
        let sha = place_jar(&mods_dir(td.path()), "stuck.jar", b"stuck-bytes").await;
        let v2 = OnDisk {
            version: 2,
            mods: vec![InstalledMod {
                filename: "stuck.jar".into(),
                sha1: sha.clone(),
                source: None,
                project_id: None,
                version_id: None,
                name: "stuck.jar".into(),
                version_number: None,
                installed_at: Utc::now().to_rfc3339(),
                enabled: true,
                enrich_attempted: true,
                requires: Vec::new(),
            }],
            pack_origin: None,
        };
        write(td.path(), &v2).await.unwrap();
        let mods = list(td.path()).await.unwrap();
        let m = mods.iter().find(|m| m.sha1 == sha).unwrap();
        assert!(
            !m.enrich_attempted,
            "list() must return post-migration values"
        );
    }

    #[tokio::test]
    async fn migrate_v2_resets_enrich_attempted_on_unresolved_mods() {
        // v2 was the schema while `resolve_curseforge` misread
        // `exactMatches[].id` as the fingerprint, silently dropping
        // every CF match. Pack-bundled jars CF could have identified
        // were left `source = None` but flagged `enrich_attempted =
        // true`, permanently out of scope for the backfill. v3 resets
        // the flag on every `source = None` mod so the fixed code can
        // identify them on the next pass; mods with a resolved source
        // are untouched.
        let td = TempDir::new().unwrap();
        let v2 = OnDisk {
            version: 2,
            mods: vec![
                InstalledMod {
                    filename: "unresolved.jar".into(),
                    sha1: "aaa".into(),
                    source: None,
                    project_id: None,
                    version_id: None,
                    name: "Unresolved".into(),
                    version_number: None,
                    installed_at: Utc::now().to_rfc3339(),
                    enabled: true,
                    enrich_attempted: true, // stuck under the buggy build
                    requires: Vec::new(),
                },
                InstalledMod {
                    filename: "resolved.jar".into(),
                    sha1: "bbb".into(),
                    source: Some(ModSource::Modrinth),
                    project_id: Some("p".into()),
                    version_id: Some("v".into()),
                    name: "Resolved".into(),
                    version_number: None,
                    installed_at: Utc::now().to_rfc3339(),
                    enabled: true,
                    enrich_attempted: false,
                    requires: Vec::new(),
                },
            ],
            pack_origin: Some(sample_origin()),
        };
        write(td.path(), &v2).await.unwrap();
        // `get_pack_origin` runs migrate() and persists the change.
        // (`list()` would also reconcile against `mods/`, dropping
        // entries whose jars are absent on disk — we use
        // `read_or_empty` to inspect the migrated state directly.)
        let _ = get_pack_origin(td.path()).await.unwrap();
        let state = read_or_empty(td.path()).await.unwrap();
        let unr = state.mods.iter().find(|m| m.sha1 == "aaa").unwrap();
        assert!(!unr.enrich_attempted, "unresolved mod's flag should reset");
        let res = state.mods.iter().find(|m| m.sha1 == "bbb").unwrap();
        assert_eq!(
            res.source,
            Some(ModSource::Modrinth),
            "resolved mod's identity must be preserved",
        );
        assert!(!res.enrich_attempted);
        let raw =
            String::from_utf8(tokio::fs::read(registry_path(td.path())).await.unwrap()).unwrap();
        assert!(raw.contains("\"version\": 4"), "got {raw}");
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
                resolved_missing: Vec::new(),
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
    async fn migrate_v3_to_v4_adds_empty_requires_and_bumps_version() {
        // A v3 file with one mod and no `requires` field.
        let legacy = br#"{"version":3,"mods":[{"filename":"a.jar","sha1":"aa","source":"modrinth","project_id":"p1","version_id":"v1","name":"A","version_number":"1.0","installed_at":"2026-01-01T00:00:00Z","enabled":true,"enrich_attempted":false}]}"#;
        let mut state: OnDisk = serde_json::from_slice(legacy).unwrap();
        assert_eq!(state.version, 3);
        let changed = migrate(&mut state);
        assert!(changed, "v3 file must be migrated");
        assert_eq!(state.version, FILE_VERSION);
        assert_eq!(state.version, 4);
        assert!(state.mods[0].requires.is_empty(), "requires defaults empty");
    }

    #[tokio::test]
    async fn concurrent_list_migration_does_not_race_on_temp_file() {
        // The Installed view fires several commands that each call list()
        // (modsListInstalled + modsPackOriginSummary + mods_dependency_graph),
        // and a first-open v3→v4 migration makes every one of them write. With
        // a shared `*.json.tmp` name those writes raced on the same path — the
        // first rename won, the rest failed with os error 2 ("cannot find the
        // file"). A unique per-write temp name fixes it; this guards the fix.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let dir = registry_dir(root);
        fs::create_dir_all(&dir).await.unwrap();
        // Seed a pre-migration v3 registry so every list() migrates + writes.
        fs::write(registry_path(root), br#"{"version":3,"mods":[]}"#)
            .await
            .unwrap();

        let results = futures_util::future::join_all((0..16).map(|_| list(root))).await;
        for r in &results {
            assert!(
                r.is_ok(),
                "concurrent list() must not fail: {:?}",
                r.as_ref().err()
            );
        }
        let raw = String::from_utf8(fs::read(registry_path(root)).await.unwrap()).unwrap();
        assert!(raw.contains("\"version\": 4"), "migrated to v4: {raw}");
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

    #[tokio::test]
    async fn apply_enrichment_fills_identity_and_sets_attempted() {
        let td = TempDir::new().unwrap();
        let sha = place_jar(&mods_dir(td.path()), "sodium.jar", b"sodium").await;
        // Synthesize the source=None entry by listing once.
        let _ = list(td.path()).await.unwrap();
        let mut resolved = HashMap::new();
        resolved.insert(
            sha.clone(),
            crate::mods::platform::VersionRef {
                source: ModSource::Modrinth,
                project_id: "AANobbMI".into(),
                version_id: "vvv".into(),
            },
        );
        let mut attempted = std::collections::HashSet::new();
        attempted.insert(sha.clone());
        apply_enrichment(td.path(), &resolved, &attempted)
            .await
            .unwrap();
        let mods = list(td.path()).await.unwrap();
        assert_eq!(mods[0].source, Some(ModSource::Modrinth));
        assert_eq!(mods[0].project_id.as_deref(), Some("AANobbMI"));
        assert_eq!(mods[0].version_id.as_deref(), Some("vvv"));
        assert!(mods[0].enrich_attempted);
    }

    #[tokio::test]
    async fn apply_enrichment_marks_attempted_even_without_a_match() {
        let td = TempDir::new().unwrap();
        let sha = place_jar(&mods_dir(td.path()), "mystery.jar", b"mystery").await;
        let _ = list(td.path()).await.unwrap();
        let resolved = HashMap::new(); // no match for this jar
        let mut attempted = std::collections::HashSet::new();
        attempted.insert(sha.clone());
        apply_enrichment(td.path(), &resolved, &attempted)
            .await
            .unwrap();
        let mods = list(td.path()).await.unwrap();
        assert!(mods[0].source.is_none());
        assert!(mods[0].enrich_attempted);
    }

    #[tokio::test]
    async fn reset_enrichment_attempts_for_unresolved_flips_only_source_none_mods() {
        // The helper resets enrich_attempted on mods still missing a
        // platform identity (source=None). Mods that DID resolve (source
        // set) are left alone — they have no reason to be re-queried.
        let td = TempDir::new().unwrap();
        let sha_unr = place_jar(&mods_dir(td.path()), "unresolved.jar", b"u").await;
        let sha_res = place_jar(&mods_dir(td.path()), "resolved.jar", b"r").await;
        // Synthesize entries via list(), then mark them with the
        // attempted-but-source-mismatch starting state we want.
        let _ = list(td.path()).await.unwrap();
        // Manually set up the starting state: both flagged
        // attempted=true; only the resolved one has a source.
        let mut state = read_or_empty(td.path()).await.unwrap();
        for m in state.mods.iter_mut() {
            m.enrich_attempted = true;
            if m.sha1 == sha_res {
                m.source = Some(ModSource::Modrinth);
                m.project_id = Some("p".into());
                m.version_id = Some("v".into());
            }
        }
        write(td.path(), &state).await.unwrap();

        reset_enrichment_attempts_for_unresolved(td.path())
            .await
            .unwrap();

        let state = read_or_empty(td.path()).await.unwrap();
        let unr = state.mods.iter().find(|m| m.sha1 == sha_unr).unwrap();
        let res = state.mods.iter().find(|m| m.sha1 == sha_res).unwrap();
        assert!(!unr.enrich_attempted, "source=None mod must be reset");
        assert!(
            res.enrich_attempted,
            "resolved mod's flag must be left alone"
        );
        assert_eq!(res.source, Some(ModSource::Modrinth));
    }

    #[tokio::test]
    async fn reset_enrichment_attempts_for_unresolved_is_idempotent() {
        let td = TempDir::new().unwrap();
        let sha = place_jar(&mods_dir(td.path()), "unresolved.jar", b"u").await;
        let _ = list(td.path()).await.unwrap();
        let mut state = read_or_empty(td.path()).await.unwrap();
        state.mods[0].enrich_attempted = true;
        write(td.path(), &state).await.unwrap();

        reset_enrichment_attempts_for_unresolved(td.path())
            .await
            .unwrap();
        reset_enrichment_attempts_for_unresolved(td.path())
            .await
            .unwrap();

        let state = read_or_empty(td.path()).await.unwrap();
        let m = state.mods.iter().find(|m| m.sha1 == sha).unwrap();
        assert!(!m.enrich_attempted);
    }

    #[tokio::test]
    async fn reset_enrichment_attempts_for_unresolved_no_op_on_already_clean() {
        // No source=None+attempted=true mods anywhere → helper returns
        // Ok and leaves state alone.
        let td = TempDir::new().unwrap();
        let sha = place_jar(&mods_dir(td.path()), "resolved.jar", b"r").await;
        let _ = list(td.path()).await.unwrap();
        let mut state = read_or_empty(td.path()).await.unwrap();
        state.mods[0].source = Some(ModSource::Modrinth);
        state.mods[0].project_id = Some("p".into());
        state.mods[0].version_id = Some("v".into());
        state.mods[0].enrich_attempted = true;
        write(td.path(), &state).await.unwrap();

        reset_enrichment_attempts_for_unresolved(td.path())
            .await
            .unwrap();

        let state = read_or_empty(td.path()).await.unwrap();
        let m = state.mods.iter().find(|m| m.sha1 == sha).unwrap();
        // Resolved mod kept its attempted flag (no reset for it) and
        // its identity.
        assert!(m.enrich_attempted);
        assert_eq!(m.source, Some(ModSource::Modrinth));
    }

    #[tokio::test]
    async fn set_requires_overwrites_only_the_target_mod() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        // Place a real jar so reconcile() doesn't prune the entry on list().
        let sha = place_jar(&mods_dir(root), "primary.jar", b"primary-bytes").await;
        add(
            root,
            InstalledMod {
                filename: "primary.jar".into(),
                sha1: sha.clone(),
                source: Some(ModSource::Modrinth),
                project_id: Some("prim".into()),
                version_id: Some("v".into()),
                name: "Primary".into(),
                version_number: Some("1.0".into()),
                installed_at: "2026-01-01T00:00:00Z".into(),
                enabled: true,
                enrich_attempted: false,
                requires: Vec::new(),
            },
        )
        .await
        .unwrap();

        set_requires(root, &sha, vec!["dep1".into(), "dep2".into()])
            .await
            .unwrap();

        let mods = list(root).await.unwrap();
        let prim = mods.iter().find(|m| m.sha1 == sha).unwrap();
        assert_eq!(prim.requires, vec!["dep1".to_string(), "dep2".to_string()]);
    }

    #[tokio::test]
    async fn reset_helper_clears_attempted_across_multiple_instances() {
        // Pin the iteration semantic the command uses: calling the
        // helper across N instance roots flips the flag in each.
        let td_a = TempDir::new().unwrap();
        let td_b = TempDir::new().unwrap();
        let sha_a = place_jar(&mods_dir(td_a.path()), "a.jar", b"a-bytes").await;
        let sha_b = place_jar(&mods_dir(td_b.path()), "b.jar", b"b-bytes").await;
        for td in [&td_a, &td_b] {
            let _ = list(td.path()).await.unwrap();
            let mut state = read_or_empty(td.path()).await.unwrap();
            state.mods[0].enrich_attempted = true;
            write(td.path(), &state).await.unwrap();
        }

        // The command's iteration:
        for root in [td_a.path(), td_b.path()] {
            reset_enrichment_attempts_for_unresolved(root)
                .await
                .unwrap();
        }

        let s_a = read_or_empty(td_a.path()).await.unwrap();
        let s_b = read_or_empty(td_b.path()).await.unwrap();
        assert!(
            !s_a.mods
                .iter()
                .find(|m| m.sha1 == sha_a)
                .unwrap()
                .enrich_attempted
        );
        assert!(
            !s_b.mods
                .iter()
                .find(|m| m.sha1 == sha_b)
                .unwrap()
                .enrich_attempted
        );
    }
}
