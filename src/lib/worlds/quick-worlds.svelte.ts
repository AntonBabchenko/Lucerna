// Owns the lightweight world list for the sidebar Play-button dropdown.
// Loads folder names + recency for the active instance via the cheap
// `list_world_names` command (no size/backup walk), and reloads after the
// game exits since a session may have created or touched a world.
//
// Errors are swallowed to an empty list on purpose: the dropdown is an
// enhancement layered on top of Play and must never raise a banner — Play
// itself keeps working, the menu just doesn't appear. A monotonic `seq`
// guard drops a slow stale response (same idiom as createMcVersions).
//
// No `$effect` here: plain `$state` plus an imperative event listener torn
// down in `dispose()`. The page instantiates one, drives it with
// `load(id)` / `clear()`, and calls `dispose()` on destroy.

import { commands, events, type WorldQuickEntry } from '$lib/ipc/bindings';

export function createQuickWorlds() {
  let worlds = $state<WorldQuickEntry[]>([]);

  let seq = 0;
  let instanceId: string | null = null;
  let unlisten: (() => void) | null = null;
  let disposed = false;

  async function fetchFor(id: string): Promise<void> {
    const mine = ++seq;
    const r = await commands.listWorldNames(id);
    if (mine !== seq || disposed) return; // superseded or torn down
    worlds = r.status === 'ok' ? r.data : [];
  }

  void events.processExited
    .listen(() => {
      if (instanceId !== null) void fetchFor(instanceId);
    })
    .then((u) => {
      if (disposed) u();
      else unlisten = u;
    });

  return {
    get worlds() {
      return worlds;
    },
    // Load (or switch to) the given instance's worlds.
    load(id: string): void {
      instanceId = id;
      void fetchFor(id);
    },
    // Drop the list — instance not eligible for quick-play, or none selected.
    clear(): void {
      instanceId = null;
      seq++; // cancel any in-flight write
      worlds = [];
    },
    dispose(): void {
      disposed = true;
      seq++; // belt-and-suspenders; the `disposed` guard already drops in-flight writes
      if (unlisten) unlisten();
    },
  };
}

export type QuickWorlds = ReturnType<typeof createQuickWorlds>;
