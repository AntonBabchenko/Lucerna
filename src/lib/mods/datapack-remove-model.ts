// The placement rules of the datapack removal dialogs, shared by the single-pack dialog
// (`DatapackRemoveDialog`) and the batch one (`DatapackBulkRemoveDialog`), so one world is judged
// the same way in both.
import type { DatapackPlacementView } from '$lib/ipc/bindings';

export type RemoveDialogMode = 'library' | 'worlds-only' | 'this-world';

/**
 * Unknown state in a folder that has a level file (or whose level file could not be told): could
 * not check. A folder with neither level file has no list to hold a state (`absent`) — it is an
 * affected world the cascade unlinks. An `unreadable` entry is Lucerna failing to read it, not the
 * game ignoring it: the cascade cannot compare it with the library copy either. The same mark with
 * no state is a placement Lucerna could not check at all (the world's datapacks/ unreadable, or R2
 * could not tell), in any folder — the one predicate covers both.
 */
export function isUncheckedPlacement(p: DatapackPlacementView): boolean {
  return (p.state === null && p.level_dat !== 'absent') || p.ignored_reason === 'unreadable';
}

export type PlacementSplit = {
  /** The worlds a removal will try. */
  tried: DatapackPlacementView[];
  /** D2: worlds-only removal goes world by world through the world writers, which refuse a folder
   *  with no level.dat or only level.dat_old — those are listed apart and never tried. The library
   *  cascade differs: it unlinks the file in a folder with no level file (A3) and fails an only-old
   *  world, which the only-old note says up front. */
  unchanged: DatapackPlacementView[];
  /** Tried worlds Lucerna could check. */
  affected: DatapackPlacementView[];
  /** Tried worlds Lucerna could not check — their own list, never dropped (Fallback Q2). */
  unchecked: DatapackPlacementView[];
  anyOnlyOld: boolean;
};

export function splitPlacements(
  kind: RemoveDialogMode,
  placements: readonly DatapackPlacementView[],
): PlacementSplit {
  const unchanged =
    kind === 'worlds-only'
      ? placements.filter((p) => p.level_dat === 'absent' || p.level_dat === 'only_old')
      : [];
  const tried = placements.filter((p) => !unchanged.includes(p));
  return {
    tried,
    unchanged,
    affected: tried.filter((p) => !isUncheckedPlacement(p)),
    unchecked: tried.filter(isUncheckedPlacement),
    anyOnlyOld: placements.some((p) => p.level_dat === 'only_old'),
  };
}
