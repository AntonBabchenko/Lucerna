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

/** Apply + persist a mode change (user toggle). */
export async function setCompact(next: boolean): Promise<void> {
  compactState.value = next;
  await commands.windowSetCompact(next);
  const get = await commands.appSettingsGet();
  if (get.status !== 'ok') return;
  await commands.appSettingsSetGeneral({ ...get.data.general, compact_mode: next });
}

/** Toggle between compact and expanded. */
export function toggleCompact(): Promise<void> {
  return setCompact(!compactState.value);
}

/** Startup: apply the persisted mode to window + rune without re-persisting. */
export async function initCompact(persisted: boolean): Promise<void> {
  compactState.value = persisted;
  if (persisted) await commands.windowSetCompact(true);
}
