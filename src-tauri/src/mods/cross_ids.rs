//! A mod is one mod on both platforms (spec 2026-10-08).
//!
//! A mod published on Modrinth and on CurseForge has a project id on each. An installed jar knows
//! only the one it was installed from, so a dependency another mod declares by the OTHER
//! platform's id read as missing — the main mod of a pack installed from Modrinth, its addons from
//! CurseForge. This module learns each installed jar's identity on the other platform from its
//! bytes — CurseForge by Murmur2 fingerprint confirmed by SHA-1, Modrinth by SHA-1 — and keeps it
//! in `{instance}/lucerna/cross-ids.json`:
//!
//! - `aliases`: own project key → the same project's id on the other platform. Project to project,
//!   so an update (new bytes) keeps it at once.
//! - `checked`: per jar SHA-1, what each platform answered about these bytes and when, and the
//!   cached fingerprint. Only answers are stored — found or absent; a question that could not be
//!   asked or answered (no key, offline, a refused key, a failed request) stays unknown and is
//!   asked again next time. An absent answer is asked again after a week.
//!
//! [`alias_map`] turns the file into the map every «is this project installed?» question
//! consults: the dependency graph, the install guards, the update and migration prunes. Update
//! checks, holds, changelogs and project pages keep the jar's own platform.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::error::Error;
use crate::mods::deps::ProjectKey;
use crate::mods::installed;
use crate::mods::platform::{InstalledMod, ModSource};

const FILE_NAME: &str = "cross-ids.json";
const FILE_VERSION: u32 = 1;
/// An «absent» answer is asked again after this long: a platform can approve the file, or its
/// author upload it, later.
const ABSENT_RECHECK_SECS: i64 = 7 * 24 * 60 * 60;
/// How long CurseForge is left alone after it refused the key: a dead key is not asked again on
/// every refresh of the list. The key itself is never touched here.
const CF_REFUSED_BACKOFF: Duration = Duration::from_secs(60 * 60);
/// Hashes per request, as the platform clients batch them.
const CHUNK: usize = 100;

static WRITE_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ProbeState {
    Found,
    Absent,
}

/// What one platform answered about one jar's bytes, and when (unix seconds).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Probe {
    state: ProbeState,
    at: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct Checked {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fingerprint: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    modrinth: Option<Probe>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    curseforge: Option<Probe>,
}

impl Checked {
    fn probe(&self, platform: ModSource) -> Option<Probe> {
        match platform {
            ModSource::Modrinth => self.modrinth,
            ModSource::Curseforge => self.curseforge,
            _ => None,
        }
    }
    fn set_probe(&mut self, platform: ModSource, probe: Probe) {
        match platform {
            ModSource::Modrinth => self.modrinth = Some(probe),
            ModSource::Curseforge => self.curseforge = Some(probe),
            _ => {}
        }
    }
}

/// The sidecar as it is on disk.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OnDisk {
    #[serde(default)]
    version: u32,
    /// Own project key (`modrinth:MJX7HTHx`) → { other platform → its project id there }.
    #[serde(default)]
    aliases: BTreeMap<String, BTreeMap<String, String>>,
    /// Jar SHA-1 (lowercased) → what the other platform said about these bytes.
    #[serde(default)]
    checked: BTreeMap<String, Checked>,
}

/// One project identity: its platform and its id there.
pub type Ident = (ModSource, String);

fn platform_name(source: ModSource) -> Option<&'static str> {
    match source {
        ModSource::Modrinth => Some("modrinth"),
        ModSource::Curseforge => Some("curseforge"),
        _ => None,
    }
}

fn platform_named(name: &str) -> Option<ModSource> {
    match name {
        "modrinth" => Some(ModSource::Modrinth),
        "curseforge" => Some(ModSource::Curseforge),
        _ => None,
    }
}

/// The platform a jar is asked about: the one it was NOT installed from.
fn other_platform(source: ModSource) -> Option<ModSource> {
    match source {
        ModSource::Modrinth => Some(ModSource::Curseforge),
        ModSource::Curseforge => Some(ModSource::Modrinth),
        _ => None,
    }
}

fn key(ident: &Ident) -> String {
    crate::mods::depgraph::key(ident.0, &ident.1)
}

/// A row's own identity — only Modrinth and CurseForge rows take part (pack-only sources and
/// source-less jars have no other platform to be asked about).
fn own_of(m: &InstalledMod) -> Option<Ident> {
    let source = m.source?;
    other_platform(source)?;
    Some((source, m.project_id.clone()?))
}

/// Where the sidecar lives.
pub fn path(instance_root: &Path) -> PathBuf {
    installed::registry_dir(instance_root).join(FILE_NAME)
}

/// Serialises the read-merge-write. One lock for every instance: a pass is a background task.
/// Not `registry_lock` — that one is the two registries' own
/// (`tests/structural_registry_rmw_lock.rs`, rule 3). Never held across a network call.
fn sidecar_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

/// The sidecar, or an empty one: it is a derived cache, so an unreadable file is relearned, never
/// an error for the caller.
pub async fn load(instance_root: &Path) -> OnDisk {
    match read(instance_root).await {
        Ok(d) => d,
        Err(e) => {
            crate::diag!(
                "[cross-ids] cannot read {}: {e} — relearning",
                path(instance_root).display()
            );
            OnDisk::default()
        }
    }
}

/// The sidecar as [`load`] reads it, but a file that is there and cannot be read is an error, not
/// an empty map: for a caller that keeps what it had rather than act on «no aliases» it cannot
/// vouch for (`mods_cross_aliases`). Absent is empty; unparsable is empty too — the next pass
/// rewrites it whole, as it would a missing one.
pub async fn read(instance_root: &Path) -> std::io::Result<OnDisk> {
    let p = path(instance_root);
    let bytes = match fs::read(&p).await {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(OnDisk::default()),
        Err(e) => return Err(e),
    };
    match serde_json::from_slice::<OnDisk>(&bytes) {
        Ok(d) => Ok(d),
        Err(e) => {
            crate::diag!(
                "[cross-ids] {} is unreadable: {e} — relearning",
                p.display()
            );
            Ok(OnDisk::default())
        }
    }
}

/// Temp-then-rename, the `holds::save` shape.
async fn save(instance_root: &Path, disk: &OnDisk) -> std::io::Result<()> {
    fs::create_dir_all(installed::registry_dir(instance_root)).await?;
    let final_path = path(instance_root);
    let seq = WRITE_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = final_path.with_extension(format!("json.tmp.{}.{seq}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(disk).map_err(std::io::Error::other)?;
    if let Err(e) = write_then_rename(&tmp, &final_path, &bytes).await {
        // The temp file is this write's own and useless now: removed. One that never came to be
        // is fine; one that cannot be removed is a stray file no read ever opens — said. The
        // write's error is what the caller gets.
        match fs::remove_file(&tmp).await {
            Ok(()) => {}
            Err(rm) if rm.kind() == std::io::ErrorKind::NotFound => {}
            Err(rm) => crate::diag!("[cross-ids] cannot remove {}: {rm}", tmp.display()),
        }
        return Err(e);
    }
    Ok(())
}

async fn write_then_rename(tmp: &Path, final_path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    fs::write(tmp, bytes).await?;
    fs::rename(tmp, final_path).await
}

/// Write `disk` if the instance is still there, and say whether it was written. A pass spans
/// network time: an instance deleted meanwhile does not get its `lucerna/` back.
async fn write_if_present(instance_root: &Path, disk: &OnDisk) -> bool {
    match fs::metadata(instance_root).await {
        Ok(meta) if meta.is_dir() => match save(instance_root, disk).await {
            Ok(()) => true,
            Err(e) => {
                crate::diag!(
                    "[cross-ids] cannot write {}: {e} — asked again next pass",
                    path(instance_root).display()
                );
                false
            }
        },
        Ok(_) => {
            crate::diag!(
                "[cross-ids] {} is not a directory — not written",
                instance_root.display()
            );
            false
        }
        Err(e) => {
            crate::diag!(
                "[cross-ids] {} is gone ({e}) — not written",
                instance_root.display()
            );
            false
        }
    }
}

/// Alias identity → the installed row's own identity: what every «is this project installed?»
/// question consults besides the rows' own keys.
#[derive(Debug, Clone, Default)]
pub struct AliasMap {
    by_alias: HashMap<Ident, Ident>,
}

impl AliasMap {
    /// The installed project `(source, project_id)` is the same as, when it is an alias.
    pub fn own_of(&self, source: ModSource, project_id: &str) -> Option<&Ident> {
        self.by_alias.get(&(source, project_id.to_string()))
    }

    /// Every alias, with the row's own identity it stands for.
    pub fn pairs(&self) -> impl Iterator<Item = (&Ident, &Ident)> {
        self.by_alias.iter()
    }

    /// Every alias as an install-pruning key. A CurseForge id that is not a number cannot be a
    /// `ProjectKey`: it is left out (said), never a panic.
    pub fn project_keys(&self) -> HashSet<ProjectKey> {
        self.project_keys_where(|_| true)
    }

    /// The aliases of the given own identities only.
    pub fn project_keys_of<'a, I>(&self, owns: I) -> HashSet<ProjectKey>
    where
        I: IntoIterator<Item = &'a Ident>,
    {
        let owns: HashSet<&Ident> = owns.into_iter().collect();
        self.project_keys_where(|own| owns.contains(own))
    }

    fn project_keys_where(&self, keep: impl Fn(&Ident) -> bool) -> HashSet<ProjectKey> {
        self.by_alias
            .iter()
            .filter(|(_, own)| keep(own))
            .filter_map(|((source, pid), _)| match source {
                ModSource::Modrinth => Some(ProjectKey::Modrinth(pid.clone())),
                ModSource::Curseforge => match pid.parse() {
                    Ok(id) => Some(ProjectKey::Curseforge(id)),
                    Err(e) => {
                        crate::diag!("[cross-ids] CurseForge alias {pid:?} is not an id: {e}");
                        None
                    }
                },
                _ => None,
            })
            .collect()
    }

    #[cfg(test)]
    pub fn from_pairs(pairs: &[(Ident, Ident)]) -> AliasMap {
        AliasMap {
            by_alias: pairs.iter().cloned().collect(),
        }
    }
}

/// The alias map for the CURRENT rows (spec §5): an alias that is itself the own identity of a
/// current row is dropped — a jar never stands in for a project installed in its own right, or
/// two rows would swap identities through mutual aliases; one step only; two rows claiming one
/// alias — an enabled row's wins, then the lower own key.
pub fn alias_map(disk: &OnDisk, rows: &[InstalledMod]) -> AliasMap {
    let mut owns: HashMap<Ident, bool> = HashMap::new();
    for m in rows {
        if let Some(own) = own_of(m) {
            *owns.entry(own).or_insert(false) |= m.enabled;
        }
    }
    let mut candidates: Vec<(Ident, Ident, bool)> = Vec::new();
    for (own, enabled) in &owns {
        let Some(entry) = disk.aliases.get(&key(own)) else {
            continue;
        };
        for (name, pid) in entry {
            let Some(source) = platform_named(name) else {
                continue;
            };
            let alias = (source, pid.clone());
            if source == own.0 || owns.contains_key(&alias) {
                continue;
            }
            candidates.push((alias, own.clone(), *enabled));
        }
    }
    candidates.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| key(&a.1).cmp(&key(&b.1))));
    let mut by_alias = HashMap::new();
    for (alias, own, _) in candidates {
        by_alias.entry(alias).or_insert(own);
    }
    AliasMap { by_alias }
}

/// The alias map of an instance: its sidecar over its current rows.
pub async fn load_alias_map(instance_root: &Path, rows: &[InstalledMod]) -> AliasMap {
    alias_map(&load(instance_root).await, rows)
}

/// What a pass learned about one jar from one platform.
struct Answer {
    sha: String,
    own: Ident,
    platform: ModSource,
    /// The project id there, or `None` — the platform does not have these bytes.
    found: Option<String>,
}

/// One jar a pass asks about.
#[derive(Clone)]
struct Ask {
    sha: String,
    own: Ident,
    path: PathBuf,
    fingerprint: Option<u32>,
}

/// Whether a jar's other platform still has to be asked: never answered, or found but its alias
/// is gone (the row was removed and came back), or absent for longer than a week.
fn needs_asking(probe: Option<Probe>, alias_present: bool, now: i64) -> bool {
    match probe {
        None => true,
        Some(Probe {
            state: ProbeState::Found,
            ..
        }) => !alias_present,
        Some(Probe {
            state: ProbeState::Absent,
            at,
        }) => now - at >= ABSENT_RECHECK_SECS,
    }
}

/// When CurseForge (at `base`) last refused a key, per base and key: a dead key is not asked
/// again on every refresh, and a new key is asked at once. The key is kept only as a hash.
fn cf_refused() -> &'static Mutex<HashMap<String, Instant>> {
    static REFUSED: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();
    REFUSED.get_or_init(|| Mutex::new(HashMap::new()))
}

fn refusal_key(base: &str, key: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut h);
    format!("{base}#{:x}", h.finish())
}

fn cf_refused_recently(base: &str, key: &str) -> bool {
    let k = refusal_key(base, key);
    let recent = |map: &HashMap<String, Instant>| {
        map.get(&k)
            .is_some_and(|at| at.elapsed() < CF_REFUSED_BACKOFF)
    };
    match cf_refused().lock() {
        Ok(map) => recent(&map),
        // A poisoned map only ever held instants: read it as it is.
        Err(poisoned) => recent(&poisoned.into_inner()),
    }
}

fn mark_cf_refused(base: &str, key: &str) {
    let k = refusal_key(base, key);
    match cf_refused().lock() {
        Ok(mut map) => {
            map.insert(k, Instant::now());
        }
        Err(poisoned) => {
            poisoned.into_inner().insert(k, Instant::now());
        }
    }
}

#[derive(Deserialize)]
struct CfEnvelope {
    data: CfData,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfData {
    exact_matches: Vec<CfMatch>,
}

#[derive(Deserialize)]
struct CfMatch {
    file: CfFile,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfFile {
    mod_id: u32,
    file_fingerprint: u32,
    #[serde(default)]
    hashes: Vec<CfHash>,
}

#[derive(Deserialize)]
struct CfHash {
    value: String,
    algo: u32,
}

/// CurseForge's `hashes[].algo` for SHA-1.
const CF_ALGO_SHA1: u32 = 1;

enum CfOutcome {
    /// Answered. `found`: SHA-1 → mod id, for the jars it has. `unconfirmed`: jars whose
    /// fingerprint it matched with no SHA-1 to confirm it by — could not tell. Any other asked jar
    /// is absent.
    Answered {
        found: HashMap<String, String>,
        unconfirmed: HashSet<String>,
    },
    /// 401 / 403: the key was refused.
    Refused,
    /// Could not tell: transport, another status, an unreadable answer.
    Failed,
}

/// Ask CurseForge which of these jars (one chunk) it has. A fingerprint match counts only when
/// the file's SHA-1 is the jar's: a 32-bit fingerprint alone can collide with an unrelated file.
/// Never clears the key on a refusal (unlike `CurseForgeClient::files_by_fingerprint`): this
/// runs in the background and must not end the user's interactive session.
async fn resolve_curseforge(base: &str, key: &str, chunk: &[(u32, String)]) -> CfOutcome {
    let fingerprints: Vec<u32> = chunk.iter().map(|(fp, _)| *fp).collect();
    let url = format!("{base}/v1/fingerprints");
    let body = serde_json::to_vec(&serde_json::json!({ "fingerprints": fingerprints }))
        .expect("a fixed-shape JSON object always serializes");
    let resp = match crate::network::request::post(
        &url,
        &[("x-api-key", key), ("content-type", "application/json")],
        &body,
        "mods",
    )
    .await
    {
        Ok(r) => r,
        Err(e) => {
            crate::diag!("[cross-ids] curseforge fingerprints failed: {e}");
            return CfOutcome::Failed;
        }
    };
    if resp.status == 401 || resp.status == 403 {
        crate::diag!(
            "[cross-ids] curseforge refused the key (HTTP {})",
            resp.status
        );
        return CfOutcome::Refused;
    }
    if !(200..300).contains(&resp.status) {
        crate::diag!("[cross-ids] curseforge fingerprints HTTP {}", resp.status);
        return CfOutcome::Failed;
    }
    let parsed: CfEnvelope = match serde_json::from_slice(&resp.body) {
        Ok(p) => p,
        Err(e) => {
            crate::diag!("[cross-ids] curseforge fingerprints unreadable: {e}");
            return CfOutcome::Failed;
        }
    };
    let mut found = HashMap::new();
    let mut unconfirmed = HashSet::new();
    for m in parsed.data.exact_matches {
        let ours: Vec<&String> = chunk
            .iter()
            .filter(|(fp, _)| *fp == m.file.file_fingerprint)
            .map(|(_, sha)| sha)
            .collect();
        if ours.is_empty() {
            continue;
        }
        let file_sha = m
            .file
            .hashes
            .iter()
            .find(|h| h.algo == CF_ALGO_SHA1)
            .map(|h| h.value.to_ascii_lowercase());
        match file_sha {
            Some(file_sha) => {
                if ours.iter().any(|sha| **sha == file_sha) {
                    found.insert(file_sha, m.file.mod_id.to_string());
                }
            }
            None => unconfirmed.extend(ours.into_iter().cloned()),
        }
    }
    CfOutcome::Answered { found, unconfirmed }
}

/// Fingerprint the jars that have none cached. A jar that cannot be read is left out (said): it
/// stays unknown and is asked about next time.
async fn fingerprint(asks: Vec<Ask>) -> Result<Vec<Ask>, Error> {
    tokio::task::spawn_blocking(move || {
        asks.into_iter()
            .filter_map(|mut a| {
                if a.fingerprint.is_some() {
                    return Some(a);
                }
                match std::fs::read(&a.path) {
                    Ok(bytes) => {
                        a.fingerprint = Some(crate::mods::enrich::curseforge_fingerprint(&bytes));
                        Some(a)
                    }
                    Err(e) => {
                        crate::diag!("[cross-ids] cannot read {}: {e}", a.path.display());
                        None
                    }
                }
            })
            .collect()
    })
    .await
    .map_err(|e| Error::ModsCacheIo {
        details: format!("fingerprint task join: {e}"),
    })
}

/// One learning pass over an instance (spec §4). Returns how many aliases are new or changed.
/// Nothing to ask → no request and no file read. Best-effort: platform failures leave their jars
/// unknown; only reading the registry is an error. This has no AppHandle: a command that runs a
/// pass emits `ModsCrossIdsLearned` when it learned something, so the views re-read (spec
/// 2026-10-08 aliases-everywhere D4).
pub async fn learn(
    instance_root: &Path,
    modrinth_base: &str,
    cf_base: &str,
    cf_key: Option<&str>,
    now: i64,
) -> Result<u32, Error> {
    let rows = installed::list(instance_root).await?;
    let disk = load(instance_root).await;
    let mods_dir = installed::mods_dir(instance_root);
    let mut ask_cf: Vec<Ask> = Vec::new();
    let mut ask_mr: Vec<Ask> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for m in &rows {
        let Some(own) = own_of(m) else {
            continue;
        };
        let Some(other) = other_platform(own.0) else {
            continue;
        };
        let sha = m.sha1.to_ascii_lowercase();
        if !seen.insert(sha.clone()) {
            continue;
        }
        let checked = disk.checked.get(&sha);
        let alias_present = platform_name(other).is_some_and(|name| {
            disk.aliases
                .get(&key(&own))
                .is_some_and(|e| e.contains_key(name))
        });
        if !needs_asking(checked.and_then(|c| c.probe(other)), alias_present, now) {
            continue;
        }
        let path = mods_dir.join(installed::on_disk_name(m));
        let ask = Ask {
            sha,
            own,
            path,
            fingerprint: checked.and_then(|c| c.fingerprint),
        };
        match other {
            ModSource::Curseforge => ask_cf.push(ask),
            _ => ask_mr.push(ask),
        }
    }
    if ask_cf.is_empty() && ask_mr.is_empty() {
        return Ok(0);
    }

    let mut answers: Vec<Answer> = Vec::new();
    let mut fingerprints: Vec<(String, u32)> = Vec::new();

    // CurseForge: only with a key that was not just refused — otherwise these jars stay unknown,
    // and none of them is read. One request per chunk: a chunk that fails leaves only its own jars
    // unknown.
    match cf_key {
        Some(k) if !ask_cf.is_empty() && !cf_refused_recently(cf_base, k) => {
            let asked = fingerprint(ask_cf).await?;
            for a in &asked {
                if let Some(fp) = a.fingerprint {
                    fingerprints.push((a.sha.clone(), fp));
                }
            }
            for chunk in asked.chunks(CHUNK) {
                let jars: Vec<(u32, String)> = chunk
                    .iter()
                    .filter_map(|a| Some((a.fingerprint?, a.sha.clone())))
                    .collect();
                match resolve_curseforge(cf_base, k, &jars).await {
                    CfOutcome::Answered { found, unconfirmed } => {
                        for a in chunk.iter().filter(|a| !unconfirmed.contains(&a.sha)) {
                            answers.push(Answer {
                                sha: a.sha.clone(),
                                own: a.own.clone(),
                                platform: ModSource::Curseforge,
                                found: found.get(&a.sha).cloned(),
                            });
                        }
                    }
                    CfOutcome::Refused => {
                        mark_cf_refused(cf_base, k);
                        break;
                    }
                    CfOutcome::Failed => {}
                }
            }
        }
        _ => {}
    }

    // Modrinth: by SHA-1, no key; per chunk, like CurseForge.
    let client = crate::mods::modrinth::ModrinthClient::with_base(modrinth_base);
    for chunk in ask_mr.chunks(CHUNK) {
        let shas: Vec<&str> = chunk.iter().map(|a| a.sha.as_str()).collect();
        match client.project_ids_by_hash(&shas).await {
            Ok(found) => {
                for a in chunk {
                    answers.push(Answer {
                        sha: a.sha.clone(),
                        own: a.own.clone(),
                        platform: ModSource::Modrinth,
                        found: found.get(&a.sha).cloned(),
                    });
                }
            }
            Err(e) => crate::diag!("[cross-ids] modrinth version_files failed: {e}"),
        }
    }

    // Nothing answered and nothing read: an unknown is never written.
    if answers.is_empty() && fingerprints.is_empty() {
        return Ok(0);
    }
    merge(instance_root, &answers, &fingerprints, now).await
}

/// Fold a pass's answers into the sidecar under its lock, against the registry as it is NOW (an
/// overlapping pass's entries survive; rows removed meanwhile are pruned), and write if anything
/// changed and the instance still exists. Returns how many aliases the WRITTEN file has that it
/// did not have before: what is not written is not learned — every reader takes the file.
async fn merge(
    instance_root: &Path,
    answers: &[Answer],
    fingerprints: &[(String, u32)],
    now: i64,
) -> Result<u32, Error> {
    let _guard = sidecar_lock().lock().await;
    let rows = installed::list(instance_root).await?;
    let before = load(instance_root).await;
    let mut disk = before.clone();

    for (sha, fp) in fingerprints {
        disk.checked.entry(sha.clone()).or_default().fingerprint = Some(*fp);
    }
    for a in answers {
        let state = if a.found.is_some() {
            ProbeState::Found
        } else {
            ProbeState::Absent
        };
        disk.checked
            .entry(a.sha.clone())
            .or_default()
            .set_probe(a.platform, Probe { state, at: now });
        let (Some(id), Some(name)) = (&a.found, platform_name(a.platform)) else {
            continue;
        };
        let entry = disk.aliases.entry(key(&a.own)).or_default();
        if let Some(old) = entry.insert(name.to_string(), id.clone()) {
            if &old != id {
                crate::diag!(
                    "[cross-ids] {} on {name} is now {id} (was {old})",
                    key(&a.own)
                );
            }
        }
    }

    // Prune against the current rows: bookkeeping of bytes no longer installed, aliases of
    // projects no longer installed.
    let shas: HashSet<String> = rows.iter().map(|m| m.sha1.to_ascii_lowercase()).collect();
    let owns: HashSet<String> = rows.iter().filter_map(own_of).map(|o| key(&o)).collect();
    disk.checked.retain(|sha, _| shas.contains(sha));
    disk.aliases.retain(|own, _| owns.contains(own));

    if disk == before {
        return Ok(0);
    }
    let learned = disk
        .aliases
        .iter()
        .flat_map(|(own, by)| by.iter().map(move |(name, id)| (own, name, id)))
        .filter(|(own, name, id)| before.aliases.get(*own).and_then(|b| b.get(*name)) != Some(*id))
        .count();
    disk.version = FILE_VERSION;
    if !write_if_present(instance_root, &disk).await {
        return Ok(0);
    }
    // safe: bounded by the installed rows, nowhere near 2^32.
    Ok(learned as u32)
}

/// Record that `other` is `own`'s project on the other platform, learned from the two jars' own
/// descriptor ids rather than their bytes (spec 2026-10-09 same-mod-by-id D5): an install met a
/// jar carrying exactly the ids the installed row carries. Written only
/// - from one platform to the other (an alias maps Modrinth and CurseForge onto each other);
/// - for a row still installed — re-listed under the lock, like `merge`, so an alias of a row
///   removed meanwhile is never written (`merge` would prune it anyway);
/// - into an empty slot — an alias learned from bytes, or an earlier one, is never replaced: the
///   bytes say more than a descriptor (a later pass that FINDS the bytes still replaces this one);
/// - when no other row claims `other` already — `alias_map` would break that tie silently.
///
/// `Ok(true)` when the file now has the alias and did not before. Not written (an instance gone,
/// a failed write) is `Ok(false)`, said in the log by `write_if_present`.
pub async fn remember(instance_root: &Path, own: &Ident, other: &Ident) -> Result<bool, Error> {
    let Some(name) = platform_name(other.0) else {
        return Ok(false);
    };
    if other_platform(own.0) != Some(other.0) {
        return Ok(false);
    }
    let _guard = sidecar_lock().lock().await;
    let rows = installed::list(instance_root).await?;
    let owns: HashSet<String> = rows.iter().filter_map(own_of).map(|o| key(&o)).collect();
    // A project installed in its own right is no alias (`alias_map` drops it).
    if !owns.contains(&key(own)) || owns.contains(&key(other)) {
        return Ok(false);
    }
    let mut disk = load(instance_root).await;
    // Another current row's alias already — a stale entry of a row gone is `merge`'s to prune.
    let claimed = disk.aliases.iter().any(|(by, slots)| {
        by != &key(own) && owns.contains(by) && slots.get(name) == Some(&other.1)
    });
    if claimed {
        crate::diag!(
            "[cross-ids] {} is another row's alias already — not {}'s",
            key(other),
            key(own)
        );
        return Ok(false);
    }
    let slot = disk.aliases.entry(key(own)).or_default();
    match slot.get(name) {
        Some(id) if id == &other.1 => return Ok(false),
        Some(id) => {
            crate::diag!(
                "[cross-ids] {} on {name} stays {id}; its ids also match {}",
                key(own),
                other.1
            );
            return Ok(false);
        }
        None => {
            slot.insert(name.to_string(), other.1.clone());
        }
    }
    disk.version = FILE_VERSION;
    Ok(write_if_present(instance_root, &disk).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    use sha1::{Digest, Sha1};
    use tempfile::TempDir;
    use wiremock::matchers::{method, path as wpath};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const NOW: i64 = 1_760_000_000;

    /// Put a jar in `mods/` and register it under `source:pid`; returns its SHA-1.
    async fn add_jar(
        root: &Path,
        filename: &str,
        bytes: &[u8],
        source: ModSource,
        pid: &str,
        enabled: bool,
    ) -> String {
        let dir = installed::mods_dir(root);
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let on_disk = if enabled {
            filename.to_string()
        } else {
            format!("{filename}.disabled")
        };
        tokio::fs::write(dir.join(on_disk), bytes).await.unwrap();
        let sha = hex::encode(Sha1::digest(bytes));
        installed::add(
            root,
            InstalledMod {
                filename: filename.into(),
                sha1: sha.clone(),
                source: Some(source),
                project_id: Some(pid.into()),
                version_id: Some("v".into()),
                name: pid.into(),
                version_number: Some("1".into()),
                installed_at: "2026-01-01T00:00:00Z".into(),
                enabled,
                enrich_attempted: false,
                requires: Vec::new(),
            },
        )
        .await
        .unwrap();
        sha
    }

    fn cf_body(fp: u32, mod_id: u32, sha: &str) -> serde_json::Value {
        serde_json::json!({ "data": { "exactMatches": [
            { "id": mod_id, "file": { "id": 1, "modId": mod_id, "fileFingerprint": fp,
                "hashes": [ { "value": sha, "algo": 1 }, { "value": "md5", "algo": 2 } ] } }
        ] } })
    }

    fn allow_local() -> crate::test_seam::SeamScope {
        crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")])
    }

    #[tokio::test]
    async fn a_modrinth_jar_learns_its_curseforge_alias_when_the_sha1_matches() {
        let td = TempDir::new().unwrap();
        let bytes = b"srparasites jar bytes";
        let sha = add_jar(
            td.path(),
            "srp.jar",
            bytes,
            ModSource::Modrinth,
            "MJX",
            true,
        )
        .await;
        let fp = crate::mods::enrich::curseforge_fingerprint(bytes);
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wpath("/v1/fingerprints"))
            .respond_with(ResponseTemplate::new(200).set_body_json(cf_body(fp, 258587, &sha)))
            .expect(1)
            .mount(&s)
            .await;
        let _seam = allow_local();

        let learned = learn(td.path(), &s.uri(), &s.uri(), Some("k"), NOW)
            .await
            .unwrap();

        assert_eq!(learned, 1);
        let rows = installed::list(td.path()).await.unwrap();
        let map = load_alias_map(td.path(), &rows).await;
        assert_eq!(
            map.own_of(ModSource::Curseforge, "258587"),
            Some(&(ModSource::Modrinth, "MJX".to_string()))
        );
    }

    #[tokio::test]
    async fn a_fingerprint_match_with_another_sha1_is_no_alias() {
        let td = TempDir::new().unwrap();
        let bytes = b"collides by fingerprint only";
        let sha = add_jar(td.path(), "x.jar", bytes, ModSource::Modrinth, "MX", true).await;
        let fp = crate::mods::enrich::curseforge_fingerprint(bytes);
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wpath("/v1/fingerprints"))
            .respond_with(ResponseTemplate::new(200).set_body_json(cf_body(fp, 1, &"0".repeat(40))))
            .mount(&s)
            .await;
        let _seam = allow_local();

        assert_eq!(
            learn(td.path(), &s.uri(), &s.uri(), Some("k"), NOW)
                .await
                .unwrap(),
            0
        );
        let disk = load(td.path()).await;
        assert!(disk.aliases.is_empty());
        assert_eq!(
            disk.checked[&sha].curseforge.map(|p| p.state),
            Some(ProbeState::Absent)
        );
    }

    #[tokio::test]
    async fn a_curseforge_jar_learns_its_modrinth_alias() {
        let td = TempDir::new().unwrap();
        let sha = add_jar(
            td.path(),
            "c.jar",
            b"cf bytes",
            ModSource::Curseforge,
            "514409",
            true,
        )
        .await;
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wpath("/v2/version_files"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ sha.clone(): { "project_id": "MRPROJ" } })),
            )
            .expect(1)
            .mount(&s)
            .await;
        let _seam = allow_local();

        assert_eq!(
            learn(td.path(), &s.uri(), &s.uri(), None, NOW)
                .await
                .unwrap(),
            1
        );
        let rows = installed::list(td.path()).await.unwrap();
        let map = load_alias_map(td.path(), &rows).await;
        assert_eq!(
            map.own_of(ModSource::Modrinth, "MRPROJ"),
            Some(&(ModSource::Curseforge, "514409".to_string()))
        );
    }

    #[tokio::test]
    async fn without_a_key_curseforge_is_not_asked_and_nothing_is_stored() {
        let td = TempDir::new().unwrap();
        add_jar(
            td.path(),
            "m.jar",
            b"mr bytes",
            ModSource::Modrinth,
            "M1",
            true,
        )
        .await;
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&s)
            .await;
        let _seam = allow_local();

        assert_eq!(
            learn(td.path(), &s.uri(), &s.uri(), None, NOW)
                .await
                .unwrap(),
            0
        );
        assert!(!path(td.path()).exists(), "an unknown is never written");
    }

    // ── remember: an alias the jars' descriptor ids taught (spec 2026-10-09 D5) ──

    fn mr(id: &str) -> Ident {
        (ModSource::Modrinth, id.to_string())
    }

    fn cf(id: &str) -> Ident {
        (ModSource::Curseforge, id.to_string())
    }

    async fn alias_of(root: &Path, other: &Ident) -> Option<Ident> {
        let rows = installed::list(root).await.unwrap();
        load_alias_map(root, &rows)
            .await
            .own_of(other.0, &other.1)
            .cloned()
    }

    #[tokio::test]
    async fn remember_fills_an_empty_slot() {
        let td = TempDir::new().unwrap();
        add_jar(
            td.path(),
            "balm.jar",
            b"mr balm",
            ModSource::Modrinth,
            "MBA",
            true,
        )
        .await;

        let wrote = remember(td.path(), &mr("MBA"), &cf("531761"))
            .await
            .unwrap();

        assert!(wrote);
        assert_eq!(alias_of(td.path(), &cf("531761")).await, Some(mr("MBA")));
    }

    #[tokio::test]
    async fn remember_never_replaces_an_alias_already_there() {
        let td = TempDir::new().unwrap();
        add_jar(
            td.path(),
            "balm.jar",
            b"mr balm",
            ModSource::Modrinth,
            "MBA",
            true,
        )
        .await;
        assert!(remember(td.path(), &mr("MBA"), &cf("1")).await.unwrap());

        let wrote = remember(td.path(), &mr("MBA"), &cf("2")).await.unwrap();

        assert!(!wrote);
        assert_eq!(alias_of(td.path(), &cf("1")).await, Some(mr("MBA")));
        assert_eq!(alias_of(td.path(), &cf("2")).await, None);
    }

    #[tokio::test]
    async fn remember_writes_nothing_for_a_row_that_is_not_installed() {
        let td = TempDir::new().unwrap();
        add_jar(
            td.path(),
            "other.jar",
            b"other",
            ModSource::Modrinth,
            "OTH",
            true,
        )
        .await;

        let wrote = remember(td.path(), &mr("MBA"), &cf("531761"))
            .await
            .unwrap();

        assert!(!wrote);
        assert!(!path(td.path()).exists());
    }

    #[tokio::test]
    async fn remember_maps_one_platform_to_the_other_only() {
        let td = TempDir::new().unwrap();
        add_jar(
            td.path(),
            "balm.jar",
            b"mr balm",
            ModSource::Modrinth,
            "MBA",
            true,
        )
        .await;

        let wrote = remember(td.path(), &mr("MBA"), &mr("FORK")).await.unwrap();

        assert!(!wrote);
        assert!(!path(td.path()).exists());
    }

    #[tokio::test]
    async fn remember_leaves_a_project_another_row_already_claims() {
        let td = TempDir::new().unwrap();
        add_jar(
            td.path(),
            "balm.jar",
            b"mr balm",
            ModSource::Modrinth,
            "MBA",
            true,
        )
        .await;
        add_jar(
            td.path(),
            "balm2.jar",
            b"mr balm 2",
            ModSource::Modrinth,
            "MB2",
            false,
        )
        .await;
        assert!(remember(td.path(), &mr("MBA"), &cf("531761"))
            .await
            .unwrap());

        let wrote = remember(td.path(), &mr("MB2"), &cf("531761"))
            .await
            .unwrap();

        assert!(!wrote);
        assert_eq!(alias_of(td.path(), &cf("531761")).await, Some(mr("MBA")));
    }

    #[tokio::test]
    async fn remember_leaves_a_project_installed_in_its_own_right() {
        let td = TempDir::new().unwrap();
        add_jar(
            td.path(),
            "balm.jar",
            b"mr balm",
            ModSource::Modrinth,
            "MBA",
            true,
        )
        .await;
        add_jar(
            td.path(),
            "cf.jar",
            b"cf balm",
            ModSource::Curseforge,
            "531761",
            true,
        )
        .await;

        let wrote = remember(td.path(), &mr("MBA"), &cf("531761"))
            .await
            .unwrap();

        assert!(!wrote);
        assert!(!path(td.path()).exists());
    }

    #[tokio::test]
    async fn an_alias_remembered_survives_a_pass_that_finds_the_bytes_absent() {
        let td = TempDir::new().unwrap();
        add_jar(
            td.path(),
            "balm.jar",
            b"mr balm",
            ModSource::Modrinth,
            "MBA",
            true,
        )
        .await;
        assert!(remember(td.path(), &mr("MBA"), &cf("531761"))
            .await
            .unwrap());
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wpath("/v1/fingerprints"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "data": { "exactMatches": [] } })),
            )
            .expect(1)
            .mount(&s)
            .await;
        let _seam = allow_local();

        learn(td.path(), &s.uri(), &s.uri(), Some("k"), NOW)
            .await
            .unwrap();

        assert_eq!(alias_of(td.path(), &cf("531761")).await, Some(mr("MBA")));
    }

    #[tokio::test]
    async fn an_absent_answer_is_asked_again_only_after_a_week() {
        let td = TempDir::new().unwrap();
        add_jar(
            td.path(),
            "c.jar",
            b"cf only",
            ModSource::Curseforge,
            "77",
            true,
        )
        .await;
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wpath("/v2/version_files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .expect(2)
            .mount(&s)
            .await;
        let _seam = allow_local();

        learn(td.path(), &s.uri(), &s.uri(), None, NOW)
            .await
            .unwrap();
        learn(td.path(), &s.uri(), &s.uri(), None, NOW + 60)
            .await
            .unwrap();
        learn(
            td.path(),
            &s.uri(),
            &s.uri(),
            None,
            NOW + ABSENT_RECHECK_SECS,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn a_failed_request_stores_nothing_and_is_asked_again() {
        let td = TempDir::new().unwrap();
        add_jar(
            td.path(),
            "c.jar",
            b"cf flaky",
            ModSource::Curseforge,
            "88",
            true,
        )
        .await;
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wpath("/v2/version_files"))
            .respond_with(ResponseTemplate::new(503))
            .expect(2)
            .mount(&s)
            .await;
        let _seam = allow_local();

        learn(td.path(), &s.uri(), &s.uri(), None, NOW)
            .await
            .unwrap();
        assert!(!path(td.path()).exists());
        learn(td.path(), &s.uri(), &s.uri(), None, NOW + 1)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn a_refused_key_leaves_curseforge_alone_for_the_hour() {
        let td = TempDir::new().unwrap();
        add_jar(
            td.path(),
            "m.jar",
            b"mr refused",
            ModSource::Modrinth,
            "M2",
            true,
        )
        .await;
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wpath("/v1/fingerprints"))
            .respond_with(ResponseTemplate::new(401))
            .expect(1)
            .mount(&s)
            .await;
        let _seam = allow_local();

        assert_eq!(
            learn(td.path(), &s.uri(), &s.uri(), Some("k"), NOW)
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            learn(td.path(), &s.uri(), &s.uri(), Some("k"), NOW)
                .await
                .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn nothing_to_ask_makes_no_request() {
        let td = TempDir::new().unwrap();
        let sha = add_jar(
            td.path(),
            "c.jar",
            b"cf known",
            ModSource::Curseforge,
            "99",
            true,
        )
        .await;
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wpath("/v2/version_files"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ sha.clone(): { "project_id": "MR99" } })),
            )
            .expect(1)
            .mount(&s)
            .await;
        let _seam = allow_local();

        assert_eq!(
            learn(td.path(), &s.uri(), &s.uri(), None, NOW)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            learn(td.path(), &s.uri(), &s.uri(), None, NOW + 1)
                .await
                .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn bookkeeping_and_aliases_of_what_is_gone_are_pruned() {
        let td = TempDir::new().unwrap();
        let sha = add_jar(
            td.path(),
            "c.jar",
            b"cf gone",
            ModSource::Curseforge,
            "55",
            true,
        )
        .await;
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wpath("/v2/version_files"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ sha.clone(): { "project_id": "MR55" } })),
            )
            .mount(&s)
            .await;
        let _seam = allow_local();
        learn(td.path(), &s.uri(), &s.uri(), None, NOW)
            .await
            .unwrap();
        assert!(!load(td.path()).await.aliases.is_empty());

        installed::remove(td.path(), &sha).await.unwrap();
        tokio::fs::remove_file(installed::mods_dir(td.path()).join("c.jar"))
            .await
            .unwrap();
        add_jar(
            td.path(),
            "o.jar",
            b"other",
            ModSource::Curseforge,
            "56",
            true,
        )
        .await;
        learn(td.path(), &s.uri(), &s.uri(), None, NOW + 1)
            .await
            .unwrap();

        let disk = load(td.path()).await;
        assert!(!disk.checked.contains_key(&sha));
        assert!(!disk.aliases.contains_key("curseforge:55"));
    }

    #[tokio::test]
    async fn a_fingerprint_match_with_no_sha1_to_confirm_it_stays_unknown() {
        let td = TempDir::new().unwrap();
        let bytes = b"matched but unconfirmed";
        let sha = add_jar(td.path(), "u.jar", bytes, ModSource::Modrinth, "MU", true).await;
        let fp = crate::mods::enrich::curseforge_fingerprint(bytes);
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wpath("/v1/fingerprints"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": { "exactMatches": [ { "file": { "modId": 7, "fileFingerprint": fp,
                    "hashes": [ { "value": "md5", "algo": 2 } ] } } ] }
            })))
            .expect(2)
            .mount(&s)
            .await;
        let _seam = allow_local();

        for now in [NOW, NOW + 1] {
            assert_eq!(
                learn(td.path(), &s.uri(), &s.uri(), Some("k-unconfirmed"), now)
                    .await
                    .unwrap(),
                0
            );
        }
        let disk = load(td.path()).await;
        assert!(disk.aliases.is_empty());
        assert_eq!(disk.checked[&sha].curseforge, None, "not «absent»");
        assert_eq!(disk.checked[&sha].fingerprint, Some(fp));
    }

    #[tokio::test]
    async fn a_failed_chunk_keeps_the_other_chunks_answers() {
        let td = TempDir::new().unwrap();
        let mut shas = Vec::new();
        for i in 0..=CHUNK {
            let pid = (1000 + i).to_string();
            let bytes = format!("cf jar {i}");
            shas.push(
                add_jar(
                    td.path(),
                    &format!("{pid}.jar"),
                    bytes.as_bytes(),
                    ModSource::Curseforge,
                    &pid,
                    true,
                )
                .await,
            );
        }
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wpath("/v2/version_files"))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(1)
            .with_priority(1)
            .mount(&s)
            .await;
        Mock::given(method("POST"))
            .and(wpath("/v2/version_files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .expect(1)
            .mount(&s)
            .await;
        let _seam = allow_local();

        learn(td.path(), &s.uri(), &s.uri(), None, NOW)
            .await
            .unwrap();

        let disk = load(td.path()).await;
        let answered: Vec<&String> = disk
            .checked
            .iter()
            .filter(|(_, c)| c.modrinth.is_some())
            .map(|(sha, _)| sha)
            .collect();
        assert_eq!(
            answered.len(),
            1,
            "only the one-jar chunk answered: {answered:?}"
        );
        assert!(shas.contains(answered[0]));
    }

    #[tokio::test]
    async fn a_jar_left_unknown_without_a_key_is_asked_once_a_key_is_there() {
        let td = TempDir::new().unwrap();
        let bytes = b"keyless first";
        let sha = add_jar(td.path(), "k.jar", bytes, ModSource::Modrinth, "MK", true).await;
        let fp = crate::mods::enrich::curseforge_fingerprint(bytes);
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wpath("/v1/fingerprints"))
            .respond_with(ResponseTemplate::new(200).set_body_json(cf_body(fp, 31, &sha)))
            .expect(1)
            .mount(&s)
            .await;
        let _seam = allow_local();

        learn(td.path(), &s.uri(), &s.uri(), None, NOW)
            .await
            .unwrap();
        let learned = learn(td.path(), &s.uri(), &s.uri(), Some("k-later"), NOW + 1)
            .await
            .unwrap();

        assert_eq!(learned, 1);
    }

    #[tokio::test]
    async fn a_new_key_is_asked_at_once_after_another_was_refused() {
        let td = TempDir::new().unwrap();
        let bytes = b"refused then replaced";
        let sha = add_jar(td.path(), "r.jar", bytes, ModSource::Modrinth, "MR", true).await;
        let fp = crate::mods::enrich::curseforge_fingerprint(bytes);
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wpath("/v1/fingerprints"))
            .and(wiremock::matchers::header("x-api-key", "dead"))
            .respond_with(ResponseTemplate::new(403))
            .expect(1)
            .mount(&s)
            .await;
        Mock::given(method("POST"))
            .and(wpath("/v1/fingerprints"))
            .and(wiremock::matchers::header("x-api-key", "fresh"))
            .respond_with(ResponseTemplate::new(200).set_body_json(cf_body(fp, 32, &sha)))
            .expect(1)
            .mount(&s)
            .await;
        let _seam = allow_local();

        learn(td.path(), &s.uri(), &s.uri(), Some("dead"), NOW)
            .await
            .unwrap();
        learn(td.path(), &s.uri(), &s.uri(), Some("dead"), NOW)
            .await
            .unwrap();
        let learned = learn(td.path(), &s.uri(), &s.uri(), Some("fresh"), NOW)
            .await
            .unwrap();

        assert_eq!(learned, 1);
    }

    #[tokio::test]
    async fn what_cannot_be_written_is_not_learned_and_leaves_no_temp_file() {
        let td = TempDir::new().unwrap();
        let sha = add_jar(
            td.path(),
            "w.jar",
            b"cf unwritable",
            ModSource::Curseforge,
            "66",
            true,
        )
        .await;
        // A directory where the file goes: the rename onto it fails on every platform.
        tokio::fs::create_dir_all(path(td.path())).await.unwrap();
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wpath("/v2/version_files"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ sha.clone(): { "project_id": "MR66" } })),
            )
            .mount(&s)
            .await;
        let _seam = allow_local();

        let learned = learn(td.path(), &s.uri(), &s.uri(), None, NOW)
            .await
            .unwrap();

        assert_eq!(learned, 0);
        let mut left = Vec::new();
        let mut dir = tokio::fs::read_dir(installed::registry_dir(td.path()))
            .await
            .unwrap();
        while let Some(e) = dir.next_entry().await.unwrap() {
            left.push(e.file_name().to_string_lossy().into_owned());
        }
        assert!(
            left.iter().all(|n| !n.contains(".tmp.")),
            "temp files left: {left:?}"
        );
    }

    #[tokio::test]
    async fn an_instance_gone_meanwhile_does_not_get_its_folder_back() {
        let td = TempDir::new().unwrap();
        let gone = td.path().join("deleted-instance");

        assert!(!write_if_present(&gone, &OnDisk::default()).await);
        assert!(!gone.exists());
    }

    fn row(source: ModSource, pid: &str, enabled: bool) -> InstalledMod {
        InstalledMod {
            filename: format!("{pid}.jar"),
            sha1: format!("sha-{pid}"),
            source: Some(source),
            project_id: Some(pid.into()),
            version_id: None,
            name: pid.into(),
            version_number: None,
            installed_at: String::new(),
            enabled,
            enrich_attempted: false,
            requires: Vec::new(),
        }
    }

    fn disk_with(aliases: &[(&str, &str, &str)]) -> OnDisk {
        let mut d = OnDisk::default();
        for (own, platform, id) in aliases {
            d.aliases
                .entry((*own).to_string())
                .or_default()
                .insert((*platform).to_string(), (*id).to_string());
        }
        d
    }

    #[test]
    fn an_enabled_row_wins_an_alias_over_a_disabled_one_then_the_lower_key() {
        let disk = disk_with(&[
            ("modrinth:zz", "curseforge", "1"),
            ("modrinth:aa", "curseforge", "1"),
            ("modrinth:bb", "curseforge", "2"),
            ("modrinth:cc", "curseforge", "2"),
        ]);
        let rows = [
            row(ModSource::Modrinth, "zz", true),
            row(ModSource::Modrinth, "aa", false),
            row(ModSource::Modrinth, "bb", true),
            row(ModSource::Modrinth, "cc", true),
        ];
        let map = alias_map(&disk, &rows);
        assert_eq!(
            map.own_of(ModSource::Curseforge, "1").map(|o| o.1.as_str()),
            Some("zz")
        );
        assert_eq!(
            map.own_of(ModSource::Curseforge, "2").map(|o| o.1.as_str()),
            Some("bb")
        );
    }

    #[test]
    fn an_alias_that_is_a_project_installed_in_its_own_right_is_dropped() {
        // Mutual aliases: each row's bytes also exist under the other's project.
        let disk = disk_with(&[
            ("modrinth:X", "curseforge", "9"),
            ("curseforge:9", "modrinth", "X"),
        ]);
        let rows = [
            row(ModSource::Modrinth, "X", true),
            row(ModSource::Curseforge, "9", false),
        ];
        let map = alias_map(&disk, &rows);
        assert_eq!(map.own_of(ModSource::Curseforge, "9"), None);
        assert_eq!(map.own_of(ModSource::Modrinth, "X"), None);
    }

    // Review L1 (spec 2026-10-08 aliases-everywhere): `mods_cross_aliases` keeps the browser's map
    // when the sidecar is there but cannot be read; only `load`, the guards' derived cache, turns
    // that into «nothing learned».
    #[tokio::test]
    async fn read_tells_an_unreadable_sidecar_from_an_absent_one() {
        let td = tempfile::tempdir().unwrap();
        assert!(
            read(td.path()).await.unwrap().aliases.is_empty(),
            "absent is empty"
        );
        // A directory where the file goes: `fs::read` fails with something other than NotFound.
        std::fs::create_dir_all(path(td.path())).unwrap();
        assert!(read(td.path()).await.is_err(), "unreadable is an error");
        assert!(
            load(td.path()).await.aliases.is_empty(),
            "load stays a derived cache"
        );
    }

    // One step only: row A's id on CurseForge is 1; a stale entry for curseforge:1 (its row is
    // gone) still names modrinth:C. C is not A — an alias never chains (pin: `alias_map` reads
    // only the current rows' entries).
    #[test]
    fn an_alias_of_an_alias_is_no_alias() {
        let disk = disk_with(&[
            ("modrinth:A", "curseforge", "1"),
            ("curseforge:1", "modrinth", "C"),
        ]);
        let map = alias_map(&disk, &[row(ModSource::Modrinth, "A", true)]);
        assert!(map.own_of(ModSource::Curseforge, "1").is_some());
        assert_eq!(map.own_of(ModSource::Modrinth, "C"), None);
    }

    #[test]
    fn an_alias_of_a_project_no_longer_installed_means_nothing() {
        let disk = disk_with(&[("modrinth:gone", "curseforge", "3")]);
        let map = alias_map(&disk, &[row(ModSource::Modrinth, "kept", true)]);
        assert_eq!(map.own_of(ModSource::Curseforge, "3"), None);
    }

    #[test]
    fn an_alias_that_is_no_curseforge_id_is_left_out_of_the_install_keys() {
        let map = AliasMap::from_pairs(&[
            (
                (ModSource::Curseforge, "not-a-number".into()),
                (ModSource::Modrinth, "a".into()),
            ),
            (
                (ModSource::Curseforge, "42".into()),
                (ModSource::Modrinth, "b".into()),
            ),
        ]);
        assert_eq!(
            map.project_keys(),
            HashSet::from([ProjectKey::Curseforge(42)])
        );
    }
}
