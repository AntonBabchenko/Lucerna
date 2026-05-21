import type { InstalledMod, ModpackStatus } from '$lib/ipc/bindings';

// Last-loaded ImportedDetailDrawer payload per instance id, for the
// current app session. The drawer is destroyed when closed, so without
// this it re-mounts empty and flashes "Loading…" on every reopen.
// Seeding from this cache makes a reopen instant; load() then silently
// revalidates. Module-scoped — never persisted to disk.
export type DrawerSnapshot = {
  mods: InstalledMod[];
  status: ModpackStatus | null;
  nameMap: Map<string, string>;
};

export const drawerCache = new Map<string, DrawerSnapshot>();
