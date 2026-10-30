//! Pure decisions for remembering the main window's position and size between
//! launches (#532). No Tauri types: `persist` turns the live window and the
//! monitors into these values, so every rule here is unit-tested.
//!
//! Units. A size is always the EXPANDED window's inner size in logical px. A
//! position is in the platform's native global unit: physical px on Windows
//! (its virtual screen is unambiguous across DPIs), logical px on macOS and
//! Linux/X11 (tao converts positions there with the window's own scale), and
//! absent on Wayland (no readable or settable position). `MonitorArea` rects use
//! the same unit, and `per_logical` converts the logical thresholds below into it.

use super::{
    DEFAULT_EXPANDED_HEIGHT, DEFAULT_EXPANDED_WIDTH, EXPANDED_MIN_WIDTH, FALLBACK_MIN_HEIGHT,
};
use serde::{Deserialize, Serialize};

/// Height of the window's top edge (the title bar) that must be on a screen
/// for the saved position to be reachable (logical px).
const TITLE_BAND: f64 = 32.0;
/// How much of the title bar must be on a screen horizontally (logical px).
const MIN_VISIBLE: f64 = 160.0;
/// How far above a screen's top the window's top may sit — invisible resize
/// borders and rounding (logical px).
const EDGE_SLACK: f64 = 8.0;
/// Frame and title bar around the inner size, kept free when capping a saved
/// size to a work area (logical px).
const CHROME_W: f64 = 16.0;
const CHROME_H: f64 = 40.0;
/// Larger than any real window; a bigger value means a damaged file.
const MAX_DIMENSION: f64 = 100_000.0;

pub const DEFAULT_SIZE: Size = Size {
    width: DEFAULT_EXPANDED_WIDTH,
    height: DEFAULT_EXPANDED_HEIGHT,
};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// What `window-state.json` holds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SavedWindow {
    /// The window's outer top-left in native units; `None` lets the OS place it.
    #[serde(default)]
    pub position: Option<Point>,
    /// The expanded window's inner size, logical px — also while compact.
    pub size: Size,
    #[serde(default)]
    pub maximized: bool,
}

/// One monitor in native units. `per_logical` is native units per logical px:
/// the monitor's scale factor on Windows, 1.0 where rects are already logical.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MonitorArea {
    pub bounds: Rect,
    pub work: Rect,
    pub per_logical: f64,
    pub primary: bool,
}

/// Why a placement is what it is — for the log line `persist` writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing saved: today's first-start placement.
    Fresh,
    /// A saved state exists but no usable monitor was reported to check it on.
    NoMonitors,
    /// The saved state applies (its position, if it had one, is on a screen).
    Restored,
    /// The saved position is on no screen; the OS places the window.
    OffScreen,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub position: Option<Point>,
    pub size: Size,
    pub maximized: bool,
    /// A saved size was applied: the startup hug must not shrink it.
    pub restored: bool,
    pub outcome: Outcome,
}

/// One reading of the live window, built only when every getter succeeded.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Observed {
    /// `None` on a platform without positions.
    pub position: Option<Point>,
    pub logical_size: Size,
    pub maximized: bool,
    pub minimized: bool,
    pub visible: bool,
}

/// The compact-mode facts `next_saved` needs from `WindowSizeState`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModeSnapshot {
    pub compact: bool,
    /// The expanded size remembered when compact was entered.
    pub captured_expanded: Option<Size>,
}

/// The saved state, or `None` when a value is impossible (a damaged file).
pub fn validated(raw: SavedWindow) -> Option<SavedWindow> {
    let dimension_ok = |v: f64| v.is_finite() && v > 0.0 && v <= MAX_DIMENSION;
    let position_ok = raw
        .position
        .is_none_or(|p| p.x.is_finite() && p.y.is_finite());
    (dimension_ok(raw.size.width) && dimension_ok(raw.size.height) && position_ok).then_some(raw)
}

/// Where and how to open the window.
///
/// Nothing saved, or no usable monitor to check a saved state against ("could
/// not tell"), is today's first-start placement. A saved position is kept only
/// when the window's title bar is on a screen; otherwise the OS places the
/// window, as on a first start. The saved size is cut down to the work area of
/// the monitor the window opens on — never below the expanded minimum width,
/// and never below the placeholder minimum height unless it already was.
pub fn plan_restore(saved: Option<SavedWindow>, monitors: &[MonitorArea]) -> Placement {
    let Some(saved) = saved else {
        return fresh(Outcome::Fresh);
    };
    let usable: Vec<&MonitorArea> = monitors.iter().filter(|m| m.is_usable()).collect();
    let Some(first) = usable.first().copied() else {
        return fresh(Outcome::NoMonitors);
    };
    let reachable = saved
        .position
        .and_then(|p| reachable_on(p, saved.size, &usable).map(|m| (p, m)));
    let (position, target, outcome) = match (reachable, saved.position) {
        (Some((p, m)), _) => (Some(p), m, Outcome::Restored),
        (None, had_position) => {
            let primary = usable.iter().copied().find(|m| m.primary).unwrap_or(first);
            let outcome = if had_position.is_some() {
                Outcome::OffScreen
            } else {
                Outcome::Restored
            };
            (None, primary, outcome)
        }
    };
    Placement {
        position,
        size: fit_to(saved.size, target),
        maximized: saved.maximized,
        restored: true,
        outcome,
    }
}

/// The state to save after `observed`, or `None` when nothing changed or the
/// reading must not be saved.
///
/// One position serves both modes: compact shrinks and expand grows the window
/// from its top-left, so the strip's top-left is the expanded window's. The
/// saved size is always the EXPANDED one; a maximized window keeps the last
/// normal rect it had (that is what un-maximizing returns to).
pub fn next_saved(
    prev: Option<SavedWindow>,
    observed: &Observed,
    mode: &ModeSnapshot,
) -> Option<SavedWindow> {
    // A minimized Windows window reports (−32000, −32000) and 0×0; a hidden
    // one (tray) has not moved since it was last seen.
    if observed.minimized || !observed.visible {
        return None;
    }
    let prev_position = prev.and_then(|p| p.position);
    let prev_size = prev.map_or(DEFAULT_SIZE, |p| p.size);
    let next = if observed.maximized {
        SavedWindow {
            position: prev_position,
            size: prev_size,
            maximized: true,
        }
    } else if mode.compact {
        SavedWindow {
            position: observed.position.or(prev_position),
            size: mode.captured_expanded.unwrap_or(prev_size),
            maximized: false,
        }
    } else {
        // A strip-width window outside compact mode is a transition (the
        // strip mid-expand): its size is not the expanded one.
        let in_transition = observed.logical_size.width < EXPANDED_MIN_WIDTH - 1.0
            || observed.logical_size.height <= 0.0;
        SavedWindow {
            position: observed.position.or(prev_position),
            size: if in_transition {
                prev_size
            } else {
                observed.logical_size
            },
            maximized: false,
        }
    };
    (Some(next) != prev).then_some(next)
}

/// Whether this session may save the window's place. Not when the saved state
/// could not be READ (as opposed to read and found damaged), nor when there
/// was no monitor to check it on: the first reading would replace a file that
/// may well be good with a placement made without it.
pub fn may_save(read_failed: bool, outcome: Outcome) -> bool {
    !read_failed && outcome != Outcome::NoMonitors
}

/// Whether GTK certainly runs on X11, from the environment `get` reads.
/// Anything uncertain is "no": a position applied on Wayland is ignored, and one
/// read there is a cached (0, 0).
pub fn x11_from_env(get: impl Fn(&str) -> Option<String>) -> bool {
    let set = |key: &str| get(key).filter(|v| !v.is_empty());
    match set("GDK_BACKEND") {
        // A list ("wayland,x11") is tried in order; only x11 first is certain.
        Some(backend) => backend.starts_with("x11"),
        None => set("WAYLAND_DISPLAY").is_none() && set("DISPLAY").is_some(),
    }
}

impl MonitorArea {
    /// A monitor whose rects or scale could not be read reports zeros.
    fn is_usable(&self) -> bool {
        let positive = |v: f64| v.is_finite() && v > 0.0;
        positive(self.bounds.width)
            && positive(self.bounds.height)
            && positive(self.work.width)
            && positive(self.work.height)
            && positive(self.per_logical)
    }
}

/// The monitor holding most of the window's title bar, if any holds enough of
/// it to grab: the top edge within the monitor vertically, and at least
/// `MIN_VISIBLE` of the width over it — at the width the window would open with
/// there, after `fit_to`. Bounds, not the work area — a title bar under a top
/// taskbar is still on the screen.
fn reachable_on<'m>(
    position: Point,
    size: Size,
    monitors: &[&'m MonitorArea],
) -> Option<&'m MonitorArea> {
    monitors
        .iter()
        .copied()
        .filter_map(|m| {
            let k = m.per_logical;
            let b = m.bounds;
            let top_on_screen =
                position.y >= b.y - EDGE_SLACK * k && position.y <= b.y + b.height - TITLE_BAND * k;
            let right = position.x + fit_to(size, m).width * k;
            let overlap = right.min(b.x + b.width) - position.x.max(b.x);
            (top_on_screen && overlap >= MIN_VISIBLE * k).then_some((m, overlap))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(m, _)| m)
}

/// `saved` cut down to `m`'s work area, in logical px.
fn fit_to(saved: Size, m: &MonitorArea) -> Size {
    let work_width = m.work.width / m.per_logical;
    let work_height = m.work.height / m.per_logical;
    Size {
        width: saved
            .width
            .min(work_width - CHROME_W)
            .max(EXPANDED_MIN_WIDTH),
        height: saved
            .height
            .min(work_height - CHROME_H)
            .max(saved.height.min(FALLBACK_MIN_HEIGHT)),
    }
}

impl Placement {
    /// The window builder's minimum inner size. The 520 px height is a
    /// placeholder the measured sidebar floor replaces moments later; it must
    /// not clamp a restored height that was already below it (a hugged 506).
    pub fn builder_min_size(&self) -> Size {
        Size {
            width: EXPANDED_MIN_WIDTH,
            height: self.size.height.min(FALLBACK_MIN_HEIGHT),
        }
    }
}

fn fresh(outcome: Outcome) -> Placement {
    Placement {
        position: None,
        size: DEFAULT_SIZE,
        maximized: false,
        restored: false,
        outcome,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    /// A 1920×1080 monitor at `x` with a 48 px bottom taskbar, at 100 %.
    fn monitor_at(x: f64, primary: bool) -> MonitorArea {
        MonitorArea {
            bounds: rect(x, 0.0, 1920.0, 1080.0),
            work: rect(x, 0.0, 1920.0, 1032.0),
            per_logical: 1.0,
            primary,
        }
    }

    fn primary() -> MonitorArea {
        monitor_at(0.0, true)
    }

    fn saved(x: f64, y: f64, width: f64, height: f64) -> SavedWindow {
        SavedWindow {
            position: Some(Point { x, y }),
            size: Size { width, height },
            maximized: false,
        }
    }

    fn size(width: f64, height: f64) -> Size {
        Size { width, height }
    }

    #[test]
    fn nothing_saved_is_the_first_start_placement() {
        let p = plan_restore(None, &[primary()]);
        assert_eq!(p.outcome, Outcome::Fresh);
        assert_eq!(p.position, None);
        assert_eq!(p.size, DEFAULT_SIZE);
        assert!(!p.maximized);
        assert!(!p.restored);
    }

    #[test]
    fn no_usable_monitor_is_the_first_start_placement() {
        let s = Some(saved(100.0, 100.0, 1000.0, 700.0));
        assert_eq!(plan_restore(s, &[]).outcome, Outcome::NoMonitors);
        let zero = MonitorArea {
            bounds: rect(0.0, 0.0, 0.0, 0.0),
            work: rect(0.0, 0.0, 0.0, 0.0),
            per_logical: 1.0,
            primary: true,
        };
        let p = plan_restore(s, &[zero]);
        assert_eq!(p.outcome, Outcome::NoMonitors);
        assert_eq!(p.position, None);
        assert!(!p.restored);
    }

    #[test]
    fn an_on_screen_window_reopens_where_it_was() {
        let p = plan_restore(Some(saved(100.0, 100.0, 1000.0, 700.0)), &[primary()]);
        assert_eq!(p.outcome, Outcome::Restored);
        assert_eq!(p.position, Some(Point { x: 100.0, y: 100.0 }));
        assert_eq!(p.size, size(1000.0, 700.0));
        assert!(p.restored);
    }

    #[test]
    fn a_position_on_a_missing_monitor_is_dropped_but_the_size_is_kept() {
        let p = plan_restore(Some(saved(5000.0, 100.0, 1000.0, 700.0)), &[primary()]);
        assert_eq!(p.outcome, Outcome::OffScreen);
        assert_eq!(p.position, None);
        assert_eq!(p.size, size(1000.0, 700.0));
        assert!(p.restored);
    }

    #[test]
    fn a_sliver_on_screen_is_not_reachable() {
        // 10 px of a 1000 px window overlap the screen's left edge.
        let p = plan_restore(Some(saved(-990.0, 100.0, 1000.0, 700.0)), &[primary()]);
        assert_eq!(p.outcome, Outcome::OffScreen);
        assert_eq!(p.position, None);
    }

    #[test]
    fn a_title_bar_above_the_screen_is_not_reachable() {
        let p = plan_restore(Some(saved(100.0, -200.0, 1000.0, 700.0)), &[primary()]);
        assert_eq!(p.outcome, Outcome::OffScreen);
    }

    #[test]
    fn a_title_bar_below_the_screen_is_not_reachable() {
        let p = plan_restore(Some(saved(100.0, 1070.0, 1000.0, 700.0)), &[primary()]);
        assert_eq!(p.outcome, Outcome::OffScreen);
    }

    #[test]
    fn a_title_bar_under_a_top_taskbar_is_still_on_the_screen() {
        let top_taskbar = MonitorArea {
            bounds: rect(0.0, 0.0, 1920.0, 1080.0),
            work: rect(0.0, 48.0, 1920.0, 1032.0),
            per_logical: 1.0,
            primary: true,
        };
        let p = plan_restore(Some(saved(100.0, 10.0, 1000.0, 700.0)), &[top_taskbar]);
        assert_eq!(p.outcome, Outcome::Restored);
        assert_eq!(p.position, Some(Point { x: 100.0, y: 10.0 }));
    }

    #[test]
    fn a_window_on_the_second_monitor_reopens_there() {
        let monitors = [primary(), monitor_at(-1920.0, false)];
        let p = plan_restore(Some(saved(-1500.0, 100.0, 1000.0, 700.0)), &monitors);
        assert_eq!(p.outcome, Outcome::Restored);
        assert_eq!(
            p.position,
            Some(Point {
                x: -1500.0,
                y: 100.0
            })
        );
    }

    #[test]
    fn the_monitor_holding_most_of_the_title_bar_caps_the_size() {
        // A small left monitor holds 1300 px of the window, the big one 700.
        let small = MonitorArea {
            bounds: rect(-1366.0, 0.0, 1366.0, 768.0),
            work: rect(-1366.0, 0.0, 1366.0, 728.0),
            per_logical: 1.0,
            primary: false,
        };
        let p = plan_restore(
            Some(saved(-1300.0, 20.0, 2000.0, 900.0)),
            &[primary(), small],
        );
        assert_eq!(p.outcome, Outcome::Restored);
        assert_eq!(p.size, size(1366.0 - 16.0, 728.0 - 40.0));
    }

    #[test]
    fn a_window_larger_than_the_screen_is_cut_down_to_its_work_area() {
        let p = plan_restore(Some(saved(0.0, 0.0, 3000.0, 2000.0)), &[primary()]);
        assert_eq!(p.size, size(1904.0, 992.0));
    }

    #[test]
    fn the_width_never_drops_below_the_expanded_minimum() {
        let p = plan_restore(Some(saved(0.0, 0.0, 500.0, 600.0)), &[primary()]);
        assert_eq!(p.size.width, EXPANDED_MIN_WIDTH);
        let narrow = MonitorArea {
            bounds: rect(0.0, 0.0, 800.0, 600.0),
            work: rect(0.0, 0.0, 800.0, 560.0),
            per_logical: 1.0,
            primary: true,
        };
        let p = plan_restore(Some(saved(0.0, 0.0, 1000.0, 500.0)), &[narrow]);
        assert_eq!(p.size.width, EXPANDED_MIN_WIDTH);
    }

    #[test]
    fn a_hugged_height_below_the_placeholder_minimum_is_kept() {
        let p = plan_restore(Some(saved(0.0, 0.0, 820.0, 506.0)), &[primary()]);
        assert_eq!(p.size, size(820.0, 506.0));
    }

    #[test]
    fn a_small_screen_never_pushes_the_height_below_the_placeholder_minimum() {
        let short = MonitorArea {
            bounds: rect(0.0, 0.0, 1280.0, 440.0),
            work: rect(0.0, 0.0, 1280.0, 400.0),
            per_logical: 1.0,
            primary: true,
        };
        let p = plan_restore(Some(saved(0.0, 0.0, 1000.0, 900.0)), &[short]);
        assert_eq!(p.size.height, FALLBACK_MIN_HEIGHT);
    }

    #[test]
    fn thresholds_scale_with_the_monitor() {
        // 150 %: 200 physical px on screen is less than 160 logical px.
        let hidpi = MonitorArea {
            bounds: rect(0.0, 0.0, 2880.0, 1620.0),
            work: rect(0.0, 0.0, 2880.0, 1548.0),
            per_logical: 1.5,
            primary: true,
        };
        // 1000 logical px wide = 1500 physical; x = -1300 leaves 200 px on screen.
        let p = plan_restore(Some(saved(-1300.0, 100.0, 1000.0, 700.0)), &[hidpi]);
        assert_eq!(p.outcome, Outcome::OffScreen);
        // At 100 % the same 200 px is reachable.
        let p = plan_restore(Some(saved(-800.0, 100.0, 1000.0, 700.0)), &[primary()]);
        assert_eq!(p.outcome, Outcome::Restored);
        // The cap converts the work area to logical px: 2880 / 1.5 = 1920.
        let p = plan_restore(Some(saved(0.0, 0.0, 3000.0, 2000.0)), &[hidpi]);
        assert_eq!(p.size, size(1904.0, 992.0));
    }

    #[test]
    fn a_rescued_window_is_capped_by_the_primary_monitor() {
        let small_first = MonitorArea {
            bounds: rect(-1366.0, 0.0, 1366.0, 768.0),
            work: rect(-1366.0, 0.0, 1366.0, 728.0),
            per_logical: 1.0,
            primary: false,
        };
        let p = plan_restore(
            Some(saved(9000.0, 0.0, 1800.0, 1000.0)),
            &[small_first, primary()],
        );
        assert_eq!(p.outcome, Outcome::OffScreen);
        assert_eq!(p.size, size(1800.0, 992.0));
    }

    #[test]
    fn reachability_is_judged_at_the_size_the_window_opens_with() {
        // Saved 3000 wide at x=-1800: 1200 px of the SAVED width overlap the
        // screen, but it opens capped to 1904, leaving only 104 px on it.
        let p = plan_restore(Some(saved(-1800.0, 100.0, 3000.0, 700.0)), &[primary()]);
        assert_eq!(p.outcome, Outcome::OffScreen);
        assert_eq!(p.position, None);
    }

    #[test]
    fn without_a_primary_flag_the_first_monitor_caps_a_rescue() {
        let unflagged = monitor_at(0.0, false);
        let small = MonitorArea {
            bounds: rect(-1366.0, 0.0, 1366.0, 768.0),
            work: rect(-1366.0, 0.0, 1366.0, 728.0),
            per_logical: 1.0,
            primary: false,
        };
        let p = plan_restore(
            Some(saved(9000.0, 0.0, 1800.0, 1000.0)),
            &[small, unflagged],
        );
        assert_eq!(p.outcome, Outcome::OffScreen);
        assert_eq!(p.size, size(1350.0, 688.0));
    }

    #[test]
    fn saving_is_held_back_when_the_saved_state_could_not_be_checked() {
        // An unreadable file (not a damaged one) or no monitor to check it on:
        // the first reading would overwrite a good file with a guess.
        assert!(!may_save(true, Outcome::Fresh));
        assert!(!may_save(false, Outcome::NoMonitors));
        assert!(may_save(false, Outcome::Fresh));
        assert!(may_save(false, Outcome::Restored));
        // Off-screen: the position was dropped on purpose; the new place is
        // the one to remember.
        assert!(may_save(false, Outcome::OffScreen));
    }

    #[test]
    fn a_saved_state_without_a_position_lets_the_os_place_the_window() {
        let s = SavedWindow {
            position: None,
            size: size(1000.0, 700.0),
            maximized: true,
        };
        let p = plan_restore(Some(s), &[primary()]);
        assert_eq!(p.outcome, Outcome::Restored);
        assert_eq!(p.position, None);
        assert_eq!(p.size, size(1000.0, 700.0));
        assert!(p.maximized);
    }

    #[test]
    fn the_maximized_flag_is_carried() {
        let mut s = saved(100.0, 100.0, 1000.0, 700.0);
        s.maximized = true;
        assert!(plan_restore(Some(s), &[primary()]).maximized);
    }

    #[test]
    fn impossible_values_are_rejected() {
        let ok = saved(-1500.0, 20.0, 1000.0, 700.0);
        assert_eq!(validated(ok), Some(ok));
        for (w, h) in [
            (0.0, 700.0),
            (1000.0, -1.0),
            (200_000.0, 700.0),
            (f64::NAN, 700.0),
        ] {
            assert_eq!(validated(saved(0.0, 0.0, w, h)), None, "{w}x{h}");
        }
        assert_eq!(validated(saved(f64::INFINITY, 0.0, 1000.0, 700.0)), None);
    }

    fn observed(x: f64, y: f64, width: f64, height: f64) -> Observed {
        Observed {
            position: Some(Point { x, y }),
            logical_size: size(width, height),
            maximized: false,
            minimized: false,
            visible: true,
        }
    }

    const EXPANDED: ModeSnapshot = ModeSnapshot {
        compact: false,
        captured_expanded: None,
    };

    #[test]
    fn a_normal_expanded_window_is_recorded_as_it_is() {
        let next = next_saved(None, &observed(10.0, 20.0, 1084.0, 861.0), &EXPANDED);
        assert_eq!(next, Some(saved(10.0, 20.0, 1084.0, 861.0)));
    }

    #[test]
    fn a_minimized_or_hidden_window_changes_nothing() {
        let prev = Some(saved(10.0, 20.0, 1084.0, 861.0));
        let mut o = observed(-32000.0, -32000.0, 0.0, 0.0);
        o.minimized = true;
        assert_eq!(next_saved(prev, &o, &EXPANDED), None);
        let mut o = observed(50.0, 50.0, 1084.0, 861.0);
        o.visible = false;
        assert_eq!(next_saved(prev, &o, &EXPANDED), None);
    }

    #[test]
    fn a_maximized_window_keeps_its_normal_rect() {
        let prev = Some(saved(10.0, 20.0, 1084.0, 861.0));
        let mut o = observed(-8.0, -8.0, 1920.0, 1009.0);
        o.maximized = true;
        let next = next_saved(prev, &o, &EXPANDED);
        assert_eq!(
            next,
            Some(SavedWindow {
                maximized: true,
                ..saved(10.0, 20.0, 1084.0, 861.0)
            })
        );
    }

    #[test]
    fn a_maximized_window_with_nothing_saved_has_no_position() {
        let mut o = observed(-8.0, -8.0, 1920.0, 1009.0);
        o.maximized = true;
        let next = next_saved(None, &o, &EXPANDED);
        assert_eq!(
            next,
            Some(SavedWindow {
                position: None,
                size: DEFAULT_SIZE,
                maximized: true,
            })
        );
    }

    #[test]
    fn a_compact_strip_saves_its_position_and_the_expanded_size() {
        let prev = Some(saved(10.0, 20.0, 1084.0, 861.0));
        let mode = ModeSnapshot {
            compact: true,
            captured_expanded: Some(size(1084.0, 861.0)),
        };
        let next = next_saved(prev, &observed(300.0, 40.0, 240.0, 707.0), &mode);
        assert_eq!(next, Some(saved(300.0, 40.0, 1084.0, 861.0)));
    }

    #[test]
    fn a_compact_strip_without_a_captured_size_keeps_the_saved_one() {
        let prev = Some(saved(10.0, 20.0, 1200.0, 800.0));
        let mode = ModeSnapshot {
            compact: true,
            captured_expanded: None,
        };
        let next = next_saved(prev, &observed(300.0, 40.0, 240.0, 707.0), &mode);
        assert_eq!(next, Some(saved(300.0, 40.0, 1200.0, 800.0)));
        let next = next_saved(None, &observed(300.0, 40.0, 240.0, 707.0), &mode);
        assert_eq!(next.map(|s| s.size), Some(DEFAULT_SIZE));
    }

    #[test]
    fn a_strip_width_window_outside_compact_is_a_transition() {
        let prev = Some(saved(10.0, 20.0, 1084.0, 861.0));
        let next = next_saved(prev, &observed(300.0, 40.0, 240.0, 707.0), &EXPANDED);
        assert_eq!(next, Some(saved(300.0, 40.0, 1084.0, 861.0)));
    }

    #[test]
    fn an_unchanged_window_writes_nothing() {
        let prev = Some(saved(10.0, 20.0, 1084.0, 861.0));
        assert_eq!(
            next_saved(prev, &observed(10.0, 20.0, 1084.0, 861.0), &EXPANDED),
            None
        );
    }

    #[test]
    fn a_platform_without_positions_keeps_the_saved_one() {
        let prev = Some(saved(10.0, 20.0, 1084.0, 861.0));
        let mut o = observed(0.0, 0.0, 1200.0, 900.0);
        o.position = None;
        let next = next_saved(prev, &o, &EXPANDED);
        assert_eq!(next, Some(saved(10.0, 20.0, 1200.0, 900.0)));
    }

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |key| {
            pairs
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| (*v).to_string())
        }
    }

    #[test]
    fn x11_is_detected_only_when_certain() {
        assert!(x11_from_env(env(&[("GDK_BACKEND", "x11")])));
        assert!(x11_from_env(env(&[("GDK_BACKEND", "x11,wayland")])));
        assert!(!x11_from_env(env(&[("GDK_BACKEND", "wayland,x11")])));
        assert!(!x11_from_env(env(&[
            ("WAYLAND_DISPLAY", "wayland-0"),
            ("DISPLAY", ":0")
        ])));
        assert!(x11_from_env(env(&[("DISPLAY", ":0")])));
        assert!(x11_from_env(env(&[
            ("WAYLAND_DISPLAY", ""),
            ("DISPLAY", ":0")
        ])));
        assert!(!x11_from_env(env(&[])));
    }
}
