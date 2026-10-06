//! Startup recovery of interrupted content updates (spec 2026-10-04 §4.6).
//!
//! `start_update_recovery` runs inside `setup`, which completes before the
//! webview's first IPC: every instance with a pending content transaction is
//! claimed there, synchronously, so Play and every writer refuse it until its
//! undo has run. The undo itself runs on the async runtime; the frontend reads
//! the outcome once through `take_update_recovery_report`.
//!
//! After an undo that could not put everything back the instance is NOT kept
//! locked: a lock with no in-app way out is worse than a profile the notice
//! warns about, and the registry's reconcile brings its rows in line with the
//! disk. The log carries the full list.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Serialize;
use tauri::Manager;
use tauri_specta::Event;

use crate::instances::maintenance::MaintenanceGuard;
use crate::mods::txn::{self, RecoveredOutcome};

/// How the startup undo of one interrupted update ended.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RecoveryOutcome {
    Restored,
    Incomplete { folder: String, details: String },
}

/// One interrupted update the startup recovery dealt with.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct RecoveredUpdate {
    pub instance_id: String,
    pub instance_name: String,
    pub outcome: RecoveryOutcome,
}

/// Managed state: `done` turns true once the recovery task has stored its
/// report (at once when nothing is recovered).
pub struct UpdateRecovery {
    done: tokio::sync::watch::Receiver<bool>,
    report: tokio::sync::Mutex<Option<Vec<RecoveredUpdate>>>,
}

/// What startup undid, once: waits for the recovery task, returns its report
/// and clears it. Every later call returns an empty list.
#[tauri::command]
#[specta::specta]
pub async fn take_update_recovery_report(
    state: tauri::State<'_, UpdateRecovery>,
) -> Result<Vec<RecoveredUpdate>, crate::error::Error> {
    let mut done = state.done.clone();
    if done.wait_for(|finished| *finished).await.is_err() {
        // The sender is gone without signalling: the task ended early. What it
        // stored, if anything, is all there is to report.
        crate::diag!("[update-recovery] the recovery task ended without signalling");
    }
    Ok(state.report.lock().await.take().unwrap_or_default())
}

struct Target {
    id: String,
    name: String,
    root: PathBuf,
    /// `Some` when the instance has a pending transaction to undo.
    claim: Option<MaintenanceGuard>,
}

/// Called once from `setup`. `recover` is false in a recovery session (a
/// throwaway root with nothing of the user's to undo).
pub fn start_update_recovery(
    app: &tauri::AppHandle,
    instances_dir: Option<PathBuf>,
    recover: bool,
    session_start: SystemTime,
) {
    let (tx, rx) = tokio::sync::watch::channel(false);
    app.manage(UpdateRecovery {
        done: rx,
        report: tokio::sync::Mutex::new(None),
    });
    if !recover {
        signal_done(&tx);
        return;
    }
    let Some(dir) = instances_dir else {
        crate::diag!("[update-recovery] skipped: the instances folder could not be resolved");
        signal_done(&tx);
        return;
    };
    let session_start_ms = crate::mods::trash::unix_ms(session_start);
    let targets = targets(&dir);
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut report = Vec::new();
        for Target {
            id,
            name,
            root,
            claim,
        } in targets
        {
            let results = match claim {
                Some(claim) => {
                    let results = txn::recover_pending(&root, session_start_ms).await;
                    if let Err(e) = (crate::commands::ModsReconciled {
                        instance_id: id.clone(),
                    })
                    .emit(&handle)
                    {
                        crate::diag!("[update-recovery] could not announce {id}: {e}");
                    }
                    drop(claim);
                    results
                }
                None => txn::sweep_closed(&root, session_start_ms).await,
            };
            for r in results {
                let outcome = match r.outcome {
                    RecoveredOutcome::Restored => RecoveryOutcome::Restored,
                    RecoveredOutcome::Incomplete { folder, details } => {
                        RecoveryOutcome::Incomplete { folder, details }
                    }
                };
                crate::diag!("{}", recovery_log_line(&id, &name, &outcome));
                report.push(RecoveredUpdate {
                    instance_id: id.clone(),
                    instance_name: name.clone(),
                    outcome,
                });
            }
        }
        *handle.state::<UpdateRecovery>().report.lock().await = Some(report);
        signal_done(&tx);
    });
}

/// Every instance directory, with a maintenance claim on each that has a
/// pending transaction. Synchronous on purpose — see the module doc.
fn targets(dir: &Path) -> Vec<Target> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            crate::diag!("[update-recovery] cannot list {}: {e}", dir.display());
            return Vec::new();
        }
    };
    let mut out = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                crate::diag!("[update-recovery] cannot read {}: {e}", dir.display());
                continue;
            }
        };
        match entry.file_type() {
            Ok(t) if t.is_dir() => {}
            // A file or a link is not an instance directory.
            Ok(_) => continue,
            Err(e) => {
                crate::diag!(
                    "[update-recovery] cannot stat {}: {e}",
                    entry.path().display()
                );
                continue;
            }
        }
        let root = entry.path();
        let id = entry.file_name().to_string_lossy().into_owned();
        let pending = match txn::pending_tokens(&root) {
            Ok(tokens) => !tokens.is_empty(),
            Err(e) => {
                // Could not tell: claim it and let the recovery pass decide —
                // it sets aside, never acts on, what it cannot read.
                crate::diag!(
                    "[update-recovery] {id}: cannot tell whether an update is pending ({e})"
                );
                true
            }
        };
        let claim = if pending {
            match crate::instances::maintenance::claim_write(&id) {
                Ok(guard) => Some(guard),
                Err(e) => {
                    crate::diag!(
                        "[update-recovery] {id}: claim refused ({e}); left for the next start"
                    );
                    continue;
                }
            }
        } else {
            None
        };
        let name = instance_name(&root).unwrap_or_else(|| id.clone());
        out.push(Target {
            id,
            name,
            root,
            claim,
        });
    }
    out
}

/// The profile's display name for the notice; `None` when `instance.json`
/// cannot be read (the caller shows the directory name instead).
fn instance_name(root: &Path) -> Option<String> {
    match crate::instances::store::read_instance_json(&root.join("instance.json")) {
        Ok(inst) => Some(inst.name),
        Err(e) => {
            crate::diag!("[update-recovery] no name for {}: {e}", root.display());
            None
        }
    }
}

/// The log line for one update the start dealt with. An incomplete outcome is
/// not always an undo that stopped — a token whose record could not be read is
/// set aside untouched — so it names the folder and the reason as given.
fn recovery_log_line(id: &str, name: &str, outcome: &RecoveryOutcome) -> String {
    match outcome {
        RecoveryOutcome::Restored => {
            format!("[update-recovery] «{name}» ({id}): undid an update that was cut off")
        }
        RecoveryOutcome::Incomplete { folder, details } => format!(
            "[update-recovery] «{name}» ({id}): left in {folder}, not fully put back: {details}"
        ),
    }
}

fn signal_done(tx: &tokio::sync::watch::Sender<bool>) {
    if tx.send(true).is_err() {
        // Only possible when no receiver is left, i.e. nobody can wait for it.
        crate::diag!("[update-recovery] nobody is waiting for the report");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Startup claims exactly the instances with an unfinished update, and the
    /// claim makes Play refuse them until it is dropped.
    #[test]
    fn targets_claims_only_instances_with_a_pending_update() {
        let td = tempfile::tempdir().unwrap();
        let pending_id = format!("recovery-test-pending-{}", std::process::id());
        let clean_id = format!("recovery-test-clean-{}", std::process::id());
        let token = txn::txn_root(&td.path().join(&pending_id)).join("1000-abcdef12");
        std::fs::create_dir_all(&token).unwrap();
        std::fs::write(token.join("record.json"), b"{}").unwrap();
        std::fs::create_dir_all(td.path().join(&clean_id)).unwrap();

        let found = targets(td.path());

        let pending = found.iter().find(|t| t.id == pending_id).unwrap();
        let clean = found.iter().find(|t| t.id == clean_id).unwrap();
        assert!(pending.claim.is_some());
        assert!(clean.claim.is_none());
        assert!(crate::instances::maintenance::maintenance_is_active(
            &pending_id
        ));
        assert_eq!(
            pending.name, pending_id,
            "no instance.json: the folder name"
        );
        drop(found);
        assert!(!crate::instances::maintenance::maintenance_is_active(
            &pending_id
        ));
    }

    /// Every update the start dealt with leaves a line naming the instance — a
    /// clean undo too, which used to leave none (the notice was its only trace).
    #[test]
    fn recovery_log_line_names_the_instance_and_what_happened() {
        assert_eq!(
            recovery_log_line("Pack-1", "Pack", &RecoveryOutcome::Restored),
            "[update-recovery] «Pack» (Pack-1): undid an update that was cut off"
        );
        let set_aside = recovery_log_line(
            "Pack-1",
            "Pack",
            &RecoveryOutcome::Incomplete {
                folder: "f-kept".into(),
                details: "its record could not be read: eof".into(),
            },
        );
        assert!(
            set_aside.starts_with("[update-recovery] «Pack» (Pack-1): "),
            "{set_aside}"
        );
        assert!(set_aside.contains("f-kept"), "{set_aside}");
        assert!(
            set_aside.contains("its record could not be read: eof"),
            "{set_aside}"
        );
    }
}
