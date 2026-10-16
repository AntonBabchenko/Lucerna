//! The copy itself (spec §A4). Phase 1 stages plain byte copies of the source
//! under `<target>/lucerna/transfer/<op>/stage/` and hashes them; nothing of
//! the target is touched. Phase 2 commits through `mods::txn`: the plan is
//! recorded before anything moves, a replaced file is set aside by rename,
//! every placed file is undone by its hash, and an interrupted copy is undone
//! at the next start. Replaced files are kept (`SetAsideDisposal::Keep`) and go
//! to the Recycle Bin after the commit.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use specta::Type;

use super::catalog::TransferItemId;
use crate::error::Error;

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

pub async fn run(req: Request, seams: Seams, progress: Progress) -> Result<TransferOutcome, Error> {
    let _ = (req, seams, progress);
    todo!("red-first stub: the engine lands in the next commit")
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
        assert!(
            !ops.exists() || std::fs::read_dir(&ops).unwrap().next().is_none(),
            "no staging left behind"
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
