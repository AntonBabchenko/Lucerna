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
type Ident = (ModSource, String);

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
    let p = path(instance_root);
    let bytes = match fs::read(&p).await {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return OnDisk::default(),
        Err(e) => {
            crate::diag!("[cross-ids] cannot read {}: {e} — relearning", p.display());
            return OnDisk::default();
        }
    };
    match serde_json::from_slice::<OnDisk>(&bytes) {
        Ok(d) => d,
        Err(e) => {
            crate::diag!(
                "[cross-ids] {} is unreadable: {e} — relearning",
                p.display()
            );
            OnDisk::default()
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
    fs::write(&tmp, &bytes).await?;
    if let Err(e) = fs::rename(&tmp, &final_path).await {
        // The temp file is this write's own and useless now; one that cannot be removed is a
        // stray file no read ever opens — said, and the rename's error is what the caller gets.
        if let Err(rm) = fs::remove_file(&tmp).await {
            crate::diag!("[cross-ids] cannot remove {}: {rm}", tmp.display());
        }
        return Err(e);
    }
    Ok(())
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

/// The last time CurseForge (at `base`) refused the key, per API base: a dead key is not asked
/// again on every refresh.
fn cf_refused() -> &'static Mutex<HashMap<String, Instant>> {
    static REFUSED: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();
    REFUSED.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cf_refused_recently(base: &str) -> bool {
    match cf_refused().lock() {
        Ok(map) => map
            .get(base)
            .is_some_and(|at| at.elapsed() < CF_REFUSED_BACKOFF),
        // A poisoned map only ever held instants: ask again rather than stay silent forever.
        Err(poisoned) => poisoned
            .into_inner()
            .get(base)
            .is_some_and(|at| at.elapsed() < CF_REFUSED_BACKOFF),
    }
}

fn mark_cf_refused(base: &str) {
    match cf_refused().lock() {
        Ok(mut map) => {
            map.insert(base.to_string(), Instant::now());
        }
        Err(poisoned) => {
            poisoned
                .into_inner()
                .insert(base.to_string(), Instant::now());
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
    /// Answered: SHA-1 → mod id, for the jars it has. Any asked jar not in it is absent.
    Answered(HashMap<String, String>),
    /// 401 / 403: the key was refused.
    Refused,
    /// Could not tell: transport, another status, an unreadable answer.
    Failed,
}

/// Ask CurseForge which of these jars it has. A fingerprint match counts only when the file's
/// SHA-1 is the jar's: a 32-bit fingerprint alone can collide with an unrelated file. Never
/// clears the key on a refusal (unlike `CurseForgeClient::files_by_fingerprint`): this runs in
/// the background and must not end the user's interactive session.
async fn resolve_curseforge(base: &str, key: &str, jars: &[(u32, String)]) -> CfOutcome {
    let mut found = HashMap::new();
    for chunk in jars.chunks(CHUNK) {
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
        for m in parsed.data.exact_matches {
            let Some(file_sha) = m
                .file
                .hashes
                .iter()
                .find(|h| h.algo == CF_ALGO_SHA1)
                .map(|h| h.value.to_ascii_lowercase())
            else {
                continue;
            };
            let ours = chunk
                .iter()
                .any(|(fp, sha)| *fp == m.file.file_fingerprint && *sha == file_sha);
            if ours {
                found.insert(file_sha, m.file.mod_id.to_string());
            }
        }
    }
    CfOutcome::Answered(found)
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
/// unknown; only reading the registry is an error.
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
        let path = if m.enabled {
            mods_dir.join(&m.filename)
        } else {
            mods_dir.join(format!("{}.disabled", m.filename))
        };
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
    // and none of them is read.
    match cf_key {
        Some(k) if !ask_cf.is_empty() && !cf_refused_recently(cf_base) => {
            let asked = fingerprint(ask_cf).await?;
            for a in &asked {
                if let Some(fp) = a.fingerprint {
                    fingerprints.push((a.sha.clone(), fp));
                }
            }
            let jars: Vec<(u32, String)> = asked
                .iter()
                .filter_map(|a| Some((a.fingerprint?, a.sha.clone())))
                .collect();
            match resolve_curseforge(cf_base, k, &jars).await {
                CfOutcome::Answered(found) => {
                    for a in asked {
                        let id = found.get(&a.sha).cloned();
                        answers.push(Answer {
                            sha: a.sha,
                            own: a.own,
                            platform: ModSource::Curseforge,
                            found: id,
                        });
                    }
                }
                CfOutcome::Refused => mark_cf_refused(cf_base),
                CfOutcome::Failed => {}
            }
        }
        _ => {}
    }

    // Modrinth: by SHA-1, no key.
    if !ask_mr.is_empty() {
        let client = crate::mods::modrinth::ModrinthClient::with_base(modrinth_base);
        let shas: Vec<&str> = ask_mr.iter().map(|a| a.sha.as_str()).collect();
        match client.project_ids_by_hash(&shas).await {
            Ok(found) => {
                for a in ask_mr {
                    let id = found.get(&a.sha).cloned();
                    answers.push(Answer {
                        sha: a.sha,
                        own: a.own,
                        platform: ModSource::Modrinth,
                        found: id,
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
/// changed and the instance still exists. A failed write is said; what was learned still counts.
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
    let mut learned = 0u32;

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
        if entry.get(name) != Some(id) {
            if let Some(old) = entry.insert(name.to_string(), id.clone()) {
                crate::diag!(
                    "[cross-ids] {} on {name} is now {id} (was {old})",
                    key(&a.own)
                );
            }
            learned += 1;
        }
    }

    // Prune against the current rows: bookkeeping of bytes no longer installed, aliases of
    // projects no longer installed.
    let shas: HashSet<String> = rows.iter().map(|m| m.sha1.to_ascii_lowercase()).collect();
    let owns: HashSet<String> = rows.iter().filter_map(own_of).map(|o| key(&o)).collect();
    disk.checked.retain(|sha, _| shas.contains(sha));
    disk.aliases.retain(|own, _| owns.contains(own));

    if disk != before {
        disk.version = FILE_VERSION;
        match fs::metadata(instance_root).await {
            Ok(meta) if meta.is_dir() => {
                if let Err(e) = save(instance_root, &disk).await {
                    crate::diag!(
                        "[cross-ids] cannot write {}: {e} — redone next pass",
                        path(instance_root).display()
                    );
                }
            }
            // The instance went away while the platforms answered: its `lucerna/` is not
            // brought back.
            Ok(_) => crate::diag!(
                "[cross-ids] {} is not a directory — not written",
                instance_root.display()
            ),
            Err(e) => crate::diag!(
                "[cross-ids] {} is gone ({e}) — not written",
                instance_root.display()
            ),
        }
    }
    Ok(learned)
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
