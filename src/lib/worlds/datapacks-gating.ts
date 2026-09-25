import type { TranslationKey } from '$lib/i18n/keys.generated';
import type { DatapackPlacementView, WorldDatapack } from '$lib/ipc/bindings';

export interface DatapacksGateState {
  /** The instance's game process is alive. */
  running: boolean;
  /** Another datapack mutation is already in flight. */
  busy: boolean;
}

/**
 * Why a datapack change is unavailable, or null when it is available.
 * Pure so the tooltip text and its test share one source of truth.
 */
export function datapacksDisabledKey(s: DatapacksGateState): TranslationKey | null {
  if (s.running) return 'worlds.datapacks.blockedRunning';
  if (s.busy) return 'worlds.datapacks.blockedBusy';
  return null;
}

/**
 * The library row's one-line world-state summary. Pure and placed beside
 * `datapacksDisabledKey` for the same reason it is: wording and test share
 * one source of truth (slice-2 design §7.4).
 *
 * «Ни в одном мире» is an ACCENT state, not an absent value — it is the state
 * the whole library screen exists to surface (a pack that is installed but
 * live nowhere). `emphasis` carries that so the view doesn't re-derive it.
 *
 * A placement whose state is unknown (`null` — the world's level.dat was
 * unreadable) counts toward the total but never toward `enabled`, and it
 * blocks the «Выключен везде» claim: we cannot assert "everywhere off" about
 * a world we could not read.
 *
 * §0.5 A22: a world the pack is not live in is not counted at all — an entry
 * the game ignores (`state === 'ignored'`), or a folder whose level.dat is
 * missing (`absent`) or only `level.dat_old` (`only_old`). `level_dat ===
 * null` (could not tell) still counts, as an unknown state, so it keeps
 * blocking "disabled everywhere". So does an entry Lucerna could not read
 * (`ignored_reason === 'unreadable'`): that is "could not tell", not "the game
 * ignores it" (Fallback discipline Q2).
 */
export function datapackWorldSummary(
  placements: Pick<DatapackPlacementView, 'state' | 'level_dat' | 'ignored_reason'>[],
): {
  key: TranslationKey;
  args: { enabled: number; total: number };
  emphasis: 'accent' | 'muted' | 'normal';
} {
  const counted = placements.filter(
    (p) =>
      (p.state !== 'ignored' || p.ignored_reason === 'unreadable') &&
      p.level_dat !== 'absent' &&
      p.level_dat !== 'only_old',
  );
  const total = counted.length;
  if (total === 0) {
    return {
      key: 'addons.datapacks.summaryInNoWorld',
      args: { enabled: 0, total: 0 },
      emphasis: 'accent',
    };
  }
  const enabled = counted.filter((p) => p.state === 'enabled').length;
  if (enabled === 0 && counted.every((p) => p.state === 'disabled')) {
    return {
      key: 'addons.datapacks.summaryDisabledEverywhere',
      args: { enabled: 0, total },
      emphasis: 'muted',
    };
  }
  return {
    key: 'addons.datapacks.summaryEnabledIn',
    args: { enabled, total },
    emphasis: 'normal',
  };
}

/** How a world-tab row behaves; see {@link worldRowKind}. */
export type WorldRowKind = 'ignored' | 'ghost' | 'addable' | 'live';

/**
 * One classifier for every world-tab row (spec 2026-09-24 §4 U3):
 *  - `ignored` — an entry the game does not load (D1); checked first, so a ghost
 *    NAME that coincides with an ignored on-disk entry never offers "Clear
 *    entry" (that removal would `remove_dir_all` the entry — its trash goes
 *    through the this-world confirmation instead);
 *  - `ghost` — a level.dat name whose file is gone: `orphaned`, or a
 *    Disabled-only `not_added` that is not in the library (its "Add" would
 *    always fail). Minecraft skips it and drops the id at its next save, so
 *    it is quiet, and its one action only tidies the list sooner;
 *  - `addable` — a library pack this world does not reference;
 *  - `live` — enabled or disabled.
 */
export function worldRowKind(pack: Pick<WorldDatapack, 'state' | 'in_library'>): WorldRowKind {
  switch (pack.state) {
    case 'ignored':
      return 'ignored';
    case 'orphaned':
      return 'ghost';
    case 'not_added':
      return pack.in_library ? 'addable' : 'ghost';
    case 'enabled':
    case 'disabled':
      return 'live';
    default: {
      const unreachable: never = pack.state;
      return unreachable;
    }
  }
}
