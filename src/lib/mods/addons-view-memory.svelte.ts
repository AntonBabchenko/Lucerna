import type { ModSort } from '$lib/ipc/bindings';
import type { InstanceContentKind } from './content-kind';

export type AddonsView = 'browse' | 'installed';

// The Add-ons sub-view last chosen per content kind, for the session (spec D10). Module state on
// purpose: AddonsTab remounts on every top-level tab switch, so component state forgets. A plain
// Map — read at mount and on a kind change, never rendered.
const remembered = new Map<InstanceContentKind, AddonsView>();

export function rememberAddonsView(kind: InstanceContentKind, view: AddonsView): void {
  remembered.set(kind, view);
}

/** The kind's remembered view; else, on a first visit, Installed for mods when the active profile
 *  has any — or while that is not known yet (`null`: the count lands a moment after a start or a
 *  profile switch; Installed then shows what is there, an empty list its full drop area); else
 *  Browse. */
export function initialAddonsView(
  kind: InstanceContentKind,
  hasInstalledMods: boolean | null,
): AddonsView {
  return (
    remembered.get(kind) ?? (kind === 'mod' && hasInstalledMods !== false ? 'installed' : 'browse')
  );
}

// What the Browse catalogue last searched, per content kind, for the session.
// The Settings tab replaced a dialog that opened OVER Add-ons, so a trip to
// change the loader now unmounts the catalogue; without this the typed search
// was gone on the way back. Only the query and the sort: the MC / loader
// filters re-seed from the instance on purpose (a loader change is often the
// reason for the trip), and the page and results reload.
export type BrowseMemory = { query: string; sort: ModSort };
const browse = new Map<InstanceContentKind, BrowseMemory>();

export function rememberBrowse(kind: InstanceContentKind, memory: BrowseMemory): void {
  browse.set(kind, memory);
}

export function initialBrowse(kind: InstanceContentKind): BrowseMemory {
  return browse.get(kind) ?? { query: '', sort: 'downloads' };
}

/** Tests only. */
export function resetAddonsViewMemory(): void {
  remembered.clear();
  browse.clear();
}
