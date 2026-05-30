//! Lifecycle helpers for the system-tray icon that appears while the
//! launcher window is hidden during a Minecraft session. Created on
//! `hide_to_tray`, destroyed on `restore_from_tray`. The icon is NOT
//! always-on — see the 2026-05-26 tray-minimize design spec.

use crate::error::{Error, Result};
use std::sync::{Mutex, OnceLock};
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager,
};

static TRAY: OnceLock<Mutex<Option<TrayIcon>>> = OnceLock::new();

fn tray_slot() -> &'static Mutex<Option<TrayIcon>> {
    TRAY.get_or_init(|| Mutex::new(None))
}

/// Hide the main window and create a tray icon. Idempotent — if the
/// tray already exists (e.g. previous session's restore failed), the
/// existing icon is reused and the window just hides again.
pub fn hide_to_tray(app: &AppHandle) -> Result<()> {
    if let Some(window) = app.get_webview_window("main") {
        window.hide().map_err(|e| Error::TrayIo {
            details: format!("hide window: {e}"),
        })?;
    }

    let mut slot = tray_slot().lock().expect("tray slot mutex poisoned");
    if slot.is_some() {
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

    let tray = TrayIconBuilder::with_id("lucerna-tray")
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

    *slot = Some(tray);
    Ok(())
}

/// Remove the tray icon and bring the main window back to the front.
/// No-op if no tray icon exists.
pub fn restore_from_tray(app: &AppHandle) -> Result<()> {
    let mut slot = tray_slot().lock().expect("tray slot mutex poisoned");
    if let Some(tray) = slot.take() {
        drop(tray); // dropping TrayIcon removes it from the system tray
    }

    if let Some(window) = app.get_webview_window("main") {
        window.show().map_err(|e| Error::TrayIo {
            details: format!("show window: {e}"),
        })?;
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    Ok(())
}
