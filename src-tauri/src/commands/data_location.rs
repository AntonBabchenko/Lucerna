//! Get / plan / change the data-root location.
//!
//! The move itself is `data_root::relocate` — a pipeline with no `AppHandle`.
//! This file is the adapter: it validates at the boundary, builds the hooks
//! that need the app (progress events, the redirect, the startup resolver,
//! the running check), and maps the outcome onto the process-wide
//! `RelocationState` and the IPC contract.

use crate::data_root::blockers::{self, RestartBlock};
use crate::data_root::redirect::{self, Redirect};
use crate::data_root::relocate::{self, Hooks, Io, NotSwitched, Outcome, Phase};
use crate::data_root::state::{self, MovePhase, RelocationStatus};
use crate::data_root::transient::{self, SAFE_OVERLAP, WEBVIEW_DIR};
use crate::data_root::validate::Invalid;
use crate::data_root::{cleanup_note, looks_like_data_root, migrate, startup};
use crate::error::{Error, Result};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tauri_specta::Event;

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct DataLocationStatus {
    pub effective: String,
    pub configured: Option<String>,
    pub fell_back: bool,
    /// The OS-default data folder — where "Reset to default" moves the data.
    pub default_dir: String,
    /// Survives a page reload: the UI re-shows the move dialog from this.
    pub relocation: RelocationStatus,
}

/// Streamed progress for a data-root relocation.
#[derive(Debug, Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct DataMigrationProgress {
    pub copied_bytes: f64,
    pub total_bytes: f64,
    pub phase: MovePhase,
}

/// What `set_data_location` returns when it returns at all — a clean move
/// restarts the app instead.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DataMoveOutcome {
    Cancelled,
    /// Committed; details are in `DataLocationStatus::relocation`.
    RestartRequired,
}

/// A classified data-location change for a user-picked directory.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DataLocationPlan {
    /// `path` is an existing Lucerna data root — offer to point at it.
    Adopt { path: String },
    /// `path` is the effective migration target (the `LucernaData` subfolder
    /// applied exactly once). `required_bytes` is an estimate of what the copy
    /// needs; `free_bytes` is `None` when it could not be checked.
    Migrate {
        path: String,
        required_bytes: f64,
        free_bytes: Option<f64>,
    },
    /// The pick resolves to the current effective root — nothing to change.
    AlreadyCurrent { path: String },
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct DataResetPlan {
    /// The OS-default folder the data would move back to.
    pub path: String,
    /// While fallen back: only the redirect is removed, nothing is copied.
    pub pointer_only: bool,
    pub required_bytes: f64,
    pub free_bytes: Option<f64>,
    /// Top-level names in `path` that make a reset impossible.
    pub blocking_entries: Vec<String>,
}

fn invalid(reason: Invalid) -> Error {
    Error::DataLocationInvalid {
        reason: reason.reason_key().to_string(),
    }
}

fn task_failed(what: &'static str, e: tokio::task::JoinError) -> Error {
    Error::io(what, format!("task panicked: {e}"))
}

/// Refuse unless nothing is running, starting or claimed — and that could be checked.
fn refuse_if_blocked(app: &AppHandle) -> Result<()> {
    match blockers::observe(app) {
        RestartBlock::None => Ok(()),
        RestartBlock::Running | RestartBlock::Busy | RestartBlock::Unknown => {
            Err(Error::DataLocationBusy)
        }
    }
}

/// Links refused, size measured — both skipping what the running launcher owns.
fn measure(current: &Path) -> Result<u64> {
    if migrate::contains_link_strict(current, &transient::is_skipped_top_level)? {
        return Err(invalid(Invalid::ContainsLinks));
    }
    migrate::dir_size_strict(current, &transient::is_skipped_top_level)
}

fn wire_phase(phase: Phase) -> MovePhase {
    match phase {
        Phase::Copying => MovePhase::Copying,
        Phase::Verifying => MovePhase::Verifying,
        Phase::Switching => MovePhase::Switching,
        Phase::Deleting => MovePhase::Deleting,
    }
}

/// Would the NEXT start resolve to `target`, and is `target` an available,
/// complete root right now? Anything but a confident yes is a no.
fn lands_on_target(default: &Path, redirect_file: &Path, target: &Path) -> bool {
    let next = startup::resolve_at_startup(&startup::StartupInputs {
        default_root: default.to_path_buf(),
        redirect_file: Some(redirect_file.to_path_buf()),
        exe_dir: std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf)),
        portable_allowed: startup::portable_allowed(),
    });
    !next.fell_back
        && migrate::is_same_path(&next.root, target)
        && migrate::is_available(target)
        && looks_like_data_root(target)
}

/// Top-level entries still in `root` that the launcher does not own.
fn remaining_entries(root: &Path) -> Vec<String> {
    match migrate::blocking_entries(root, &transient::SKIPPED_TOP_LEVEL) {
        Ok(names) => names,
        Err(e) => {
            crate::diag!("[data-move] cannot list what is left in the old root: {e}");
            Vec::new()
        }
    }
}

/// Current effective data-root location, its configured (possibly
/// unavailable) target, and the state of a move. Deliberately cheap — plain
/// reads of in-process state: this command is sync (main thread) and runs on
/// the startup path, so it must never touch the filesystem tree.
#[tauri::command]
#[specta::specta]
pub fn get_data_location(app: AppHandle) -> Result<DataLocationStatus> {
    let st = app.state::<crate::data_root::DataRoot>();
    let default_dir =
        crate::paths::default_app_data_dir(&app).map_err(|e| Error::io("<default>", e))?;
    Ok(DataLocationStatus {
        effective: st.0.root.display().to_string(),
        configured: st.0.configured.as_ref().map(|p| p.display().to_string()),
        fell_back: st.0.fell_back,
        default_dir: default_dir.display().to_string(),
        relocation: state::global().status(),
    })
}

/// Total size in bytes of everything under the effective data root (display
/// estimate). Async + `spawn_blocking`: the walk takes seconds on a cold cache.
/// f64 not u64: specta forbids exporting BigInt-style types to TS.
#[tauri::command]
#[specta::specta]
pub async fn data_root_size_bytes(app: AppHandle) -> Result<f64> {
    let root = app.state::<crate::data_root::DataRoot>().0.root.clone();
    let size = tokio::task::spawn_blocking(move || migrate::dir_size(&root))
        .await
        .map_err(|e| task_failed("<data_root_size>", e))?;
    Ok(size as f64)
}

/// Classify a picked directory into adopt / migrate / already-current, and —
/// for a migration — refuse a tree with links up front and report the size it
/// needs and the space the target volume has. Read-only; commit-time
/// validation still happens in `set_data_location` / `adopt_data_location`.
#[tauri::command]
#[specta::specta]
pub async fn plan_data_location_change(app: AppHandle, picked: String) -> Result<DataLocationPlan> {
    let current = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    tokio::task::spawn_blocking(move || -> Result<DataLocationPlan> {
        use crate::data_root::plan::{plan_change, PlanKind};
        let plan = plan_change(&PathBuf::from(picked), &looks_like_data_root);
        let path = match &plan {
            PlanKind::Adopt(p) | PlanKind::Migrate(p) => p.clone(),
        };
        let shown = path.display().to_string();
        if migrate::is_same_path(&path, &current) {
            return Ok(DataLocationPlan::AlreadyCurrent { path: shown });
        }
        match plan {
            PlanKind::Adopt(_) => Ok(DataLocationPlan::Adopt { path: shown }),
            PlanKind::Migrate(_) => Ok(DataLocationPlan::Migrate {
                path: shown,
                required_bytes: measure(&current)? as f64,
                free_bytes: crate::platform::free_disk_bytes_nearest(&path).map(|b| b as f64),
            }),
        }
    })
    .await
    .map_err(|e| task_failed("<plan_data_location>", e))?
}

/// What "Reset to default" would do. While fallen back it is pointer-only.
#[tauri::command]
#[specta::specta]
pub async fn plan_data_location_reset(app: AppHandle) -> Result<DataResetPlan> {
    let root = app.state::<crate::data_root::DataRoot>().0.clone();
    let default =
        crate::paths::default_app_data_dir(&app).map_err(|e| Error::io("<default>", e))?;
    let shown = default.display().to_string();
    if root.fell_back {
        return Ok(DataResetPlan {
            path: shown,
            pointer_only: true,
            required_bytes: 0.0,
            free_bytes: None,
            blocking_entries: Vec::new(),
        });
    }
    tokio::task::spawn_blocking(move || -> Result<DataResetPlan> {
        if migrate::is_same_path(&root.root, &default) {
            return Err(invalid(Invalid::SameAsCurrent));
        }
        let blocking_entries = migrate::blocking_entries(&default, &SAFE_OVERLAP)?;
        Ok(DataResetPlan {
            path: shown,
            pointer_only: false,
            required_bytes: measure(&root.root)? as f64,
            free_bytes: crate::platform::free_disk_bytes_nearest(&default).map(|b| b as f64),
            blocking_entries,
        })
    })
    .await
    .map_err(|e| task_failed("<plan_data_location_reset>", e))?
}

/// Everything the blocking move task needs, owned.
struct MoveJob {
    app: AppHandle,
    current: PathBuf,
    target: PathBuf,
    default: PathBuf,
    redirect_file: PathBuf,
    /// The redirect exactly as it was — what a rollback restores.
    previous: Option<Redirect>,
    new_path: Option<String>,
    total_bytes: u64,
    /// Set inside the commit hook: a panic after this is NOT "data unchanged".
    committed: Arc<AtomicBool>,
    /// The pending-cleanup note could not be written: the old `webview/`
    /// will not be swept, so it is reported as a leftover.
    note_failed: Arc<AtomicBool>,
}

/// Relocate the data root to `new_path`, or back to the OS default when
/// `None`. See `data_root::relocate` for the pipeline and its guarantees.
///
/// A clean move restarts the app and never returns. It returns `Cancelled`
/// when the user cancelled while copying, and `RestartRequired` when the move
/// is committed but something of the old root could not be removed — this
/// process then still runs from the old root, and every create/launch command
/// refuses until the restart (`data_root::reject_if_root_unusable`).
///
/// Errors: `DataLocationBusy` (something runs, is claimed, or could not be
/// checked; a move is already in flight; fallen back and not a reset),
/// `DataLocationInvalid`, and `DataLocationMigrationFailed` — which always
/// means the launcher still runs from, and will restart into, the ORIGINAL
/// folder.
#[tauri::command]
#[specta::specta]
pub async fn set_data_location(
    app: AppHandle,
    new_path: Option<String>,
) -> Result<DataMoveOutcome> {
    // One move at a time. Dropping the session on any early return frees it.
    let session = state::global().begin().ok_or(Error::DataLocationBusy)?;

    let fell_back = app.state::<crate::data_root::DataRoot>().0.fell_back;
    let is_reset = new_path.is_none();
    let redirect_file =
        crate::paths::redirect_file(&app).map_err(|e| Error::io("<redirect>", e))?;
    match fallback_gate(fell_back, is_reset) {
        // Never MOVE the temporary fallback root: it is the wrong tree.
        FallbackGate::RejectBusy => return Err(Error::DataLocationBusy),
        // A RESET while fallen back is pointer-only: the launcher already runs
        // from the default dir, so removing the redirect makes that permanent.
        // It is the only in-app recovery from a folder that will never return.
        FallbackGate::PointerOnlyReset => {
            refuse_if_blocked(&app)?;
            redirect::remove(&redirect_file)?;
            session.park_running();
            app.restart();
        }
        FallbackGate::Normal => {}
    }
    refuse_if_blocked(&app)?;

    let current = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let default =
        crate::paths::default_app_data_dir(&app).map_err(|e| Error::io("<default>", e))?;
    let target = match &new_path {
        Some(p) => PathBuf::from(p),
        None => default.clone(),
    };
    // An unreadable redirect is a reason not to start: a rollback could not
    // restore it. (A corrupt one reads as "none", which is also how startup reads it.)
    let previous = redirect::read(&redirect_file)?;

    // Commit-time validation is filesystem work over the whole tree.
    let (current_v, target_v) = (current.clone(), target.clone());
    let total_bytes = tokio::task::spawn_blocking(move || -> Result<u64> {
        let empty = if is_reset {
            migrate::empty_or_only_safe(&target_v, &SAFE_OVERLAP)
        } else {
            migrate::target_is_empty_or_transient(&target_v)
        };
        crate::data_root::validate::validate_target(&current_v, &target_v, empty)
            .map_err(invalid)?;
        // `validate_target`'s nested check is lexical; this one is canonical
        // and case-folded — the guard that keeps the delete phase from ever
        // seeing a target inside the source. (Skipped for reset: the default
        // dir is by construction not nested in a relocated `current`.)
        if !is_reset && migrate::is_same_or_nested(&current_v, &target_v) {
            return Err(invalid(Invalid::NestedInCurrent));
        }
        measure(&current_v)
    })
    .await
    .map_err(|e| task_failed("<set_data_location>", e))??;

    let committed = Arc::new(AtomicBool::new(false));
    let note_failed = Arc::new(AtomicBool::new(false));
    let job = MoveJob {
        app: app.clone(),
        current: current.clone(),
        target: target.clone(),
        default,
        redirect_file,
        previous,
        new_path,
        total_bytes,
        committed: committed.clone(),
        note_failed: note_failed.clone(),
    };
    let joined = tokio::task::spawn_blocking(move || run_move(&job)).await;

    let old_root = current.display().to_string();
    let new_root = target.display().to_string();
    let with_unswept_webview = |mut entries: Vec<String>| {
        if note_failed.load(Ordering::SeqCst) && current.join(WEBVIEW_DIR).is_dir() {
            entries.push(WEBVIEW_DIR.to_string());
        }
        entries
    };
    match joined {
        Ok(Ok(Outcome::Moved)) => {
            let leftovers = with_unswept_webview(Vec::new());
            if leftovers.is_empty() {
                session.park_running();
                app.restart();
            }
            session.park_restart_required(old_root, new_root, leftovers, false);
            Ok(DataMoveOutcome::RestartRequired)
        }
        Ok(Ok(Outcome::MovedWithLeftovers {
            entries,
            old_root_intact,
        })) => {
            session.park_restart_required(
                old_root,
                new_root,
                with_unswept_webview(entries),
                old_root_intact,
            );
            Ok(DataMoveOutcome::RestartRequired)
        }
        // The session drops here → Idle.
        Ok(Ok(Outcome::Cancelled)) => Ok(DataMoveOutcome::Cancelled),
        Ok(Err(NotSwitched {
            phase,
            reason,
            partial_copy_left,
            restore_incomplete,
        })) => Err(Error::DataLocationMigrationFailed {
            reason: format!("{phase:?}: {reason}"),
            partial_copy_left: partial_copy_left.map(|p| p.display().to_string()),
            restore_incomplete,
        }),
        // The pointer was committed before the task died: this is NOT "data
        // unchanged". Report what is left and require the restart.
        Err(e) if committed.load(Ordering::SeqCst) => {
            crate::diag!("[data-move] the move task stopped after the switch: {e}");
            let leftovers = with_unswept_webview(remaining_entries(&current));
            session.park_restart_required(old_root, new_root, leftovers, false);
            Ok(DataMoveOutcome::RestartRequired)
        }
        Err(e) => Err(Error::DataLocationMigrationFailed {
            reason: format!("the move task stopped unexpectedly: {e}"),
            partial_copy_left: std::fs::symlink_metadata(&target)
                .is_ok()
                .then(|| target.display().to_string()),
            restore_incomplete: false,
        }),
    }
}

/// Ask the running move to stop. A request: it is honoured while files are
/// being copied or verified and ignored once the switch has started.
#[tauri::command]
#[specta::specta]
pub fn cancel_data_location_move() {
    state::global().request_cancel();
}

/// Try again to remove what a committed move left in the old root.
#[tauri::command]
#[specta::specta]
pub async fn retry_data_move_cleanup() -> Result<RelocationStatus> {
    let RelocationStatus::RestartRequired {
        old_root,
        new_root,
        leftovers,
        old_root_intact: false,
    } = state::global().status()
    else {
        return Ok(state::global().status());
    };
    let remaining = tokio::task::spawn_blocking(move || {
        // A launcher-owned name (an un-swept `webview`) is never removed while
        // this process runs; keep it listed rather than pretend it is gone.
        let (owned, removable): (Vec<String>, Vec<String>) = leftovers
            .into_iter()
            .partition(|name| transient::is_skipped_top_level(std::ffi::OsStr::new(name)));
        let mut still_there = relocate::retry_leftovers(
            Path::new(&old_root),
            Path::new(&new_root),
            &removable,
            &Io::real(),
        );
        still_there.extend(owned);
        still_there
    })
    .await
    .map_err(|e| task_failed("<retry_data_move_cleanup>", e))?;
    Ok(state::global().replace_leftovers(remaining))
}

/// Open the old root of the move this process committed. Takes no path: the
/// frontend never tells the backend what to open.
#[tauri::command]
#[specta::specta]
pub async fn open_data_move_leftovers(app: AppHandle) -> Result<()> {
    use tauri_plugin_opener::OpenerExt;
    let RelocationStatus::RestartRequired { old_root, .. } = state::global().status() else {
        return Err(Error::io(
            "<open_data_move_leftovers>",
            "no finished move in this session",
        ));
    };
    app.opener()
        .open_path(old_root.clone(), None::<&str>)
        .map_err(|e| Error::io(old_root, format!("opener: {e}")))
}

/// Point the data root at `path` — an EXISTING Lucerna data root — without
/// copying, verifying, or deleting anything, then restart. The current root's
/// data stays on disk untouched (including its `webview/`).
#[tauri::command]
#[specta::specta]
pub async fn adopt_data_location(app: AppHandle, path: String) -> Result<()> {
    let session = state::global().begin().ok_or(Error::DataLocationBusy)?;
    if app.state::<crate::data_root::DataRoot>().0.fell_back {
        return Err(Error::DataLocationBusy);
    }
    refuse_if_blocked(&app)?;

    let current = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let default =
        crate::paths::default_app_data_dir(&app).map_err(|e| Error::io("<default>", e))?;
    let target = PathBuf::from(path);

    let target_probe = target.clone();
    let adopting_default = tokio::task::spawn_blocking(move || {
        classify_adopt(
            &target_probe,
            &current,
            &default,
            &looks_like_data_root,
            &migrate::is_available,
            &|a, b| migrate::is_same_path(a, b),
        )
    })
    .await
    .map_err(|e| task_failed("<adopt_data_location>", e))?
    .map_err(invalid)?;

    let redirect_file =
        crate::paths::redirect_file(&app).map_err(|e| Error::io("<redirect>", e))?;
    if adopting_default {
        redirect::remove(&redirect_file)?;
    } else {
        redirect::write(&redirect_file, &Redirect { path: target })?;
    }
    session.park_running();
    app.restart();
}

/// What, if anything, stops a data-root change or a restart right now. The
/// frontend enables its buttons only on an exact `none`.
#[tauri::command]
#[specta::specta]
pub async fn restart_blocked(app: AppHandle) -> RestartBlock {
    blockers::observe(&app)
}

/// Restart the launcher process. After a committed move this is the way out
/// and skips the running check: nothing could have started (the gate refuses),
/// and scanning the emptied old root may only answer "unknown". While a move
/// runs it refuses; otherwise it refuses when anything runs, is claimed, or
/// could not be checked. On success this never returns.
#[tauri::command]
#[specta::specta]
pub async fn restart_launcher(app: AppHandle) -> Result<()> {
    match state::global().status() {
        RelocationStatus::RestartRequired { .. } => app.restart(),
        RelocationStatus::Running { .. } => {
            return Err(Error::DataRelocationInProgress {
                restart_required: false,
            })
        }
        RelocationStatus::Idle => {}
    }
    refuse_if_blocked(&app)?;
    app.restart();
}

/// The blocking half of `set_data_location`: builds the hooks and runs the pipeline.
fn run_move(job: &MoveJob) -> std::result::Result<Outcome, NotSwitched> {
    // A real root has tens of thousands of small files; one event per file
    // floods the IPC channel. Emit on every phase change, and at most once per
    // 16 MiB while copying.
    const EMIT_EVERY: u64 = 16 * 1024 * 1024;
    let mut last_phase: Option<Phase> = None;
    let mut last_emit = 0u64;
    let mut on_progress = |phase: Phase, copied: u64| {
        let phase_changed = last_phase != Some(phase);
        if !phase_changed && copied.saturating_sub(last_emit) < EMIT_EVERY {
            return;
        }
        last_phase = Some(phase);
        last_emit = copied;
        if phase_changed {
            state::global().set_phase(wire_phase(phase));
        }
        let _ = DataMigrationProgress {
            copied_bytes: copied as f64,
            total_bytes: job.total_bytes as f64,
            phase: wire_phase(phase),
        }
        .emit(&job.app);
    };
    let is_cancelled = || state::global().is_cancelled();
    let still_safe_to_switch = || blockers::observe(&job.app) == RestartBlock::None;
    let mut commit_pointer = || -> std::result::Result<(), String> {
        // The note first: a process killed during the delete phase must not
        // orphan the old profile for good. Not fatal — see `note_failed`.
        let old_webview = job.current.join(WEBVIEW_DIR);
        if old_webview.is_dir() {
            if let Err(e) = cleanup_note::append(&job.default, &old_webview) {
                crate::diag!("[data-move] pending-cleanup note not written: {e}");
                job.note_failed.store(true, Ordering::SeqCst);
            }
        }
        let written = match &job.new_path {
            Some(p) => redirect::write(
                &job.redirect_file,
                &Redirect {
                    path: PathBuf::from(p),
                },
            ),
            None => redirect::remove(&job.redirect_file),
        };
        written.map_err(|e| e.to_string())?;
        job.committed.store(true, Ordering::SeqCst);
        Ok(())
    };
    let mut rollback_pointer = || -> std::result::Result<(), String> {
        let restored = match &job.previous {
            Some(previous) => redirect::write(&job.redirect_file, previous),
            None => redirect::remove(&job.redirect_file),
        };
        restored.map_err(|e| e.to_string())?;
        job.committed.store(false, Ordering::SeqCst);
        Ok(())
    };
    let lands = || lands_on_target(&job.default, &job.redirect_file, &job.target);
    // Without this, `logs/` is a leftover on every move from a volume that
    // cannot delete an open file (exFAT, FAT, SMB).
    let mut on_switched = || crate::diag::release_file();

    let mut hooks = Hooks {
        on_progress: &mut on_progress,
        is_cancelled: &is_cancelled,
        still_safe_to_switch: &still_safe_to_switch,
        commit_pointer: &mut commit_pointer,
        rollback_pointer: &mut rollback_pointer,
        lands_on_target: &lands,
        on_switched: &mut on_switched,
    };
    relocate::relocate(&job.current, &job.target, &Io::real(), &mut hooks)
}

/// What `set_data_location` may do given the fallback state. Pure so the
/// gating truth-table is unit-testable without an `AppHandle`.
#[derive(Debug, PartialEq, Eq)]
enum FallbackGate {
    /// Fallen back + a MOVE requested → refuse (migrating the temporary root
    /// would copy the wrong tree).
    RejectBusy,
    /// Fallen back + a RESET requested → allowed, but pointer-only (remove
    /// the redirect, restart; nothing is copied).
    PointerOnlyReset,
    /// Not fallen back → the normal migrate/reset pipeline.
    Normal,
}

fn fallback_gate(fell_back: bool, is_reset: bool) -> FallbackGate {
    match (fell_back, is_reset) {
        (false, _) => FallbackGate::Normal,
        (true, true) => FallbackGate::PointerOnlyReset,
        (true, false) => FallbackGate::RejectBusy,
    }
}

/// Pure commit-time gate for [`adopt_data_location`]: decide the rejection
/// reason or accept, and whether the accept adopts the DEFAULT root (redirect
/// removed instead of written). Predicates are injected so the decision table
/// is unit testable without a filesystem; the command passes the real
/// `data_root::looks_like_data_root` / `migrate::is_available` probes and the canonical
/// `migrate::is_same_path` compare.
///
/// Deliberately has NO nesting check in either direction — nothing moves, and
/// "current root nested inside the adopted root" is exactly the doubled-path
/// recovery adopt exists for.
fn classify_adopt(
    target: &Path,
    current: &Path,
    default: &Path,
    is_root: &dyn Fn(&Path) -> bool,
    is_avail: &dyn Fn(&Path) -> bool,
    same: &dyn Fn(&Path, &Path) -> bool,
) -> std::result::Result<bool, crate::data_root::validate::Invalid> {
    use crate::data_root::validate::Invalid;
    if !target.is_absolute() {
        return Err(Invalid::NotAbsolute);
    }
    if !is_root(target) {
        return Err(Invalid::NotADataRoot);
    }
    if same(target, current) {
        return Err(Invalid::SameAsCurrent);
    }
    if !is_avail(target) {
        return Err(Invalid::NotWritable);
    }
    Ok(same(target, default))
}

#[cfg(test)]
mod tests {
    use super::classify_adopt;
    use super::{fallback_gate, FallbackGate};
    use crate::data_root::validate::Invalid;
    use std::path::{Path, PathBuf};

    #[test]
    fn fallback_gate_truth_table() {
        // Not fallen back → normal pipeline regardless of reset/move.
        assert_eq!(fallback_gate(false, true), FallbackGate::Normal);
        assert_eq!(fallback_gate(false, false), FallbackGate::Normal);
        // Fallen back: reset is the pointer-only recovery path; a move stays
        // refused (migrating the temporary root would copy the wrong tree).
        assert_eq!(fallback_gate(true, true), FallbackGate::PointerOnlyReset);
        assert_eq!(fallback_gate(true, false), FallbackGate::RejectBusy);
    }

    // Absolute on BOTH platforms (mirrors validate.rs tests): `/x` is not
    // absolute on Windows, which would short-circuit every case on
    // NotAbsolute under the windows CI runner.
    fn abs(rel: &str) -> PathBuf {
        #[cfg(windows)]
        {
            PathBuf::from(format!("C:\\{}", rel.replace('/', "\\")))
        }
        #[cfg(not(windows))]
        {
            PathBuf::from(format!("/{rel}"))
        }
    }

    fn always(_: &Path) -> bool {
        true
    }
    fn never(_: &Path) -> bool {
        false
    }
    fn eq(a: &Path, b: &Path) -> bool {
        a == b
    }

    #[test]
    fn relative_target_is_rejected_first() {
        let r = classify_adopt(
            Path::new("rel"),
            &abs("cur"),
            &abs("def"),
            &always,
            &always,
            &eq,
        );
        assert_eq!(r, Err(Invalid::NotAbsolute));
    }

    #[test]
    fn non_root_target_is_rejected() {
        let r = classify_adopt(&abs("t"), &abs("cur"), &abs("def"), &never, &always, &eq);
        assert_eq!(r, Err(Invalid::NotADataRoot));
    }

    #[test]
    fn same_as_current_is_rejected() {
        let cur = abs("cur");
        let r = classify_adopt(&cur, &cur, &abs("def"), &always, &always, &eq);
        assert_eq!(r, Err(Invalid::SameAsCurrent));
    }

    #[test]
    fn unwritable_target_is_rejected() {
        let r = classify_adopt(&abs("t"), &abs("cur"), &abs("def"), &always, &never, &eq);
        assert_eq!(r, Err(Invalid::NotWritable));
    }

    #[test]
    fn custom_root_accepts_and_writes_redirect() {
        let r = classify_adopt(&abs("t"), &abs("cur"), &abs("def"), &always, &always, &eq);
        assert_eq!(r, Ok(false));
    }

    #[test]
    fn default_root_accepts_and_removes_redirect() {
        let def = abs("def");
        let r = classify_adopt(&def, &abs("cur"), &def, &always, &always, &eq);
        assert_eq!(r, Ok(true));
    }

    #[test]
    fn current_nested_inside_target_is_allowed() {
        // The doubled-path recovery: current root lives INSIDE the adopted
        // root. Only exact-same is a conflict; nesting must pass.
        let target = abs("root");
        let current = target.join("LucernaData");
        let r = classify_adopt(&target, &current, &abs("def"), &always, &always, &eq);
        assert_eq!(r, Ok(false));
    }
}
