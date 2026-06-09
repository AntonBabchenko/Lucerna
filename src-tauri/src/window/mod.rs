//! Physical resize of the main launcher window between the full layout and
//! the compact "mini launch-pad" strip. The CSS-side collapse of the right
//! column lives in the frontend; this module only drives the OS window size.
//!
//! Width-only: compact shrinks the window to the sidebar strip and keeps the
//! current height. The pre-compact width is remembered in `WindowSizeState`
//! so expanding restores whatever width the user had (falling back to the
//! default expanded width on a fresh start-in-compact session).

use crate::error::{Error, Result};
use std::sync::Mutex;
use tauri::{AppHandle, LogicalSize, Manager};

/// Target width of the compact strip (logical px). Wide enough for the 240px
/// sidebar content plus window chrome.
const COMPACT_WIDTH: f64 = 280.0;
/// Minimum width allowed while compact — lets the window reach COMPACT_WIDTH.
const COMPACT_MIN_WIDTH: f64 = 260.0;
/// Minimum width restored when expanded (matches tauri.conf.json).
const EXPANDED_MIN_WIDTH: f64 = 820.0;
/// Minimum height — unchanged in both modes (matches tauri.conf.json).
const MIN_HEIGHT: f64 = 520.0;
/// Fallback expanded width when no pre-compact width was captured
/// (e.g. the app started directly in compact mode).
const DEFAULT_EXPANDED_WIDTH: f64 = 820.0;

/// Remembers the window width captured just before entering compact mode, so
/// expanding can restore it. Registered as Tauri managed state in `lib.rs`.
#[derive(Default)]
pub struct WindowSizeState {
    expanded_width: Mutex<Option<f64>>,
}

/// Resize the `main` window to/from the compact strip. No-op if the window is
/// absent. Width-only; the current height is preserved.
pub fn set_compact(app: &AppHandle, compact: bool, state: &WindowSizeState) -> Result<()> {
    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };

    let scale = window.scale_factor().map_err(|e| Error::WindowIo {
        details: format!("scale_factor: {e}"),
    })?;
    let phys = window.inner_size().map_err(|e| Error::WindowIo {
        details: format!("inner_size: {e}"),
    })?;
    let current_width = phys.width as f64 / scale;
    let current_height = phys.height as f64 / scale;

    if compact {
        // Capture the expanded width to restore later. Guard against storing a
        // width that is already ~compact (e.g. a double "go compact"), which
        // would otherwise overwrite the real expanded width with the strip width.
        if current_width > COMPACT_WIDTH + 1.0 {
            // lock only wraps a trivial Option write; poisoning is unreachable.
            *state
                .expanded_width
                .lock()
                .expect("WindowSizeState mutex poisoned") = Some(current_width);
        }
        window
            .set_min_size(Some(LogicalSize::new(COMPACT_MIN_WIDTH, MIN_HEIGHT)))
            .map_err(|e| Error::WindowIo {
                details: format!("set_min_size compact: {e}"),
            })?;
        window
            .set_size(LogicalSize::new(COMPACT_WIDTH, current_height))
            .map_err(|e| Error::WindowIo {
                details: format!("set_size compact: {e}"),
            })?;
    } else {
        window
            .set_min_size(Some(LogicalSize::new(EXPANDED_MIN_WIDTH, MIN_HEIGHT)))
            .map_err(|e| Error::WindowIo {
                details: format!("set_min_size expanded: {e}"),
            })?;
        // lock only wraps a trivial Option take; poisoning is unreachable.
        let restored = state
            .expanded_width
            .lock()
            .expect("WindowSizeState mutex poisoned")
            .take()
            .unwrap_or(DEFAULT_EXPANDED_WIDTH);
        window
            .set_size(LogicalSize::new(restored, current_height))
            .map_err(|e| Error::WindowIo {
                details: format!("set_size expanded: {e}"),
            })?;
    }

    Ok(())
}
