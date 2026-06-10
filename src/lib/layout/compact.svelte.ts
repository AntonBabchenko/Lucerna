// Compact / mini launcher-mode runtime. A `$state(...)` rune module (same
// cross-component pattern as `settings/state.svelte.ts`) that drives the
// reactive grid in `+page.svelte` and is toggled from `Sidebar.svelte`.
//
// `setCompact` is the user-driven path: it flips the rune, resizes the OS
// window via the Rust command, and persists the choice into GeneralSettings
// (read-modify-write, mirroring `theme/state.svelte.ts`). `initCompact` is the
// startup path: it applies the persisted mode without re-persisting.

import { commands } from '$lib/ipc/bindings';

export const compactState = $state<{ value: boolean }>({ value: false });

/**
 * Measure the sidebar's natural content height (logical px) so the compact
 * window can shrink its height to end at the bottom buttons instead of
 * inheriting the taller expanded height. Returns null when it can't be measured
 * (no DOM / sidebar not mounted), in which case the backend keeps the current
 * height. The sidebar is `h-full` and taller than its content when expanded, so
 * `scrollHeight` reports the container height, not the content — instead we
 * measure from the sidebar's top to its last child's bottom plus bottom padding.
 */
function measureSidebarContentHeight(): number | null {
  if (typeof document === 'undefined') return null;
  const aside = document.querySelector('[data-sidebar]');
  if (!(aside instanceof HTMLElement)) return null;
  const last = aside.lastElementChild;
  if (!last) return null;
  const asideTop = aside.getBoundingClientRect().top;
  const contentBottom = last.getBoundingClientRect().bottom;
  const padBottom = Number.parseFloat(getComputedStyle(aside).paddingBottom) || 0;
  const height = Math.ceil(contentBottom - asideTop + padBottom);
  return height > 0 ? height : null;
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
  const resized = await commands.windowSetCompact(next, measureSidebarContentHeight());
  if (resized.status !== 'ok') {
    compactState.value = prev;
    return;
  }
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
  await commands.windowSetCompact(persisted, measureSidebarContentHeight());
}
