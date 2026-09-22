// Runtime visibility state for the hideable sidebar buttons. A `$state(...)`
// rune module (same cross-component pattern as `compact.svelte.ts`): the sidebar
// reads `isVisible(id)`, the settings panel calls `setHidden(id, hidden)`.
//
// `setHidden` is the user-driven path: optimistically flips the local set (so the
// UI updates immediately), then read-modify-writes GeneralSettings, rolling back
// if the load or the persist fails (mirrors `setCompact`). `initSidebarButtons`
// is the startup path: it seeds the set from the loaded GeneralSettings WITHOUT
// re-persisting. The set defaults to empty (everything visible), so the sidebar
// renders every button until the persisted state is applied.

import { patchGeneral } from '$lib/settings/app-settings.svelte';
import type { SidebarButtonId } from './sidebar-buttons';

const state = $state<{ hidden: string[] }>({ hidden: [] });

/** Startup seed from loaded GeneralSettings; does NOT persist. */
export function initSidebarButtons(hidden: string[]): void {
  state.hidden = [...hidden];
}

export function isVisible(id: SidebarButtonId): boolean {
  return !state.hidden.includes(id);
}

/** Toggle a button's visibility and persist the hidden set through the one
 *  settings contract. Optimistic; rolls back on failure — but only the set
 *  still displayed: a newer toggle may have landed meanwhile, and reverting to
 *  the set before THIS one would un-hide it. */
export async function setHidden(id: SidebarButtonId, hidden: boolean): Promise<void> {
  const prev = state.hidden;
  const next = hidden ? [...new Set([...prev, id])] : prev.filter((x) => x !== id);
  state.hidden = next; // optimistic → drives the UI now
  const r = await patchGeneral({ hidden_sidebar_buttons: next });
  if (!r.ok && sameSet(state.hidden, next)) state.hidden = prev;
}

function sameSet(a: string[], b: string[]): boolean {
  return a.length === b.length && a.every((x) => b.includes(x));
}
