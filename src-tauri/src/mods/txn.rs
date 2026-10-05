//! Content transaction: what lets an update fail without losing anything (spec
//! `2026-10-04-non-destructive-content-update`, §4.1–§4.2 and §11a).
//!
//! An update writes its whole plan to `{instance}/lucerna/txn/<token>/record.json`
//! BEFORE it touches anything: the files it will set aside (with their registry
//! rows verbatim), the files it will place, and — for a pack — the prior
//! `pack_origin` and instance fields. Old files are then MOVED into
//! `<token>/files/<rel>` with `fs::rename`, never deleted: a hardlinked jar stays
//! one inode with the store, and no byte is copied. On success the token is
//! closed; on failure — or at the next start after a crash — the plan is undone.
//!
//! Undoing is content-addressed: a planned file counts as placed only when its
//! bytes hash to the planned SHA-1, and a set-aside file comes back only from
//! `files/`. So it is idempotent at every crash point.
//!
//! Closing (a commit, or a clean undo): an empty `closed` marker, then the record
//! is removed, then the folder. A token is pending iff its record exists and the
//! marker does not; closing fails only when both of the first two steps fail.
//!
//! One transaction per instance at a time: plan building through closing runs
//! under [`lock`], a per-instance mutex of its own. Phase 2 of an update is
//! local and fast, and the lock removes the race of two updates sharing a
//! dependency. An install that does not use a transaction (`install_batch`)
//! racing an undo keeps the ms-scale window #310 documents.
//!
//! `begin` refuses while a pending token exists: under the lock no other
//! transaction is live, so a pending one is a closing that failed — acting on top
//! of it could let that stale record later remove files a newer update placed.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use tokio::fs;
use tokio::io::AsyncReadExt;
use tokio::sync::OwnedMutexGuard;

use crate::error::Error;
use crate::instances::schema::{InstanceFile, LoaderKind};
use crate::mods::installed::{self, PackOrigin};
use crate::mods::platform::InstalledMod;
use crate::mods::trash;

const RECORD_FILE: &str = "record.json";
const CLOSED_FILE: &str = "closed";
const FILES_DIR: &str = "files";
const RECORD_VERSION: u32 = 1;
/// A unique temp name per record write — the `installed::write` shape.
static WRITE_SEQ: AtomicU64 = AtomicU64::new(0);

/// What the transaction is, for the log and the startup notice.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TxnKind {
    PackUpdate {
        pack: String,
        from: String,
        to: String,
    },
    ModUpdate {
        name: String,
        from: Option<String>,
        to: String,
    },
}

/// A file the update sets aside. `rel` is its path under `.minecraft/`,
/// `/`-separated, in its on-disk spelling (`mods/a.jar.disabled`); `row` is its
/// registry row verbatim, or `None` for a file without one (an asset).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageEntry {
    pub rel: String,
    pub sha1: String,
    pub row: Option<InstalledMod>,
}

/// A file the update places. `pre_existing`: a file was already at `rel` when the
/// plan was built (and the plan does not set it aside) — an undo never removes
/// it. `prior_row`: the row an idempotent install rewrites, put back on undo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateEntry {
    pub rel: String,
    pub sha1: String,
    pub project_id: Option<String>,
    pub pre_existing: bool,
    pub prior_row: Option<InstalledMod>,
}

/// The pack fields of `instance.json` before a pack update. Written back
/// verbatim on undo: they were a real state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriorInstance {
    pub mrpack_version: Option<String>,
    pub mrpack_version_id: Option<String>,
    pub mc_version: String,
    pub loader: LoaderKind,
    pub loader_version: Option<String>,
}

impl PriorInstance {
    pub fn of(i: &InstanceFile) -> Self {
        Self {
            mrpack_version: i.mrpack_version.clone(),
            mrpack_version_id: i.mrpack_version_id.clone(),
            mc_version: i.mc_version.clone(),
            loader: i.loader,
            loader_version: i.loader_version.clone(),
        }
    }

    fn write_into(&self, i: &mut InstanceFile) {
        i.mrpack_version = self.mrpack_version.clone();
        i.mrpack_version_id = self.mrpack_version_id.clone();
        i.mc_version = self.mc_version.clone();
        i.loader = self.loader;
        i.loader_version = self.loader_version.clone();
    }
}

/// The whole plan, known before anything is touched.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub kind: TxnKind,
    pub stage: Vec<StageEntry>,
    pub create: Vec<CreateEntry>,
    pub prior_pack_origin: Option<PackOrigin>,
    pub prior_instance: Option<PriorInstance>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Record {
    version: u32,
    created_unix_ms: u64,
    #[serde(flatten)]
    plan: Plan,
}

/// How a startup recovery of one token ended.
#[derive(Debug, Clone)]
pub enum RecoveredOutcome {
    Restored,
    Incomplete { folder: String, details: String },
}

#[derive(Debug, Clone)]
pub struct Recovered {
    pub token: String,
    pub kind: Option<TxnKind>,
    pub outcome: RecoveredOutcome,
}

enum Unwound {
    Clean,
    Stuck { folder: PathBuf, stuck: Vec<String> },
}

enum TokenState {
    Pending,
    Closed,
}

/// `{instance}/lucerna/txn` — beside `installed-mods.json` and `trash/`, outside
/// `.minecraft/`.
pub fn txn_root(instance_root: &Path) -> PathBuf {
    installed::registry_dir(instance_root).join("txn")
}

/// `.minecraft/<rel>` of `instance_root`.
pub fn content_path(instance_root: &Path, rel: &str) -> PathBuf {
    join_rel(&mc_dir(instance_root), rel)
}

/// The create entry for a file an update is about to place at
/// `.minecraft/<rel>`. `staged` holds the lowercased `rel`s the plan sets
/// aside: those names are free by the time the file lands, so they are not
/// pre-existing. Case-blind on purpose — on Windows and macOS `Sodium.jar` and
/// `sodium.jar` are one file. A stat that cannot answer refuses the update
/// before anything moved.
pub async fn plan_create(
    instance_root: &Path,
    rows: &[InstalledMod],
    rel: String,
    sha1: String,
    project_id: Option<String>,
    staged: &HashSet<String>,
) -> Result<CreateEntry, Error> {
    let sha1 = sha1.to_ascii_lowercase();
    let staged_away = staged.contains(&rel.to_ascii_lowercase());
    let path = content_path(instance_root, &rel);
    let pre_existing = !staged_away && fs::try_exists(&path).await.map_err(|e| io_err(&path, e))?;
    // An install over a same-bytes file rewrites that file's row (#461 (3)):
    // keep the row as it was, to put it back on undo.
    let prior_row = if pre_existing && rel.starts_with("mods/") {
        rows.iter()
            .find(|m| m.sha1.eq_ignore_ascii_case(&sha1))
            .cloned()
    } else {
        None
    };
    Ok(CreateEntry {
        rel,
        sha1,
        project_id,
        pre_existing,
        prior_row,
    })
}

/// Exclusive use of an instance's content for one transaction. Plan building
/// happens under it, so what the plan saw is what `begin` records.
pub struct TxnLock {
    guard: OwnedMutexGuard<()>,
    instance_root: PathBuf,
}

pub async fn lock(instance_root: &Path) -> TxnLock {
    let key: PathBuf = txn_root(instance_root).components().collect();
    let gate = {
        // A poisoned map is still a valid map: the only code under this std
        // lock is an entry lookup and an `Arc` clone. Never held across an await.
        let mut gates = TXN_GATES.lock().unwrap_or_else(|p| p.into_inner());
        Arc::clone(
            gates
                .entry(key)
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
        )
    };
    TxnLock {
        guard: gate.lock_owned().await,
        instance_root: instance_root.to_path_buf(),
    }
}

/// One async mutex per instance's `txn` directory, created on first use and
/// never pruned (one small `Arc` per instance this process touched — the bound
/// `registry_lock` has). Its own map on purpose: `registry_lock` is the
/// registries' chokepoint and is taken inside every `installed::` call a
/// transaction makes, while this mutex spans a whole transaction; they must
/// never be the same mutex. The key is lexical (`components()` folds `/` vs
/// `\` and doubled separators), as `registry_lock`'s is.
static TXN_GATES: LazyLock<Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// The tokens of `instance_root` whose record exists and which are not closed.
/// Synchronous: startup calls it before the async runtime serves anything.
pub fn pending_tokens(instance_root: &Path) -> std::io::Result<Vec<String>> {
    let root = txn_root(instance_root);
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut out = Vec::new();
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if trash::parse_token(&name).is_none() {
            continue;
        }
        let dir = entry.path();
        if dir.join(CLOSED_FILE).try_exists()? {
            continue;
        }
        if dir.join(RECORD_FILE).try_exists()? {
            out.push(name);
        }
    }
    out.sort();
    Ok(out)
}

/// Record `plan` and start the transaction. Nothing of the instance is touched
/// here; the record is complete before the token directory gets its name, so a
/// `<token>` directory always has one until it is closed.
pub async fn begin(lock: TxnLock, plan: Plan) -> Result<Txn, Error> {
    let TxnLock {
        guard,
        instance_root,
    } = lock;
    let root = txn_root(&instance_root);
    match pending_tokens(&instance_root) {
        Ok(pending) => {
            if let Some(first) = pending.first() {
                return Err(Error::ContentUpdateUnfinished {
                    folder: root.join(first).display().to_string(),
                });
            }
        }
        Err(e) => return Err(io_err(&root, e)),
    }
    fs::create_dir_all(&root)
        .await
        .map_err(|e| io_err(&root, e))?;
    let now = SystemTime::now();
    let token = trash::new_token(now);
    let tmp = root.join(format!("{token}.tmp"));
    // `create_dir`, not `_all`: a colliding token fails here, before anything.
    fs::create_dir(&tmp).await.map_err(|e| io_err(&tmp, e))?;
    let record = Record {
        version: RECORD_VERSION,
        created_unix_ms: trash::unix_ms(now),
        plan,
    };
    if let Err(e) = write_record(&tmp, &record).await {
        discard_dir(&tmp, "after a failed record write").await;
        return Err(io_err(&tmp.join(RECORD_FILE), e));
    }
    let dir = root.join(&token);
    if let Err(e) = fs::rename(&tmp, &dir).await {
        discard_dir(&tmp, "after it could not be named").await;
        return Err(io_err(&dir, e));
    }
    Ok(Txn {
        dir,
        instance_root,
        plan: record.plan,
        _guard: Some(guard),
    })
}

/// A running transaction. Every `Err` after `begin` must go through
/// [`Txn::finish`] or [`Txn::rollback`]; dropping it unfinished leaves the token
/// pending, which the next start undoes.
pub struct Txn {
    dir: PathBuf,
    instance_root: PathBuf,
    plan: Plan,
    /// `None` only inside a recovery pass, which holds the lock for every token.
    _guard: Option<OwnedMutexGuard<()>>,
}

impl Txn {
    pub fn instance_root(&self) -> &Path {
        &self.instance_root
    }

    /// The plan `begin` recorded — the steps the caller now runs.
    pub fn plan(&self) -> &Plan {
        &self.plan
    }

    /// Set `entry` aside: file first, then its row, so the registry never
    /// describes a jar that is not there. A row-removal error is returned with
    /// the file still set aside — the caller's rollback moves it home.
    pub async fn stage(&self, entry: &StageEntry) -> Result<(), Error> {
        if !safe_rel(&entry.rel) {
            return Err(Error::ModpackOverridesPathEscape {
                entry: entry.rel.clone(),
            });
        }
        let from = join_rel(&mc_dir(&self.instance_root), &entry.rel);
        let held = join_rel(&self.dir.join(FILES_DIR), &entry.rel);
        if let Some(parent) = held.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| io_err(parent, e))?;
        }
        fs::rename(&from, &held)
            .await
            .map_err(|e| io_err(&from, e))?;
        if let Some(row) = &entry.row {
            installed::remove(&self.instance_root, &row.sha1).await?;
        }
        Ok(())
    }

    /// Commit on `Ok` (rolling back when the commit cannot be recorded), roll
    /// back on `Err`.
    pub async fn finish<T>(self, outcome: Result<T, Error>) -> Result<T, Error> {
        match outcome {
            Ok(value) => match self.close().await {
                Ok(()) => Ok(value),
                Err(e) => Err(self.rollback(e).await),
            },
            Err(e) => Err(self.rollback(e).await),
        }
    }

    /// Undo everything and return `cause` — or, when something could not go
    /// back, `ContentUpdateRollbackIncomplete` naming the kept folder.
    pub async fn rollback(self, cause: Error) -> Error {
        match self.unwind().await {
            Unwound::Clean => cause,
            Unwound::Stuck { folder, stuck } => Error::ContentUpdateRollbackIncomplete {
                folder: folder.display().to_string(),
                details: format!("{cause}; could not be put back: {}", stuck.join(", ")),
            },
        }
    }

    async fn close(&self) -> Result<(), Error> {
        let marker = self.dir.join(CLOSED_FILE);
        let record = self.dir.join(RECORD_FILE);
        let marked = fs::write(&marker, b"").await;
        let removed = match fs::remove_file(&record).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        };
        match (marked, removed) {
            (Err(m), Err(r)) => {
                return Err(Error::ModsInstancePath {
                    path: record.display().to_string(),
                    details: format!(
                        "could not close the update record (marker: {m}; record: {r})"
                    ),
                })
            }
            (Err(m), Ok(())) => crate::diag!(
                "content txn: closed {} without its marker ({m})",
                self.dir.display()
            ),
            (Ok(()), Err(r)) => crate::diag!(
                "content txn: closed {} by its marker; the record stays ({r})",
                self.dir.display()
            ),
            (Ok(()), Ok(())) => {}
        }
        discard_dir(&self.dir, "after closing it (the next start removes it)").await;
        Ok(())
    }

    async fn unwind(self) -> Unwound {
        let stuck = undo(&self.instance_root, &self.dir, &self.plan).await;
        if stuck.is_empty() {
            if let Err(e) = self.close().await {
                crate::diag!(
                    "content txn: undid {} but could not close it ({e}); it is undone again at the next start",
                    self.dir.display()
                );
            }
            return Unwound::Clean;
        }
        crate::diag!(
            "content txn: {} could not be fully undone: {}",
            self.dir.display(),
            stuck.join(", ")
        );
        let folder = match trash::keep_out_of_purge(&self.dir).await {
            Ok(kept) => kept,
            Err(e) => {
                crate::diag!(
                    "content txn: could not set {} aside ({e}); it is undone again at the next start",
                    self.dir.display()
                );
                self.dir.clone()
            }
        };
        Unwound::Stuck { folder, stuck }
    }
}

/// Undo `plan`: placed files out (newest first), set-aside files back, then the
/// pack record. Returns one line per entry that could not be put right; the
/// loop never stops at the first, so one stuck jar never strands the rest.
async fn undo(instance_root: &Path, dir: &Path, plan: &Plan) -> Vec<String> {
    let mc = mc_dir(instance_root);
    let mut stuck = Vec::new();
    for c in plan.create.iter().rev() {
        if let Err(why) = undo_create(instance_root, &mc, c).await {
            stuck.push(format!("{} ({why})", c.rel));
        }
    }
    for s in plan.stage.iter().rev() {
        if let Err(why) = undo_stage(instance_root, &mc, dir, s).await {
            stuck.push(format!("{} ({why})", s.rel));
        }
    }
    if let Some(prior) = &plan.prior_pack_origin {
        if let Err(why) = restore_pack_origin(instance_root, prior).await {
            stuck.push(format!("pack_origin ({why})"));
        }
    }
    if let Some(prior) = &plan.prior_instance {
        if let Err(why) = restore_instance(instance_root, prior) {
            stuck.push(format!("instance.json ({why})"));
        }
    }
    stuck
}

async fn undo_create(instance_root: &Path, mc: &Path, c: &CreateEntry) -> Result<(), String> {
    if !safe_rel(&c.rel) {
        return Err("an unsafe path in the record".into());
    }
    let plain = join_rel(mc, &c.rel);
    let is_mod = c.rel.starts_with("mods/");
    let disabled = with_disabled_suffix(&plain);
    let spellings: Vec<&Path> = if is_mod {
        vec![plain.as_path(), disabled.as_path()]
    } else {
        vec![plain.as_path()]
    };
    for p in spellings {
        let sha = file_sha1(p)
            .await
            .map_err(|e| format!("could not read it: {e}"))?;
        // Absent, or another file — the old one, or the user's: not ours.
        let Some(sha) = sha else { continue };
        if !sha.eq_ignore_ascii_case(&c.sha1) {
            continue;
        }
        if c.pre_existing {
            if p != plain.as_path() {
                // A pre-existing mod found under `.disabled` was switched off by
                // carry-disable; it was on when the update found it.
                let taken = fs::try_exists(&plain)
                    .await
                    .map_err(|e| format!("could not check its name: {e}"))?;
                if taken {
                    return Err("its name is taken".into());
                }
                fs::rename(p, &plain)
                    .await
                    .map_err(|e| format!("could not switch it back on: {e}"))?;
            }
        } else {
            fs::remove_file(p)
                .await
                .map_err(|e| format!("could not remove it: {e}"))?;
        }
    }
    if is_mod {
        match (&c.prior_row, c.pre_existing) {
            (Some(row), _) => {
                // Only with its file: a row without a jar would describe nothing.
                if holds_sha(&plain, &disabled, &row.sha1).await? {
                    installed::add(instance_root, row.clone())
                        .await
                        .map_err(|e| e.to_string())?;
                }
            }
            (None, false) => installed::remove(instance_root, &c.sha1)
                .await
                .map_err(|e| e.to_string())?,
            (None, true) => {
                // A file that was there before and had no row with this sha:
                // the install stopped at its name conflict, so this update
                // wrote no row for it and has none to take back.
            }
        }
    }
    Ok(())
}

async fn undo_stage(
    instance_root: &Path,
    mc: &Path,
    dir: &Path,
    s: &StageEntry,
) -> Result<(), String> {
    if !safe_rel(&s.rel) {
        return Err("an unsafe path in the record".into());
    }
    let held = join_rel(&dir.join(FILES_DIR), &s.rel);
    match fs::try_exists(&held).await {
        Ok(true) => {}
        // Never moved: nothing to bring back.
        Ok(false) => return Ok(()),
        Err(e) => return Err(format!("could not check the kept copy: {e}")),
    }
    let back = join_rel(mc, &s.rel);
    // Never over another file: `rename` replaces its target on Windows, and the
    // name may have been taken meanwhile.
    if let Some(why) = trash::way_back_blocked(&back).await {
        if !same_bytes_in_place(&back, &s.sha1).await {
            return Err(why);
        }
        // The same bytes are already back (an earlier pass got this far).
        fs::remove_file(&held)
            .await
            .map_err(|e| format!("could not drop the duplicate kept copy: {e}"))?;
    } else {
        if let Some(parent) = back.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("could not recreate its folder: {e}"))?;
        }
        fs::rename(&held, &back)
            .await
            .map_err(|e| format!("could not move it back: {e}"))?;
    }
    if let Some(row) = &s.row {
        if let Err(e) = installed::add(instance_root, row.clone()).await {
            // A jar without its row would come back as an anonymous manual mod
            // and lose its provenance: move it out again (`trash::restore_item`).
            return Err(match fs::rename(&back, &held).await {
                Ok(()) => format!("its record could not be written ({e}); the file stays in the kept folder"),
                Err(again) => format!(
                    "its record could not be written ({e}) and the file stayed in place without it ({again})"
                ),
            });
        }
    }
    Ok(())
}

async fn restore_pack_origin(instance_root: &Path, prior: &PackOrigin) -> Result<(), String> {
    let current = installed::get_pack_origin(instance_root)
        .await
        .map_err(|e| e.to_string())?;
    if current.as_ref() == Some(prior) {
        return Ok(());
    }
    installed::set_pack_origin(instance_root, prior.clone())
        .await
        .map_err(|e| e.to_string())
}

fn restore_instance(instance_root: &Path, prior: &PriorInstance) -> Result<(), String> {
    let path = instance_root.join("instance.json");
    let mut inst = crate::instances::store::read_instance_json(&path).map_err(|e| e.to_string())?;
    let before = inst.clone();
    prior.write_into(&mut inst);
    if inst != before {
        crate::instances::store::write_instance_json(&path, &inst).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Startup: undo every pending token of `instance_root` (newest first), and
/// clear what earlier sessions left closed. Holds the instance's lock for the
/// whole pass.
pub async fn recover_pending(instance_root: &Path, session_start_ms: u64) -> Vec<Recovered> {
    scan(instance_root, session_start_ms, true).await
}

/// Startup, for an instance with no pending token: clear what earlier sessions
/// left closed. Never acts on a pending token, nor on a `.tmp` this session made.
pub async fn sweep_closed(instance_root: &Path, session_start_ms: u64) -> Vec<Recovered> {
    scan(instance_root, session_start_ms, false).await
}

async fn scan(instance_root: &Path, session_start_ms: u64, undo_pending: bool) -> Vec<Recovered> {
    let _lock = lock(instance_root).await;
    let root = txn_root(instance_root);
    let mut out = Vec::new();
    let names = match list_names(&root).await {
        Ok(names) => names,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return out,
        Err(e) => {
            crate::diag!("content txn: cannot list {}: {e}", root.display());
            return out;
        }
    };
    let mut pending: Vec<(u64, String)> = Vec::new();
    for name in names {
        if let Some(stem) = name.strip_suffix(".tmp") {
            if trash::parse_token(stem).is_some_and(|ms| ms < session_start_ms) {
                discard_dir(&root.join(&name), "(an update that never began)").await;
            }
            continue;
        }
        // `-kept` folders and strangers are not tokens: never ours to touch.
        let Some(ms) = trash::parse_token(&name) else {
            continue;
        };
        let dir = root.join(&name);
        match token_state(&dir).await {
            Ok(TokenState::Closed) => discard_dir(&dir, "(a closed update)").await,
            Ok(TokenState::Pending) => pending.push((ms, name)),
            Err(e) => out.push(
                set_aside(
                    &dir,
                    &name,
                    None,
                    format!("could not tell whether it finished: {e}"),
                )
                .await,
            ),
        }
    }
    if !undo_pending {
        return out;
    }
    pending.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, name) in pending {
        let dir = root.join(&name);
        match read_record(&dir).await {
            Ok(record) => {
                let kind = Some(record.plan.kind.clone());
                let txn = Txn {
                    dir,
                    instance_root: instance_root.to_path_buf(),
                    plan: record.plan,
                    _guard: None,
                };
                let outcome = match txn.unwind().await {
                    Unwound::Clean => RecoveredOutcome::Restored,
                    Unwound::Stuck { folder, stuck } => RecoveredOutcome::Incomplete {
                        folder: folder.display().to_string(),
                        details: stuck.join(", "),
                    },
                };
                out.push(Recovered {
                    token: name,
                    kind,
                    outcome,
                });
            }
            Err(why) => out.push(
                set_aside(
                    &dir,
                    &name,
                    None,
                    format!("its record could not be read: {why}"),
                )
                .await,
            ),
        }
    }
    out
}

async fn set_aside(dir: &Path, name: &str, kind: Option<TxnKind>, why: String) -> Recovered {
    let folder = match trash::keep_out_of_purge(dir).await {
        Ok(kept) => kept,
        Err(e) => {
            crate::diag!("content txn: could not set {} aside: {e}", dir.display());
            dir.to_path_buf()
        }
    };
    crate::diag!("content txn: set {} aside: {why}", folder.display());
    Recovered {
        token: name.to_string(),
        kind,
        outcome: RecoveredOutcome::Incomplete {
            folder: folder.display().to_string(),
            details: why,
        },
    }
}

async fn token_state(dir: &Path) -> std::io::Result<TokenState> {
    if fs::try_exists(dir.join(CLOSED_FILE)).await? {
        return Ok(TokenState::Closed);
    }
    if fs::try_exists(dir.join(RECORD_FILE)).await? {
        Ok(TokenState::Pending)
    } else {
        Ok(TokenState::Closed)
    }
}

async fn read_record(dir: &Path) -> Result<Record, String> {
    let bytes = fs::read(dir.join(RECORD_FILE))
        .await
        .map_err(|e| e.to_string())?;
    let record: Record = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if record.version != RECORD_VERSION {
        return Err(format!("unknown record version {}", record.version));
    }
    Ok(record)
}

/// Temp-then-rename, the `trash::write_record` shape.
async fn write_record(dir: &Path, record: &Record) -> std::io::Result<()> {
    let final_path = dir.join(RECORD_FILE);
    let seq = WRITE_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = final_path.with_extension(format!("json.tmp.{}.{seq}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(record).map_err(std::io::Error::other)?;
    fs::write(&tmp, &bytes).await?;
    fs::rename(&tmp, &final_path).await
}

async fn list_names(root: &Path) -> std::io::Result<Vec<String>> {
    let mut rd = fs::read_dir(root).await?;
    let mut names = Vec::new();
    while let Some(entry) = rd.next_entry().await? {
        names.push(entry.file_name().to_string_lossy().into_owned());
    }
    Ok(names)
}

async fn discard_dir(dir: &Path, when: &str) {
    match fs::remove_dir_all(dir).await {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => crate::diag!(
            "content txn: could not remove {} {when}: {e}",
            dir.display()
        ),
    }
}

/// The file's SHA-1, streamed; `None` when there is no file.
pub async fn file_sha1(path: &Path) -> std::io::Result<Option<String>> {
    let mut file = match fs::File::open(path).await {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut hasher = Sha1::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(Some(hex::encode(hasher.finalize())))
}

/// Whether either spelling of a mod holds `sha1`.
async fn holds_sha(plain: &Path, disabled: &Path, sha1: &str) -> Result<bool, String> {
    for p in [plain, disabled] {
        let sha = file_sha1(p)
            .await
            .map_err(|e| format!("could not read it: {e}"))?;
        if sha.is_some_and(|s| s.eq_ignore_ascii_case(sha1)) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether the name `back` answers to (either spelling) already holds `sha1`.
/// "Could not tell" is `false`: the caller then treats the name as taken.
async fn same_bytes_in_place(back: &Path, sha1: &str) -> bool {
    let (Some(folder), Some(file)) = (back.parent(), back.file_name()) else {
        return false;
    };
    let file = file.to_string_lossy().into_owned();
    let base = file.strip_suffix(".disabled").unwrap_or(file.as_str());
    let plain = folder.join(base);
    let disabled = folder.join(format!("{base}.disabled"));
    holds_sha(&plain, &disabled, sha1).await.unwrap_or(false)
}

fn mc_dir(instance_root: &Path) -> PathBuf {
    instance_root.join(".minecraft")
}

/// Join a `/`-separated relative path segment by segment, so the result uses
/// the platform's separators.
fn join_rel(base: &Path, rel: &str) -> PathBuf {
    rel.split('/')
        .filter(|seg| !seg.is_empty())
        .fold(base.to_path_buf(), |path, seg| path.join(seg))
}

fn safe_rel(rel: &str) -> bool {
    crate::mods::modpack::path_safety::is_safe_relative_path(rel)
}

fn with_disabled_suffix(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".disabled");
    PathBuf::from(s)
}

fn io_err(path: &Path, e: impl std::fmt::Display) -> Error {
    Error::ModsInstancePath {
        path: path.display().to_string(),
        details: e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::platform::ModSource;
    use tempfile::TempDir;

    fn sha_of(bytes: &[u8]) -> String {
        hex::encode(Sha1::digest(bytes))
    }

    fn mods(root: &Path) -> PathBuf {
        installed::mods_dir(root)
    }

    /// Put `bytes` at `.minecraft/<rel>`; returns their SHA-1.
    fn place(root: &Path, rel: &str, bytes: &[u8]) -> String {
        let p = join_rel(&mc_dir(root), rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, bytes).unwrap();
        sha_of(bytes)
    }

    fn row(filename: &str, sha: &str, enabled: bool) -> InstalledMod {
        InstalledMod {
            filename: filename.into(),
            sha1: sha.into(),
            source: Some(ModSource::Modrinth),
            project_id: Some(format!("proj-{filename}")),
            version_id: Some("v1".into()),
            name: filename.into(),
            version_number: Some("1.0".into()),
            installed_at: "2026-01-01T00:00:00Z".into(),
            enabled,
            enrich_attempted: false,
            requires: vec!["lib-a".into()],
        }
    }

    fn plan(stage: Vec<StageEntry>, create: Vec<CreateEntry>) -> Plan {
        Plan {
            kind: TxnKind::ModUpdate {
                name: "M".into(),
                from: Some("1".into()),
                to: "2".into(),
            },
            stage,
            create,
            prior_pack_origin: None,
            prior_instance: None,
        }
    }

    fn create(rel: &str, sha: &str) -> CreateEntry {
        CreateEntry {
            rel: rel.into(),
            sha1: sha.into(),
            project_id: None,
            pre_existing: false,
            prior_row: None,
        }
    }

    fn staged(rel: &str, sha: &str, row: Option<InstalledMod>) -> StageEntry {
        StageEntry {
            rel: rel.into(),
            sha1: sha.into(),
            row,
        }
    }

    fn txn_entries(root: &Path) -> Vec<String> {
        match std::fs::read_dir(txn_root(root)) {
            Ok(rd) => rd
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => panic!("{e}"),
        }
    }

    fn origin(version: &str) -> PackOrigin {
        PackOrigin {
            project_id: Some("p".into()),
            source: ModSource::Modrinth,
            project_name: "Pack".into(),
            version: version.into(),
            files: vec![],
            missing_mods: vec![],
            skipped_overrides: vec![],
            resolved_missing: vec![],
            inert_loader_jars: vec![],
        }
    }

    fn instance() -> InstanceFile {
        InstanceFile {
            id: "i".into(),
            uid: None,
            name: "Pack".into(),
            mc_version: "1.20.1".into(),
            loader: LoaderKind::Fabric,
            loader_version: Some("0.16.5".into()),
            max_heap_mb: 4096,
            min_heap_mb: None,
            extra_jvm_args: String::new(),
            created_unix_ms: 0.0,
            mrpack_name: Some("Pack".into()),
            mrpack_version: Some("1.0".into()),
            mrpack_project_id: Some("p".into()),
            mrpack_source: Some(ModSource::Modrinth),
            mrpack_summary: None,
            mrpack_version_id: None,
            integrity: None,
            imported_from: None,
            created_from_server: None,
            handled_log_sig: None,
        }
    }

    #[tokio::test]
    async fn begin_writes_the_record_before_anything_moves() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        let sha = place(root, "mods/a.jar", b"A");
        let txn = begin(
            lock(root).await,
            plan(vec![staged("mods/a.jar", &sha, None)], vec![]),
        )
        .await
        .unwrap();
        let record = read_record(&txn.dir).await.unwrap();
        assert_eq!(record.plan.stage[0].rel, "mods/a.jar");
        assert!(mods(root).join("a.jar").exists(), "begin touches nothing");
        let names = txn_entries(root);
        assert!(names.iter().all(|n| !n.ends_with(".tmp")), "{names:?}");
        txn.finish(Ok(())).await.unwrap();
    }

    #[tokio::test]
    async fn begin_refuses_while_a_pending_token_exists() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        let stale = txn_root(root).join("1000-abcdef12");
        std::fs::create_dir_all(&stale).unwrap();
        std::fs::write(stale.join(RECORD_FILE), b"{}").unwrap();
        let err = begin(lock(root).await, plan(vec![], vec![]))
            .await
            .err()
            .unwrap();
        assert!(
            matches!(err, Error::ContentUpdateUnfinished { .. }),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn stage_moves_the_file_and_drops_its_row() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        let sha = place(root, "mods/a.jar.disabled", b"A");
        installed::add(root, row("a.jar", &sha, false))
            .await
            .unwrap();
        let entry = staged("mods/a.jar.disabled", &sha, Some(row("a.jar", &sha, false)));
        let txn = begin(lock(root).await, plan(vec![entry.clone()], vec![]))
            .await
            .unwrap();
        txn.stage(&entry).await.unwrap();
        assert!(!mods(root).join("a.jar.disabled").exists());
        assert!(join_rel(&txn.dir.join(FILES_DIR), "mods/a.jar.disabled").exists());
        assert!(installed::list(root).await.unwrap().is_empty());
        txn.finish(Ok(())).await.unwrap();
    }

    #[tokio::test]
    async fn a_failed_stage_changes_nothing() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        let sha = place(root, "mods/a.jar", b"A");
        installed::add(root, row("a.jar", &sha, true))
            .await
            .unwrap();
        let entry = staged("mods/a.jar", &sha, Some(row("a.jar", &sha, true)));
        let txn = begin(lock(root).await, plan(vec![entry.clone()], vec![]))
            .await
            .unwrap();
        // A non-empty directory where the file would go blocks the rename.
        let blocker = join_rel(&txn.dir.join(FILES_DIR), "mods/a.jar");
        std::fs::create_dir_all(blocker.join("x")).unwrap();
        assert!(txn.stage(&entry).await.is_err());
        assert!(mods(root).join("a.jar").exists());
        assert_eq!(installed::list(root).await.unwrap().len(), 1);
        std::fs::remove_dir_all(&blocker).unwrap();
        let err = txn.rollback(Error::InstanceBusy).await;
        assert!(matches!(err, Error::InstanceBusy), "{err:?}");
    }

    #[tokio::test]
    async fn finish_ok_closes_the_token() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        let sha = place(root, "mods/a.jar", b"A");
        let entry = staged("mods/a.jar", &sha, None);
        let txn = begin(lock(root).await, plan(vec![entry.clone()], vec![]))
            .await
            .unwrap();
        txn.stage(&entry).await.unwrap();
        assert_eq!(txn.finish(Ok(5)).await.unwrap(), 5);
        assert!(txn_entries(root).is_empty(), "{:?}", txn_entries(root));
        assert!(!mods(root).join("a.jar").exists());
    }

    #[tokio::test]
    async fn rollback_puts_a_staged_disabled_jar_back_with_its_row_verbatim() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        let sha = place(root, "mods/a.jar.disabled", b"A");
        let mut r = row("a.jar", &sha, false);
        r.requires = vec!["lib-x".into(), "lib-y".into()];
        r.installed_at = "2025-05-05T05:05:05Z".into();
        installed::add(root, r.clone()).await.unwrap();
        let entry = staged("mods/a.jar.disabled", &sha, Some(r.clone()));
        let txn = begin(lock(root).await, plan(vec![entry.clone()], vec![]))
            .await
            .unwrap();
        txn.stage(&entry).await.unwrap();

        let err = txn.rollback(Error::InstanceBusy).await;

        assert!(matches!(err, Error::InstanceBusy), "{err:?}");
        assert!(mods(root).join("a.jar.disabled").exists());
        let rows = installed::list(root).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].enabled);
        assert_eq!(rows[0].requires, r.requires);
        assert_eq!(rows[0].installed_at, r.installed_at);
        assert!(txn_entries(root).is_empty(), "{:?}", txn_entries(root));
    }

    #[tokio::test]
    async fn rollback_removes_a_created_file_only_when_its_sha_matches() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        let planned = sha_of(b"new-bytes");
        let _theirs = place(root, "mods/b.jar", b"someone-elses");
        let txn = begin(
            lock(root).await,
            plan(vec![], vec![create("mods/b.jar", &planned)]),
        )
        .await
        .unwrap();
        let err = txn.rollback(Error::InstanceBusy).await;
        assert!(matches!(err, Error::InstanceBusy), "{err:?}");
        assert!(mods(root).join("b.jar").exists(), "not ours: left alone");

        std::fs::remove_file(mods(root).join("b.jar")).unwrap();
        place(root, "mods/b.jar", b"new-bytes");
        installed::add(root, row("b.jar", &planned, true))
            .await
            .unwrap();
        let txn = begin(
            lock(root).await,
            plan(vec![], vec![create("mods/b.jar", &planned)]),
        )
        .await
        .unwrap();
        let err = txn.rollback(Error::InstanceBusy).await;
        assert!(matches!(err, Error::InstanceBusy), "{err:?}");
        assert!(!mods(root).join("b.jar").exists(), "ours: removed");
        assert!(installed::list(root).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn rollback_restores_the_row_an_idempotent_install_rewrote() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        let sha = place(root, "mods/c.jar", b"C");
        let mut prior = row("c.jar", &sha, true);
        prior.requires = vec!["dep".into()];
        let mut rewritten = prior.clone();
        rewritten.requires = vec![];
        installed::add(root, rewritten).await.unwrap();
        let c = CreateEntry {
            rel: "mods/c.jar".into(),
            sha1: sha.clone(),
            project_id: None,
            pre_existing: true,
            prior_row: Some(prior),
        };
        let txn = begin(lock(root).await, plan(vec![], vec![c]))
            .await
            .unwrap();
        let err = txn.rollback(Error::InstanceBusy).await;
        assert!(matches!(err, Error::InstanceBusy), "{err:?}");
        assert!(mods(root).join("c.jar").exists());
        assert_eq!(
            installed::list(root).await.unwrap()[0].requires,
            vec!["dep".to_string()]
        );
    }

    #[tokio::test]
    async fn rollback_re_enables_a_pre_existing_mod_carry_disable_switched_off() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        let sha = place(root, "mods/c.jar.disabled", b"C");
        let c = CreateEntry {
            rel: "mods/c.jar".into(),
            sha1: sha,
            project_id: None,
            pre_existing: true,
            prior_row: None,
        };
        let txn = begin(lock(root).await, plan(vec![], vec![c]))
            .await
            .unwrap();
        let err = txn.rollback(Error::InstanceBusy).await;
        assert!(matches!(err, Error::InstanceBusy), "{err:?}");
        assert!(mods(root).join("c.jar").exists());
        assert!(!mods(root).join("c.jar.disabled").exists());
    }

    #[tokio::test]
    async fn rollback_restores_pack_origin_and_instance_fields() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        let path = root.join("instance.json");
        crate::instances::store::write_instance_json(&path, &instance()).unwrap();
        installed::set_pack_origin(root, origin("1.0"))
            .await
            .unwrap();
        let mut p = plan(vec![], vec![]);
        p.kind = TxnKind::PackUpdate {
            pack: "Pack".into(),
            from: "1.0".into(),
            to: "2.0".into(),
        };
        p.prior_pack_origin = Some(origin("1.0"));
        p.prior_instance = Some(PriorInstance::of(&instance()));
        let txn = begin(lock(root).await, p).await.unwrap();
        // The update got as far as writing its new record.
        installed::set_pack_origin(root, origin("2.0"))
            .await
            .unwrap();
        let mut inst = crate::instances::store::read_instance_json(&path).unwrap();
        crate::instances::apply_pack_update_fields(
            &mut inst,
            "2.0".into(),
            "1.21.1".into(),
            LoaderKind::Vanilla,
            None,
            "newId".into(),
        );
        crate::instances::store::write_instance_json(&path, &inst).unwrap();

        let err = txn.rollback(Error::InstanceBusy).await;

        assert!(matches!(err, Error::InstanceBusy), "{err:?}");
        let origin_now = installed::get_pack_origin(root).await.unwrap().unwrap();
        assert_eq!(origin_now.version, "1.0");
        let after = crate::instances::store::read_instance_json(&path).unwrap();
        assert_eq!(after.mrpack_version.as_deref(), Some("1.0"));
        assert_eq!(after.mrpack_version_id, None);
        assert_eq!(after.mc_version, "1.20.1");
        assert_eq!(after.loader, LoaderKind::Fabric);
        assert_eq!(after.loader_version.as_deref(), Some("0.16.5"));
    }

    #[tokio::test]
    async fn a_taken_name_on_the_way_back_keeps_the_folder_and_says_so() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        let sha = place(root, "mods/a.jar", b"A");
        installed::add(root, row("a.jar", &sha, true))
            .await
            .unwrap();
        let entry = staged("mods/a.jar", &sha, Some(row("a.jar", &sha, true)));
        let txn = begin(lock(root).await, plan(vec![entry.clone()], vec![]))
            .await
            .unwrap();
        txn.stage(&entry).await.unwrap();
        place(root, "mods/a.jar", b"SOMEONE-ELSE");

        match txn.rollback(Error::InstanceBusy).await {
            Error::ContentUpdateRollbackIncomplete { folder, details } => {
                assert!(folder.ends_with("-kept"), "{folder}");
                assert!(details.contains("mods/a.jar"), "{details}");
                assert!(join_rel(&Path::new(&folder).join(FILES_DIR), "mods/a.jar").exists());
            }
            other => panic!("expected ContentUpdateRollbackIncomplete, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn recover_pending_undoes_a_crashed_update_and_is_idempotent() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        let old = place(root, "mods/old.jar", b"OLD");
        installed::add(root, row("old.jar", &old, true))
            .await
            .unwrap();
        let new = sha_of(b"NEW");
        let entry = staged("mods/old.jar", &old, Some(row("old.jar", &old, true)));
        let txn = begin(
            lock(root).await,
            plan(vec![entry.clone()], vec![create("mods/new.jar", &new)]),
        )
        .await
        .unwrap();
        txn.stage(&entry).await.unwrap();
        place(root, "mods/new.jar", b"NEW");
        installed::add(root, row("new.jar", &new, true))
            .await
            .unwrap();
        drop(txn); // the launcher died here

        let report = recover_pending(root, u64::MAX).await;

        assert_eq!(report.len(), 1);
        assert!(matches!(report[0].outcome, RecoveredOutcome::Restored));
        assert!(mods(root).join("old.jar").exists());
        assert!(!mods(root).join("new.jar").exists());
        let rows = installed::list(root).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].sha1, old);
        assert!(recover_pending(root, u64::MAX).await.is_empty());
        assert!(mods(root).join("old.jar").exists());
    }

    #[tokio::test]
    async fn recover_pending_sets_an_unreadable_record_aside() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        let dir = txn_root(root).join("1000-abcdef12");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(RECORD_FILE), b"not json").unwrap();

        let report = recover_pending(root, 2000).await;

        assert_eq!(report.len(), 1);
        assert!(
            matches!(&report[0].outcome, RecoveredOutcome::Incomplete { folder, .. } if folder.ends_with("-kept")),
            "{:?}",
            report[0].outcome
        );
        assert!(txn_root(root).join("1000-abcdef12-kept").exists());
        assert!(!dir.exists());
    }

    #[tokio::test]
    async fn recover_pending_sweeps_closed_and_stale_tmp_but_not_fresh_tmp() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        let t = txn_root(root);
        let closed = t.join("1000-aaaaaaaa");
        std::fs::create_dir_all(&closed).unwrap();
        std::fs::write(closed.join(CLOSED_FILE), b"").unwrap();
        std::fs::write(closed.join(RECORD_FILE), b"{}").unwrap();
        let recordless = t.join("1001-bbbbbbbb");
        std::fs::create_dir_all(recordless.join(FILES_DIR)).unwrap();
        let old_tmp = t.join("1002-cccccccc.tmp");
        std::fs::create_dir_all(&old_tmp).unwrap();
        let fresh_tmp = t.join("5000-dddddddd.tmp");
        std::fs::create_dir_all(&fresh_tmp).unwrap();

        let report = sweep_closed(root, 3000).await;

        assert!(report.is_empty(), "{report:?}");
        assert!(!closed.exists());
        assert!(!recordless.exists());
        assert!(!old_tmp.exists());
        assert!(fresh_tmp.exists());
    }
}
