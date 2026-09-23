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
use crate::error::{Error, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tauri_specta::Event;

/// The backend asks the frontend to show the close dialog. `generation`
/// identifies this ask: the frontend acknowledges it once its modal is up, and
/// a stale acknowledgement for an older ask is refused.
#[derive(Debug, Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct CloseConfirmNeeded {
    pub generation: u32,
    pub losses: CloseLosses,
}

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
pub fn close_gate(status: &RelocationStatus) -> CloseGate {
    match status {
        RelocationStatus::Running { .. } => CloseGate::Prevent,
        RelocationStatus::RestartRequired { .. } => CloseGate::Allow,
        RelocationStatus::Idle => CloseGate::Check,
    }
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
pub fn close_verdict(losses: &CloseLosses) -> CloseVerdict {
    if losses.is_clear() {
        CloseVerdict::Exit
    } else {
        CloseVerdict::Ask(*losses)
    }
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
pub fn confirm_verdict(shown: &CloseLosses, now: &CloseLosses) -> ConfirmVerdict {
    let new_kind = (now.games && !shown.games)
        || (now.servers > 0 && shown.servers == 0)
        || (now.operation && !shown.operation)
        || (now.unchecked && !shown.unchecked);
    if new_kind {
        ConfirmVerdict::Reprompt(union(shown, now))
    } else {
        ConfirmVerdict::Exit
    }
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

pub fn restore_plan(window_present: bool) -> RestorePlan {
    if window_present {
        RestorePlan::ShowThenRemove
    } else {
        RestorePlan::KeepTray
    }
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
        self.next = self.next.wrapping_add(1);
        let generation = self.next;
        self.current = Some(Ask {
            generation,
            acked: false,
            native: false,
        });
        generation
    }

    /// The frontend's "my dialog is up". True only for the current ask, and
    /// only while the native dialog has not taken over.
    pub fn ack(&mut self, generation: u32) -> bool {
        match &mut self.current {
            Some(ask) if ask.generation == generation && !ask.native => {
                ask.acked = true;
                true
            }
            _ => false,
        }
    }

    /// Called when the acknowledgement budget runs out. True = the in-app
    /// dialog was never confirmed on screen, so ask natively (and refuse any
    /// late ack from now on).
    pub fn expire(&mut self, generation: u32) -> bool {
        match &mut self.current {
            Some(ask) if ask.generation == generation && !ask.acked => {
                ask.native = true;
                true
            }
            _ => false,
        }
    }

    /// The ask was answered; forget it. An older generation is ignored, so a
    /// late answer to a superseded ask cannot clear the current one.
    pub fn finish(&mut self, generation: u32) {
        if self.current.is_some_and(|ask| ask.generation == generation) {
            self.current = None;
        }
    }

    /// Whether any ask is open — the game-start window action (minimise or hide
    /// to tray) must not take an open question away with the window.
    pub fn pending(&self) -> bool {
        self.current.is_some()
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
/// rather than producing a dialog whose answer cannot be read. A blank line
/// (a partial translation) speaks English too: a named loss must never become
/// an empty paragraph.
pub fn native_dialog(losses: &CloseLosses, labels: &CloseLabels) -> (String, String, String) {
    let english = default_close_labels();
    let line = |translated: &str, fallback: &str| -> String {
        if translated.trim().is_empty() {
            fallback.to_owned()
        } else {
            translated.to_owned()
        }
    };
    let mut lines: Vec<String> = Vec::new();
    if losses.games {
        lines.push(line(&labels.games, &english.games));
    }
    match losses.servers {
        0 => {}
        1 => lines.push(line(&labels.servers_one, &english.servers_one)),
        _ => lines.push(line(&labels.servers_many, &english.servers_many)),
    }
    if losses.operation {
        lines.push(line(&labels.operation, &english.operation));
    }
    if losses.unchecked {
        lines.push(line(&labels.unchecked, &english.unchecked));
    }
    let body = lines.join("\n\n");

    let kills = losses.games || losses.servers > 0 || losses.operation;
    let pick = |l: &CloseLabels| {
        if kills {
            l.close_everything.clone()
        } else {
            l.close_lucerna.clone()
        }
    };
    let confirm = pick(labels);
    let readable = !confirm.is_empty() && !labels.cancel.is_empty() && confirm != labels.cancel;
    if readable {
        (body, confirm, labels.cancel.clone())
    } else {
        (body, pick(&english), english.cancel)
    }
}

// ---------------------------------------------------------------------------
// The impure flow: the window's ×, the tray's Quit, and the two answers
// ---------------------------------------------------------------------------

/// How long the close waits for "what would be lost" before it asks anyway.
/// The probe reads PID files under the data root, which can sit on a dead
/// network share; past this the answer is `unchecked`, never "nothing".
const OBSERVE_BUDGET: Duration = Duration::from_secs(5);

/// How long the in-app dialog has to say it is on screen before the native
/// dialog asks instead. `emit` succeeding proves nothing: it returns Ok with no
/// listener at all (a crashed or still-loading page).
const ACK_BUDGET: Duration = Duration::from_secs(3);

/// One close check at a time, shared by the × and the tray's Quit. A second
/// press while one is in flight is dropped: the close stays prevented and the
/// running check answers.
static CLOSE_CHECK: AtomicBool = AtomicBool::new(false);

/// Holding this is holding the single-flight flag. Dropped on every path —
/// return, early exit or unwind — so a check that fails cannot leave the app
/// unable to close.
struct CheckGuard;

impl Drop for CheckGuard {
    fn drop(&mut self) {
        CLOSE_CHECK.store(false, Ordering::Release);
    }
}

fn try_begin_check() -> Option<CheckGuard> {
    CLOSE_CHECK
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .ok()
        .map(|_| CheckGuard)
}

static ASK: Mutex<AskState> = Mutex::new(AskState {
    next: 0,
    current: None,
});

fn asks() -> MutexGuard<'static, AskState> {
    // Two plain fields with no invariant spanning a panic: a poisoned lock is
    // read through rather than turned into a second panic on the path that
    // decides whether the app may close.
    ASK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Whether a close question is open. The game-start window action (minimise or
/// hide to tray) checks this so it does not take an open question with it.
pub fn ask_pending() -> bool {
    asks().pending()
}

// Process-wide; `close_set_labels` writes it whenever the page loads or the
// language changes. Absent → the English set, never blank.
static LABELS: Mutex<Option<CloseLabels>> = Mutex::new(None);

pub fn set_labels(labels: CloseLabels) {
    *LABELS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(labels);
}

fn labels_or_default() -> CloseLabels {
    LABELS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .unwrap_or_else(default_close_labels)
}

fn current_gate() -> CloseGate {
    close_gate(&crate::data_root::state::global().status())
}

/// The last step of every path that ends the process: the gate again, because
/// a data move can start while a dialog is open, and `app.exit` never passes
/// through the window's close handler.
fn exit_if_gate_allows(app: &AppHandle, why: &str) -> Result<()> {
    match current_gate() {
        CloseGate::Prevent => {
            crate::diag!("close: not exiting ({why}) — a data move started meanwhile");
            Err(Error::DataRelocationInProgress {
                restart_required: false,
            })
        }
        CloseGate::Allow | CloseGate::Check => {
            app.exit(0);
            Ok(())
        }
    }
}

/// `observe_losses`, bounded in time and panic-safe. A poisoned registry lock
/// panics inside the blocking task; that becomes a join error, which becomes
/// `unchecked` — an ask, not a crash and not a silent exit.
async fn observe_budgeted(app: &AppHandle) -> CloseLosses {
    let probe_app = app.clone();
    let probe = tauri::async_runtime::spawn_blocking(move || {
        crate::data_root::blockers::observe_losses(&probe_app)
    });
    match tokio::time::timeout(OBSERVE_BUDGET, probe).await {
        Ok(Ok(losses)) => losses,
        Ok(Err(e)) => {
            crate::diag!("close: the check failed ({e}) — asking as unchecked");
            CloseLosses::unchecked()
        }
        Err(_) => {
            crate::diag!(
                "close: the check took longer than {OBSERVE_BUDGET:?} — asking as unchecked"
            );
            CloseLosses::unchecked()
        }
    }
}

/// The window's close handler (×, Alt+F4, taskbar Close). Runs on the main
/// thread, and `prevent_close()` counts only if sent before it returns — so the
/// decision to hold the close is made here, synchronously, and everything slow
/// happens afterwards on a task that never sees `api`.
pub fn on_close_requested(window: &tauri::Window, api: &tauri::CloseRequestApi) {
    match current_gate() {
        CloseGate::Prevent => api.prevent_close(),
        CloseGate::Allow => {}
        CloseGate::Check => {
            api.prevent_close();
            let Some(guard) = try_begin_check() else {
                return;
            };
            let app = window.app_handle().clone();
            tauri::async_runtime::spawn(async move {
                let _guard = guard;
                let losses = observe_budgeted(&app).await;
                match close_verdict(&losses) {
                    CloseVerdict::Exit => {
                        if let Err(e) = exit_if_gate_allows(&app, "window close") {
                            crate::diag!("close: {e}");
                        }
                    }
                    CloseVerdict::Ask(losses) => ask(&app, losses).await,
                }
            });
        }
    }
}

/// Bring the window forward, show the question, and make sure SOMEONE saw it:
/// the in-app dialog if it says so within `ACK_BUDGET`, the native dialog
/// otherwise.
async fn ask(app: &AppHandle, losses: CloseLosses) {
    // A dialog painted in a minimized or hidden window is invisible. Posted to
    // the same FIFO as the emit below, so the window is up before the dialog.
    // The question is registered FIRST: a game-start minimise or hide queued on
    // the main thread checks `ask_pending()`, and must see this question even if
    // it runs between the restore and the emit.
    let generation = asks().begin();
    let restore_app = app.clone();
    if let Err(e) = app.run_on_main_thread(move || {
        crate::tray::restore_or_log(&restore_app, "close ask");
    }) {
        // No event loop means the process is already on its way out.
        crate::diag!("close: cannot ask — the event loop is gone: {e}");
        asks().finish(generation);
        return;
    }
    if let Err(e) = (CloseConfirmNeeded { generation, losses }).emit(app) {
        crate::diag!("close: the in-app ask was not sent ({e}) — asking natively");
    } else {
        tokio::time::sleep(ACK_BUDGET).await;
    }
    if asks().expire(generation) {
        crate::diag!("close: the in-app ask was not confirmed on screen — asking natively");
        native_ask(app, generation, losses);
    }
}

/// The same question through the OS, for when the page cannot show it.
/// `show(callback)`, never `blocking_show`: the latter unwraps a receive that
/// fails when the post is dropped.
fn native_ask(app: &AppHandle, generation: u32, losses: CloseLosses) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

    let labels = labels_or_default();
    let (body, confirm_label, cancel_label) = native_dialog(&losses, &labels);
    let title = if labels.title.is_empty() {
        default_close_labels().title
    } else {
        labels.title
    };
    let mut dialog = app
        .dialog()
        .message(body)
        .title(title)
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            confirm_label,
            cancel_label,
        ));
    if let Some(window) = app.get_webview_window("main") {
        dialog = dialog.parent(&window);
    }
    let answer_app = app.clone();
    dialog.show(move |confirmed| {
        if !confirmed {
            asks().finish(generation);
            return;
        }
        tauri::async_runtime::spawn(async move {
            if let Err(e) = confirm(&answer_app, generation, losses).await {
                crate::diag!("close: the native confirm did not close: {e}");
            }
        });
    });
}

/// The user answered Cancel: the question is over.
pub fn cancel(generation: u32) {
    asks().finish(generation);
}

/// The in-app dialog is on screen. False = superseded, or the native dialog
/// already took over; the page closes its copy.
pub fn ask_shown(generation: u32) -> bool {
    asks().ack(generation)
}

/// The user chose to close, having seen `shown`. Both dialogs end here.
///
/// The world may have changed while the dialog was open, so this looks again:
/// it exits only if every kind of loss seen now was named; otherwise it asks
/// again, naming the union.
pub async fn confirm(app: &AppHandle, generation: u32, shown: CloseLosses) -> Result<()> {
    match current_gate() {
        CloseGate::Prevent => {
            asks().finish(generation);
            return Err(Error::DataRelocationInProgress {
                restart_required: false,
            });
        }
        CloseGate::Allow => {
            asks().finish(generation);
            return exit_if_gate_allows(app, "confirmed close");
        }
        CloseGate::Check => {}
    }
    let now = observe_budgeted(app).await;
    match confirm_verdict(&shown, &now) {
        ConfirmVerdict::Exit => {
            asks().finish(generation);
            exit_if_gate_allows(app, "confirmed close")
        }
        ConfirmVerdict::Reprompt(union) => {
            // Its own task: the caller's promise settles now, and the new ask
            // replaces the dialog the caller is showing.
            let reask_app = app.clone();
            tauri::async_runtime::spawn(async move { ask(&reask_app, union).await });
            Ok(())
        }
    }
}

/// The tray's Quit. The window is hidden, so there is nothing to ask on: it
/// keeps REFUSING when something runs (the maintainer's call in 11b), but now
/// passes the same gate and the same single-flight flag as the ×, and looks
/// off the main thread.
pub fn tray_quit(app: &AppHandle) {
    use crate::data_root::blockers::RestartBlock;
    match current_gate() {
        // A data move is an operation in flight; the move's own dialog is
        // what the restored window shows.
        CloseGate::Prevent => crate::tray::refuse_quit(app, RestartBlock::Busy),
        CloseGate::Allow => {
            if let Err(e) = exit_if_gate_allows(app, "tray quit") {
                crate::diag!("close: {e}");
                crate::tray::refuse_quit(app, RestartBlock::Busy);
            }
        }
        CloseGate::Check => {
            let Some(guard) = try_begin_check() else {
                return;
            };
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let _guard = guard;
                let block = observe_block_budgeted(&app).await;
                match crate::tray::quit_verdict(block) {
                    crate::tray::QuitVerdict::Exit => {
                        if let Err(e) = exit_if_gate_allows(&app, "tray quit") {
                            crate::diag!("close: {e}");
                            crate::tray::refuse_quit(&app, RestartBlock::Busy);
                        }
                    }
                    crate::tray::QuitVerdict::Refuse(block) => {
                        crate::tray::refuse_quit(&app, block)
                    }
                }
            });
        }
    }
}

/// `observe()` under the same budget; a panic or a timeout is `Unknown`, which
/// refuses.
async fn observe_block_budgeted(app: &AppHandle) -> crate::data_root::blockers::RestartBlock {
    use crate::data_root::blockers::RestartBlock;
    let probe_app = app.clone();
    let probe = tauri::async_runtime::spawn_blocking(move || {
        crate::data_root::blockers::observe(&probe_app)
    });
    match tokio::time::timeout(OBSERVE_BUDGET, probe).await {
        Ok(Ok(block)) => block,
        Ok(Err(e)) => {
            crate::diag!("tray: the quit check failed ({e}) — refusing");
            RestartBlock::Unknown
        }
        Err(_) => {
            crate::diag!("tray: the quit check took longer than {OBSERVE_BUDGET:?} — refusing");
            RestartBlock::Unknown
        }
    }
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
    fn a_blank_translated_line_speaks_english_rather_than_saying_nothing() {
        // A partial translation must not turn a named loss into an empty
        // paragraph: the user would confirm without being told what stops.
        let mut labels = default_close_labels();
        labels.games = String::new();
        labels.servers_many = "   ".into();
        let english = default_close_labels();
        let (body, _, _) = native_dialog(&l(true, 2, false, false), &labels);
        assert!(body.contains(&english.games), "{body}");
        assert!(body.contains(&english.servers_many), "{body}");
    }

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
