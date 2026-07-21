// Race-safe Tauri event subscription for components.
//
// `events.*.listen()` resolves asynchronously. A component that unmounts
// before registration completes leaks the listener if cleanup only iterates
// an array of already-resolved unlisteners — the unlistener lands in the
// array AFTER onDestroy has already run. The fix (originally worked out in
// InstalledModsView) is to keep the raw promises: onDestroy awaits each one,
// and a promise that resolves after teardown unlistens immediately on
// arrival. This helper is that pattern, shared.

import { onDestroy } from 'svelte';

type Unlisten = () => void;

/**
 * Register Tauri event listeners and tear them down on component destroy,
 * race-safely. Must be called during component initialization (it registers
 * an `onDestroy`). Pass the raw `listen()` promises:
 *
 *   listenUntilDestroyed([
 *     events.modInstalled.listen(() => refresh()),
 *     events.modUninstalled.listen(() => refresh()),
 *   ]);
 */
export function listenUntilDestroyed(promises: Array<Promise<Unlisten>>): void {
  let destroyed = false;
  for (const p of promises) {
    void p.then((un) => {
      if (destroyed) un();
    });
  }
  onDestroy(() => {
    destroyed = true;
    for (const p of promises) void p.then((un) => un());
  });
}
