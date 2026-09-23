//! The window's close asks before it kills anything (Settings audit, batch 11c).
//!
//! Closing the last window raises `RunEvent::ExitRequested`, whose hook in
//! `lib.rs` force-kills every tracked game and server. The title-bar ×,
//! Alt+F4 and the taskbar's "Close window" all get there, and for most users
//! the window stays visible while they play — so the × was the common way to
//! lose a running game without a word. The window's close now asks first.
//!
//! This module holds the DECISIONS as pure functions, so each can be tested
//! without an `AppHandle`; the impure flow that drives them lives beside them.

use crate::data_root::blockers::CloseLosses;
use crate::data_root::state::RelocationStatus;

// ---------------------------------------------------------------------------
// The gate every path that can end the process passes
// ---------------------------------------------------------------------------

/// What an attempt to end the process may do, given a data-root move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseGate {
    /// A move is copying: ending now kills the copy with no cleanup.
    Prevent,
    /// The move is done and waits for a restart; the next start lands on the
    /// new root, so closing is exactly what the user should do.
    Allow,
    /// Nothing about the data root stops it: ask what closing would lose.
    Check,
}

/// NOT `check_usable()`: that refuses `RestartRequired`, the one state in
/// which closing is explicitly safe.
pub fn close_gate(_status: &RelocationStatus) -> CloseGate {
    // STUB (red).
    CloseGate::Check
}

// ---------------------------------------------------------------------------
// Whether to ask
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseVerdict {
    /// Nothing would be lost.
    Exit,
    /// Something would be — or could not be checked. Ask.
    Ask(CloseLosses),
}

/// `unchecked` alone asks too: "could not tell" is not "nothing is running".
pub fn close_verdict(_losses: &CloseLosses) -> CloseVerdict {
    // STUB (red).
    CloseVerdict::Exit
}

// ---------------------------------------------------------------------------
// Whether a confirm may go ahead
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmVerdict {
    /// Everything that would be lost now was named in the dialog.
    Exit,
    /// Something new appeared while the dialog was open: ask again, naming it.
    Reprompt(CloseLosses),
}

/// Everything shown or seen now. Kinds are OR-ed; the server count is the
/// larger, so the re-prompt names every server that would stop.
pub fn union(a: &CloseLosses, b: &CloseLosses) -> CloseLosses {
    CloseLosses {
        games: a.games || b.games,
        servers: a.servers.max(b.servers),
        operation: a.operation || b.operation,
        unchecked: a.unchecked || b.unchecked,
    }
}

/// A confirm acts on what was SHOWN, but the world can change while the
/// dialog is open — a shortcut's `--launch` can start a game. Exit only when
/// every KIND of loss seen now was already named; otherwise re-prompt with the
/// union. Kinds, not counts: a third server is the same loss the user already
/// agreed to, a game is not. The union only grows over four kinds, so there
/// are at most four re-prompts — changing state cannot trap the user.
pub fn confirm_verdict(_shown: &CloseLosses, _now: &CloseLosses) -> ConfirmVerdict {
    // STUB (red).
    ConfirmVerdict::Exit
}

// ---------------------------------------------------------------------------
// Bringing the window back
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestorePlan {
    /// There is no window to show: keep the tray, the only way back.
    KeepTray,
    /// Show the window first; remove the tray only once it is up.
    ShowThenRemove,
}

pub fn restore_plan(_window_present: bool) -> RestorePlan {
    // STUB (red).
    RestorePlan::ShowThenRemove
}

// ---------------------------------------------------------------------------
// Asking, and knowing the ask was seen
// ---------------------------------------------------------------------------

/// `emit` returns Ok when no listener is registered, so an event is not proof
/// that anyone saw the dialog. The frontend acknowledges each ask after its
/// modal has mounted; an ask not acknowledged in time goes native. The clock
/// is the caller's — this is only the bookkeeping, and it is pure.
#[derive(Debug, Default)]
pub struct AskState {
    next: u32,
    current: Option<Ask>,
}

#[derive(Debug, Clone, Copy)]
struct Ask {
    generation: u32,
    acked: bool,
    /// The native dialog took over: a late in-app ack must be refused.
    native: bool,
}

impl AskState {
    /// Open a new ask. Any earlier one is superseded.
    pub fn begin(&mut self) -> u32 {
        // STUB (red).
        0
    }

    /// The frontend's "my dialog is up". True only for the current ask, and
    /// only while the native dialog has not taken over.
    pub fn ack(&mut self, _generation: u32) -> bool {
        // STUB (red).
        true
    }

    /// Called when the acknowledgement budget runs out. True = the in-app
    /// dialog was never confirmed on screen, so ask natively (and refuse any
    /// late ack from now on).
    pub fn expire(&mut self, _generation: u32) -> bool {
        // STUB (red).
        false
    }

    /// The ask was answered; forget it.
    pub fn finish(&mut self, _generation: u32) {}

    /// Whether any ask is open — the scheduled hide-to-tray must not hide an
    /// open question along with the window.
    pub fn pending(&self) -> bool {
        // STUB (red).
        false
    }
}

// ---------------------------------------------------------------------------
// The native fallback's words
// ---------------------------------------------------------------------------

/// Pre-translated strings for the native dialog, sent by the frontend (which
/// owns the language) alongside the tray labels. Plural-free: Rust does not
/// run ICU, so the frontend sends the one-server and many-servers lines.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, specta::Type)]
pub struct CloseLabels {
    pub title: String,
    pub close_everything: String,
    pub close_lucerna: String,
    pub cancel: String,
    pub games: String,
    pub servers_one: String,
    pub servers_many: String,
    pub operation: String,
    pub unchecked: String,
}

/// The English set: what the native dialog says before the frontend has spoken.
pub fn default_close_labels() -> CloseLabels {
    CloseLabels {
        title: "Close Lucerna?".into(),
        close_everything: "Close everything".into(),
        close_lucerna: "Close Lucerna".into(),
        cancel: "Cancel".into(),
        games: "Minecraft is still running. Closing Lucerna forces it to close — unsaved progress may be lost.".into(),
        servers_one: "A server is still running. Closing Lucerna forces it to stop — recent world changes may be lost.".into(),
        servers_many: "Servers are still running. Closing Lucerna forces them to stop — recent world changes may be lost.".into(),
        operation: "An operation is still running, such as an update, a backup, an upload or a translation. Closing Lucerna interrupts it.".into(),
        unchecked: "Lucerna couldn't check whether a server from an earlier session is still running. If one is, it keeps running after Lucerna closes.".into(),
    }
}

/// The body (one line per set kind) and the two button labels.
///
/// The confirm reads "Close everything" when closing kills something, and
/// "Close Lucerna" when only `unchecked` is set — nothing is being killed then.
///
/// The native dialog maps its answer back by comparing labels, so the two
/// buttons must be distinct and non-empty; a bad pair falls back to English
/// rather than producing a dialog whose answer cannot be read.
pub fn native_dialog(_losses: &CloseLosses, _labels: &CloseLabels) -> (String, String, String) {
    // STUB (red).
    (String::new(), String::new(), String::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l(games: bool, servers: u32, operation: bool, unchecked: bool) -> CloseLosses {
        CloseLosses {
            games,
            servers,
            operation,
            unchecked,
        }
    }

    // --- gate ---

    #[test]
    fn a_copying_move_prevents_the_close() {
        use crate::data_root::state::MovePhase;
        let s = RelocationStatus::Running {
            phase: MovePhase::Copying,
        };
        assert_eq!(close_gate(&s), CloseGate::Prevent);
    }

    #[test]
    fn a_move_waiting_for_restart_lets_the_close_through() {
        let s = RelocationStatus::RestartRequired {
            old_root: "a".into(),
            new_root: "b".into(),
            leftovers: vec![],
            old_root_intact: true,
            old_root_is_default: false,
            retry_possible: false,
        };
        assert_eq!(close_gate(&s), CloseGate::Allow);
    }

    #[test]
    fn no_move_means_check_what_closing_would_lose() {
        assert_eq!(close_gate(&RelocationStatus::Idle), CloseGate::Check);
    }

    // --- verdict ---

    #[test]
    fn nothing_to_lose_exits_without_asking() {
        assert_eq!(close_verdict(&CloseLosses::default()), CloseVerdict::Exit);
    }

    #[test]
    fn any_loss_asks() {
        for losses in [
            l(true, 0, false, false),
            l(false, 2, false, false),
            l(false, 0, true, false),
        ] {
            assert_eq!(close_verdict(&losses), CloseVerdict::Ask(losses));
        }
    }

    #[test]
    fn could_not_check_asks_too() {
        // The fallback direction: "could not tell" never exits unasked.
        let u = CloseLosses::unchecked();
        assert_eq!(close_verdict(&u), CloseVerdict::Ask(u));
    }

    // --- confirm ---

    #[test]
    fn a_confirm_that_names_everything_exits() {
        let shown = l(true, 2, false, false);
        assert_eq!(
            confirm_verdict(&shown, &l(true, 1, false, false)),
            ConfirmVerdict::Exit
        );
    }

    #[test]
    fn a_game_started_while_the_dialog_was_open_is_named_first() {
        let shown = l(false, 1, false, false);
        let now = l(true, 1, false, false);
        assert_eq!(
            confirm_verdict(&shown, &now),
            ConfirmVerdict::Reprompt(l(true, 1, false, false))
        );
    }

    #[test]
    fn a_third_server_is_the_same_loss_the_user_agreed_to() {
        // Kinds, not counts.
        assert_eq!(
            confirm_verdict(&l(false, 2, false, false), &l(false, 3, false, false)),
            ConfirmVerdict::Exit
        );
    }

    #[test]
    fn a_first_server_is_a_new_kind() {
        let shown = l(true, 0, false, false);
        let now = l(true, 2, false, false);
        assert_eq!(
            confirm_verdict(&shown, &now),
            ConfirmVerdict::Reprompt(l(true, 2, false, false))
        );
    }

    #[test]
    fn changing_state_cannot_trap_the_user() {
        // Worst case: a new kind appears on every confirm. The union only
        // grows, so it settles within four re-prompts.
        let mut shown = CloseLosses::default();
        let sequence = [
            l(true, 0, false, false),
            l(false, 1, false, false),
            l(false, 0, true, false),
            l(false, 0, false, true),
            l(true, 1, true, true),
            l(true, 5, true, true),
        ];
        let mut reprompts = 0;
        for now in sequence {
            match confirm_verdict(&shown, &now) {
                ConfirmVerdict::Exit => break,
                ConfirmVerdict::Reprompt(next) => {
                    reprompts += 1;
                    shown = next;
                }
            }
        }
        assert!(reprompts <= 4, "{reprompts} re-prompts");
        assert_eq!(
            confirm_verdict(&shown, &l(true, 9, true, true)),
            ConfirmVerdict::Exit
        );
    }

    // --- restore ---

    #[test]
    fn with_no_window_the_tray_is_kept() {
        assert_eq!(restore_plan(false), RestorePlan::KeepTray);
    }

    #[test]
    fn with_a_window_it_is_shown_before_the_tray_goes() {
        assert_eq!(restore_plan(true), RestorePlan::ShowThenRemove);
    }

    // --- ask bookkeeping ---

    #[test]
    fn an_ask_acknowledged_in_time_does_not_go_native() {
        let mut a = AskState::default();
        let g = a.begin();
        assert!(a.pending());
        assert!(a.ack(g));
        assert!(!a.expire(g));
    }

    #[test]
    fn an_ask_never_acknowledged_goes_native() {
        let mut a = AskState::default();
        let g = a.begin();
        assert!(a.expire(g));
    }

    #[test]
    fn a_late_ack_after_the_native_dialog_took_over_is_refused() {
        let mut a = AskState::default();
        let g = a.begin();
        assert!(a.expire(g));
        assert!(!a.ack(g));
    }

    #[test]
    fn a_new_ask_supersedes_the_old_one() {
        let mut a = AskState::default();
        let first = a.begin();
        let second = a.begin();
        assert_ne!(first, second);
        assert!(!a.ack(first));
        assert!(a.ack(second));
    }

    #[test]
    fn a_finished_ask_is_no_longer_pending() {
        let mut a = AskState::default();
        let g = a.begin();
        a.finish(g);
        assert!(!a.pending());
    }

    // --- native words ---

    #[test]
    fn the_native_body_names_each_kind_on_its_own_line() {
        let labels = default_close_labels();
        let (body, confirm, cancel) = native_dialog(&l(true, 2, true, false), &labels);
        assert!(body.contains(&labels.games));
        assert!(body.contains(&labels.servers_many));
        assert!(body.contains(&labels.operation));
        assert!(!body.contains(&labels.unchecked));
        assert_eq!(confirm, labels.close_everything);
        assert_eq!(cancel, labels.cancel);
    }

    #[test]
    fn one_server_reads_in_the_singular() {
        let labels = default_close_labels();
        let (body, _, _) = native_dialog(&l(false, 1, false, false), &labels);
        assert!(body.contains(&labels.servers_one));
        assert!(!body.contains(&labels.servers_many));
    }

    #[test]
    fn only_unchecked_says_close_lucerna_not_close_everything() {
        let labels = default_close_labels();
        let (_, confirm, _) = native_dialog(&CloseLosses::unchecked(), &labels);
        assert_eq!(confirm, labels.close_lucerna);
    }

    #[test]
    fn an_unreadable_button_pair_falls_back_to_english() {
        let mut labels = default_close_labels();
        labels.close_everything = "Same".into();
        labels.cancel = "Same".into();
        let (_, confirm, cancel) = native_dialog(&l(true, 0, false, false), &labels);
        assert_ne!(confirm, cancel);
        assert_eq!(confirm, "Close everything");
        assert_eq!(cancel, "Cancel");
    }
}
