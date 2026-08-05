import { displayLoaderTag } from '$lib/instances/loader-display';
import type { ModpackVersionEntry } from '$lib/ipc/bindings';

// Why a given pack version is the one an install button will fetch. The rule
// lives in the callers — the backend returns versions newest-first and the UI
// takes [0], optionally after filtering by the browse toolbar's MC version —
// this only *names* it so the button can say so out loud.
export type PickReason = 'newest' | 'newestFiltered';

export type InstallPick = {
  reason: PickReason;
  // Joined display strings, or null when the source reports nothing. Null is
  // load-bearing: CurseForge and ATLauncher pack versions can carry no loader,
  // and an empty segment must disappear rather than render as an empty slot.
  mc: string | null;
  loaders: string | null;
};

export function summarisePick(v: ModpackVersionEntry, mcFilter: string | null): InstallPick {
  return {
    reason: mcFilter ? 'newestFiltered' : 'newest',
    mc: v.game_versions.length > 0 ? v.game_versions.join(', ') : null,
    loaders: v.loaders.length > 0 ? v.loaders.map(displayLoaderTag).join(', ') : null,
  };
}

// Join summary segments with a middot, dropping the ones the source could not
// tell us, so a missing segment never leaves a dangling separator.
export function joinSummary(parts: (string | null | undefined)[]): string {
  return parts.filter((p): p is string => typeof p === 'string' && p.trim() !== '').join(' · ');
}
