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
use crate::mods::modpack::schema::{
    EnvSupport, InertLoaderJar, ModpackUnresolvable, SkippedOverride,
};
use crate::mods::platform::{InstalledMod, ModSource};

const FILE_VERSION: u32 = 4;

/// Process-lifetime SHA-1 cache for files in `mods/`, keyed by path.
/// `reconcile()` re-uses the stored digest when a file's (mtime, size)
/// are unchanged, turning a full read+hash into a cheap `stat`.
static HASH_CACHE: LazyLock<Mutex<HashMap<PathBuf, (SystemTime, u64, String)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Seed [`HASH_CACHE`] for a freshly-written `mods/` file whose digest the
/// caller already knows (the install path SHA-verifies every byte it writes).
/// Without this, the first `list()` after an install/import re-reads and
/// re-hashes every new jar — on a large modpack that is a full extra pass
/// over gigabytes. Best-effort: a failed stat just means `reconcile()` hashes
/// the file the slow way, as before.
pub(crate) fn seed_hash_cache(path: &Path, sha1: &str) {
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    let Ok(mtime) = meta.modified() else {
        return;
    };
    let mut cache = HASH_CACHE.lock().unwrap_or_else(|p| p.into_inner());
    cache.insert(
        path.to_path_buf(),
        (mtime, meta.len(), sha1.to_ascii_lowercase()),
    );
}

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

/// SHA-1 of the bytes the jar `filename` has in `mods_dir` RIGHT NOW, trying
/// the `.disabled` spelling second — the same two paths, in the same order, as
/// [`crate::mods::local::read_jar_for`], so a cache keyed on this digest and a
/// reader that opens the jar can never be talking about different files.
///
/// NOT [`InstalledMod::sha1`]. [`reconcile`] deliberately RETAINS a record's
/// expected digest when the file under that name hashes differently — that is
/// the corrupted-or-externally-replaced jar, and the record is kept so a verify
/// pass can say "expected X, found Y". The registry's digest is therefore the
/// jar the launcher installed, which is not always the jar on disk. Anything
/// keyed on it — the jar-scan cache above all — would answer for bytes that are
/// no longer there.
///
/// Cheap by construction: [`list`]'s `reconcile` has just hashed every file in
/// this directory through the same `(mtime, size)`-keyed [`HASH_CACHE`], so a
/// call right after it is a `stat` plus a map lookup. It re-reads only when the
/// metadata moved since — which is exactly when the cached digest would be
/// wrong. It inherits that shortcut's one bound and adds none: a replacement
/// with identical size AND identical mtime is invisible here, as it already is
/// to the registry every surface in the launcher reads.
///
/// `None` is "could not tell" — no such file, unreadable metadata, failed read
/// — never "no jar". The caller must fall back to reading the bytes, which is
/// exactly what it did before there was a cache.
pub(crate) async fn on_disk_sha1(mods_dir: &Path, filename: &str) -> Option<String> {
    for name in [filename.to_string(), format!("{filename}.disabled")] {
        let path = mods_dir.join(&name);
        let Ok(meta) = fs::metadata(&path).await else {
            continue; // not this spelling — try the other, then give up
        };
        if !meta.is_file() {
            continue;
        }
        let Ok(mtime) = meta.modified() else {
            // A filesystem that cannot report mtime cannot feed HASH_CACHE, and
            // hashing megabytes here to save one unzip is a losing trade.
            return None;
        };
        return match cached_sha1(&path, mtime, meta.len(), || async {
            let bytes = fs::read(&path).await.map_err(|e| io_err(&path, e))?;
            Ok(hex::encode(Sha1::digest(&bytes)))
        })
        .await
        {
            Ok(sha) => Some(sha),
            // A file that stat'd and would not read is ignorance about THIS jar,
            // not licence to answer with the `.disabled` sibling's bytes.
            Err(e) => {
                crate::diag!("[mods] on-disk sha1 for {} failed: {e}", path.display());
                None
            }
        };
    }
    None
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
    /// Bundled `overrides/` files the import deliberately skipped because
    /// they exceeded the per-file size cap (inert non-mod blobs — e.g. a
    /// `.rar` left in `mods/`). `#[serde(default)]` so registry files
    /// written before this feature load with an empty list.
    #[serde(default)]
    pub skipped_overrides: Vec<SkippedOverride>,
    /// User-chosen substitutes for `missing_mods` entries (installed from
    /// another source when the pack's CurseForge file is distribution
    /// disabled). `#[serde(default)]` so registry files written before this
    /// feature load with an empty list.
    #[serde(default)]
    pub resolved_missing: Vec<ResolvedMissing>,
    /// Installed jars built for a loader family this instance cannot load
    /// (inert — e.g. a Fabric jar on a Forge instance). `#[serde(default)]`
    /// so pre-feature registry files load with an empty list.
    #[serde(default)]
    pub inert_loader_jars: Vec<InertLoaderJar>,
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
pub(crate) struct OnDisk {
    #[serde(default = "default_version")]
    version: u32,
    #[serde(default)]
    pub(crate) mods: Vec<InstalledMod>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pack_origin: Option<PackOrigin>,
    /// An external change has been reconciled but not yet announced.
    ///
    /// `reconcile` sets it; only the command that emits `ModsReconciled` clears
    /// it, via [`list_taking_external_change`]. A transient return value cannot
    /// work here — ~20 other commands reconcile this directory, and whichever
    /// ran first would swallow the change silently.
    #[serde(default)]
    pub(crate) external_change_pending: bool,
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

/// Repair registry rows whose `name` predates the project-title convention.
///
/// Rows written before that convention hold `ModVersion.name` — Modrinth's
/// VERSION title ("b0.25.8") or CurseForge's file display name — so every
/// surface reading the registry printed a version string where a mod name
/// belongs. This rewrites them from whatever the caller can resolve offline.
///
/// Closure-injected so the behaviour is unit-testable without an `AppHandle`:
/// the command layer passes a resolver backed by
/// `summary_cache::get_many_cached`, which by its signature cannot reach the
/// network. Nothing here may add a network round — it runs on every read of the
/// installed list AND on every dependency pre-flight.
///
/// Only platform-identified rows are considered. A manual jar's `name` is
/// derived from its filename and is the only thing known about it; rewriting
/// that would destroy information rather than repair it.
///
/// The overwrite is unconditional for a resolved row: for a platform mod the
/// project summary IS the authority on its name, and there is no reliable
/// predicate for "this string is already a project title".
pub async fn backfill_display_names<F, Fut>(instance_root: &Path, resolve: F) -> Result<(), Error>
where
    F: FnOnce(Vec<(ModSource, String)>) -> Fut,
    Fut: std::future::Future<Output = std::collections::HashMap<(ModSource, String), String>>,
{
    let mut state = read_or_empty(instance_root).await?;
    // Deduplicated: two jars of the same project must not be asked for twice.
    // Sorted by id afterwards because `ModSource` is not `Ord` (a `BTreeSet`
    // would need it) and a stable order keeps the resolver's batching — and
    // these tests — deterministic.
    let mut wanted: Vec<(ModSource, String)> = state
        .mods
        .iter()
        .filter_map(|m| Some((m.source?, m.project_id.clone()?)))
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    wanted.sort_by(|a, b| a.1.cmp(&b.1));
    let names = resolve(wanted).await;
    if apply_display_names(&mut state.mods, &names) {
        write(instance_root, &state).await?;
    }
    Ok(())
}

/// Pure half of [`backfill_display_names`]. Returns whether anything changed —
/// the caller persists only then, so a steady state costs no write.
pub(crate) fn apply_display_names(
    mods: &mut [InstalledMod],
    names: &std::collections::HashMap<(ModSource, String), String>,
) -> bool {
    let mut changed = false;
    for m in mods.iter_mut() {
        let (Some(source), Some(pid)) = (m.source, m.project_id.clone()) else {
            continue;
        };
        if let Some(name) = names.get(&(source, pid)) {
            if m.name != *name {
                m.name = name.clone();
                changed = true;
            }
        }
    }
    changed
}

/// `list`, and takes the pending external-change marker: reports whether one was
/// set and clears it in the same write.
///
/// Only a caller that will ANNOUNCE the change may use this. Everything else
/// uses [`list`], which leaves the marker standing.
pub async fn list_taking_external_change(
    instance_root: &Path,
) -> Result<(Vec<InstalledMod>, bool), Error> {
    let mut state = read_or_empty(instance_root).await?;
    let migrated = migrate(&mut state);
    let reconciled = reconcile(instance_root, &mut state).await?;
    let pending = state.external_change_pending;
    state.external_change_pending = false;
    if migrated || reconciled || pending {
        write(instance_root, &state).await?;
    }
    Ok((state.mods, pending))
}

pub(crate) async fn read_or_empty(instance_root: &Path) -> Result<OnDisk, Error> {
    let path = registry_path(instance_root);
    if !fs::try_exists(&path).await.map_err(|e| io_err(&path, e))? {
        return Ok(OnDisk {
            version: FILE_VERSION,
            external_change_pending: false,
            mods: vec![],
            pack_origin: None,
        });
    }
    let bytes = fs::read(&path).await.map_err(|e| io_err(&path, e))?;
    // Corrupt JSON: treat as empty; reconcile will rebuild from disk.
    Ok(serde_json::from_slice::<OnDisk>(&bytes).unwrap_or(OnDisk {
        version: FILE_VERSION,
        external_change_pending: false,
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

    // 1. Update existing JSON entries, fixing filename / enabled drift.
    //    Match order, most specific first:
    //      a. SHA — a renamed but byte-identical jar;
    //      b. exact filename — a jar whose BYTES changed (corruption, or an
    //         external replacement);
    //      c. case-insensitive filename — the same case, on the
    //         case-insensitive filesystems (Windows, default macOS) where
    //         `Sodium.jar` and `sodium.jar` ARE one file.
    //    (b) must be tried before (c): on Linux both names can exist as two
    //    different files, and a case-insensitive-only match would pair the
    //    record with whichever `read_dir` happened to yield first, silently
    //    moving one mod's provenance onto another's file.
    for m in state.mods.iter_mut() {
        let hit = on_disk
            .iter()
            .find(|(_, sha, _)| sha.eq_ignore_ascii_case(&m.sha1))
            .or_else(|| on_disk.iter().find(|(name, _, _)| *name == m.filename))
            .or_else(|| {
                on_disk
                    .iter()
                    .find(|(name, _, _)| name.eq_ignore_ascii_case(&m.filename))
            });
        if let Some((on_disk_name, _, on_disk_enabled)) = hit {
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

    // 2. Drop JSON entries with no file on disk at all. A record whose
    //    FILENAME is still present is RETAINED even when the bytes hash
    //    differently: that is a corrupted or externally-replaced jar, and
    //    dropping it would destroy the source / project / version a repair
    //    needs. The record keeps its EXPECTED sha1, so a later verify pass can
    //    say "expected X, found Y".
    let before = state.mods.len();
    let on_disk_shas: HashSet<String> = on_disk
        .iter()
        .map(|(_, s, _)| s.to_ascii_lowercase())
        .collect();
    let on_disk_names: HashSet<String> = on_disk
        .iter()
        .map(|(n, _, _)| n.to_ascii_lowercase())
        .collect();
    state.mods.retain(|m| {
        on_disk_shas.contains(&m.sha1.to_ascii_lowercase())
            || on_disk_names.contains(&m.filename.to_ascii_lowercase())
    });
    if state.mods.len() != before {
        changed = true;
    }

    // 3. Add synthesized entries for files on disk with no record at all — a
    //    jar the user dropped in. A file whose NAME is already claimed by a
    //    retained record is NOT synthesized: that is the corrupt-jar case from
    //    step 2, and synthesizing it would recreate the anonymous duplicate
    //    step 2 exists to prevent.
    //
    //    `claimed_names` compares EXACTLY, unlike step 2's retention check.
    //    Step 1 already rewrote each retained record's filename to the on-disk
    //    spelling, so the corrupt-jar case matches exactly anyway — while on a
    //    case-sensitive filesystem a genuine second file differing only in case
    //    still gets its own entry. Lowercasing here would instead drop it from
    //    the registry entirely: invisible in the Installed view, impossible to
    //    disable or uninstall, yet still loaded by the game.
    let known_shas: HashSet<String> = state
        .mods
        .iter()
        .map(|m| m.sha1.to_ascii_lowercase())
        .collect();
    let claimed_names: HashSet<String> = state.mods.iter().map(|m| m.filename.clone()).collect();
    for (filename, sha, enabled) in on_disk.iter() {
        if known_shas.contains(&sha.to_ascii_lowercase()) || claimed_names.contains(filename) {
            continue;
        }
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

    // Persisted, not returned: `list` has twenty-one callers and `changed` is a
    // transition. Whichever of them ran first after an external write would
    // consume it, and the one command that announces would always see `false`.
    if changed {
        state.external_change_pending = true;
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

/// Remove every entry whose SHA-1 (case-insensitive) is in `sha1s`, in one
/// read-modify-write. The batch counterpart of [`remove`], used by the
/// install rollback so N deregistrations don't need N registry rewrites.
pub async fn remove_many(instance_root: &Path, sha1s: &HashSet<String>) -> Result<(), Error> {
    if sha1s.is_empty() {
        return Ok(());
    }
    let lowered: HashSet<String> = sha1s.iter().map(|s| s.to_ascii_lowercase()).collect();
    let mut state = read_or_empty(instance_root).await?;
    state
        .mods
        .retain(|x| !lowered.contains(&x.sha1.to_ascii_lowercase()));
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
    resolved: &HashMap<String, crate::mods::platform::ResolvedIdentity>,
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
            // `version_id` is `None` for a loader/MC-ambiguous Modrinth match:
            // record the project (icon/name) but not a misleading version.
            m.version_id = id.version_id.clone();
        }
    }
    write(instance_root, &state).await
}

/// Insert freshly-imported mod records into the registry, replacing any
/// existing entry with the same SHA-1 (idempotent re-import). Used by the
/// launcher-instance import pipeline after copying loose jars.
pub async fn register_imported_mods(
    instance_root: &Path,
    mods: Vec<InstalledMod>,
) -> Result<(), Error> {
    let mut state = read_or_empty(instance_root).await?;
    for m in mods {
        let key = m.sha1.to_ascii_lowercase();
        state.mods.retain(|e| e.sha1.to_ascii_lowercase() != key);
        state.mods.push(m);
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

    /// A record with full provenance, for the corruption tests below.
    fn provenanced(filename: &str, sha1: String) -> InstalledMod {
        InstalledMod {
            filename: filename.into(),
            sha1,
            source: Some(ModSource::Modrinth),
            project_id: Some("AANobbMI".into()),
            version_id: Some("v1".into()),
            name: "Sodium".into(),
            version_number: Some("0.5.8".into()),
            installed_at: "2026-01-01T00:00:00Z".into(),
            enabled: true,
            enrich_attempted: false,
            requires: Vec::new(),
        }
    }

    /// Corruption must not destroy the provenance a repair needs. Matching
    /// records to disk by SHA alone dropped the record ("no file with that
    /// hash") and re-added the file as an anonymous entry — silently turning a
    /// known mod into an unknown local jar. Hardlinks widen that failure from
    /// one instance to every instance sharing the jar.
    #[tokio::test]
    async fn corrupted_known_jar_keeps_its_provenance() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        let good = place_jar(&mods_dir(root), "sodium.jar", b"GOOD-BYTES").await;
        add(root, provenanced("sodium.jar", good.clone()))
            .await
            .unwrap();

        // Corrupt the jar in place — the exact shape an in-place write through
        // a shared hardlink would produce.
        fs::write(mods_dir(root).join("sodium.jar"), b"TRUNCATED")
            .await
            .unwrap();

        let mods = list(root).await.unwrap();

        assert_eq!(
            mods.len(),
            1,
            "must not split into a dropped record + an anonymous entry"
        );
        assert_eq!(
            mods[0].project_id.as_deref(),
            Some("AANobbMI"),
            "provenance must survive"
        );
        assert_eq!(mods[0].version_id.as_deref(), Some("v1"));
        assert_eq!(mods[0].source, Some(ModSource::Modrinth));
        assert_eq!(
            mods[0].sha1, good,
            "the EXPECTED hash is retained so a repair knows what it wants"
        );
    }

    /// The registry's digest and the file's digest are two different facts, and
    /// the jar-scan cache must be keyed on the second. `reconcile` step 2 keeps
    /// the EXPECTED digest on a replaced jar on purpose (pinned by
    /// `corrupted_known_jar_keeps_its_provenance` above); this asserts the two
    /// genuinely diverge and that `on_disk_sha1` reports the file, not the
    /// record.
    #[tokio::test]
    async fn on_disk_sha1_reports_the_file_not_the_registrys_expectation() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        let dir = mods_dir(root);
        let installed_sha = place_jar(&dir, "sodium.jar", b"GOOD-BYTES").await;
        add(root, provenanced("sodium.jar", installed_sha.clone()))
            .await
            .unwrap();

        // Replaced in place, same filename. A DIFFERENT LENGTH on purpose:
        // HASH_CACHE is keyed by (mtime, size), and a same-size rewrite inside
        // one mtime tick is invisible to it — a bound this helper inherits and
        // does not pretend to fix.
        fs::write(dir.join("sodium.jar"), b"REPLACED-WITH-OTHER-BYTES")
            .await
            .unwrap();

        let mods = list(root).await.unwrap();
        assert_eq!(
            mods[0].sha1, installed_sha,
            "the registry keeps what it installed — this is the hazard, not a bug"
        );
        assert_eq!(
            on_disk_sha1(&dir, "sodium.jar").await,
            Some(hex::encode(Sha1::digest(b"REPLACED-WITH-OTHER-BYTES"))),
            "the cache key must describe the bytes that are actually there"
        );
    }

    /// The `.disabled` spelling is the second candidate, matching
    /// `local::read_jar_for`'s order — a key computed from one file and bytes
    /// read from another would be a cache that lies by construction.
    #[tokio::test]
    async fn on_disk_sha1_falls_back_to_the_disabled_spelling_then_gives_up() {
        let td = TempDir::new().unwrap();
        let dir = mods_dir(td.path());
        let sha = place_jar(&dir, "off.jar.disabled", b"DISABLED-BYTES").await;
        assert_eq!(on_disk_sha1(&dir, "off.jar").await, Some(sha));
        assert_eq!(
            on_disk_sha1(&dir, "absent.jar").await,
            None,
            "could not tell — never an invented digest"
        );
    }

    #[tokio::test]
    async fn a_genuinely_new_local_jar_is_still_synthesized() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        place_jar(&mods_dir(root), "dropped.jar", b"USER-JAR").await;

        let mods = list(root).await.unwrap();

        assert_eq!(mods.len(), 1);
        assert_eq!(mods[0].filename, "dropped.jar");
        assert!(
            mods[0].project_id.is_none(),
            "a jar matching no record stays anonymous"
        );
    }

    #[tokio::test]
    async fn corrupt_jar_filename_match_is_case_insensitive() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        let good = place_jar(&mods_dir(root), "Sodium.jar", b"GOOD-BYTES").await;
        // Registry filename differs in case — Windows and macOS filesystems are
        // case-insensitive, so this is one file, not two.
        add(root, provenanced("sodium.jar", good)).await.unwrap();
        fs::write(mods_dir(root).join("Sodium.jar"), b"TRUNCATED")
            .await
            .unwrap();

        let mods = list(root).await.unwrap();

        assert_eq!(mods.len(), 1, "a case-differing filename must still match");
        assert_eq!(mods[0].project_id.as_deref(), Some("AANobbMI"));
    }

    #[tokio::test]
    async fn record_is_still_dropped_when_its_file_is_gone() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        let good = place_jar(&mods_dir(root), "sodium.jar", b"GOOD-BYTES").await;
        add(root, provenanced("sodium.jar", good)).await.unwrap();
        fs::remove_file(mods_dir(root).join("sodium.jar"))
            .await
            .unwrap();

        let mods = list(root).await.unwrap();

        assert!(
            mods.is_empty(),
            "retention is by filename PRESENCE, not by having a record"
        );
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
            skipped_overrides: vec![],
            resolved_missing: Vec::new(),
            inert_loader_jars: vec![],
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
            external_change_pending: false,
            mods: vec![],
            pack_origin: Some(PackOrigin {
                project_id: None,
                source: ModSource::Modrinth,
                project_name: "P".into(),
                version: "1".into(),
                files: vec![mods_file, rp],
                missing_mods: vec![],
                skipped_overrides: vec![],
                resolved_missing: Vec::new(),
                inert_loader_jars: vec![],
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
            external_change_pending: false,
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
            external_change_pending: false,
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
            external_change_pending: false,
            mods: vec![],
            pack_origin: Some(PackOrigin {
                project_id: None,
                source: ModSource::Modrinth,
                project_name: "P".into(),
                version: "1".into(),
                files: vec![rp],
                missing_mods: vec![],
                skipped_overrides: vec![],
                resolved_missing: Vec::new(),
                inert_loader_jars: vec![],
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
        // skipped_overrides is also #[serde(default)] — a registry written
        // before this feature must load with an empty list, not fail.
        assert!(origin.skipped_overrides.is_empty());
        // inert_loader_jars is #[serde(default)] too — pre-feature JSON has no
        // such key and must load as an empty list rather than "missing field".
        assert!(origin.inert_loader_jars.is_empty());
    }

    #[tokio::test]
    async fn pack_origin_inert_loader_jars_round_trip() {
        use crate::mods::modpack::schema::InertLoaderJar;
        let td = TempDir::new().unwrap();
        place_jar(&mods_dir(td.path()), "any.jar", b"any").await;
        let _ = list(td.path()).await.unwrap();
        let mut origin = sample_origin();
        origin.inert_loader_jars = vec![InertLoaderJar {
            filename: "sodium-fabric.jar".into(),
            detected_loader: "Fabric".into(),
        }];
        set_pack_origin(td.path(), origin.clone()).await.unwrap();
        let got = get_pack_origin(td.path()).await.unwrap();
        assert_eq!(got, Some(origin));
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
            crate::mods::platform::ResolvedIdentity {
                source: ModSource::Modrinth,
                project_id: "AANobbMI".into(),
                version_id: Some("vvv".into()),
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
    async fn apply_enrichment_records_project_without_version() {
        // A loader/MC-ambiguous Modrinth match: project recorded for the icon,
        // version_id left None so update-check stays honest (Unknown).
        let td = TempDir::new().unwrap();
        let sha = place_jar(&mods_dir(td.path()), "universal.jar", b"universal").await;
        let _ = list(td.path()).await.unwrap();
        let mut resolved = HashMap::new();
        resolved.insert(
            sha.clone(),
            crate::mods::platform::ResolvedIdentity {
                source: ModSource::Modrinth,
                project_id: "AANobbMI".into(),
                version_id: None,
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
        assert_eq!(mods[0].version_id, None);
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
    async fn register_imported_mods_persists_records() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let recs = vec![
            InstalledMod {
                filename: "sodium.jar".into(),
                sha1: "abc123".into(),
                source: Some(crate::mods::platform::ModSource::Modrinth),
                project_id: Some("AANobbMI".into()),
                version_id: Some("v1".into()),
                name: "sodium.jar".into(),
                version_number: None,
                installed_at: "2026-06-14T00:00:00Z".into(),
                enabled: true,
                enrich_attempted: true,
                requires: vec![],
            },
            InstalledMod {
                filename: "unknown.jar".into(),
                sha1: "def456".into(),
                source: None,
                project_id: None,
                version_id: None,
                name: "unknown.jar".into(),
                version_number: None,
                installed_at: "2026-06-14T00:00:00Z".into(),
                enabled: true,
                enrich_attempted: false,
                requires: vec![],
            },
        ];
        register_imported_mods(root, recs).await.unwrap();
        let state = read_or_empty(root).await.unwrap();
        assert_eq!(state.mods.len(), 2);
        assert!(state
            .mods
            .iter()
            .any(|m| m.filename == "sodium.jar" && m.source.is_some()));
        assert!(state
            .mods
            .iter()
            .any(|m| m.filename == "unknown.jar" && m.source.is_none()));
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
    async fn remove_many_drops_only_listed_shas_in_one_write() {
        let td = TempDir::new().unwrap();
        let sha_a = place_jar(&mods_dir(td.path()), "a.jar", b"aaa").await;
        let sha_b = place_jar(&mods_dir(td.path()), "b.jar", b"bbb").await;
        let _ = list(td.path()).await.unwrap(); // synthesize records
        let mut gone = std::collections::HashSet::new();
        gone.insert(sha_a.to_ascii_uppercase()); // case-insensitive match
        remove_many(td.path(), &gone).await.unwrap();
        let state = read_or_empty(td.path()).await.unwrap();
        assert_eq!(state.mods.len(), 1);
        assert!(state.mods[0].sha1.eq_ignore_ascii_case(&sha_b));
    }

    #[tokio::test]
    async fn remove_many_with_empty_set_is_a_no_op() {
        let td = TempDir::new().unwrap();
        let sha = place_jar(&mods_dir(td.path()), "keep.jar", b"keep").await;
        let _ = list(td.path()).await.unwrap();
        remove_many(td.path(), &std::collections::HashSet::new())
            .await
            .unwrap();
        let state = read_or_empty(td.path()).await.unwrap();
        assert_eq!(state.mods.len(), 1);
        assert!(state.mods[0].sha1.eq_ignore_ascii_case(&sha));
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

    /// A jar that appears in `mods/` without going through us — a pack's own
    /// downloader mod, or the user dropping a file in — must stay announceable
    /// until somebody announces it. Twenty other commands reconcile this same
    /// directory; a transient flag would be eaten by whichever ran first.
    #[tokio::test]
    async fn an_external_change_survives_until_it_is_taken() {
        let td = tempfile::TempDir::new().unwrap();
        let root = td.path();
        let dir = mods_dir(root);
        tokio::fs::create_dir_all(&dir).await.unwrap();

        // Quiet instance: nothing to announce.
        let (mods, pending) = list_taking_external_change(root).await.unwrap();
        assert!(mods.is_empty());
        assert!(!pending);

        // A jar appears without us.
        tokio::fs::write(dir.join("outsider.jar"), b"not really a jar")
            .await
            .unwrap();

        // Another command lists first. It reconciles and persists — and must
        // NOT consume the marker.
        let mods = list(root).await.unwrap();
        assert_eq!(mods.len(), 1);
        assert_eq!(mods[0].filename, "outsider.jar");
        assert!(mods[0].source.is_none(), "not ours: a manual mod");
        assert_eq!(list(root).await.unwrap().len(), 1, "still not consumed");

        // The announcing caller finally takes it.
        let (_, pending) = list_taking_external_change(root).await.unwrap();
        assert!(pending, "the marker outlived the other listings");

        // Taken once, never twice.
        let (_, pending) = list_taking_external_change(root).await.unwrap();
        assert!(!pending);
    }

    // ── display-name backfill ────────────────────────────────────────────
    //
    // Rows written before the project-title convention hold `ModVersion.name`
    // — the platform VERSION title ("b0.25.8"), not the mod's name. These
    // tests read back through `read_or_empty` rather than `list`, because
    // `list` reconciles against `mods/` and would drop fixture rows that have
    // no jar on disk, measuring the wrong thing.

    /// A row with a resolvable project id takes the cached project title.
    #[tokio::test]
    async fn backfill_rewrites_platform_rows_from_the_resolver() {
        let td = TempDir::new().unwrap();
        let mut row = provenanced("opac.jar", "sha-a".into());
        row.project_id = Some("bo89PdrX".into());
        row.name = "b0.25.8".into();
        let state = OnDisk {
            mods: vec![row],
            ..Default::default()
        };
        write(td.path(), &state).await.unwrap();

        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let rec = calls.clone();
        backfill_display_names(td.path(), |wanted| {
            rec.lock().unwrap().push(wanted.clone());
            async move {
                let mut m = std::collections::HashMap::new();
                for k in wanted {
                    m.insert(k, "Open Parties and Claims".to_string());
                }
                m
            }
        })
        .await
        .unwrap();

        let after = read_or_empty(td.path()).await.unwrap();
        assert_eq!(after.mods[0].name, "Open Parties and Claims");
        assert_eq!(
            calls.lock().unwrap().as_slice(),
            [vec![(ModSource::Modrinth, "bo89PdrX".to_string())]]
        );
    }

    /// A cold cache degrades to a no-op. Never to a worse name, and never to a
    /// network round to go find one.
    #[tokio::test]
    async fn backfill_leaves_the_row_alone_when_the_resolver_knows_nothing() {
        let td = TempDir::new().unwrap();
        let mut row = provenanced("opac.jar", "sha-a".into());
        row.name = "b0.25.8".into();
        let state = OnDisk {
            mods: vec![row],
            ..Default::default()
        };
        write(td.path(), &state).await.unwrap();

        backfill_display_names(td.path(), |_| async { std::collections::HashMap::new() })
            .await
            .unwrap();

        let after = read_or_empty(td.path()).await.unwrap();
        assert_eq!(after.mods[0].name, "b0.25.8");
    }

    /// A manual jar's name is derived from its filename and is the only thing
    /// known about it. Rewriting that would destroy information, so such rows
    /// are not even offered to the resolver.
    #[tokio::test]
    async fn backfill_never_touches_a_jar_with_no_platform_identity() {
        let td = TempDir::new().unwrap();
        let mut manual = provenanced("manual.jar", "sha-m".into());
        manual.source = None;
        manual.project_id = None;
        manual.name = "manual.jar".into();
        let state = OnDisk {
            mods: vec![manual],
            ..Default::default()
        };
        write(td.path(), &state).await.unwrap();

        backfill_display_names(td.path(), |wanted| {
            assert!(
                wanted.is_empty(),
                "a row with no platform identity must never be queried"
            );
            async { std::collections::HashMap::new() }
        })
        .await
        .unwrap();

        let after = read_or_empty(td.path()).await.unwrap();
        assert_eq!(after.mods[0].name, "manual.jar");
    }

    /// Idempotence: a second run over already-correct rows must not rewrite the
    /// file. Both reading commands call this on every read, so a version that
    /// dirtied the registry each time would rewrite it on every list.
    #[tokio::test]
    async fn backfill_does_not_rewrite_when_nothing_changes() {
        let td = TempDir::new().unwrap();
        let mut row = provenanced("opac.jar", "sha-a".into());
        row.name = "Sodium".into();
        let state = OnDisk {
            mods: vec![row],
            ..Default::default()
        };
        write(td.path(), &state).await.unwrap();
        let before = tokio::fs::metadata(registry_path(td.path()))
            .await
            .unwrap()
            .modified()
            .unwrap();

        backfill_display_names(td.path(), |wanted| async move {
            wanted
                .into_iter()
                .map(|k| (k, "Sodium".to_string()))
                .collect()
        })
        .await
        .unwrap();

        let after = tokio::fs::metadata(registry_path(td.path()))
            .await
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(before, after, "an unchanged registry must not be rewritten");
    }

    /// Two rows of the same project ask once, not twice.
    #[tokio::test]
    async fn backfill_deduplicates_the_ids_it_asks_for() {
        let td = TempDir::new().unwrap();
        let mut a = provenanced("a.jar", "sha-a".into());
        a.name = "old".into();
        let mut b = provenanced("b.jar", "sha-b".into());
        b.name = "old".into();
        let state = OnDisk {
            mods: vec![a, b],
            ..Default::default()
        };
        write(td.path(), &state).await.unwrap();

        backfill_display_names(td.path(), |wanted| {
            assert_eq!(wanted.len(), 1, "the same project must be asked for once");
            async move {
                wanted
                    .into_iter()
                    .map(|k| (k, "Sodium".to_string()))
                    .collect()
            }
        })
        .await
        .unwrap();

        let after = read_or_empty(td.path()).await.unwrap();
        assert!(after.mods.iter().all(|m| m.name == "Sodium"));
    }
}
