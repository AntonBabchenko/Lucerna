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

use crate::error::{Error, Result};
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager,
};

/// Stable id for the single launcher tray icon. Used both to build it and
/// to look it up / remove it through the `AppHandle`.
const TRAY_ID: &str = "lucerna-tray";

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
