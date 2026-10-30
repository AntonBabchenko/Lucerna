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

pub mod geometry;
pub mod geometry_file;
pub mod persist;

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

/// The sizing facts the window commands share. Registered as Tauri managed
/// state in `lib.rs`. Every lock is one read or write — never held across a
/// window call (those block on the main thread when made from a worker, and the
/// main thread takes this lock in `persist`'s event hook).
pub struct WindowSizeState {
    inner: Mutex<SizingState>,
}

#[derive(Debug)]
struct SizingState {
    /// `(width, height)` in logical px captured before the last compact entry,
    /// so expanding can restore it.
    expanded_size: Option<(f64, f64)>,
    /// The mode the window is in. Set only after a `set_compact` succeeded: on
    /// a failure the frontend rolls its own flag back, and this one stays too.
    compact: bool,
    /// The startup hug may still shrink the window to the content height: true
    /// only while nothing has sized the window — no saved geometry applied
    /// (`set_restored`), no compact toggle, no earlier hug.
    hug_allowed: bool,
    /// A floor that arrived while the window was maximized or minimized, where
    /// any constraint call un-maximizes or resizes an iconic window. Applied by
    /// the first `Resized` that finds the window restored (`apply_pending_floor`).
    pending_floor: Option<f64>,
    /// A `set_compact` is running. Its un-maximize fires a `Resized`; a
    /// deferred floor applied then would land after the strip is set up and
    /// stretch it back to the expanded minimum width.
    transitioning: bool,
}

impl Default for WindowSizeState {
    fn default() -> Self {
        Self {
            inner: Mutex::new(SizingState {
                expanded_size: None,
                compact: false,
                hug_allowed: true,
                pending_floor: None,
                transitioning: false,
            }),
        }
    }
}

/// Ends a `set_compact`'s transition when dropped — on success or on any `?`.
struct TransitionGuard<'a> {
    state: &'a WindowSizeState,
}

impl Drop for TransitionGuard<'_> {
    fn drop(&mut self) {
        self.state.lock().transitioning = false;
    }
}

impl WindowSizeState {
    /// Poison-tolerant: the lock only guards plain field reads and writes.
    fn lock(&self) -> std::sync::MutexGuard<'_, SizingState> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Startup: a saved size was applied, so the hug must never shrink it.
    pub fn set_restored(&self, restored: bool) {
        self.lock().hug_allowed = !restored;
    }

    /// What `persist` needs to save the right size while compact.
    pub fn mode_snapshot(&self) -> geometry::ModeSnapshot {
        let state = self.lock();
        geometry::ModeSnapshot {
            compact: state.compact,
            captured_expanded: state
                .expanded_size
                .map(|(width, height)| geometry::Size { width, height }),
        }
    }

    /// True once — for the first hug of a window nothing has sized yet.
    fn take_hug_allowance(&self) -> bool {
        std::mem::replace(&mut self.lock().hug_allowed, false)
    }

    fn set_pending_floor(&self, height: Option<f64>) {
        self.lock().pending_floor = height;
    }

    /// A pending floor, unless the window is compact (whose floor is its
    /// height) or on its way there or back (`begin_transition`).
    pub fn pending_floor(&self) -> Option<f64> {
        let state = self.lock();
        state
            .pending_floor
            .filter(|_| !state.compact && !state.transitioning)
    }

    /// `pending_floor`, removed: of several events racing to apply it, one
    /// does. Applying re-defers it if the window is still maximized.
    fn take_pending_floor(&self) -> Option<f64> {
        let mut state = self.lock();
        if state.compact || state.transitioning {
            return None;
        }
        state.pending_floor.take()
    }

    /// Put back a deferred floor whose application failed — unless a newer
    /// one arrived meanwhile.
    fn restore_pending_floor(&self, height: f64) {
        self.lock().pending_floor.get_or_insert(height);
    }

    /// A `set_compact` starts: no deferred floor is offered until it ends.
    fn begin_transition(&self) -> TransitionGuard<'_> {
        self.lock().transitioning = true;
        TransitionGuard { state: self }
    }

    /// A `set_compact` succeeded: record the mode, and the window now has a
    /// size the user chose (or the strip) — no later hug may shrink it.
    fn finish_transition(&self, compact: bool) {
        let mut state = self.lock();
        state.compact = compact;
        state.hug_allowed = false;
        state.pending_floor = None;
    }
}

/// How an expanded floor application may resize the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FloorMode {
    /// Resize to exactly the content height — the first hug of a window
    /// nothing has sized yet.
    Hug,
    /// An arming application (startup, reload, after expand) on a window that
    /// already has a size: issue the arming `set_size`, but never shrink.
    Arm,
    /// A later content change: grow only when the bottom buttons would clip.
    Maintain,
}

/// What the expanded branch should do to the window. Pure data so the decision
/// is unit-testable without a live Tauri window. See `plan_expanded`.
#[derive(Debug, PartialEq)]
struct ExpandedPlan {
    /// Re-enable `set_resizable(true)` + `set_maximizable(true)` — only needed
    /// when returning from compact, which locked them.
    unlock: bool,
    /// Apply the min-size floor `(EXPANDED_MIN_WIDTH, min_height)`.
    apply_min_size: bool,
    /// Resize to this `(width, height)`, or `None` to leave the current size.
    resize_to: Option<(f64, f64)>,
}

/// Decide what the expanded branch should touch.
///
/// - A real compact→expand transition (`restored = Some`) unlocks, applies the
///   floor, and restores the captured size. The window is never maximized here
///   (entering compact unmaximizes + locks it).
/// - A startup/reload while already expanded (`restored = None`) must NOT touch
///   a maximized window: re-applying size/constraints makes Windows restore it
///   to its pre-maximize geometry (the F5-resets-the-window bug). When not
///   maximized it applies the floor and only resizes if the window is
///   stale-narrow (a leftover compact width from a previous session).
fn plan_expanded(
    restored: Option<(f64, f64)>,
    is_maximized: bool,
    current_width: f64,
) -> ExpandedPlan {
    match restored {
        Some((width, height)) => ExpandedPlan {
            unlock: true,
            apply_min_size: true,
            resize_to: Some((width, height)),
        },
        None if is_maximized => ExpandedPlan {
            unlock: false,
            apply_min_size: false,
            resize_to: None,
        },
        None => ExpandedPlan {
            unlock: false,
            apply_min_size: true,
            resize_to: if current_width < EXPANDED_MIN_WIDTH - 1.0 {
                Some((DEFAULT_EXPANDED_WIDTH, DEFAULT_EXPANDED_HEIGHT))
            } else {
                None
            },
        },
    }
}

/// What `set_expanded_floor` should touch. Pure data, unit-testable without a
/// live window. See `plan_expanded_floor`.
#[derive(Debug, PartialEq)]
struct ExpandedFloorPlan {
    /// `set_min_size((width, height))`, or `None` to leave it.
    min_size: Option<(f64, f64)>,
    /// `set_size((width, height))`, or `None` to leave the current size.
    resize_to: Option<(f64, f64)>,
    /// The window cannot take the floor now (maximized or minimized):
    /// remember it for the first `Resized` that finds the window restored.
    defer: bool,
}

/// Decide how to apply the expanded min-height floor.
///
/// The #131 finding was that a programmatic `set_min_size` only held once a
/// later `set_size` "armed" it, so every arming application (`Hug`, `Arm`)
/// still issues one. `Hug` resizes to exactly the content height; `Arm` keeps
/// the current height unless the content is taller; `Maintain` only grows the
/// window when the content would otherwise clip.
///
/// A maximized or minimized window is never touched: in tao any min-size or
/// size call clears the maximized state (the F5-reload bug), and an iconic
/// window reports a 0×0 size. The floor is deferred instead.
fn plan_expanded_floor(
    height: f64,
    current_width: f64,
    current_height: f64,
    is_maximized: bool,
    is_minimized: bool,
    mode: FloorMode,
) -> ExpandedFloorPlan {
    if is_maximized || is_minimized {
        return ExpandedFloorPlan {
            min_size: None,
            resize_to: None,
            defer: true,
        };
    }
    let width = current_width.max(EXPANDED_MIN_WIDTH);
    let resize_to = match mode {
        FloorMode::Hug => Some((width, height)),
        FloorMode::Arm => Some((width, current_height.max(height))),
        FloorMode::Maintain if current_height < height => Some((width, height)),
        FloorMode::Maintain if (width - current_width).abs() > f64::EPSILON => {
            Some((width, current_height))
        }
        FloorMode::Maintain => None,
    };
    ExpandedFloorPlan {
        min_size: Some((EXPANDED_MIN_WIDTH, height)),
        resize_to,
        defer: false,
    }
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
    // Until this returns, no deferred floor may land on the window: the
    // un-maximize below fires a `Resized` that would otherwise apply one after
    // the strip is set up, stretching it back to the expanded minimum width.
    let _transition = state.begin_transition();

    let scale = window.scale_factor().map_err(|e| Error::WindowIo {
        details: format!("scale_factor: {e}"),
    })?;
    // "Could not tell" must not read as "not maximized" (see `apply_expanded_floor`).
    let is_maximized = window.is_maximized().map_err(|e| Error::WindowIo {
        details: format!("is_maximized compact: {e}"),
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
        if is_maximized {
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
            state.lock().expanded_size = Some((current_width, current_height));
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
        state.finish_transition(true);
    } else {
        // Startup/reload OR a compact→expand transition. Decide what to touch
        // via `plan_expanded` so a maximized window on a plain reload (F5) is
        // left alone — touching its size/constraints makes Windows restore it.
        let phys = window.inner_size().map_err(|e| Error::WindowIo {
            details: format!("inner_size expanded: {e}"),
        })?;
        let current_width = phys.width as f64 / scale;
        let restored = state.lock().expanded_size.take();
        let plan = plan_expanded(restored, is_maximized, current_width);

        if plan.unlock {
            // Re-enable resizing/maximizing (only disabled by compact mode).
            window.set_resizable(true).map_err(|e| Error::WindowIo {
                details: format!("set_resizable true: {e}"),
            })?;
            window.set_maximizable(true).map_err(|e| Error::WindowIo {
                details: format!("set_maximizable true: {e}"),
            })?;
        }
        if plan.apply_min_size {
            // Floor the expanded window at the sidebar content height (not the
            // fixed config minimum), so it can be shrunk to the bottom buttons.
            window
                .set_min_size(Some(LogicalSize::new(EXPANDED_MIN_WIDTH, min_height)))
                .map_err(|e| Error::WindowIo {
                    details: format!("set_min_size expanded: {e}"),
                })?;
        }
        if let Some((width, height)) = plan.resize_to {
            window
                .set_size(LogicalSize::new(width, height))
                .map_err(|e| Error::WindowIo {
                    details: format!("set_size expanded: {e}"),
                })?;
        }
        state.finish_transition(false);
    }

    Ok(())
}

/// Apply the expanded window's min-height floor (the measured sidebar content
/// height) and arm it. See `plan_expanded_floor`. No-op if the window is absent
/// or `height` is non-positive.
///
/// `hug` marks an arming application (startup, reload, after expand). Whether
/// it may shrink the window is the backend's call: only for a window nothing
/// has sized yet (`WindowSizeState::take_hug_allowance`); otherwise it arms
/// without shrinking, so a restored, reloaded or toggled window keeps its size.
pub fn set_expanded_floor(app: &AppHandle, height: f64, hug: bool) -> Result<()> {
    let request = if hug {
        FloorRequest::Arming
    } else {
        FloorRequest::ContentChange
    };
    apply_expanded_floor(app, height, request)
}

/// Why a floor is being applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FloorRequest {
    /// The frontend's arming application (startup, reload, after expand).
    Arming,
    /// The sidebar content changed.
    ContentChange,
    /// A floor deferred while the window was maximized or minimized, now that
    /// it is restored. It arms like the startup application it stands in for
    /// (#131: a programmatic min-size holds once a `set_size` follows it), but
    /// never shrinks the rect the window was restored to.
    Deferred,
}

/// The mode for a floor applied to a window that can take it now.
/// `take_hug_allowance` is called only for an arming request, so the one-time
/// hug is never spent on anything else.
fn floor_mode(request: FloorRequest, take_hug_allowance: impl FnOnce() -> bool) -> FloorMode {
    match request {
        FloorRequest::Arming if take_hug_allowance() => FloorMode::Hug,
        FloorRequest::Arming | FloorRequest::Deferred => FloorMode::Arm,
        FloorRequest::ContentChange => FloorMode::Maintain,
    }
}

fn apply_expanded_floor(app: &AppHandle, height: f64, request: FloorRequest) -> Result<()> {
    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };
    if height <= 0.0 {
        return Ok(());
    }
    let state = app.state::<WindowSizeState>();
    let scale = window.scale_factor().map_err(|e| Error::WindowIo {
        details: format!("scale_factor: {e}"),
    })?;
    // "Could not tell" must not read as "not maximized": a floor applied to a
    // maximized window un-maximizes it. An error leaves the window alone.
    let is_maximized = window.is_maximized().map_err(|e| Error::WindowIo {
        details: format!("is_maximized floor: {e}"),
    })?;
    let is_minimized = window.is_minimized().map_err(|e| Error::WindowIo {
        details: format!("is_minimized floor: {e}"),
    })?;
    let phys = window.inner_size().map_err(|e| Error::WindowIo {
        details: format!("inner_size floor: {e}"),
    })?;
    let current_width = phys.width as f64 / scale;
    let current_height = phys.height as f64 / scale;
    // A window that cannot take the floor now defers it whatever the mode.
    let mode = if is_maximized || is_minimized {
        FloorMode::Maintain
    } else {
        floor_mode(request, || state.take_hug_allowance())
    };
    let plan = plan_expanded_floor(
        height,
        current_width,
        current_height,
        is_maximized,
        is_minimized,
        mode,
    );
    state.set_pending_floor(plan.defer.then_some(height));

    if let Some((w, h)) = plan.min_size {
        window
            .set_min_size(Some(LogicalSize::new(w, h)))
            .map_err(|e| Error::WindowIo {
                details: format!("set_min_size floor: {e}"),
            })?;
    }
    if let Some((w, h)) = plan.resize_to {
        window
            .set_size(LogicalSize::new(w, h))
            .map_err(|e| Error::WindowIo {
                details: format!("set_size floor: {e}"),
            })?;
    }
    Ok(())
}

/// Apply a floor deferred while the window was maximized or minimized, once a
/// `Resized` finds it restored. Called off the event callback (`persist`), so
/// the window calls run after the event that triggered it. `set_expanded_floor`
/// re-checks the state, so a window still maximized just keeps it pending.
pub fn apply_pending_floor(app: &AppHandle) {
    let state = app.state::<WindowSizeState>();
    let Some(height) = state.take_pending_floor() else {
        return;
    };
    if let Err(e) = apply_expanded_floor(app, height, FloorRequest::Deferred) {
        // Not applied: keep it for the next `Resized` to try again.
        state.restore_pending_floor(height);
        crate::diag!("[window] could not apply the deferred minimum height: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_from_compact_unlocks_and_restores() {
        // restored = Some → genuine compact→expand toggle.
        let plan = plan_expanded(Some((1000.0, 700.0)), false, 240.0);
        assert_eq!(
            plan,
            ExpandedPlan {
                unlock: true,
                apply_min_size: true,
                resize_to: Some((1000.0, 700.0)),
            }
        );
    }

    #[test]
    fn maximized_startup_is_a_noop() {
        // restored = None + maximized → leave the window completely alone
        // (this is the F5-resets-the-window fix).
        let plan = plan_expanded(None, true, 1920.0);
        assert_eq!(
            plan,
            ExpandedPlan {
                unlock: false,
                apply_min_size: false,
                resize_to: None,
            }
        );
    }

    #[test]
    fn normal_startup_applies_floor_without_resizing() {
        // restored = None + not maximized + wide enough → floor only.
        let plan = plan_expanded(None, false, 1024.0);
        assert_eq!(
            plan,
            ExpandedPlan {
                unlock: false,
                apply_min_size: true,
                resize_to: None,
            }
        );
    }

    #[test]
    fn stale_narrow_startup_resizes_to_default() {
        // restored = None + not maximized + narrower than the expanded minimum
        // (e.g. a leftover compact width) → force the default expanded size.
        let plan = plan_expanded(None, false, 240.0);
        assert_eq!(
            plan,
            ExpandedPlan {
                unlock: false,
                apply_min_size: true,
                resize_to: Some((DEFAULT_EXPANDED_WIDTH, DEFAULT_EXPANDED_HEIGHT)),
            }
        );
    }

    #[test]
    fn floor_hug_sizes_to_content_and_arms() {
        // Startup hug: resize to exactly the content height (a real set_size that
        // arms the min-size on wry), min floored at the content height.
        let plan = plan_expanded_floor(506.0, 820.0, 520.0, false, false, FloorMode::Hug);
        assert_eq!(plan.min_size, Some((EXPANDED_MIN_WIDTH, 506.0)));
        assert_eq!(plan.resize_to, Some((820.0, 506.0)));
    }

    #[test]
    fn floor_grows_when_content_exceeds_window() {
        // Not hug, content taller than the window → grow so buttons stay visible.
        let plan = plan_expanded_floor(560.0, 900.0, 520.0, false, false, FloorMode::Maintain);
        assert_eq!(plan.min_size, Some((EXPANDED_MIN_WIDTH, 560.0)));
        assert_eq!(plan.resize_to, Some((900.0, 560.0)));
    }

    #[test]
    fn floor_only_updates_min_when_window_already_tall_enough() {
        // Not hug, window already ≥ content → update the min, don't touch size
        // (don't fight a window the user enlarged).
        let plan = plan_expanded_floor(480.0, 1000.0, 700.0, false, false, FloorMode::Maintain);
        assert_eq!(plan.min_size, Some((EXPANDED_MIN_WIDTH, 480.0)));
        assert_eq!(plan.resize_to, None);
    }

    #[test]
    fn floor_clamps_width_below_minimum() {
        // Width below the expanded minimum → clamp width up, keep height.
        let plan = plan_expanded_floor(480.0, 700.0, 700.0, false, false, FloorMode::Maintain);
        assert_eq!(plan.resize_to, Some((EXPANDED_MIN_WIDTH, 700.0)));
    }

    #[test]
    fn floor_defers_when_maximized() {
        // Never disturb a maximized window (the F5 guard): any min-size call
        // un-maximizes it in tao. The floor waits for the restore instead.
        let plan = plan_expanded_floor(506.0, 1920.0, 1080.0, true, false, FloorMode::Hug);
        assert_eq!(
            plan,
            ExpandedFloorPlan {
                min_size: None,
                resize_to: None,
                defer: true,
            }
        );
    }

    #[test]
    fn floor_defers_when_minimized() {
        // An iconic window reports 0×0: "content taller than the window" would
        // set_size it. A content change during a game-start minimise must wait.
        let plan = plan_expanded_floor(707.0, 0.0, 0.0, false, true, FloorMode::Maintain);
        assert_eq!(
            plan,
            ExpandedFloorPlan {
                min_size: None,
                resize_to: None,
                defer: true,
            }
        );
    }

    #[test]
    fn arm_never_shrinks_a_window_the_user_enlarged() {
        // F5 / restored start / after expand on a 1084×861 window, content 707:
        // the arming set_size keeps 861 (it shrank to 707 before #532).
        let plan = plan_expanded_floor(707.0, 1084.0, 861.0, false, false, FloorMode::Arm);
        assert_eq!(plan.min_size, Some((EXPANDED_MIN_WIDTH, 707.0)));
        assert_eq!(plan.resize_to, Some((1084.0, 861.0)));
    }

    #[test]
    fn arm_grows_a_window_shorter_than_its_content() {
        let plan = plan_expanded_floor(707.0, 900.0, 600.0, false, false, FloorMode::Arm);
        assert_eq!(plan.resize_to, Some((900.0, 707.0)));
    }

    #[test]
    fn only_an_arming_request_may_spend_the_hug() {
        assert_eq!(floor_mode(FloorRequest::Arming, || true), FloorMode::Hug);
        assert_eq!(floor_mode(FloorRequest::Arming, || false), FloorMode::Arm);
        // A floor deferred over a maximize arms on restore like the startup
        // application it stands in for, but never shrinks the restored rect.
        let spent = std::cell::Cell::new(false);
        let take = || {
            spent.set(true);
            true
        };
        assert_eq!(floor_mode(FloorRequest::Deferred, take), FloorMode::Arm);
        assert_eq!(
            floor_mode(FloorRequest::ContentChange, take),
            FloorMode::Maintain
        );
        assert!(!spent.get(), "only an arming request asks for the hug");
    }

    #[test]
    fn the_hug_is_allowed_once_for_a_window_nothing_has_sized() {
        let state = WindowSizeState::default();
        assert!(state.take_hug_allowance());
        assert!(!state.take_hug_allowance());
    }

    #[test]
    fn a_restored_window_is_never_hugged() {
        let state = WindowSizeState::default();
        state.set_restored(true);
        assert!(!state.take_hug_allowance());
    }

    #[test]
    fn a_compact_toggle_ends_the_hug_and_records_the_mode() {
        let state = WindowSizeState::default();
        state.lock().expanded_size = Some((1084.0, 861.0));
        state.set_pending_floor(Some(707.0));
        state.finish_transition(true);
        assert!(!state.take_hug_allowance());
        assert_eq!(
            state.mode_snapshot(),
            geometry::ModeSnapshot {
                compact: true,
                captured_expanded: Some(geometry::Size {
                    width: 1084.0,
                    height: 861.0,
                }),
            }
        );
        assert_eq!(state.pending_floor(), None);
    }

    #[test]
    fn a_pending_floor_is_not_offered_while_compact() {
        let state = WindowSizeState::default();
        state.set_pending_floor(Some(707.0));
        assert_eq!(state.pending_floor(), Some(707.0));
        state.lock().compact = true;
        assert_eq!(state.pending_floor(), None);
        assert_eq!(state.take_pending_floor(), None);
    }

    #[test]
    fn a_compact_transition_hides_a_deferred_floor() {
        // set_compact's un-maximize fires a Resized while the flag still says
        // expanded; the deferred floor must not be applied over the strip.
        let state = WindowSizeState::default();
        state.set_pending_floor(Some(707.0));
        let transition = state.begin_transition();
        assert_eq!(state.pending_floor(), None);
        assert_eq!(state.take_pending_floor(), None);
        // A failed transition (dropped early) leaves the floor pending.
        drop(transition);
        assert_eq!(state.pending_floor(), Some(707.0));
    }

    #[test]
    fn a_finished_transition_drops_a_deferred_floor() {
        let state = WindowSizeState::default();
        state.set_pending_floor(Some(707.0));
        let transition = state.begin_transition();
        state.finish_transition(false);
        drop(transition);
        assert_eq!(state.pending_floor(), None);
    }

    #[test]
    fn a_failed_deferred_floor_never_replaces_a_newer_one() {
        let state = WindowSizeState::default();
        state.restore_pending_floor(707.0);
        assert_eq!(state.pending_floor(), Some(707.0));
        state.set_pending_floor(Some(720.0));
        state.restore_pending_floor(707.0);
        assert_eq!(state.pending_floor(), Some(720.0));
    }

    #[test]
    fn a_pending_floor_is_applied_by_one_of_the_racing_events() {
        let state = WindowSizeState::default();
        state.set_pending_floor(Some(707.0));
        assert_eq!(state.take_pending_floor(), Some(707.0));
        assert_eq!(state.take_pending_floor(), None);
    }
}
