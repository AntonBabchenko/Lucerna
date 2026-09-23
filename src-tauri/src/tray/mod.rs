//! Lifecycle helpers for the system-tray icon that appears while the
//! launcher window is hidden during a Minecraft session. Created on
//! `hide_to_tray`, removed on `restore_from_tray`. The icon is NOT
//! always-on — see the 2026-05-26 tray-minimize design spec.
//!
//! Ownership note: Tauri itself is the source of truth for the tray.
//! `TrayIconBuilder::build` registers the icon as a resource inside the
//! `AppHandle` (keyed by its id), and the app keeps that reference for
//! the icon's whole lifetime. Dropping a `TrayIcon` handle we hold does
//! NOT remove the icon from the system tray — only `TrayIcon::close()`
//! does, which is what `AppHandle::remove_tray_by_id` calls. So we must
//! never track the icon in our own slot and drop it; we look it up and
//! remove it through the app by id instead. (A previous implementation
//! stored the icon in a `OnceLock` and dropped it on restore, which left
//! the OS icon painted in the tray and stacked a fresh one every launch.)

use crate::data_root::blockers::RestartBlock;
use crate::error::{Error, Result};
use std::sync::Mutex;
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager,
};

/// Stable id for the single launcher tray icon. Used both to build it and
/// to look it up / remove it through the `AppHandle`.
const TRAY_ID: &str = "lucerna-tray";

/// The menu strings the tray is built with. English constants until the
/// frontend — which owns the locale — sends the translated set.
///
/// The tray exists ONLY while the window is hidden (`hide_to_tray` hides then
/// builds; `restore_from_tray` removes then shows), and the language can only
/// be changed from the window. So a language change while a tray is live is
/// unreachable, and storing the labels for the next build is enough — there is
/// no live-relabel path to write and never exercise.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, specta::Type)]
pub struct TrayLabels {
    pub open: String,
    pub quit: String,
    pub tooltip_running: String,
}

const DEFAULT_OPEN: &str = "Open Launcher";
const DEFAULT_QUIT: &str = "Quit";
const DEFAULT_TOOLTIP: &str = "Lucerna — Minecraft running";

static LABELS: Mutex<Option<TrayLabels>> = Mutex::new(None);

/// Store the translated labels for the next tray build.
pub fn set_labels(_labels: TrayLabels) {
    // STUB (red): stores nothing yet.
}

/// The stored labels, or the English set the tray has always shipped with.
/// A build that never heard from the frontend is unchanged, not blank — the
/// restrictive direction for a fallback whose input may simply be absent.
pub fn labels_or_default() -> TrayLabels {
    match LABELS.lock() {
        Ok(guard) => guard.clone().unwrap_or_else(default_labels),
        // A poisoned lock means a panic while holding it; the labels are
        // plain strings and cannot be half-written, so the defaults are a
        // safe read rather than a reason to refuse building a tray at all.
        Err(poisoned) => poisoned.into_inner().clone().unwrap_or_else(default_labels),
    }
}

fn default_labels() -> TrayLabels {
    TrayLabels {
        open: DEFAULT_OPEN.into(),
        quit: DEFAULT_QUIT.into(),
        tooltip_running: DEFAULT_TOOLTIP.into(),
    }
}

/// What the tray's Quit item should do, given what the launcher observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuitVerdict {
    /// Nothing is running: exiting is safe.
    Exit,
    /// Something is running, or could not be checked — do not exit, say why.
    Refuse(RestartBlock),
}

/// Quitting from the tray ends the launcher process, and the exit hook
/// force-kills every tracked game and server with it. Decision 1 settled this
/// shape for "Update now" — refuse and name what is running — and this is the
/// other path that can end the process, so it answers the same way.
///
/// `Unknown` refuses too: "could not tell" is not "nothing is running".
pub fn quit_verdict(_block: RestartBlock) -> QuitVerdict {
    // STUB (red): always exits, which is today's behaviour.
    QuitVerdict::Exit
}

/// Hide the main window and create the tray icon. Idempotent — if the
/// tray already exists (e.g. a previous session's restore failed), the
/// existing icon is reused and the window just hides again.
pub fn hide_to_tray(app: &AppHandle) -> Result<()> {
    if let Some(window) = app.get_webview_window("main") {
        window.hide().map_err(|e| Error::TrayIo {
            details: format!("hide window: {e}"),
        })?;
    }

    // The app (not us) owns the tray. If it already has one under our id,
    // don't build a second — that would put two icons in the tray.
    if app.tray_by_id(TRAY_ID).is_some() {
        return Ok(());
    }

    let open = MenuItemBuilder::with_id("tray-open", "Open Launcher")
        .build(app)
        .map_err(|e| Error::TrayIo {
            details: format!("menu open: {e}"),
        })?;
    let quit = MenuItemBuilder::with_id("tray-quit", "Quit")
        .build(app)
        .map_err(|e| Error::TrayIo {
            details: format!("menu quit: {e}"),
        })?;
    let menu = MenuBuilder::new(app)
        .items(&[&open, &quit])
        .build()
        .map_err(|e| Error::TrayIo {
            details: format!("menu build: {e}"),
        })?;

    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| Error::TrayIo {
            details: "no default window icon to use for tray".into(),
        })?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("Lucerna — Minecraft running")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "tray-open" => {
                let _ = restore_from_tray(app);
            }
            "tray-quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // Left-click up (pressed) = restore window.
            // Note: in Tauri 2's tray API, MouseButtonState::Up means the
            // button was pressed (actioned), not released.
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = restore_from_tray(tray.app_handle());
            }
        })
        .build(app)
        .map_err(|e| Error::TrayIo {
            details: format!("tray build: {e}"),
        })?;

    Ok(())
}

/// Remove the tray icon and bring the main window back to the front.
/// No-op if no tray icon exists.
///
/// Removal goes through `AppHandle::remove_tray_by_id`, which calls
/// `TrayIcon::close()` — the only thing that actually tears the icon out
/// of the system tray. We loop so that any duplicates left over from an
/// earlier buggy run (which could stack multiple icons under the same id)
/// are all cleared in one restore.
pub fn restore_from_tray(app: &AppHandle) -> Result<()> {
    while app.remove_tray_by_id(TRAY_ID).is_some() {}

    if let Some(window) = app.get_webview_window("main") {
        window.show().map_err(|e| Error::TrayIo {
            details: format!("show window: {e}"),
        })?;
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(tag: &str) -> TrayLabels {
        TrayLabels {
            open: format!("open-{tag}"),
            quit: format!("quit-{tag}"),
            tooltip_running: format!("tip-{tag}"),
        }
    }

    #[test]
    fn stored_labels_are_what_the_next_tray_is_built_from() {
        set_labels(labels("ru"));
        assert_eq!(labels_or_default(), labels("ru"));
    }

    #[test]
    fn a_tray_that_never_heard_from_the_frontend_keeps_the_english_set() {
        // Not "" and not a panic: the tray it has always shipped.
        let d = default_labels();
        assert_eq!(d.open, "Open Launcher");
        assert_eq!(d.quit, "Quit");
        assert_eq!(d.tooltip_running, "Lucerna — Minecraft running");
    }

    #[test]
    fn quit_exits_only_when_nothing_is_running() {
        assert_eq!(quit_verdict(RestartBlock::None), QuitVerdict::Exit);
    }

    #[test]
    fn quit_refuses_while_a_game_or_server_runs() {
        assert_eq!(
            quit_verdict(RestartBlock::Running),
            QuitVerdict::Refuse(RestartBlock::Running)
        );
    }

    #[test]
    fn quit_refuses_while_a_long_operation_holds_a_claim() {
        assert_eq!(
            quit_verdict(RestartBlock::Busy),
            QuitVerdict::Refuse(RestartBlock::Busy)
        );
    }

    #[test]
    fn quit_refuses_when_it_could_not_be_checked() {
        // The fallback direction. A verdict that only covered Running would
        // let "could not tell" kill the user's game.
        assert_eq!(
            quit_verdict(RestartBlock::Unknown),
            QuitVerdict::Refuse(RestartBlock::Unknown)
        );
    }
}
