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

import { commands } from '$lib/ipc/bindings';
import type { SidebarButtonId } from './sidebar-buttons';

const state = $state<{ hidden: string[] }>({ hidden: [] });

/** Startup seed from loaded GeneralSettings; does NOT persist. */
export function initSidebarButtons(hidden: string[]): void {
  state.hidden = [...hidden];
}

export function isVisible(id: SidebarButtonId): boolean {
  return !state.hidden.includes(id);
}

/** Toggle a button's visibility and persist the hidden set (read-modify-write of
 *  GeneralSettings, like setCompact). Optimistic; rolls back on failure. */
export async function setHidden(id: SidebarButtonId, hidden: boolean): Promise<void> {
  const prev = state.hidden;
  const next = hidden ? [...new Set([...prev, id])] : prev.filter((x) => x !== id);
  state.hidden = next; // optimistic → drives the UI now
  const get = await commands.appSettingsGet();
  if (get.status !== 'ok') {
    state.hidden = prev;
    return;
  }
  const res = await commands.appSettingsSetGeneral({
    ...get.data.general,
    hidden_sidebar_buttons: next,
  });
  if (res.status !== 'ok') state.hidden = prev; // roll back on persist failure
}
