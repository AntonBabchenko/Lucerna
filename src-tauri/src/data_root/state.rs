//! Process-wide state of a data-root move, owned by the backend.
//!
//! Why the backend owns it: between the pointer commit and the restart this
//! process still runs from the OLD root, which is being — or has been —
//! emptied. Every create/launch command must refuse in that window
//! (`data_root::reject_if_root_unusable`), and the UI must be able to re-show
//! the only path to a restart after a page reload (`get_data_location`).
//!
//! `RelocationState` is an owned type with a `global()` instance, so tests
//! build their own and never race on a process-wide static.

use crate::error::{Error, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum MovePhase {
    Copying,
    Verifying,
    Switching,
    Deleting,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RelocationStatus {
    Idle,
    Running {
        phase: MovePhase,
    },
    RestartRequired {
        old_root: String,
        new_root: String,
        /// Top-level entry names still in `old_root`.
        leftovers: Vec<String>,
        /// Nothing was deleted: the old folder is still a complete copy.
        old_root_intact: bool,
    },
}

pub struct RelocationState {
    status: Mutex<RelocationStatus>,
    cancel: AtomicBool,
}

impl Default for RelocationState {
    fn default() -> Self {
        Self::new()
    }
}

/// RAII handle of the one move in flight. Dropping it un-parked returns the
/// state to `Idle` — so no early return or `?` can leave the launcher locked.
#[must_use = "dropping the session immediately ends the move's claim"]
pub struct MoveSession<'s> {
    state: &'s RelocationState,
    parked: bool,
}

impl RelocationState {
    pub const fn new() -> Self {
        Self {
            status: Mutex::new(RelocationStatus::Idle),
            cancel: AtomicBool::new(false),
        }
    }

    /// Poison-tolerant: the lock is only held for one assignment or clone.
    fn lock(&self) -> MutexGuard<'_, RelocationStatus> {
        self.status.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn status(&self) -> RelocationStatus {
        self.lock().clone()
    }

    /// Claim the move. `None` unless the state is `Idle`.
    pub fn begin(&self) -> Option<MoveSession<'_>> {
        let mut status = self.lock();
        if *status != RelocationStatus::Idle {
            return None;
        }
        *status = RelocationStatus::Running {
            phase: MovePhase::Copying,
        };
        self.cancel.store(false, Ordering::SeqCst);
        Some(MoveSession {
            state: self,
            parked: false,
        })
    }

    /// Report the phase of the running move. Ignored unless a move is `Running`.
    pub fn set_phase(&self, phase: MovePhase) {
        let mut status = self.lock();
        if matches!(*status, RelocationStatus::Running { .. }) {
            *status = RelocationStatus::Running { phase };
        }
    }

    /// Ask the running move to stop. No-op unless a move is `Running`; the
    /// pipeline stops polling once the switch starts.
    pub fn request_cancel(&self) {
        if matches!(*self.lock(), RelocationStatus::Running { .. }) {
            self.cancel.store(true, Ordering::SeqCst);
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    /// `Ok` only while `Idle`.
    pub fn check_usable(&self) -> Result<()> {
        let status = self.lock();
        match &*status {
            RelocationStatus::Idle => Ok(()),
            RelocationStatus::Running { .. } => Err(Error::DataRelocationInProgress {
                restart_required: false,
            }),
            RelocationStatus::RestartRequired { .. } => Err(Error::DataRelocationInProgress {
                restart_required: true,
            }),
        }
    }

    /// Replace the leftovers of a `RestartRequired` state (after a retry) and
    /// return the new status. Any other state is returned unchanged.
    pub fn replace_leftovers(&self, leftovers: Vec<String>) -> RelocationStatus {
        let mut status = self.lock();
        if let RelocationStatus::RestartRequired {
            leftovers: current, ..
        } = &mut *status
        {
            *current = leftovers;
        }
        status.clone()
    }
}

impl MoveSession<'_> {
    pub fn set_phase(&self, phase: MovePhase) {
        self.state.set_phase(phase);
    }

    /// Keep the claim for the life of the process: a clean move, an adopt or a
    /// pointer-only reset is about to restart the app.
    pub fn park_running(mut self) {
        self.parked = true;
    }

    /// The move is committed but this process still runs from the old root.
    pub fn park_restart_required(
        mut self,
        old_root: String,
        new_root: String,
        leftovers: Vec<String>,
        old_root_intact: bool,
    ) {
        *self.state.lock() = RelocationStatus::RestartRequired {
            old_root,
            new_root,
            leftovers,
            old_root_intact,
        };
        self.parked = true;
    }
}

impl Drop for MoveSession<'_> {
    fn drop(&mut self) {
        if !self.parked {
            *self.state.lock() = RelocationStatus::Idle;
        }
    }
}

/// The instance production code uses.
pub fn global() -> &'static RelocationState {
    static GLOBAL: RelocationState = RelocationState::new();
    &GLOBAL
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;

    #[test]
    fn only_one_move_at_a_time_and_a_dropped_session_frees_the_state() {
        let state = RelocationState::new();
        let first = state.begin().expect("idle → claimable");
        assert_eq!(
            state.status(),
            RelocationStatus::Running {
                phase: MovePhase::Copying
            }
        );
        assert!(state.begin().is_none(), "a second move must be refused");
        drop(first);
        assert_eq!(state.status(), RelocationStatus::Idle);
        assert!(state.begin().is_some(), "free again after an early return");
    }

    #[test]
    fn phases_are_visible_while_running() {
        let state = RelocationState::new();
        let session = state.begin().unwrap();
        session.set_phase(MovePhase::Deleting);
        assert_eq!(
            state.status(),
            RelocationStatus::Running {
                phase: MovePhase::Deleting
            }
        );
    }

    #[test]
    fn a_parked_restart_required_survives_the_session_and_blocks_a_new_move() {
        let state = RelocationState::new();
        state.begin().unwrap().park_restart_required(
            "C:\\old".into(),
            "D:\\new".into(),
            vec!["libraries".into()],
            false,
        );
        assert_eq!(
            state.status(),
            RelocationStatus::RestartRequired {
                old_root: "C:\\old".into(),
                new_root: "D:\\new".into(),
                leftovers: vec!["libraries".into()],
                old_root_intact: false,
            }
        );
        assert!(state.begin().is_none());
    }

    #[test]
    fn a_parked_running_session_keeps_the_claim_for_the_restart() {
        let state = RelocationState::new();
        state.begin().unwrap().park_running();
        assert!(matches!(state.status(), RelocationStatus::Running { .. }));
        assert!(state.begin().is_none());
    }

    #[test]
    fn the_root_is_usable_only_while_idle() {
        let state = RelocationState::new();
        assert!(state.check_usable().is_ok());

        let session = state.begin().unwrap();
        match state.check_usable() {
            Err(Error::DataRelocationInProgress { restart_required }) => {
                assert!(!restart_required)
            }
            other => panic!("expected a refusal while running, got {other:?}"),
        }
        session.park_restart_required("a".into(), "b".into(), vec![], false);
        match state.check_usable() {
            Err(Error::DataRelocationInProgress { restart_required }) => {
                assert!(restart_required)
            }
            other => panic!("expected a refusal after the switch, got {other:?}"),
        }
    }

    #[test]
    fn cancel_is_a_request_that_only_a_running_move_hears_and_a_new_move_starts_clean() {
        let state = RelocationState::new();
        state.request_cancel();
        assert!(!state.is_cancelled(), "nothing runs, nothing to cancel");

        let session = state.begin().unwrap();
        assert!(!state.is_cancelled());
        state.request_cancel();
        assert!(state.is_cancelled());
        drop(session);

        let _again = state.begin().unwrap();
        assert!(
            !state.is_cancelled(),
            "a new move must not inherit the flag"
        );
    }

    #[test]
    fn a_retry_replaces_the_leftovers_and_nothing_else() {
        let state = RelocationState::new();
        assert_eq!(
            state.replace_leftovers(vec!["x".into()]),
            RelocationStatus::Idle
        );

        state.begin().unwrap().park_restart_required(
            "old".into(),
            "new".into(),
            vec!["libraries".into(), "logs".into()],
            false,
        );
        assert_eq!(
            state.replace_leftovers(vec!["logs".into()]),
            RelocationStatus::RestartRequired {
                old_root: "old".into(),
                new_root: "new".into(),
                leftovers: vec!["logs".into()],
                old_root_intact: false,
            }
        );
    }

    #[test]
    fn status_serializes_with_a_kind_tag_the_frontend_can_switch_on() {
        let json = serde_json::to_string(&RelocationStatus::Running {
            phase: MovePhase::Switching,
        })
        .unwrap();
        assert_eq!(json, r#"{"kind":"running","phase":"switching"}"#);
        assert_eq!(
            serde_json::to_string(&RelocationStatus::Idle).unwrap(),
            r#"{"kind":"idle"}"#
        );
    }
}
