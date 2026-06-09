//! Physical resize of the main launcher window between the full layout and
//! the compact "mini launch-pad" strip. The CSS-side collapse of the right
//! column lives in the frontend; this module only drives the OS window size.
//!
//! Compact shrinks the window to the sidebar strip — both width (to the fixed
//! sidebar column) and height (to the measured sidebar content). The sidebar's
//! content height, supplied by the frontend, is also used as the window's
//! minimum height in BOTH modes: the sidebar is the left column's must-show
//! element, so the window should never be shorter than its bottom buttons (the
//! right column scrolls). The pre-compact size is remembered in `WindowSizeState`
//! so expanding restores whatever size the user had (falling back to the default
//! expanded size on a fresh start-in-compact session).
//!
//! Compact mode also locks the window (`set_resizable(false)` +
//! `set_maximizable(false)`): the strip is a fixed launch pad, and the only way
//! back to the full layout is the in-app toggle. Without this, dragging or
//! maximizing the window would stretch the single-column sidebar across the
//! whole screen, since the compact/expanded layout is driven by a CSS state
//! flag rather than the live window size.

use crate::error::{Error, Result};
use std::sync::Mutex;
use tauri::{AppHandle, LogicalSize, Manager};

/// Target width of the compact strip (logical px). Matches the sidebar's
/// fixed grid column (`grid-template-columns: 240px 1fr` in `+page.svelte`),
/// so the sidebar renders pixel-identically in compact and expanded modes
/// instead of stretching wider in the single-column compact layout.
const COMPACT_WIDTH: f64 = 240.0;
/// Minimum width allowed while compact — must be <= COMPACT_WIDTH so the
/// window can actually shrink to the strip.
const COMPACT_MIN_WIDTH: f64 = 220.0;
/// Minimum width restored when expanded (the two-column layout needs the room).
const EXPANDED_MIN_WIDTH: f64 = 820.0;
/// Fallback minimum height when the sidebar content height couldn't be measured
/// (matches tauri.conf.json's initial window).
const FALLBACK_MIN_HEIGHT: f64 = 520.0;
/// Fallback expanded size when none was captured (e.g. the app started directly
/// in compact mode). Matches tauri.conf.json's initial window.
const DEFAULT_EXPANDED_WIDTH: f64 = 820.0;
const DEFAULT_EXPANDED_HEIGHT: f64 = 520.0;

/// Remembers the window size captured just before entering compact mode, so
/// expanding can restore it. Registered as Tauri managed state in `lib.rs`.
#[derive(Default)]
pub struct WindowSizeState {
    /// `(width, height)` in logical px captured before the last compact entry.
    expanded_size: Mutex<Option<(f64, f64)>>,
}

/// Resize the `main` window to/from the compact strip. No-op if the window is
/// absent.
///
/// `content_height` is the sidebar's measured content height (logical px) from
/// the frontend, or `None` when it couldn't be measured. It drives the minimum
/// window height in both modes (so the window can't be shrunk below the
/// sidebar's bottom buttons), and in compact mode it's also the window's target
/// height (so the strip ends at the bottom buttons instead of leaving empty
/// space below).
///
/// Compact mode also locks the window (`set_resizable(false)` +
/// `set_maximizable(false)`): the strip is a fixed launch pad, and the only way
/// back to the full layout is the in-app toggle.
pub fn set_compact(
    app: &AppHandle,
    compact: bool,
    content_height: Option<f64>,
    state: &WindowSizeState,
) -> Result<()> {
    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };

    let scale = window.scale_factor().map_err(|e| Error::WindowIo {
        details: format!("scale_factor: {e}"),
    })?;
    // The sidebar content height is the window's minimum height in both modes;
    // fall back to the config minimum if the frontend couldn't measure it.
    let min_height = content_height
        .filter(|h| *h > 0.0)
        .unwrap_or(FALLBACK_MIN_HEIGHT);

    if compact {
        // A compact strip must never be maximized. If the window is maximized
        // (e.g. the user maximized while expanded, then toggled compact), restore
        // it first so we shrink from the normal windowed geometry — and so the
        // size we remember below is the real expanded size, not the screen size.
        if window.is_maximized().unwrap_or(false) {
            window.unmaximize().map_err(|e| Error::WindowIo {
                details: format!("unmaximize: {e}"),
            })?;
        }

        let phys = window.inner_size().map_err(|e| Error::WindowIo {
            details: format!("inner_size compact: {e}"),
        })?;
        let current_width = phys.width as f64 / scale;
        let current_height = phys.height as f64 / scale;

        // Capture the expanded size to restore later. Guard against storing a
        // width that is already ~compact (e.g. a double "go compact"), which
        // would otherwise overwrite the real expanded size with the strip size.
        if current_width > COMPACT_WIDTH + 1.0 {
            // lock only wraps a trivial Option write; poisoning is unreachable.
            *state
                .expanded_size
                .lock()
                .expect("WindowSizeState mutex poisoned") = Some((current_width, current_height));
        }

        // Shrink the height to the measured sidebar content when provided, so the
        // strip ends at the bottom buttons instead of leaving empty space below.
        let target_height = content_height
            .filter(|h| *h > 0.0)
            .unwrap_or(current_height);

        window
            .set_min_size(Some(LogicalSize::new(COMPACT_MIN_WIDTH, min_height)))
            .map_err(|e| Error::WindowIo {
                details: format!("set_min_size compact: {e}"),
            })?;
        window
            .set_size(LogicalSize::new(COMPACT_WIDTH, target_height))
            .map_err(|e| Error::WindowIo {
                details: format!("set_size compact: {e}"),
            })?;
        // Lock the strip last, after it has reached the target size.
        window.set_resizable(false).map_err(|e| Error::WindowIo {
            details: format!("set_resizable false: {e}"),
        })?;
        window.set_maximizable(false).map_err(|e| Error::WindowIo {
            details: format!("set_maximizable false: {e}"),
        })?;
    } else {
        // Re-enable resizing/maximizing before growing back to the full layout.
        window.set_resizable(true).map_err(|e| Error::WindowIo {
            details: format!("set_resizable true: {e}"),
        })?;
        window.set_maximizable(true).map_err(|e| Error::WindowIo {
            details: format!("set_maximizable true: {e}"),
        })?;

        // Floor the expanded window at the sidebar content height (not the
        // fixed config minimum), so it can be shrunk down to the bottom buttons.
        window
            .set_min_size(Some(LogicalSize::new(EXPANDED_MIN_WIDTH, min_height)))
            .map_err(|e| Error::WindowIo {
                details: format!("set_min_size expanded: {e}"),
            })?;
        // lock only wraps a trivial Option take; poisoning is unreachable.
        let restored = state
            .expanded_size
            .lock()
            .expect("WindowSizeState mutex poisoned")
            .take();
        if let Some((restored_width, restored_height)) = restored {
            window
                .set_size(LogicalSize::new(restored_width, restored_height))
                .map_err(|e| Error::WindowIo {
                    details: format!("set_size expanded: {e}"),
                })?;
        } else {
            // No captured size — this is a startup/expanded call that only needs
            // to apply the min-height floor, not resize. Only force the default
            // size if the window is somehow narrower than the expanded minimum
            // (e.g. a stale compact width from a previous session).
            let phys = window.inner_size().map_err(|e| Error::WindowIo {
                details: format!("inner_size expanded: {e}"),
            })?;
            let current_width = phys.width as f64 / scale;
            if current_width < EXPANDED_MIN_WIDTH - 1.0 {
                window
                    .set_size(LogicalSize::new(
                        DEFAULT_EXPANDED_WIDTH,
                        DEFAULT_EXPANDED_HEIGHT,
                    ))
                    .map_err(|e| Error::WindowIo {
                        details: format!("set_size expanded: {e}"),
                    })?;
            }
        }
    }

    Ok(())
}
