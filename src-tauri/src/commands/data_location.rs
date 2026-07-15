//! Get/set the effective data-root location, plus the running-guard that
//! blocks relocation while a game or server is live.

use crate::error::{Error, Result};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Manager};
use tauri_specta::Event;

/// True if any Minecraft instance process is currently live, or any saved
/// server reports a running status. Reuses the existing liveness
/// chokepoints — `launch::spawn::is_any_running` (any running client instance)
/// and `commands::server_list`'s per-server `running` field (the same
/// PID-reconciled status the Servers UI and preflight diagnosis use) — so
/// this introduces no new process bookkeeping.
pub fn any_game_running(app: &AppHandle) -> bool {
    if crate::launch::spawn::is_any_running() {
        return true;
    }
    crate::commands::server_list(app.clone())
        .map(|servers| servers.iter().any(|s| s.running))
        .unwrap_or(false)
}

/// Process-wide guard so two overlapping relocation calls (a double-fired
/// click, or a reset racing a set) can never run the copy/verify/delete
/// pipeline concurrently against the same source. The second caller returns
/// `DataLocationBusy`. The success path ends in `app.restart()`, which tears
/// the process down, so it need not clear the flag; every early-return / error
/// path MUST clear it (handled via `MigrationGuard`'s Drop).
static MIGRATION_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

/// RAII holder for `MIGRATION_IN_PROGRESS`. Acquired via `try_acquire`; on drop
/// it releases the flag so no error path can leak the guard. On the success
/// path `std::mem::forget` keeps it held (the process is about to restart).
struct MigrationGuard;

impl MigrationGuard {
    fn try_acquire() -> Option<Self> {
        MIGRATION_IN_PROGRESS
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .ok()
            .map(|_| MigrationGuard)
    }
}

impl Drop for MigrationGuard {
    fn drop(&mut self) {
        MIGRATION_IN_PROGRESS.store(false, Ordering::SeqCst);
    }
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct DataLocationStatus {
    pub effective: String,
    pub configured: Option<String>,
    pub fell_back: bool,
}

/// Current effective data-root location and its configured (possibly
/// unavailable) target. Deliberately cheap — a plain read of the resolved
/// `DataRoot` state. The on-disk size lives in `data_root_size_bytes`
/// instead: this command runs on the startup path (fallback gating reads
/// `fell_back` at mount), and as a sync command it executes on the main
/// thread, so it must never touch the filesystem tree.
#[tauri::command]
#[specta::specta]
pub fn get_data_location(app: AppHandle) -> Result<DataLocationStatus> {
    let st = app.state::<crate::data_root::DataRoot>();
    Ok(DataLocationStatus {
        effective: st.0.root.display().to_string(),
        configured: st.0.configured.as_ref().map(|p| p.display().to_string()),
        fell_back: st.0.fell_back,
    })
}

/// Total size in bytes of everything under the effective data root. Split
/// out of `get_data_location` because the recursive walk (assets, libraries,
/// versions, every instance's mods — easily tens of thousands of files)
/// takes seconds on a cold FS cache. Async + `spawn_blocking` so it never
/// runs on the main thread and never stalls the async runtime; the Storage
/// panel fetches it lazily when opened.
/// f64 not u64: specta forbids exporting BigInt-style types to TS.
#[tauri::command]
#[specta::specta]
pub async fn data_root_size_bytes(app: AppHandle) -> Result<f64> {
    let root = app.state::<crate::data_root::DataRoot>().0.root.clone();
    let size = tokio::task::spawn_blocking(move || crate::data_root::migrate::dir_size(&root))
        .await
        .map_err(|e| Error::io("<data_root_size>", format!("size task panicked: {e}")))?;
    Ok(size as f64)
}

/// Streamed progress for a data-root relocation.
#[derive(Debug, Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct DataMigrationProgress {
    pub copied_bytes: f64,
    pub total_bytes: f64,
    /// "copying" | "verifying" | "deleting"
    pub phase: String,
}

/// Name of the bootstrap redirect file. It always lives at the OS-default
/// app-data dir and must never be copied/deleted as part of a relocation.
const REDIRECT_FILE_NAME: &str = "data-location.json";

/// Top-level entries the OS-default dir may legitimately hold that are NOT user
/// data: the bootstrap redirect and launcher-transient scratch. A reset target
/// (the default dir) is accepted when it is empty OR contains only these.
const SAFE_OVERLAP: [&str; 3] = ["data-location.json", "logs", "updates"];

/// Relocate the data root to `new_path`, or reset to the OS default when
/// `None`. Copies the current root to the target, verifies the copy,
/// repoints the bootstrap redirect, deletes the old data, then restarts the
/// app so every chokepoint re-resolves `paths::app_dir` against the new root.
///
/// Rejected while any game/server is running (`Error::DataLocationBusy`), while
/// the launcher is already running from a fallback root (`DataLocationBusy` —
/// the temporary root is unsafe to move), when a second relocation is already
/// in progress (`DataLocationBusy`), or when the target fails validation
/// (`Error::DataLocationInvalid`). A copy or verify failure surfaces as
/// `Error::DataLocationMigrationFailed` — the original data is left untouched
/// because the redirect is written and the old data deleted only after a
/// complete, verified copy.
#[tauri::command]
#[specta::specta]
pub async fn set_data_location(app: AppHandle, new_path: Option<String>) -> Result<()> {
    // Concurrency guard first: a second concurrent call bails out immediately
    // without touching the filesystem.
    let guard = MigrationGuard::try_acquire().ok_or(Error::DataLocationBusy)?;

    // Never move the temporary fallback root — the configured root is
    // unavailable, so a move would copy the wrong (partial) tree and rewrite
    // the redirect against a root the user did not intend.
    if app.state::<crate::data_root::DataRoot>().0.fell_back {
        return Err(Error::DataLocationBusy);
    }

    if any_game_running(&app) {
        return Err(Error::DataLocationBusy);
    }

    let current = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let default =
        crate::paths::default_app_data_dir(&app).map_err(|e| Error::io("<default>", e))?;
    let is_reset = new_path.is_none();
    let target = match &new_path {
        Some(p) => PathBuf::from(p),
        None => default.clone(),
    };

    // Empty-check differs for reset vs. a fresh custom target: the OS-default
    // dir legitimately holds the redirect + launcher scratch even with no user
    // data, so a reset accepts "empty or only-safe entries"; a custom target
    // must be strictly empty.
    let empty = if is_reset {
        crate::data_root::migrate::empty_or_only_safe(&target, &SAFE_OVERLAP)
    } else {
        crate::data_root::migrate::target_is_empty(&target)
    };
    crate::data_root::validate::validate_target(&current, &target, empty).map_err(|v| {
        Error::DataLocationInvalid {
            reason: v.reason_key().to_string(),
        }
    })?;

    // BLOCKER guard: `validate_target`'s nested check is lexical + case-
    // sensitive and misses case-differing / `\\?\` verbatim / 8.3 spellings of
    // the same or a nested path. Reject robustly on canonical forms BEFORE any
    // copy, so the later delete loop can never wipe both source and target.
    // (Skip for reset — the default dir is by construction not nested in a
    // relocated `current`, and its safe-overlap contents are handled above.)
    if !is_reset && crate::data_root::migrate::is_same_or_nested(&current, &target) {
        return Err(Error::DataLocationInvalid {
            reason: crate::data_root::validate::Invalid::NestedInCurrent
                .reason_key()
                .to_string(),
        });
    }

    // Symlink/junction safety: refuse to move a tree containing reparse points
    // — a junction could point outside the tree (data loss) or form a cycle.
    if crate::data_root::migrate::contains_reparse_point(&current).map_err(|e| {
        Error::DataLocationMigrationFailed {
            reason: e.to_string(),
        }
    })? {
        return Err(Error::DataLocationMigrationFailed {
            reason:
                "the data folder contains a symbolic link or junction, which cannot be safely moved"
                    .into(),
        });
    }

    // Canonical target for the delete-loop defense-in-depth check below.
    let canonical_target = std::fs::canonicalize(&target).unwrap_or_else(|_| target.clone());

    // Everything from here — the size scan, the copy, the verify, and the
    // delete — is blocking filesystem work. Run it off the async runtime so a
    // multi-GB move never stalls other Tauri commands. The AppHandle is cloned
    // for progress emission inside the blocking task.
    let app_blocking = app.clone();
    let current_for_task = current.clone();
    let target_for_task = target.clone();
    let copied = tokio::task::spawn_blocking(move || -> Result<u64> {
        run_migration(
            &app_blocking,
            &current_for_task,
            &target_for_task,
            &canonical_target,
        )
    })
    .await
    .map_err(|e| Error::DataLocationMigrationFailed {
        reason: format!("migration task panicked: {e}"),
    })??;

    let _ = copied; // consumed inside run_migration for the verify step.

    // Point the redirect at the new root ONLY after a complete, verified copy.
    let redirect_file =
        crate::paths::redirect_file(&app).map_err(|e| Error::io("<redirect>", e))?;
    match &new_path {
        Some(p) => crate::data_root::redirect::write(
            &redirect_file,
            &crate::data_root::redirect::Redirect {
                path: PathBuf::from(p),
            },
        )?,
        None => crate::data_root::redirect::remove(&redirect_file)?,
    }

    // The migration pipeline (copy + verify + delete of the old data) already
    // completed inside `run_migration`. Keep the guard held across the restart
    // so nothing can re-enter; the process is about to be torn down.
    std::mem::forget(guard);
    app.restart();
}

/// The blocking copy → verify → delete pipeline. Returns the number of bytes
/// copied (excluding the skipped redirect). Emits `DataMigrationProgress`
/// throughout. Runs entirely on a blocking thread.
fn run_migration(
    app: &AppHandle,
    current: &Path,
    target: &Path,
    canonical_target: &Path,
) -> Result<u64> {
    // The redirect file itself lives at the default app-data dir and must
    // never be moved as part of the tree copy/delete — it is the bootstrap
    // pointer read *before* `DataRoot` is resolved.
    let redirect_name = std::ffi::OsString::from(REDIRECT_FILE_NAME);
    let skip = move |p: &Path| p.as_os_str() == redirect_name;

    let total = crate::data_root::migrate::dir_size(current) as f64;
    let mut copied = 0u64;
    {
        let app_for_progress = app.clone();
        // Throttle progress events: a real data root has tens of thousands of
        // small files (assets/objects, libraries, mods), and emitting one Tauri
        // event PER FILE floods the IPC channel and the UI, drowning the copy
        // itself. Emit at most once per `EMIT_EVERY` bytes of progress instead.
        const EMIT_EVERY: u64 = 16 * 1024 * 1024; // 16 MiB
        let mut last_emit = 0u64;
        crate::data_root::migrate::copy_tree(
            current,
            target,
            &skip,
            &mut |c| {
                if c.saturating_sub(last_emit) < EMIT_EVERY {
                    return;
                }
                last_emit = c;
                let _ = DataMigrationProgress {
                    copied_bytes: c as f64,
                    total_bytes: total,
                    phase: "copying".into(),
                }
                .emit(&app_for_progress);
            },
            &mut copied,
        )
        .map_err(|e| Error::DataLocationMigrationFailed {
            reason: e.to_string(),
        })?;
    }

    let _ = DataMigrationProgress {
        copied_bytes: copied as f64,
        total_bytes: total,
        phase: "verifying".into(),
    }
    .emit(app);

    // Verify completeness by walking the SOURCE: every copied file must exist
    // in the target with an identical byte length. This is robust to a reset
    // that overwrites pre-existing safe-overlap files (logs/updates) already in
    // the default dir — a whole-directory size delta would false-fail on such
    // an overwrite even though the copy succeeded.
    crate::data_root::migrate::verify_copy(current, target, &skip)?;

    let _ = DataMigrationProgress {
        copied_bytes: copied as f64,
        total_bytes: total,
        phase: "deleting".into(),
    }
    .emit(app);

    // Delete the old data: every top-level entry of `current` except the
    // redirect file (which only ever lives there when current == default).
    if let Ok(entries) = std::fs::read_dir(current) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name == REDIRECT_FILE_NAME {
                continue;
            }
            let path = entry.path();

            // Defense in depth: never delete a top-level entry whose canonical
            // path equals or is an ancestor of the canonical target. Even
            // though `is_same_or_nested` already rejected same/nested targets,
            // this guarantees the freshly-copied target can never be caught in
            // this loop under any path-spelling edge case.
            let canonical_entry = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
            if canonical_target == canonical_entry || canonical_target.starts_with(&canonical_entry)
            {
                continue;
            }

            let result = if path.is_dir() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            };
            if let Err(e) = result {
                if e.kind() != std::io::ErrorKind::NotFound {
                    return Err(Error::DataLocationMigrationFailed {
                        reason: format!("failed to remove old data at {}: {e}", path.display()),
                    });
                }
            }
        }
    }

    Ok(copied)
}
