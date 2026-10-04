// src/lib/worlds/datapack-bulk.ts
// Which rows of a world's data pack list a bulk action applies to — exactly the rows whose own
// control does the same thing (WorldDatapacks.svelte): a live row toggles, an addable row adds
// unless this version skips the pack (§0.5 I11), and removal covers everything that IS in the
// world's list (live, ignored, ghost) — never a row that is only in the library.
import type { WorldDatapack } from '$lib/ipc/bindings';
import { worldRowKind } from './datapacks-gating';

export type WorldBulkAction = 'enable' | 'disable' | 'add' | 'remove';

export function worldBulkApplies(pack: WorldDatapack, action: WorldBulkAction): boolean {
  const kind = worldRowKind(pack);
  switch (action) {
    case 'enable':
      return kind === 'live' && pack.state === 'disabled';
    case 'disable':
      return kind === 'live' && pack.state === 'enabled';
    case 'add':
      return kind === 'addable' && pack.compat.kind !== 'wont_load';
    case 'remove':
      return kind !== 'addable';
  }
}
