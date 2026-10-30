//! Remembers the main window's position and size between launches (#532).
//!
//! Startup: `startup_placement` reads `window-state.json` and the monitors and
//! asks `geometry::plan_restore`; `lib.rs` builds the window hidden at that size
//! and `place_and_show` moves it into place before it is first shown. Runtime:
//! every move or resize pokes a debounced saver, and closing or exiting flushes
//! it. The decisions are `geometry`'s and the file is `geometry_file`'s; this
//! module only talks to Tauri.

use super::geometry::{
    self, MonitorArea, Observed, Outcome, Placement, Point, Rect, SavedWindow, Size,
};
use super::{geometry_file, WindowSizeState};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;
use tauri::{
    AppHandle, LogicalPosition, LogicalSize, Manager, PhysicalPosition, Position, WebviewWindow,
};
use tokio::sync::mpsc;

/// Quiet time after the last move or resize before the window is read and
/// saved. tao sets its maximized flag on the `WM_SIZE` that follows the move of
/// a maximize, so a reading taken right after a `Moved` could see a stale flag;
/// one taken after the quiet cannot. Close and exit flush without waiting.
const SETTLE: Duration = Duration::from_millis(500);

/// How positions are expressed on this platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unit {
    /// Windows: the virtual screen in physical px, unambiguous across DPIs.
    Physical,
    /// macOS and Linux/X11: logical px — tao converts positions there with the
    /// window's own scale, and the global space is logical.
    Logical,
}

/// `None` where a position can be neither read nor set (Wayland), or where
/// that cannot be ruled out.
fn position_unit() -> Option<Unit> {
    if cfg!(windows) {
        Some(Unit::Physical)
    } else if cfg!(target_os = "macos") || geometry::x11_from_env(|key| std::env::var(key).ok()) {
        Some(Unit::Logical)
    } else {
        None
    }
}

/// The saver's shared state. Managed in `init`.
pub struct GeometryState {
    poke: mpsc::Sender<()>,
    save: Mutex<SaveState>,
    /// Numbers readings, so an older one that loses the race to the lock never
    /// overwrites a newer save (the saver and a close/exit flush both save).
    seq: AtomicU64,
    /// The last reading failed: log only the first failure of a streak.
    getters_failing: AtomicBool,
}

struct SaveState {
    /// What the file holds — `next_saved`'s baseline.
    prev: Option<SavedWindow>,
    last_seq: u64,
    /// The last write failed: log only the first failure of a streak.
    failing: bool,
}

/// What startup needs: where to open, what the file held, and whether this
/// session may save over it.
pub struct Startup {
    pub placement: Placement,
    pub saved: Option<SavedWindow>,
    pub may_save: bool,
}

/// Read the saved state and the monitors and decide where the window opens.
/// A saved state that is not used at all, or whose position is dropped, is
/// logged with the reason; none of it reaches the user, who just gets the
/// first-start placement (or the OS's, for a dropped position).
pub fn startup_placement(app: &AppHandle) -> Startup {
    let unit = position_unit();
    let mut read_failed = false;
    let saved = match crate::paths::window_state_file(app) {
        Ok(path) => match geometry_file::load(&path) {
            Ok(saved) => saved,
            Err(e) => {
                read_failed = matches!(e, geometry_file::LoadError::Unreadable(_));
                crate::diag!("[window] {e} — opening at the default place and size");
                None
            }
        },
        Err(e) => {
            read_failed = true;
            crate::diag!("[window] no path for the window state ({e}) — opening at the default place and size");
            None
        }
    };
    // Without a position unit a saved position cannot be applied (Wayland).
    if unit.is_none() && saved.is_some_and(|s| s.position.is_some()) {
        crate::diag!("[window] window positions cannot be set here — the system places the window");
    }
    let usable = saved.map(|s| SavedWindow {
        position: s.position.filter(|_| unit.is_some()),
        ..s
    });
    let monitors = match monitor_areas(app, unit.unwrap_or(Unit::Logical)) {
        Ok(monitors) => monitors,
        Err(e) => {
            crate::diag!("[window] cannot list the monitors ({e})");
            Vec::new()
        }
    };
    let placement = geometry::plan_restore(usable, &monitors);
    match placement.outcome {
        Outcome::OffScreen => {
            crate::diag!("[window] the saved position is on no screen — the system places the window")
        }
        Outcome::NoMonitors => crate::diag!(
            "[window] no monitor to check the saved position against — opening at the default place and size"
        ),
        Outcome::Fresh | Outcome::Restored => {}
    }
    let may_save = geometry::may_save(read_failed, placement.outcome);
    if !may_save {
        crate::diag!(
            "[window] the saved place could not be checked — this session leaves it as it is"
        );
    }
    Startup {
        placement,
        saved,
        may_save,
    }
}

/// The monitors in the platform's position unit.
fn monitor_areas(app: &AppHandle, unit: Unit) -> tauri::Result<Vec<MonitorArea>> {
    let primary = app.primary_monitor()?.map(|m| *m.position());
    let areas = app
        .available_monitors()?
        .iter()
        .map(|m| {
            let scale = m.scale_factor();
            let (divisor, per_logical) = match unit {
                Unit::Physical => (1.0, scale),
                Unit::Logical => (scale, 1.0),
            };
            let rect = |x: i32, y: i32, width: u32, height: u32| Rect {
                x: f64::from(x) / divisor,
                y: f64::from(y) / divisor,
                width: f64::from(width) / divisor,
                height: f64::from(height) / divisor,
            };
            let (position, size, work) = (m.position(), m.size(), m.work_area());
            MonitorArea {
                bounds: rect(position.x, position.y, size.width, size.height),
                work: rect(
                    work.position.x,
                    work.position.y,
                    work.size.width,
                    work.size.height,
                ),
                per_logical,
                primary: primary == Some(*position),
            }
        })
        .collect();
    Ok(areas)
}

/// Move, size and show the window `lib.rs` built hidden.
///
/// The order matters: in tao a position or size call un-maximizes, so maximize
/// comes last; the size is applied again after the move because a monitor
/// with another scale rescales the window. A failed step is logged and the
/// next still runs. Only a window that cannot be shown is an error — an
/// invisible launcher holding the single-instance lock is worse than one that
/// fails to start.
pub fn place_and_show(window: &WebviewWindow, placement: &Placement) -> tauri::Result<()> {
    if let (Some(p), Some(unit)) = (placement.position, position_unit()) {
        let position = match unit {
            Unit::Physical => Position::Physical(PhysicalPosition::new(
                p.x.round() as i32,
                p.y.round() as i32,
            )),
            Unit::Logical => Position::Logical(LogicalPosition::new(p.x, p.y)),
        };
        if let Err(e) = window.set_position(position) {
            crate::diag!("[window] could not move the window to its saved place ({e})");
        }
    }
    let size = LogicalSize::new(placement.size.width, placement.size.height);
    if let Err(e) = window.set_size(size) {
        crate::diag!("[window] could not give the window its saved size ({e})");
    }
    if placement.maximized {
        if let Err(e) = window.maximize() {
            crate::diag!("[window] could not maximize the window as it was ({e})");
        }
    }
    if let Err(first) = window.show() {
        crate::diag!("[window] could not show the window ({first}); trying again");
        window.show()?;
    }
    if let Err(e) = window.set_focus() {
        crate::diag!("[window] could not focus the window ({e})");
    }
    Ok(())
}

/// Manage the saver state and start the saver. Call before the window is
/// built, so its first events find the state.
pub fn init(app: &AppHandle, startup: &Startup) {
    if !startup.may_save {
        // No state, no saver: every event and flush finds nothing to save
        // with, and the file this session could not check stays as it is.
        return;
    }
    let saved = startup.saved;
    let (poke, mut pokes) = mpsc::channel(1);
    app.manage(GeometryState {
        poke,
        save: Mutex::new(SaveState {
            prev: saved,
            last_seq: 0,
            failing: false,
        }),
        seq: AtomicU64::new(0),
        getters_failing: AtomicBool::new(false),
    });
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        while pokes.recv().await.is_some() {
            // Every further poke restarts the quiet period.
            loop {
                match tokio::time::timeout(SETTLE, pokes.recv()).await {
                    Ok(Some(())) => continue,
                    Ok(None) => return,
                    Err(_quiet) => break,
                }
            }
            let app = app.clone();
            // The getters wait on the main thread: keep them off async workers.
            if let Err(e) =
                tauri::async_runtime::spawn_blocking(move || capture_and_save(&app)).await
            {
                crate::diag!("[window] the window-place saver failed ({e})");
            }
        }
    });
}

/// A `Moved` or `Resized` of the main window: poke the saver, and apply a
/// floor deferred while the window was maximized or minimized.
pub fn on_geometry_event(window: &tauri::Window) {
    if window.label() != "main" {
        return;
    }
    let app = window.app_handle();
    if let Some(state) = app.try_state::<GeometryState>() {
        match state.poke.try_send(()) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(())) => {
                // A poke is already queued: the save it triggers reads the
                // window after this event too.
            }
            Err(mpsc::error::TrySendError::Closed(())) => {
                // The saver ended with the async runtime: the app is exiting,
                // and the exit flush saves.
            }
        }
    }
    let pending = app
        .try_state::<WindowSizeState>()
        .and_then(|sizing| sizing.pending_floor());
    if pending.is_some() {
        let app = app.clone();
        // Off the event callback: the floor's window calls run after it.
        drop(tauri::async_runtime::spawn_blocking(move || {
            super::apply_pending_floor(&app)
        }));
    }
}

/// Save now: the window is about to close, or the app to exit.
pub fn flush(app: &AppHandle) {
    capture_and_save(app);
}

/// Read the window, decide what to save, and save it.
fn capture_and_save(app: &AppHandle) {
    let (Some(state), Some(window)) = (
        app.try_state::<GeometryState>(),
        app.get_webview_window("main"),
    ) else {
        return;
    };
    let seq = state.seq.fetch_add(1, Ordering::SeqCst) + 1;
    // Outside the save lock: from a worker these getters wait on the main
    // thread, which takes the save lock itself when it flushes.
    let observed = match observe(&window) {
        Ok(observed) => {
            state.getters_failing.store(false, Ordering::SeqCst);
            observed
        }
        Err(reason) => {
            if !state.getters_failing.swap(true, Ordering::SeqCst) {
                crate::diag!("[window] could not read the window to remember its place ({reason})");
            }
            return;
        }
    };
    let mode = app.state::<WindowSizeState>().mode_snapshot();
    let mut save = state.save.lock().unwrap_or_else(PoisonError::into_inner);
    if seq <= save.last_seq {
        // A newer reading was already saved.
        return;
    }
    save.last_seq = seq;
    let Some(next) = geometry::next_saved(save.prev, &observed, &mode) else {
        return;
    };
    match write(app, &next) {
        Ok(true) => {
            save.prev = Some(next);
            save.failing = false;
        }
        Ok(false) => {
            // Skipped on purpose (recovery session, data-root move). The
            // baseline stays, so the next reading tries again.
        }
        Err(reason) => {
            if !save.failing {
                crate::diag!("[window] could not save the window's place ({reason})");
            }
            save.failing = true;
        }
    }
}

/// One reading of the window. Any getter failing fails the reading: "could
/// not tell" must never be saved as "not maximized" or as some position.
fn observe(window: &WebviewWindow) -> Result<Observed, String> {
    let visible = window
        .is_visible()
        .map_err(|e| format!("is_visible: {e}"))?;
    let minimized = window
        .is_minimized()
        .map_err(|e| format!("is_minimized: {e}"))?;
    let maximized = window
        .is_maximized()
        .map_err(|e| format!("is_maximized: {e}"))?;
    let scale = window
        .scale_factor()
        .map_err(|e| format!("scale_factor: {e}"))?;
    if !(scale.is_finite() && scale > 0.0) {
        return Err(format!("scale_factor: {scale}"));
    }
    let inner = window
        .inner_size()
        .map_err(|e| format!("inner_size: {e}"))?;
    let position = match position_unit() {
        Some(unit) => {
            let p = window
                .outer_position()
                .map_err(|e| format!("outer_position: {e}"))?;
            Some(match unit {
                Unit::Physical => Point {
                    x: f64::from(p.x),
                    y: f64::from(p.y),
                },
                Unit::Logical => {
                    let logical = p.to_logical::<f64>(scale);
                    Point {
                        x: logical.x,
                        y: logical.y,
                    }
                }
            })
        }
        None => None,
    };
    Ok(Observed {
        position,
        logical_size: Size {
            width: f64::from(inner.width) / scale,
            height: f64::from(inner.height) / scale,
        },
        maximized,
        minimized,
        visible,
    })
}

/// `Ok(true)` written, `Ok(false)` skipped on purpose. A recovery session's
/// root is thrown away at exit; during or after a data-root move the root must
/// not change under the copy, nor come back once it is deleted.
fn write(app: &AppHandle, next: &SavedWindow) -> Result<bool, String> {
    let recovery = app
        .try_state::<crate::data_root::DataRoot>()
        .is_none_or(|root| root.resolved.fell_back());
    if recovery {
        return Ok(false);
    }
    let path = crate::paths::window_state_file(app).map_err(|e| e.to_string())?;
    match crate::data_root::state::global().while_idle(|| geometry_file::save(&path, next)) {
        Some(written) => written.map(|()| true),
        None => Ok(false),
    }
}
