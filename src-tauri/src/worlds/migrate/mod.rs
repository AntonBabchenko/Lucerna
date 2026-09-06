//! World migration between instances: move or copy a world from one
//! instance's `saves/` into another's, re-link its datapacks to the target
//! library, and carry its backups along on a move (spec
//! `2026-08-16-world-migration-design.md`).
//!
//! One phase per file: `plan` (read-only compatibility verdicts), `stage`
//! (claim a hidden stage; rename, or copy with verification), `relink`
//! (datapacks, never fatal), `finalise` (the one rename to the final name —
//! the point of no return). This file holds the IPC types, the path bundle,
//! the injected seams and the orchestrator.
//!
//! This module owns NO write primitive: bytes reach the target only through
//! `worlds::import::copy_tree`, `mods::store::materialize` and
//! `datapacks::library::install_named_at`; everything else here is
//! `create_dir`, `rename`, `remove_dir`/`remove_dir_all` and reads.

mod finalise;
mod plan;
mod relink;
mod stage;

pub(crate) use finalise::{finalise_at, remove_source_after_copy_at};
pub use plan::plan_migration_at;
pub(crate) use relink::relink_datapacks_at;
pub(crate) use stage::stage_world_at;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::DatapackRejection;
use crate::instances::schema::LoaderKind;

/// D1: copy (default — the original survives) or move.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MigrationMode {
    Copy,
    Move,
}

/// Which §4 path actually ran: one same-volume rename, or a verified copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MigrationPath {
    Renamed,
    Copied,
}

/// Why a version verdict could not be reached (§6). Typed so the dialog
/// renders each as its own honest sentence — never a raw string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum UnknownReason {
    NoLevelDat,
    Unreadable,
    NotRecorded,
    TargetVersionUnset,
    TargetNotInstalled,
    /// The target jar is installed but records no DataVersion: no
    /// `version.json` (clients before 1.14) or no integer `world_version`.
    TargetNotRecorded,
}

/// Ordered by DataVersion (integer), never by version name (A6).
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VersionVerdict {
    Same,
    WillUpgrade,
    WorldIsNewer,
    Unknown { reason: UnknownReason },
}

/// Why a world datapack stayed a plain copy instead of a library link (§5).
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LeftReason {
    NameHeldByDifferentPack,
    NotADatapack { reason: DatapackRejection },
    TooLarge,
    LinkFailed,
    Unreadable,
    Io,
}

/// Per-`.zip` result of the datapack step. `CopiedNotLinked` is
/// `materialize`'s documented fallback on a filesystem that cannot hardlink —
/// the toast must not call a copy "linked".
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DatapackResult {
    Linked,
    CopiedNotLinked,
    Adopted,
    LeftAsCopy { reason: LeftReason },
}

/// Plan-time prediction for one `.zip`: `Linked`, `Adopted`, or `LeftAsCopy`
/// — `NameHeldByDifferentPack` for different bytes, `Io` when the library
/// name could not be checked, `Unreadable` when a side could not be read.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct DatapackPlan {
    pub filename: String,
    pub predicted: DatapackResult,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct DatapackMigration {
    pub filename: String,
    pub result: DatapackResult,
}

/// What is left in the source after the point of no return (A4). `reason` is
/// the io error text for Logs / the details panel, never shown raw as the
/// toast sentence.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceState {
    Untouched,
    Removed,
    LeftIntact { reason: String },
    LeftPartial { reason: String },
}

/// The read-only plan for one world → one target (A13: size and backup count
/// come from the `World` row the dialog was opened from, not from here).
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct MigrationPlan {
    /// `Data.Version.Name` — display only, never compared.
    pub world_version_name: Option<String>,
    pub verdict: VersionVerdict,
    pub source_loader: LoaderKind,
    pub target_loader: LoaderKind,
    /// A7: source mods absent from the target, by `project_id` else `sha1`.
    pub mods_missing_in_target: u32,
    pub datapacks: Vec<DatapackPlan>,
    /// Folder packs: copied as they are, never adopted or linked.
    pub datapacks_folders: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MigrationPhase {
    Moving,
    Copying,
    Linking,
    Backups,
    Finalising,
}

/// `f64`, not `u64`: specta-typescript rejects `u64` (A10).
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct MigrationProgress {
    pub phase: MigrationPhase,
    pub current: f64,
    pub total: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct MigrationOutcome {
    /// Suffixed if the name was taken in the target.
    pub final_folder_name: String,
    pub path: MigrationPath,
    pub datapacks: Vec<DatapackMigration>,
    pub datapacks_folders_copied: u32,
    /// Copy path only: symlinks `copy_tree` did not follow.
    pub links_skipped: u32,
    pub source_state: SourceState,
    pub backups_moved: u32,
    /// Still in the source's `backups/<world>/`, where the orphan UI shows them.
    pub backups_left: u32,
}

/// Every path the core needs, resolved by the command layer — the repo's
/// no-`AppHandle` test boundary. `src_root` / `dst_root` are the instance
/// roots `datapacks::library` / `registry` and `mods::installed` take
/// (`paths::instance_dir`); `*_saves` = `<root>/.minecraft/saves`,
/// `*_backups_root` = `<root>/backups`. `target_instance_name` is the display
/// NAME, for `WorldMigratePartialLeft`.
#[derive(Debug, Clone)]
pub struct MigrationLocations {
    pub src_saves: PathBuf,
    pub src_backups_root: PathBuf,
    pub src_root: PathBuf,
    pub dst_saves: PathBuf,
    pub dst_backups_root: PathBuf,
    pub dst_root: PathBuf,
    pub world_folder: String,
    pub target_instance_name: String,
}

/// Injected `rename` / `remove` so every rollback branch is deterministic in
/// a plain `#[test]` on every platform — the `restore::swap_in_place` shape.
/// `Arc` because the orchestrator hands them across `spawn_blocking`.
#[derive(Clone)]
pub struct MigrationSeams {
    pub rename: Arc<dyn Fn(&Path, &Path) -> std::io::Result<()> + Send + Sync>,
    pub remove: Arc<dyn Fn(&Path) -> std::io::Result<()> + Send + Sync>,
}

impl MigrationSeams {
    /// Production seams: `std::fs::rename` and `std::fs::remove_dir_all`.
    pub fn real() -> Self {
        Self {
            rename: Arc::new(|from: &Path, to: &Path| std::fs::rename(from, to)),
            remove: Arc::new(|path: &Path| std::fs::remove_dir_all(path)),
        }
    }
}

/// The world sitting in its hidden stage, ready for relink + finalise.
#[derive(Debug)]
pub(crate) struct Staged {
    pub stage: PathBuf,
    pub path: MigrationPath,
    pub links_skipped: u32,
}

/// Result of the datapack step over the stage.
#[derive(Debug)]
pub(crate) struct Relinked {
    pub datapacks: Vec<DatapackMigration>,
    pub folders_copied: u32,
}

/// Run one migration end to end. Everything before `finalise_at`'s rename is
/// reversible and reported as an error; everything after it is reported in
/// the outcome.
///
/// Order: stage (`spawn_blocking`) → re-link datapacks (async) → finalise
/// (`spawn_blocking`) → **point of no return** → [Move that had to copy:
/// remove the source] → [Move whose source is gone: move the backup set] →
/// outcome.
///
/// **Point of no return.** `finalise_at` returning `Ok` means the world is
/// complete in the target under its final name. From there nothing rolls it
/// back: a source that could not be removed, a backup that could not be moved,
/// even a panic inside one of those tasks, becomes `source_state` /
/// `backups_left` — never an error whose copy would invite a retry into
/// `World (2)`. Backups follow the world only when its source is actually gone
/// (`Removed`): a source kept as `LeftIntact` / `LeftPartial` keeps its
/// backups, so the toast's "still in <source>" is true of both.
///
/// **No cancellation.** The task registry cancels queued tasks only and the
/// backend has no cancellation token, so a running migration cannot be
/// stopped; the UI offers no button (`caps.cancellable: false`). The only
/// interruption is process death.
///
/// **Stranded stages.** Process death leaves at most one stage in the target's
/// `saves/`. `.tmp-migrate-moved-<world>-<n>` holds the user's only copy:
/// `orphans::stranded_worlds_at` lists it as an interrupted move and
/// `recover_stranded_at` renames it back under the world's name.
/// `.tmp-migrate-copy-<world>-<n>` is a partial copy whose source is intact:
/// hidden from every listing and never offered for recovery (spec §4.1, §10).
/// A failed rollback before the point of no return reports the same stage by
/// name in `WorldMigratePartialLeft`, with `only_copy` telling the UI which of
/// the two it is.
///
/// Progress: `Moving` once at the start of a Move (the rename has no bytes to
/// count); `Copying` with bytes from the stage's `on_bytes` on the copy path;
/// `Linking` before and after re-linking (the re-link API has no per-entry
/// hook); `Finalising`; `Backups` when a backup set is moved.
pub async fn migrate_world_at(
    loc: MigrationLocations,
    mode: MigrationMode,
    progress: std::sync::Arc<dyn Fn(MigrationProgress) + Send + Sync>,
    seams: MigrationSeams,
) -> crate::error::Result<MigrationOutcome> {
    let is_move = matches!(mode, MigrationMode::Move);
    if is_move {
        progress(phase(MigrationPhase::Moving));
    }

    let staged = {
        let loc_task = loc.clone();
        let seams_task = seams.clone();
        let progress_task = progress.clone();
        tokio::task::spawn_blocking(move || {
            let mut on_bytes = |copied: u64, total: u64| {
                progress_task(MigrationProgress {
                    phase: MigrationPhase::Copying,
                    current: copied as f64,
                    total: total as f64,
                });
            };
            stage_world_at(&loc_task, mode, &mut on_bytes, &seams_task)
        })
        .await
        .map_err(join_error)??
    };

    progress(phase(MigrationPhase::Linking));
    let relinked = match relink_datapacks_at(
        &staged.stage,
        &loc.src_root,
        &loc.dst_root,
        staged.path.clone(),
    )
    .await
    {
        Ok(relinked) => relinked,
        // Still before the point of no return: the stage goes back where it
        // came from (or away), exactly as a finalise failure would.
        Err(cause) => {
            let loc_task = loc.clone();
            let seams_task = seams.clone();
            let err = tokio::task::spawn_blocking(move || {
                finalise::roll_back_stage(&loc_task, &staged, &seams_task, cause)
            })
            .await
            .map_err(join_error)?;
            return Err(err);
        }
    };
    let linked = relinked.datapacks.len() as f64;
    progress(MigrationProgress {
        phase: MigrationPhase::Linking,
        current: linked,
        total: linked,
    });

    progress(phase(MigrationPhase::Finalising));
    let links_skipped = staged.links_skipped;
    let path = staged.path.clone();
    let final_name = {
        let loc_task = loc.clone();
        let seams_task = seams.clone();
        tokio::task::spawn_blocking(move || finalise_at(&loc_task, &staged, &seams_task))
            .await
            .map_err(join_error)??
    };
    // ---- point of no return: the world is complete in the target ----

    let source_state = match (is_move, &path) {
        (false, _) => SourceState::Untouched,
        (true, MigrationPath::Renamed) => SourceState::Removed,
        (true, MigrationPath::Copied) => {
            let loc_task = loc.clone();
            let seams_task = seams.clone();
            let final_task = final_name.clone();
            match tokio::task::spawn_blocking(move || {
                remove_source_after_copy_at(&loc_task, &final_task, &seams_task)
            })
            .await
            {
                Ok(state) => state,
                // A panic in the removal task is an outcome too; the tree's
                // state is unknown, so the pessimistic one.
                Err(e) => {
                    crate::diag!("world migrate: source removal task failed: {e}");
                    SourceState::LeftPartial {
                        reason: format!("join: {e}"),
                    }
                }
            }
        }
    };

    let (backups_moved, backups_left) = if !is_move {
        // D4: a copy leaves the backups with the original.
        (0, 0)
    } else if matches!(source_state, SourceState::Removed) {
        progress(phase(MigrationPhase::Backups));
        let loc_task = loc.clone();
        let final_task = final_name.clone();
        match tokio::task::spawn_blocking(move || move_backups(&loc_task, &final_task)).await {
            Ok(counts) => counts,
            Err(e) => {
                crate::diag!("world migrate: backup move task failed: {e}");
                (0, count_or_zero(&loc.src_backups_root, &loc.world_folder))
            }
        }
    } else {
        // The source world is still there; its backups stay with it.
        (0, count_or_zero(&loc.src_backups_root, &loc.world_folder))
    };

    Ok(MigrationOutcome {
        final_folder_name: final_name,
        path,
        datapacks: relinked.datapacks,
        datapacks_folders_copied: relinked.folders_copied,
        links_skipped,
        source_state,
        backups_moved,
        backups_left,
    })
}

fn phase(phase: MigrationPhase) -> MigrationProgress {
    MigrationProgress {
        phase,
        current: 0.0,
        total: 0.0,
    }
}

fn join_error(e: tokio::task::JoinError) -> crate::error::Error {
    crate::error::Error::io("<migrate>", format!("join: {e}"))
}

/// Move the source world's backup set to the target. Runs after the point of
/// no return, so nothing here is ever an error: the world is complete in the
/// target and a backup that could not follow it is reported as `backups_left`,
/// where "Backups without a world" already shows it. Returns
/// `(moved, left)`.
///
/// `move_set_at` fails only before any rename (read_dir) or after every rename
/// succeeded (the emptied directory could not be removed); the counts taken
/// before and after tell which happened. A pre-existing (orphaned) target set
/// is subtracted so it is never reported as moved.
fn move_backups(loc: &MigrationLocations, final_name: &str) -> (u32, u32) {
    let src_dir = loc.src_backups_root.join(&loc.world_folder);
    let dst_dir = loc.dst_backups_root.join(final_name);
    let src_before = count_or_zero(&loc.src_backups_root, &loc.world_folder);
    let dst_before = count_or_zero(&loc.dst_backups_root, final_name);
    match crate::worlds::backup::move_set_at(&src_dir, &dst_dir) {
        Ok(report) => (report.moved, report.left),
        Err(e) => {
            crate::diag!(
                "world migrate: backups {} -> {}: {e}; reporting what is on disk",
                src_dir.display(),
                dst_dir.display()
            );
            let moved = count_or_zero(&loc.dst_backups_root, final_name).saturating_sub(dst_before);
            let left = match crate::worlds::count_backups(&loc.src_backups_root, &loc.world_folder)
            {
                Ok(n) => n,
                // Could not tell ⇒ every zip that did not visibly arrive is
                // reported as still here — the pessimistic figure; the orphan
                // panel remains the authoritative view of the directory.
                Err(err) => {
                    crate::diag!(
                        "world migrate: could not recount {}: {err}",
                        src_dir.display()
                    );
                    src_before.saturating_sub(moved)
                }
            };
            (moved, left)
        }
    }
}

/// `count_backups`, with "could not count" reported as 0 and logged. This is
/// the restrictive direction for `moved` (never claim a move that cannot be
/// seen); for `left` the caller keeps the figure it counted before the move.
fn count_or_zero(root: &std::path::Path, world: &str) -> u32 {
    match crate::worlds::count_backups(root, world) {
        Ok(n) => n,
        Err(e) => {
            crate::diag!(
                "world migrate: could not count backups under {}/{world}: {e}",
                root.display()
            );
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn data_carrying_enums_are_kind_tagged_and_snake_cased() {
        assert_eq!(
            serde_json::to_value(VersionVerdict::Unknown {
                reason: UnknownReason::NoLevelDat,
            })
            .unwrap(),
            json!({"kind": "unknown", "reason": "no_level_dat"})
        );
        assert_eq!(
            serde_json::to_value(DatapackResult::LeftAsCopy {
                reason: LeftReason::NotADatapack {
                    reason: DatapackRejection::NotAPack,
                },
            })
            .unwrap(),
            json!({"kind": "left_as_copy", "reason": {"kind": "not_a_datapack", "reason": "not_a_pack"}})
        );
        assert_eq!(
            serde_json::to_value(SourceState::LeftIntact {
                reason: "EBUSY".into(),
            })
            .unwrap(),
            json!({"kind": "left_intact", "reason": "EBUSY"})
        );
    }

    #[test]
    fn unit_enums_are_bare_snake_case_and_mode_round_trips() {
        assert_eq!(
            serde_json::to_value(MigrationPath::Renamed).unwrap(),
            json!("renamed")
        );
        assert_eq!(
            serde_json::to_value(MigrationPhase::Finalising).unwrap(),
            json!("finalising")
        );
        let mode: MigrationMode = serde_json::from_value(json!("move")).unwrap();
        assert_eq!(mode, MigrationMode::Move);
    }

    #[test]
    fn real_seams_rename_and_remove_on_disk() {
        let td = tempfile::tempdir().unwrap();
        let a = td.path().join("a");
        std::fs::create_dir(&a).unwrap();
        let b = td.path().join("b");
        let seams = MigrationSeams::real();
        (seams.rename)(&a, &b).unwrap();
        assert!(b.is_dir());
        assert!(!a.exists());
        (seams.remove)(&b).unwrap();
        assert!(!b.exists());
    }
}

#[cfg(test)]
mod orchestrator_tests {
    use super::finalise::fixture::*;
    use super::*;
    use crate::error::Error;
    use crate::worlds::orphans::{recover_stranded_at, stranded_worlds_at, StrandedKind};
    use std::fs;
    use std::io;
    use std::path::Path;
    use std::sync::Arc;

    fn quiet() -> Arc<dyn Fn(MigrationProgress) + Send + Sync> {
        Arc::new(|_| {})
    }

    fn cross_device() -> io::Error {
        // EXDEV on unix, ERROR_NOT_SAME_DEVICE on Windows — what a rename across
        // a user-made junction returns.
        io::Error::from_raw_os_error(if cfg!(windows) { 17 } else { 18 })
    }

    /// A rename seam that refuses the FINAL rename (into `dst_saves/<name>`
    /// for any non-stage name) and, optionally, the rename back into
    /// `refuse_back_to`.
    fn refusing_seams(dst_saves: &Path, refuse_back_to: Option<&Path>) -> MigrationSeams {
        let dst_saves = dst_saves.to_path_buf();
        let refuse_back_to = refuse_back_to.map(Path::to_path_buf);
        seams(
            move |from, to| {
                let is_stage = to
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with(".tmp-migrate-"))
                    .unwrap_or(false);
                let is_final = to.parent() == Some(dst_saves.as_path()) && !is_stage;
                let is_back = refuse_back_to.as_deref() == Some(to);
                if is_final || is_back {
                    return Err(io::Error::new(io::ErrorKind::PermissionDenied, "injected"));
                }
                fs::rename(from, to)
            },
            |p| fs::remove_dir_all(p),
        )
    }

    // §9.1
    #[tokio::test]
    async fn copy_places_the_world_and_leaves_the_source_untouched() {
        let fx = two_instances();
        add_backup(
            &fx.loc.src_backups_root,
            "W",
            "2026-01-01T00-00-00.zip",
            b"zip",
        );

        let out = migrate_world_at(
            fx.loc.clone(),
            MigrationMode::Copy,
            quiet(),
            MigrationSeams::real(),
        )
        .await
        .unwrap();

        assert_eq!(out.final_folder_name, "W");
        assert_eq!(out.path, MigrationPath::Copied);
        assert_eq!(out.source_state, SourceState::Untouched);
        assert_eq!(marker_of(&fx.loc.dst_saves.join("W")), b"original");
        assert_eq!(marker_of(&fx.loc.src_saves.join("W")), b"original");
        assert!(
            fx.loc
                .src_backups_root
                .join("W")
                .join("2026-01-01T00-00-00.zip")
                .is_file(),
            "D4: copy leaves the backups with the original"
        );
        assert_eq!((out.backups_moved, out.backups_left), (0, 0));
        assert!(
            tmp_dirs(&fx.loc.dst_saves).is_empty(),
            "no stage survives a successful copy"
        );
    }

    // §9.2
    #[tokio::test]
    async fn move_renames_the_world_and_takes_its_backups() {
        let fx = two_instances();
        add_backup(
            &fx.loc.src_backups_root,
            "W",
            "2026-01-01T00-00-00.zip",
            b"zip",
        );

        let out = migrate_world_at(
            fx.loc.clone(),
            MigrationMode::Move,
            quiet(),
            MigrationSeams::real(),
        )
        .await
        .unwrap();

        assert_eq!(out.path, MigrationPath::Renamed);
        assert_eq!(out.source_state, SourceState::Removed);
        assert_eq!(marker_of(&fx.loc.dst_saves.join("W")), b"original");
        assert!(!fx.loc.src_saves.join("W").try_exists().unwrap());
        assert!(
            !fx.loc.src_backups_root.join("W").try_exists().unwrap(),
            "the emptied source backup dir is removed"
        );
        assert_eq!(
            fs::read(
                fx.loc
                    .dst_backups_root
                    .join("W")
                    .join("2026-01-01T00-00-00.zip")
            )
            .unwrap(),
            b"zip"
        );
        assert_eq!((out.backups_moved, out.backups_left), (1, 0));
        assert!(tmp_dirs(&fx.loc.dst_saves).is_empty());
    }

    // §9.4 — the pre-existing world's content is compared through its marker
    // file; `level.dat` is left in place too.
    #[tokio::test]
    async fn a_taken_name_is_suffixed_and_the_existing_world_is_untouched() {
        let fx = two_instances();
        make_world(&fx.loc.dst_saves.join("W"), b"theirs");

        let out = migrate_world_at(
            fx.loc.clone(),
            MigrationMode::Copy,
            quiet(),
            MigrationSeams::real(),
        )
        .await
        .unwrap();

        assert_eq!(out.final_folder_name, "W (2)");
        assert_eq!(marker_of(&fx.loc.dst_saves.join("W")), b"theirs");
        assert_eq!(
            fs::read(fx.loc.dst_saves.join("W").join("level.dat")).unwrap(),
            b"level"
        );
        assert_eq!(marker_of(&fx.loc.dst_saves.join("W (2)")), b"original");
    }

    // §9.11 — an orphaned target set of the same name is merged, never overwritten.
    #[tokio::test]
    async fn moved_backups_merge_into_an_orphaned_set_without_overwriting() {
        let fx = two_instances();
        add_backup(
            &fx.loc.src_backups_root,
            "W",
            "2026-01-01T00-00-00.zip",
            b"mine",
        );
        add_backup(
            &fx.loc.dst_backups_root,
            "W",
            "2026-01-01T00-00-00.zip",
            b"orphan",
        );

        let out = migrate_world_at(
            fx.loc.clone(),
            MigrationMode::Move,
            quiet(),
            MigrationSeams::real(),
        )
        .await
        .unwrap();

        let dst = fx.loc.dst_backups_root.join("W");
        assert_eq!(
            fs::read(dst.join("2026-01-01T00-00-00.zip")).unwrap(),
            b"orphan"
        );
        assert_eq!(
            fs::read(dst.join("2026-01-01T00-00-00.2.zip")).unwrap(),
            b"mine"
        );
        assert_eq!((out.backups_moved, out.backups_left), (1, 0));
    }

    // §9.11 — a Move into a taken name files the backups under the suffixed name.
    #[tokio::test]
    async fn a_move_into_a_taken_name_files_backups_under_the_suffixed_name() {
        let fx = two_instances();
        make_world(&fx.loc.dst_saves.join("W"), b"theirs");
        add_backup(
            &fx.loc.src_backups_root,
            "W",
            "2026-01-01T00-00-00.zip",
            b"mine",
        );

        let out = migrate_world_at(
            fx.loc.clone(),
            MigrationMode::Move,
            quiet(),
            MigrationSeams::real(),
        )
        .await
        .unwrap();

        assert_eq!(out.final_folder_name, "W (2)");
        assert!(fx
            .loc
            .dst_backups_root
            .join("W (2)")
            .join("2026-01-01T00-00-00.zip")
            .is_file());
        assert!(!fx.loc.dst_backups_root.join("W").try_exists().unwrap());
        assert_eq!(marker_of(&fx.loc.dst_saves.join("W")), b"theirs");
        assert_eq!((out.backups_moved, out.backups_left), (1, 0));
    }

    // §9.13 — through B2's injectable copy: the stage cannot be removed either.
    #[test]
    fn a_copy_failure_whose_rollback_fails_names_the_stage() {
        let fx = two_instances();
        let seams = seams(
            |from, to| fs::rename(from, to),
            |_| {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "injected: stage cannot be removed",
                ))
            },
        );

        let r = super::stage::stage_world_at_with(
            &fx.loc,
            MigrationMode::Copy,
            &mut |_, _| {},
            &seams,
            &|_: &Path, _: &Path, _: &mut dyn FnMut(u64)| -> crate::error::Result<()> {
                Err(Error::io("<test>", "injected copy failure"))
            },
        );

        match r {
            Err(Error::WorldMigratePartialLeft {
                folder_name,
                target_instance,
                only_copy,
            }) => {
                assert!(
                    folder_name.starts_with(".tmp-migrate-copy-W-"),
                    "{folder_name}"
                );
                assert_eq!(target_instance, "Target");
                assert!(!only_copy, "the source is intact on the copy path");
                assert!(
                    fx.loc.dst_saves.join(&folder_name).is_dir(),
                    "the stage really was left behind"
                );
            }
            other => panic!("expected WorldMigratePartialLeft, got {other:?}"),
        }
        assert_eq!(marker_of(&fx.loc.src_saves.join("W")), b"original");
    }

    // §9.14 — the final rename fails on the rename path: the world goes back.
    #[tokio::test]
    async fn a_final_rename_failure_on_the_rename_path_puts_the_world_back() {
        let fx = two_instances();
        let seams = refusing_seams(&fx.loc.dst_saves, None);

        let r = migrate_world_at(fx.loc.clone(), MigrationMode::Move, quiet(), seams).await;

        assert!(matches!(r, Err(Error::Io { .. })), "got {r:?}");
        assert_eq!(marker_of(&fx.loc.src_saves.join("W")), b"original");
        assert!(!fx.loc.dst_saves.join("W").try_exists().unwrap());
        assert!(
            tmp_dirs(&fx.loc.dst_saves).is_empty(),
            "the stage was renamed back, not left"
        );
    }

    // §9.14 + §9.20 — the rename-back fails too: the stage is named, listed as
    // an interrupted move, and recoverable.
    #[tokio::test]
    async fn a_failed_rename_back_leaves_a_stage_the_orphans_module_can_recover() {
        let fx = two_instances();
        let source_slot = fx.loc.src_saves.join("W");
        let seams = refusing_seams(&fx.loc.dst_saves, Some(&source_slot));

        let r = migrate_world_at(fx.loc.clone(), MigrationMode::Move, quiet(), seams).await;

        let stage_name = match r {
            Err(Error::WorldMigratePartialLeft {
                folder_name,
                target_instance,
                only_copy,
            }) => {
                assert_eq!(target_instance, "Target");
                assert!(only_copy, "the stage holds the user's only copy");
                folder_name
            }
            other => panic!("expected WorldMigratePartialLeft, got {other:?}"),
        };
        assert!(
            stage_name.starts_with(".tmp-migrate-moved-W-"),
            "{stage_name}"
        );
        assert!(
            !source_slot.try_exists().unwrap(),
            "the source slot is empty: the stage IS the world"
        );
        let stranded = stranded_worlds_at(&fx.loc.dst_saves);
        assert_eq!(stranded.len(), 1, "{stranded:?}");
        assert_eq!(stranded[0].dir_name, stage_name);
        assert_eq!(stranded[0].world_folder, "W");
        assert!(matches!(stranded[0].kind, StrandedKind::Migration));
        assert!(!stranded[0].target_occupied);
        assert_eq!(
            recover_stranded_at(&fx.loc.dst_saves, &stage_name).unwrap(),
            "W"
        );
        assert_eq!(marker_of(&fx.loc.dst_saves.join("W")), b"original");
    }

    // §9.15 — copy fallback (EXDEV), then the source cannot be removed: Ok,
    // `LeftIntact`, never `WorldInUse`; the backups stay with the source.
    #[tokio::test]
    async fn a_source_removal_failure_after_the_copy_fallback_is_an_outcome() {
        let fx = two_instances();
        add_backup(
            &fx.loc.src_backups_root,
            "W",
            "2026-01-01T00-00-00.zip",
            b"zip",
        );
        let source = fx.loc.src_saves.join("W");
        let seams = {
            let src_for_rename = source.clone();
            let src_for_remove = source.clone();
            seams(
                move |from, to| {
                    if from == src_for_rename.as_path() {
                        return Err(cross_device());
                    }
                    fs::rename(from, to)
                },
                move |p| {
                    if p == src_for_remove.as_path() {
                        return Err(io::Error::new(
                            io::ErrorKind::PermissionDenied,
                            "injected: region file held open",
                        ));
                    }
                    fs::remove_dir_all(p)
                },
            )
        };

        let out = migrate_world_at(fx.loc.clone(), MigrationMode::Move, quiet(), seams)
            .await
            .unwrap();

        assert_eq!(out.path, MigrationPath::Copied);
        assert!(
            matches!(out.source_state, SourceState::LeftIntact { .. }),
            "{:?}",
            out.source_state
        );
        assert_eq!(marker_of(&fx.loc.dst_saves.join("W")), b"original");
        assert_eq!(marker_of(&source), b"original");
        assert!(
            fx.loc
                .src_backups_root
                .join("W")
                .join("2026-01-01T00-00-00.zip")
                .is_file(),
            "backups stay with a source that is still there"
        );
        assert_eq!((out.backups_moved, out.backups_left), (0, 1));
        assert!(tmp_dirs(&fx.loc.dst_saves).is_empty());
    }

    // Progress: a Move announces the rename phase, then finalises.
    #[tokio::test]
    async fn progress_reports_moving_then_finalising_on_the_rename_path() {
        let fx = two_instances();
        let seen: Arc<std::sync::Mutex<Vec<MigrationPhase>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = {
            let seen = seen.clone();
            Arc::new(move |p: MigrationProgress| seen.lock().unwrap().push(p.phase))
        };

        migrate_world_at(
            fx.loc.clone(),
            MigrationMode::Move,
            sink,
            MigrationSeams::real(),
        )
        .await
        .unwrap();

        let phases = seen.lock().unwrap().clone();
        assert_eq!(phases.first().cloned(), Some(MigrationPhase::Moving));
        assert!(phases.contains(&MigrationPhase::Finalising), "{phases:?}");
        assert!(phases.contains(&MigrationPhase::Backups), "{phases:?}");
    }
}
