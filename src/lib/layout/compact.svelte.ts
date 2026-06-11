// Compact / mini launcher-mode runtime. A `$state(...)` rune module (same
// cross-component pattern as `settings/state.svelte.ts`) that drives the
// reactive grid in `+page.svelte` and is toggled from `Sidebar.svelte`.
//
// `setCompact` is the user-driven path: it flips the rune, resizes the OS
// window via the Rust command, and persists the choice into GeneralSettings
// (read-modify-write, mirroring `theme/state.svelte.ts`). `initCompact` is the
// startup path: it applies the persisted mode without re-persisting.
//
// `observeCompactContent` keeps the strip honest after the initial sizing: the
// compact layout's height is not fixed (the install/mod status row appears and
// disappears, the offline-name input expands, etc.), so a one-shot measurement
// goes stale and the sidebar starts to scroll. While compact, a ResizeObserver
// re-applies the window height whenever the content height changes.

import { commands } from '$lib/ipc/bindings';

export const compactState = $state<{ value: boolean }>({ value: false });

/** Last height (logical px) the auto-resize observer applied, so it can skip
 *  redundant resizes — and so the OS resize it triggers doesn't feed back into
 *  the observer as a fresh change. */
let lastObservedHeight: number | null = null;

/**
 * Measure the natural height (logical px) of the compact layout so the window
 * can shrink to end at its content instead of inheriting the taller expanded
 * height. The compact layout is two grid rows: the sidebar (row 1) and the
 * page-level install/mod status row (row 2, `[data-phase-row]`), which appears
 * and disappears dynamically. Both must be counted — sizing to the sidebar
 * alone leaves the status row with no room, squeezing the sidebar into a
 * scrollbar.
 *
 * Returns null when nothing can be measured (no DOM / sidebar not mounted), in
 * which case the backend keeps the current height. The sidebar is `h-full` and
 * taller than its content when expanded, so `scrollHeight` reports the
 * container height, not the content — instead we measure from the sidebar's top
 * to its last child's bottom plus bottom padding, then add the status row's
 * rendered height.
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

  // Grid row 2: the install/mod progress strip beneath the sidebar. Present
  // only while an install/mod pipeline reports progress; its `auto` row means
  // its rendered height is its content height (0 when it renders nothing).
  const phaseRow = document.querySelector('[data-phase-row]');
  if (phaseRow instanceof HTMLElement) {
    height += phaseRow.getBoundingClientRect().height;
  }

  const rounded = Math.ceil(height);
  return rounded > 0 ? rounded : null;
}

/** Apply + persist a mode change (user toggle). */
export async function setCompact(next: boolean): Promise<void> {
  const prev = compactState.value;
  compactState.value = next;
  // Optimistic flip drives the CSS grid immediately. If the OS-window resize
  // fails, roll the rune back so the layout and the actual window stay in sync
  // (and skip persisting a mode we couldn't apply). The content height is sent
  // in both directions: compact uses it as the window height, expand uses it as
  // the minimum height floor.
  const height = measureCompactContentHeight();
  const resized = await commands.windowSetCompact(next, height);
  if (resized.status !== 'ok') {
    compactState.value = prev;
    return;
  }
  // Seed the observer's baseline so its first tick doesn't re-apply the height
  // we just set as if it were a new change.
  lastObservedHeight = next ? height : null;
  const get = await commands.appSettingsGet();
  if (get.status !== 'ok') return;
  await commands.appSettingsSetGeneral({ ...get.data.general, compact_mode: next });
}

/** Toggle between compact and expanded. */
export function toggleCompact(): Promise<void> {
  return setCompact(!compactState.value);
}

/**
 * Startup: apply the persisted mode to window + rune without re-persisting.
 * Always calls the backend — even when expanded — so the minimum-height floor
 * (the sidebar content height) is applied on launch, not just after a toggle.
 */
export async function initCompact(persisted: boolean): Promise<void> {
  compactState.value = persisted;
  const height = measureCompactContentHeight();
  lastObservedHeight = persisted ? height : null;
  await commands.windowSetCompact(persisted, height);
}

/**
 * While compact, keep the window height synced to the live content height.
 * Observes the sidebar and the status-row wrapper; when either changes height
 * (status row appearing/disappearing, offline-name input expanding, ...) it
 * re-applies the compact window size so the strip always ends exactly at its
 * content instead of growing a scrollbar.
 *
 * Returns a disposer; no-op (and returns a no-op disposer) when there's no DOM
 * or no ResizeObserver. Safe to call once on mount — it self-guards on
 * `compactState.value`, so it stays idle while expanded.
 */
export function observeCompactContent(): () => void {
  if (typeof document === 'undefined' || typeof ResizeObserver === 'undefined') {
    return () => {};
  }
  const aside = document.querySelector('[data-sidebar]');
  const phaseRow = document.querySelector('[data-phase-row]');

  const observer = new ResizeObserver(() => {
    if (!compactState.value) return;
    const height = measureCompactContentHeight();
    if (height === null || height === lastObservedHeight) return;
    lastObservedHeight = height;
    // Re-apply compact sizing only (idempotent for the compact branch: the
    // window is already strip-width, so the backend skips re-capturing the
    // expanded size and just adjusts the height).
    void commands.windowSetCompact(true, height);
  });

  if (aside instanceof HTMLElement) observer.observe(aside);
  if (phaseRow instanceof HTMLElement) observer.observe(phaseRow);
  return () => observer.disconnect();
}
