//! The copy itself (spec §A4). Phase 1 stages plain byte copies of the source
//! under `<target>/lucerna/transfer/<op>/stage/` and hashes them; nothing of
//! the target is touched. Phase 2 commits through `mods::txn`: the plan is
//! recorded before anything moves, a replaced file is set aside by rename,
//! every placed file is undone by its hash, and an interrupted copy is undone
//! at the next start. Replaced files are kept (`SetAsideDisposal::Keep`) and go
//! to the Recycle Bin after the commit.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use specta::Type;

use super::catalog::{self, Scope, TransferItemId};
use super::preview::{scan_root, Refusal};
use super::walk::{join_rel, WalkedFile};
use crate::error::Error;
use crate::mods::txn::{self, CreateEntry, Plan, SetAsideDisposal, StageEntry, TxnKind};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct TransferSelection {
    pub items: Vec<TransferItemId>,
    /// Top-level folder names from the preview's "Other folders".
    pub other: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum OnConflict {
    Replace,
    KeepExisting,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct TransferOutcome {
    pub copied: u32,
    pub replaced: u32,
    pub kept_existing: u32,
    pub skipped: u32,
    /// Where the replaced files are when they did not go to the Recycle Bin.
    pub replaced_kept_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum TransferPhase {
    Copying,
    Placing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
pub struct TransferProgress {
    pub phase: TransferPhase,
    pub current: u32,
    pub total: u32,
}

pub type Progress = Arc<dyn Fn(TransferProgress) + Send + Sync>;

pub struct Request {
    pub source_mc: PathBuf,
    /// The target INSTANCE root (holds `.minecraft/` and `lucerna/`).
    pub target_root: PathBuf,
    pub source_name: String,
    pub target_name: String,
    pub selection: TransferSelection,
    pub on_conflict: OnConflict,
}

type PlaceFn = dyn Fn(&Path, &Path) -> std::io::Result<()> + Send + Sync;
type RecycleFn = dyn Fn(PathBuf) -> std::io::Result<()> + Send + Sync;

/// What the engine does to the file system that tests replace.
pub struct Seams {
    pub place: Arc<PlaceFn>,
    pub recycle: Arc<RecycleFn>,
    /// Replaced files larger than this stay in `lucerna/replaced/` (spec §A4):
    /// what Windows does with an item over the Recycle Bin's quota is unverified.
    pub keep_over_bytes: u64,
}

impl Seams {
    pub fn production() -> Self {
        Self::with_recycle(Arc::new(crate::platform::recycle_bin::to_recycle_bin))
    }

    pub fn with_recycle(recycle: Arc<RecycleFn>) -> Self {
        Self {
            place: Arc::new(|from, to| std::fs::rename(from, to)),
            recycle,
            keep_over_bytes: 1 << 30,
        }
    }
}

struct Staged {
    rel: String,
    path: PathBuf,
    sha1: String,
}

/// One file of the commit: placed at `rel` from `staged`, after setting aside
/// the target file it replaces when `set_aside` is `Some`.
struct Step {
    rel: String,
    staged: PathBuf,
    sha1: String,
    set_aside: Option<StageEntry>,
}

struct Planned {
    steps: Vec<Step>,
    kept_existing: u32,
    /// Bytes of the target files `steps` sets aside.
    aside_bytes: u64,
}

pub async fn run(req: Request, seams: Seams, progress: Progress) -> Result<TransferOutcome, Error> {
    let target_mc = req.target_root.join(".minecraft");
    let ops_root = req.target_root.join("lucerna").join("transfer");
    sweep_stale(&ops_root);

    let roots = resolve(&req.source_mc, &req.selection)?;
    let mut files: Vec<(String, WalkedFile)> = Vec::new();
    let mut skipped = 0u32;
    for (key, rel, scope) in &roots {
        // Re-read from disk: the preview the dialog showed may be stale.
        let Some(scan) = scan_root(&req.source_mc, &target_mc, rel, *scope) else {
            continue; // gone since the preview: nothing of it to copy
        };
        if let Some(refusal) = scan.refusal {
            return Err(Error::TransferRefused { refusal });
        }
        skipped += scan.walked.skipped;
        files.extend(scan.walked.files.into_iter().map(|f| (key.clone(), f)));
    }

    let op_dir = ops_root.join(crate::instances::ids::new_id());
    let stage_dir = op_dir.join("stage");
    let source_mc = req.source_mc.clone();
    let p = Arc::clone(&progress);
    let staged = tokio::task::spawn_blocking(move || stage_all(&source_mc, &stage_dir, &files, &p))
        .await
        .map_err(|e| Error::io("<transfer stage>", e))
        .and_then(|r| r);
    let staged = match staged {
        Ok(s) => s,
        Err(e) => {
            remove_op_dir(&op_dir);
            return Err(Error::TransferFailed { cause: Box::new(e) });
        }
    };

    let lock = txn::lock(&req.target_root).await;
    let planned = match plan_steps(&target_mc, staged, req.on_conflict).await {
        Ok(planned) => planned,
        Err(e) => {
            remove_op_dir(&op_dir);
            return Err(e);
        }
    };
    let Planned {
        steps,
        kept_existing,
        aside_bytes,
    } = planned;
    if steps.is_empty() {
        remove_op_dir(&op_dir);
        return Ok(TransferOutcome {
            copied: 0,
            replaced: 0,
            kept_existing,
            skipped,
            replaced_kept_at: None,
        });
    }
    let replaced = steps.iter().filter(|s| s.set_aside.is_some()).count() as u32;
    let folder = replaced_folder(&req.target_name, SystemTime::now());
    let plan = Plan {
        kind: TxnKind::DataCopy {
            source: req.source_name.clone(),
        },
        stage: steps.iter().filter_map(|s| s.set_aside.clone()).collect(),
        create: steps
            .iter()
            .map(|s| CreateEntry {
                rel: s.rel.clone(),
                sha1: s.sha1.clone(),
                project_id: None,
                pre_existing: false,
                disabled_twin: false,
                prior_row: None,
            })
            .collect(),
        prior_pack_origin: None,
        prior_instance: None,
        set_aside: if replaced > 0 {
            SetAsideDisposal::Keep {
                folder: folder.clone(),
            }
        } else {
            SetAsideDisposal::Delete
        },
    };
    let transaction = match txn::begin(lock, plan).await {
        Ok(t) => t,
        Err(e) => {
            remove_op_dir(&op_dir);
            return Err(e);
        }
    };
    let mut created: Vec<PathBuf> = Vec::new();
    let committed = commit(
        &transaction,
        &steps,
        &target_mc,
        &seams,
        &mut created,
        &progress,
    )
    .await;
    if let Err(e) = finish(transaction, committed).await {
        remove_created(&created);
        remove_op_dir(&op_dir);
        return Err(e);
    }
    let replaced_kept_at = if replaced > 0 {
        dispose(&req.target_root, &folder, aside_bytes, &seams).await
    } else {
        None
    };
    remove_op_dir(&op_dir);
    // The service folders this copy may have created, once nothing is in them.
    remove_if_empty(&req.target_root.join("lucerna").join("replaced"));
    remove_if_empty(&ops_root);
    Ok(TransferOutcome {
        copied: steps.len() as u32,
        replaced,
        kept_existing,
        skipped,
        replaced_kept_at,
    })
}

/// The roots to copy, with their staging keys. An `Other` name must be one the
/// catalog offers AND a top-level folder of the source now.
fn resolve(
    source_mc: &Path,
    sel: &TransferSelection,
) -> Result<Vec<(String, String, Scope)>, Error> {
    let mut out = Vec::new();
    for id in &sel.items {
        for root in id.transfer_roots() {
            out.push((id.key().to_string(), root.rel.to_string(), root.scope));
        }
    }
    if sel.other.is_empty() {
        return Ok(out);
    }
    let offered = super::preview::other_folder_names(source_mc)
        .map_err(|e| Error::io(source_mc.display().to_string(), e))?;
    for name in &sel.other {
        if !catalog::is_offered_other(name) || !offered.iter().any(|o| o == name) {
            return Err(Error::TransferInvalidSelection { name: name.clone() });
        }
        out.push((format!("other-{name}"), name.clone(), Scope::WholeTree));
    }
    Ok(out)
}

fn stage_all(
    source_mc: &Path,
    stage_dir: &Path,
    files: &[(String, WalkedFile)],
    progress: &Progress,
) -> Result<Vec<Staged>, Error> {
    let total = files.len() as u32;
    let mut out = Vec::with_capacity(files.len());
    for (i, (key, f)) in files.iter().enumerate() {
        if !crate::mods::modpack::path_safety::is_safe_relative_path(&f.rel) {
            return Err(Error::TransferInvalidSelection {
                name: f.rel.clone(),
            });
        }
        let from = join_rel(source_mc, &f.rel);
        let to = join_rel(&stage_dir.join(key), &f.rel);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| Error::io(parent.display().to_string(), e))?;
        }
        let sha1 = copy_hashing(&from, &to)?;
        out.push(Staged {
            rel: f.rel.clone(),
            path: to,
            sha1,
        });
        progress(TransferProgress {
            phase: TransferPhase::Copying,
            current: i as u32 + 1,
            total,
        });
    }
    Ok(out)
}

/// Copy `from` into a NEW file `to` — never into an existing one — hashing it.
fn copy_hashing(from: &Path, to: &Path) -> Result<String, Error> {
    let mut src =
        std::fs::File::open(from).map_err(|e| Error::io(from.display().to_string(), e))?;
    let mut dst =
        std::fs::File::create_new(to).map_err(|e| Error::io(to.display().to_string(), e))?;
    let mut hasher = Sha1::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = src
            .read(&mut buf)
            .map_err(|e| Error::io(from.display().to_string(), e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        dst.write_all(&buf[..n])
            .map_err(|e| Error::io(to.display().to_string(), e))?;
    }
    dst.sync_all()
        .map_err(|e| Error::io(to.display().to_string(), e))?;
    Ok(hex::encode(hasher.finalize()))
}

/// Decide each staged file against the target as it is now.
async fn plan_steps(
    target_mc: &Path,
    staged: Vec<Staged>,
    on_conflict: OnConflict,
) -> Result<Planned, Error> {
    let mut planned = Planned {
        steps: Vec::new(),
        kept_existing: 0,
        aside_bytes: 0,
    };
    for s in staged {
        let target = join_rel(target_mc, &s.rel);
        let set_aside = match std::fs::symlink_metadata(&target) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(Error::io(target.display().to_string(), e)),
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(Error::TransferRefused {
                    refusal: Refusal::InnerLink { rel: s.rel },
                })
            }
            Ok(meta) if !meta.is_file() => {
                return Err(Error::TransferRefused {
                    refusal: Refusal::PrefixClash { rel: s.rel },
                })
            }
            Ok(meta) => match on_conflict {
                OnConflict::KeepExisting => {
                    planned.kept_existing += 1;
                    continue;
                }
                OnConflict::Replace => {
                    let old = txn::file_sha1(&target)
                        .await
                        .map_err(|e| Error::io(target.display().to_string(), e))?
                        .ok_or_else(|| {
                            Error::io(target.display().to_string(), "vanished while planning")
                        })?;
                    planned.aside_bytes += meta.len();
                    Some(StageEntry {
                        rel: s.rel.clone(),
                        sha1: old,
                        row: None,
                    })
                }
            },
        };
        planned.steps.push(Step {
            rel: s.rel,
            staged: s.path,
            sha1: s.sha1,
            set_aside,
        });
    }
    Ok(planned)
}

async fn commit(
    transaction: &txn::Txn,
    steps: &[Step],
    target_mc: &Path,
    seams: &Seams,
    created: &mut Vec<PathBuf>,
    progress: &Progress,
) -> Result<(), Error> {
    let total = steps.len() as u32;
    for (i, step) in steps.iter().enumerate() {
        if let Some(entry) = &step.set_aside {
            transaction.stage(entry).await?;
        }
        let to = join_rel(target_mc, &step.rel);
        ensure_parents(target_mc, &to, created)?;
        (seams.place)(&step.staged, &to).map_err(|e| place_error(&to, e))?;
        progress(TransferProgress {
            phase: TransferPhase::Placing,
            current: i as u32 + 1,
            total,
        });
    }
    Ok(())
}

/// Commit or undo, and word the error as a copy's, never an update's.
async fn finish(transaction: txn::Txn, committed: Result<(), Error>) -> Result<(), Error> {
    let result = match committed {
        // A close that cannot be recorded rolls back inside `finish`; its
        // clean error is the close error itself.
        Ok(()) => transaction.finish(Ok(())).await,
        Err(cause) => Err(transaction
            .rollback_as(cause, |cause| Error::TransferFailed {
                cause: Box::new(cause),
            })
            .await),
    };
    result.map_err(|e| match e {
        Error::ContentUpdateRollbackIncomplete {
            folder,
            cause,
            stuck,
        } => Error::TransferRollbackIncomplete {
            folder,
            cause,
            stuck,
        },
        e @ Error::TransferFailed { .. } => e,
        other => Error::TransferFailed {
            cause: Box::new(other),
        },
    })
}

/// Create every missing folder between the game dir and `to`'s parent,
/// recording each (an undo removes them again).
fn ensure_parents(target_mc: &Path, to: &Path, created: &mut Vec<PathBuf>) -> Result<(), Error> {
    let Some(parent) = to.parent() else {
        return Ok(());
    };
    let below = parent.strip_prefix(target_mc).unwrap_or(parent);
    let mut path = target_mc.to_path_buf();
    for seg in below.components() {
        path.push(seg);
        match std::fs::symlink_metadata(&path) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&path).map_err(|e| Error::io(path.display().to_string(), e))?;
                created.push(path.clone());
            }
            Err(e) => return Err(Error::io(path.display().to_string(), e)),
        }
    }
    Ok(())
}

fn place_error(to: &Path, e: std::io::Error) -> Error {
    if e.kind() == std::io::ErrorKind::CrossesDevices {
        Error::io(
            to.display().to_string(),
            "this folder links to another drive; Lucerna does not copy across drives",
        )
    } else {
        Error::io(to.display().to_string(), e)
    }
}

/// After an undo: the folders this copy created, deepest first, only while
/// empty (`remove_dir` refuses a folder with anything in it).
fn remove_created(created: &[PathBuf]) {
    for dir in created.iter().rev() {
        match std::fs::remove_dir(dir) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => crate::diag!(
                "transfer: could not remove the empty folder {} ({e})",
                dir.display()
            ),
        }
    }
}

/// Remove `dir` only when it is empty. A folder that still holds something
/// (another copy's kept files, a leftover the next copy sweeps) stays.
fn remove_if_empty(dir: &Path) {
    match std::fs::remove_dir(dir) {
        Ok(()) => {}
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
            ) => {}
        Err(e) => crate::diag!("transfer: could not tidy {} ({e})", dir.display()),
    }
}

/// The op dir holds only staged copies: removing it never loses the user's
/// data, and a failure is left to the next copy's sweep.
fn remove_op_dir(dir: &Path) {
    match std::fs::remove_dir_all(dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => crate::diag!(
            "transfer: could not remove {} ({e}); the next copy sweeps it",
            dir.display()
        ),
    }
}

/// Staging dirs a crash left: copies only, and the caller's
/// `claim_write(target)` means no other copy into this instance is running.
fn sweep_stale(ops_root: &Path) {
    let entries = match std::fs::read_dir(ops_root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) => {
            crate::diag!("transfer: could not list {} ({e})", ops_root.display());
            return;
        }
    };
    for entry in entries {
        match entry {
            Ok(entry) => remove_op_dir(&entry.path()),
            Err(e) => crate::diag!("transfer: could not read {} ({e})", ops_root.display()),
        }
    }
}

/// `lucerna/replaced/Replaced by Lucerna – <target> – <date time>`: a name a
/// person recognises in the Recycle Bin.
fn replaced_folder(target_name: &str, now: SystemTime) -> String {
    let stamp = chrono::DateTime::<chrono::Local>::from(now).format("%Y-%m-%d %H-%M-%S");
    let safe: String = target_name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || " -_()".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!(
        "lucerna/replaced/Replaced by Lucerna – {} – {stamp}",
        safe.trim()
    )
}

/// Send the replaced files to the Recycle Bin, or say where they are.
async fn dispose(target_root: &Path, folder: &str, bytes: u64, seams: &Seams) -> Option<String> {
    let kept = join_rel(target_root, folder);
    if std::fs::symlink_metadata(&kept).is_err() {
        // Still in the closed transaction's folder (its move failed and was
        // logged); the next start moves it.
        return Some(txn::txn_root(target_root).display().to_string());
    }
    if bytes > seams.keep_over_bytes {
        return Some(kept.display().to_string());
    }
    let recycle = Arc::clone(&seams.recycle);
    match crate::platform::recycle_bin::off_thread(kept.clone(), move |p| recycle(p)).await {
        Ok(()) => None,
        Err(e) => {
            crate::diag!("transfer: replaced files stay in {} ({e})", kept.display());
            Some(kept.display().to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    use super::*;
    use crate::instances::transfer::walk::join_rel;
    use crate::instances::transfer::Refusal;

    struct Pair {
        _td: tempfile::TempDir,
        src_mc: PathBuf,
        dst_root: PathBuf,
    }

    fn pair() -> Pair {
        let td = tempfile::tempdir().unwrap();
        let src_mc = td.path().join("src/.minecraft");
        let dst_root = td.path().join("dst");
        std::fs::create_dir_all(&src_mc).unwrap();
        std::fs::create_dir_all(dst_root.join(".minecraft")).unwrap();
        Pair {
            _td: td,
            src_mc,
            dst_root,
        }
    }

    fn dst_mc(p: &Pair) -> PathBuf {
        p.dst_root.join(".minecraft")
    }

    fn write(base: &Path, rel: &str, bytes: &[u8]) {
        let p = join_rel(base, rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }

    /// Every file under `dir` → its bytes, and every folder as `name/`.
    fn snapshot(dir: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
        let mut out = std::collections::BTreeMap::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap() {
                let p = e.unwrap().path();
                let rel = p
                    .strip_prefix(dir)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                if p.is_dir() {
                    out.insert(format!("{rel}/"), vec![]);
                    stack.push(p);
                } else {
                    out.insert(rel, std::fs::read(&p).unwrap());
                }
            }
        }
        out
    }

    fn request(p: &Pair, items: &[TransferItemId], on_conflict: OnConflict) -> Request {
        Request {
            source_mc: p.src_mc.clone(),
            target_root: p.dst_root.clone(),
            source_name: "Old".into(),
            target_name: "New".into(),
            selection: TransferSelection {
                items: items.to_vec(),
                other: vec![],
            },
            on_conflict,
        }
    }

    /// A Recycle Bin that records what it was handed and removes it.
    fn recorded_bin() -> (Arc<Mutex<Vec<PathBuf>>>, Arc<RecycleFn>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s = Arc::clone(&seen);
        (
            seen,
            Arc::new(move |p: PathBuf| {
                s.lock().unwrap().push(p.clone());
                std::fs::remove_dir_all(&p)
            }),
        )
    }

    fn no_progress() -> Progress {
        Arc::new(|_| {})
    }

    fn schematics() -> [TransferItemId; 1] {
        [TransferItemId::Schematics]
    }

    #[tokio::test]
    async fn copies_new_files_and_leaves_the_source_alone() {
        let p = pair();
        write(&p.src_mc, "schematics/a.nbt", b"A");
        let before = snapshot(&p.src_mc);
        let (_, bin) = recorded_bin();
        let out = run(
            request(&p, &schematics(), OnConflict::Replace),
            Seams::with_recycle(bin),
            no_progress(),
        )
        .await
        .unwrap();
        assert_eq!((out.copied, out.replaced, out.kept_existing), (1, 0, 0));
        assert_eq!(
            std::fs::read(dst_mc(&p).join("schematics/a.nbt")).unwrap(),
            b"A"
        );
        assert_eq!(snapshot(&p.src_mc), before);
        let ops = p.dst_root.join("lucerna/transfer");
        assert!(!ops.exists(), "no staging folder left behind");
        assert!(
            !p.dst_root.join("lucerna/replaced").exists(),
            "no empty service folder left behind"
        );
    }

    #[tokio::test]
    async fn replace_sends_the_old_file_to_the_bin() {
        let p = pair();
        write(&p.src_mc, "schematics/a.nbt", b"NEW");
        write(&dst_mc(&p), "schematics/a.nbt", b"OLD");
        let (seen, bin) = recorded_bin();
        let out = run(
            request(&p, &schematics(), OnConflict::Replace),
            Seams::with_recycle(bin),
            no_progress(),
        )
        .await
        .unwrap();
        assert_eq!(out.replaced, 1);
        assert_eq!(out.replaced_kept_at, None);
        assert_eq!(
            std::fs::read(dst_mc(&p).join("schematics/a.nbt")).unwrap(),
            b"NEW"
        );
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert!(
            seen[0].to_string_lossy().contains("Replaced by Lucerna"),
            "{:?}",
            seen[0]
        );
    }

    #[tokio::test]
    async fn keep_existing_never_touches_a_target_file() {
        let p = pair();
        write(&p.src_mc, "schematics/a.nbt", b"NEW");
        write(&p.src_mc, "schematics/b.nbt", b"B");
        write(&dst_mc(&p), "schematics/a.nbt", b"OLD");
        let (seen, bin) = recorded_bin();
        let out = run(
            request(&p, &schematics(), OnConflict::KeepExisting),
            Seams::with_recycle(bin),
            no_progress(),
        )
        .await
        .unwrap();
        assert_eq!((out.copied, out.kept_existing), (1, 1));
        assert_eq!(
            std::fs::read(dst_mc(&p).join("schematics/a.nbt")).unwrap(),
            b"OLD"
        );
        assert!(seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn large_replaced_files_stay_in_place() {
        let p = pair();
        write(&p.src_mc, "schematics/a.nbt", b"NEW");
        write(&dst_mc(&p), "schematics/a.nbt", b"OLD");
        let (seen, bin) = recorded_bin();
        let mut seams = Seams::with_recycle(bin);
        seams.keep_over_bytes = 0;
        let out = run(
            request(&p, &schematics(), OnConflict::Replace),
            seams,
            no_progress(),
        )
        .await
        .unwrap();
        let kept = PathBuf::from(out.replaced_kept_at.expect("kept in place"));
        assert_eq!(
            std::fs::read(kept.join("schematics/a.nbt")).unwrap(),
            b"OLD"
        );
        assert!(seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_recycle_failure_keeps_the_files_and_still_succeeds() {
        let p = pair();
        write(&p.src_mc, "schematics/a.nbt", b"NEW");
        write(&dst_mc(&p), "schematics/a.nbt", b"OLD");
        let failing: Arc<RecycleFn> = Arc::new(|_| Err(std::io::Error::other("no bin here")));
        let out = run(
            request(&p, &schematics(), OnConflict::Replace),
            Seams::with_recycle(failing),
            no_progress(),
        )
        .await
        .unwrap();
        let kept = PathBuf::from(out.replaced_kept_at.expect("kept"));
        assert_eq!(
            std::fs::read(kept.join("schematics/a.nbt")).unwrap(),
            b"OLD"
        );
    }

    #[tokio::test]
    async fn a_placement_failure_undoes_everything() {
        let p = pair();
        for name in ["a", "b", "c"] {
            write(&p.src_mc, &format!("schematics/deep/{name}.nbt"), b"NEW");
        }
        write(&dst_mc(&p), "schematics/x.nbt", b"OLD");
        let dst_before = snapshot(&dst_mc(&p));
        let src_before = snapshot(&p.src_mc);
        let calls = Arc::new(AtomicUsize::new(0));
        let c = Arc::clone(&calls);
        let (_, bin) = recorded_bin();
        let mut seams = Seams::with_recycle(bin);
        seams.place = Arc::new(move |from: &Path, to: &Path| {
            if c.fetch_add(1, Ordering::SeqCst) == 1 {
                return Err(std::io::Error::other("disk on fire"));
            }
            std::fs::rename(from, to)
        });
        let err = run(
            request(&p, &schematics(), OnConflict::Replace),
            seams,
            no_progress(),
        )
        .await
        .expect_err("the second placement fails");
        assert!(matches!(err, Error::TransferFailed { .. }), "{err:?}");
        assert_eq!(snapshot(&dst_mc(&p)), dst_before, "target as before");
        assert_eq!(snapshot(&p.src_mc), src_before);
        assert!(crate::mods::txn::pending_tokens(&p.dst_root)
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn a_failed_replace_puts_the_old_file_back() {
        let p = pair();
        write(&p.src_mc, "schematics/a.nbt", b"NEW");
        write(&dst_mc(&p), "schematics/a.nbt", b"OLD");
        let (_, bin) = recorded_bin();
        let mut seams = Seams::with_recycle(bin);
        seams.place = Arc::new(|_: &Path, _: &Path| Err(std::io::Error::other("no")));
        let err = run(
            request(&p, &schematics(), OnConflict::Replace),
            seams,
            no_progress(),
        )
        .await
        .expect_err("placement fails");
        assert!(matches!(err, Error::TransferFailed { .. }), "{err:?}");
        assert_eq!(
            std::fs::read(dst_mc(&p).join("schematics/a.nbt")).unwrap(),
            b"OLD"
        );
    }

    #[tokio::test]
    async fn a_prefix_clash_refuses_before_anything_moves() {
        let p = pair();
        write(&p.src_mc, "schematics/a.nbt", b"A");
        write(&dst_mc(&p), "schematics", b"a file");
        let before = snapshot(&p.dst_root);
        let (_, bin) = recorded_bin();
        let err = run(
            request(&p, &schematics(), OnConflict::Replace),
            Seams::with_recycle(bin),
            no_progress(),
        )
        .await
        .expect_err("refused");
        assert!(
            matches!(
                err,
                Error::TransferRefused {
                    refusal: Refusal::PrefixClash { .. }
                }
            ),
            "{err:?}"
        );
        assert_eq!(snapshot(&p.dst_root), before);
    }

    #[tokio::test]
    async fn a_stale_staging_dir_is_swept() {
        let p = pair();
        write(&p.dst_root, "lucerna/transfer/old-op/stage/x", b"x");
        write(&p.src_mc, "schematics/a.nbt", b"A");
        let (_, bin) = recorded_bin();
        run(
            request(&p, &schematics(), OnConflict::Replace),
            Seams::with_recycle(bin),
            no_progress(),
        )
        .await
        .unwrap();
        assert!(!p.dst_root.join("lucerna/transfer/old-op").exists());
    }

    #[tokio::test]
    async fn an_other_folder_is_copied_whole() {
        let p = pair();
        write(&p.src_mc, "kubejs/server_scripts/a.js", b"js");
        let mut req = request(&p, &[], OnConflict::Replace);
        req.selection.other = vec!["kubejs".into()];
        let (_, bin) = recorded_bin();
        let out = run(req, Seams::with_recycle(bin), no_progress())
            .await
            .unwrap();
        assert_eq!(out.copied, 1);
        assert_eq!(
            std::fs::read(dst_mc(&p).join("kubejs/server_scripts/a.js")).unwrap(),
            b"js"
        );
    }

    #[tokio::test]
    async fn a_managed_or_unsafe_other_name_is_refused() {
        let p = pair();
        write(&p.src_mc, "mods/m.jar", b"m");
        write(&p.src_mc, "Config/c.toml", b"c");
        for name in ["mods", "../x", "Config", "absent"] {
            let mut req = request(&p, &[], OnConflict::Replace);
            req.selection.other = vec![name.into()];
            let (_, bin) = recorded_bin();
            let err = run(req, Seams::with_recycle(bin), no_progress())
                .await
                .expect_err(name);
            assert!(
                matches!(err, Error::TransferInvalidSelection { .. }),
                "{name}: {err:?}"
            );
        }
    }
}
