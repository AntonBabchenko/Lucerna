// Compact / mini launcher-mode runtime. A `$state(...)` rune module (same
// cross-component pattern as `settings/state.svelte.ts`) that drives the
// reactive grid in `+page.svelte` and is toggled from `Sidebar.svelte`.
//
// `setCompact` is the user-driven path: it flips the rune, resizes the OS
// window via the Rust command, and persists the choice into GeneralSettings
// (read-modify-write, mirroring `theme/state.svelte.ts`). `initCompact` is the
// startup path: it applies the persisted mode without re-persisting.
//
// `observeCompactContent` keeps the window's min-height floor synced to the live
// sidebar content height in BOTH modes:
//  - compact: the content height is the window height (the strip ends exactly at
//    its content instead of growing a scrollbar);
//  - expanded: the content height is the min-size floor. The first application
//    arms it (on Windows/wry a programmatic min-size is only enforced after a
//    `set_size`, so the startup application resizes the window to the content
//    height); later content changes (a live locale switch changing button-label
//    sizes, accounts/instances appearing) re-apply it and grow the window if the
//    bottom buttons would otherwise clip.

import { get as storeGet } from 'svelte/store';
import { t } from '$lib/i18n';
import { commands } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';
import { pushWarning } from '$lib/toasts/toasts.svelte';

export const compactState = $state<{ value: boolean }>({ value: false });

/** Last height (logical px) the observer applied, so it can skip redundant
 *  resizes — and so the OS resize it triggers doesn't feed back into the
 *  observer as a fresh change. */
let lastObservedHeight: number | null = null;

/** True for the one-shot startup application of the expanded floor, which must
 *  `set_size` the window to the content height to ARM the min-size (wry only
 *  enforces a programmatic min after a set_size). Cleared after the first apply. */
let needsExpandedHug = false;

/**
 * Measure the natural height (logical px) of the sidebar content so the window
 * can hug it (compact) or floor its minimum at it (expanded). The compact
 * layout is two grid rows: the sidebar (row 1) and the page-level install/mod
 * status row (row 2, `[data-phase-row]`), which appears and disappears
 * dynamically. In compact mode it must be counted — the status row is stacked
 * below the sidebar in the single column, so omitting it leaves the row no room
 * and squeezes the sidebar into a scrollbar. In expanded mode the sidebar spans
 * BOTH grid rows (the status row sits only under the content column), so the
 * status row never steals sidebar height and is excluded from the floor —
 * otherwise the window would twitch taller every time an install footer appears.
 *
 * Returns null when nothing can be measured (no DOM / sidebar not mounted), in
 * which case the caller keeps the current height. The `<aside>` is `h-full` and
 * taller than its content when expanded, so `scrollHeight` reports the container
 * height, not the content — instead we measure from the sidebar's top to its
 * last child's bottom plus bottom padding (the last child is the
 * `[data-sidebar-content]` wrapper), then — in compact mode only — add the
 * status row's rendered height.
 */
function measureCompactContentHeight(): number | null {
  if (typeof document === 'undefined') return null;
  const aside = document.querySelector('[data-sidebar]');
  if (!(aside instanceof HTMLElement)) return null;
  const last = aside.lastElementChild;
  if (!last) return null;
  const asideTop = aside.getBoundingClientRect().top;
  const contentBottom = last.getBoundingClientRect().bottom;
  const padBottom = Number.parseFloat(getComputedStyle(aside).paddingBottom) || 0;
  let height = contentBottom - asideTop + padBottom;

  // Grid row 2: the install/mod progress strip. In compact mode it's stacked
  // below the sidebar in the single column, so its height is part of the window
  // height. In expanded mode the sidebar spans both rows and the strip sits only
  // under the content column, so it must NOT be added — else the floor (and the
  // window) grows when an install footer appears, squeezing the sidebar.
  if (compactState.value) {
    const phaseRow = document.querySelector('[data-phase-row]');
    if (phaseRow instanceof HTMLElement) {
      height += phaseRow.getBoundingClientRect().height;
    }
  }

  const rounded = Math.ceil(height);
  return rounded > 0 ? rounded : null;
}

/**
 * Apply (and, on the first call, arm) the expanded window's min-height floor.
 * `hug` is true exactly once (startup): the backend resizes the window to the
 * content height, which both hugs the bottom buttons and arms the min-size.
 */
function applyExpandedFloor(height: number): void {
  lastObservedHeight = height;
  const hug = needsExpandedHug;
  needsExpandedHug = false;
  void commands.windowSetExpandedFloor(height, hug);
}

/** Apply + persist a mode change (user toggle). */
export async function setCompact(next: boolean): Promise<void> {
  const prev = compactState.value;
  compactState.value = next;
  // Optimistic flip drives the CSS grid immediately. If the OS-window resize
  // fails, roll the rune back so the layout and the actual window stay in sync
  // (and skip persisting a mode we couldn't apply). Going compact sends the
  // content height as the window height; going expanded restores the captured
  // expanded size (and that set_size also re-arms the min-size floor).
  const height = measureCompactContentHeight();
  const resized = await commands.windowSetCompact(next, height);
  if (resized.status !== 'ok') {
    compactState.value = prev;
    return;
  }
  // Seed the observer's baseline so its first tick doesn't re-apply the height
  // we just set as if it were a new change. Expanding re-arms the floor on the
  // observer's next content measurement.
  lastObservedHeight = next ? height : null;
  needsExpandedHug = !next;
  const settings = await commands.appSettingsGet();
  const written =
    settings.status === 'ok' &&
    (await commands.appSettingsSetGeneral({ ...settings.data.general, compact_mode: next }))
      .status === 'ok';
  if (written) return;
  // Deliberately NOT a rollback, unlike setExplanationLevel / setHidden. Those
  // persist a pure UI preference, so reverting the rune fully undoes the action.
  // Here the action already SUCCEEDED — the OS window has been resized and the
  // grid has reflowed. Undoing that would mean a second window call on a
  // recovery path, whose own failure would then need handling (CLAUDE.md
  // fallback discipline, Q4), to withdraw something the user can see working.
  // Only the "remember this" half failed, and its consequence lands at the next
  // launch, so the honest move is to say so (Q3).
  const reason = settings.status === 'ok' ? null : formatError(settings.error);
  pushWarning(storeGet(t)('sidebar.compactPersistFailed'), reason ? [reason] : []);
}

/** Toggle between compact and expanded. */
export function toggleCompact(): Promise<void> {
  return setCompact(!compactState.value);
}

/**
 * Startup: apply the persisted mode to window + rune without re-persisting.
 * Compact uses the existing `set_size`-based path (which arms its own min).
 * Expanded defers the floor to a post-layout measurement and arms it once: the
 * measurement at mount-time can be null (the sidebar isn't laid out yet), in
 * which case `observeCompactContent`'s first fire performs the (still pending)
 * hug.
 */
export async function initCompact(persisted: boolean): Promise<void> {
  compactState.value = persisted;
  if (persisted) {
    const height = measureCompactContentHeight();
    lastObservedHeight = height;
    await commands.windowSetCompact(true, height);
    return;
  }
  needsExpandedHug = true;
  const height = measureCompactContentHeight();
  if (height !== null) applyExpandedFloor(height);
}

/**
 * Keep the window's min-height floor synced to the live sidebar content height
 * in both modes. Observes the `[data-sidebar-content]` wrapper (its box equals
 * the content height, unlike the `h-full` `<aside>` whose box tracks the window)
 * and the page-level status row, so it fires on content reflow (e.g. a live
 * locale switch) — not on window resizes, which avoids any feedback loop.
 *
 * Returns a disposer; no-op (and returns a no-op disposer) when there's no DOM
 * or no ResizeObserver. Safe to call once on mount.
 */
export function observeCompactContent(): () => void {
  if (typeof document === 'undefined' || typeof ResizeObserver === 'undefined') {
    return () => {};
  }
  // Prefer the content wrapper (box == content height); fall back to the sidebar
  // element when the wrapper is absent (e.g. minimal test DOM).
  const content =
    document.querySelector('[data-sidebar-content]') ?? document.querySelector('[data-sidebar]');
  const phaseRow = document.querySelector('[data-phase-row]');

  const observer = new ResizeObserver(() => {
    const height = measureCompactContentHeight();
    if (height === null) return;
    if (compactState.value) {
      if (height === lastObservedHeight) return;
      lastObservedHeight = height;
      // Re-apply compact sizing only (idempotent for the compact branch: the
      // window is already strip-width, so the backend skips re-capturing the
      // expanded size and just adjusts the height).
      void commands.windowSetCompact(true, height);
    } else if (needsExpandedHug || height !== lastObservedHeight) {
      // Expanded: arm on the first fire, then re-apply the floor whenever the
      // content height changes (locale switch / accounts / instances).
      applyExpandedFloor(height);
    }
  });

  if (content instanceof HTMLElement) observer.observe(content);
  if (phaseRow instanceof HTMLElement) observer.observe(phaseRow);
  return () => observer.disconnect();
}
