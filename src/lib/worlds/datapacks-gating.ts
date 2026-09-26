import type { TranslationKey } from '$lib/i18n/keys.generated';
import type { DatapackPlacementView, LevelDatPresence, WorldDatapack } from '$lib/ipc/bindings';

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
 * Why a world's datapacks can't be changed because of its level.dat, or null
 * (spec 2026-09-24 §3 L.8, D2): `absent` — not a world to Minecraft;
 * `only_old` — the game restores level.dat from level.dat_old and Lucerna
 * must not pre-empt that. `null` (could not tell) is not a verdict: every
 * writer re-checks presence itself before writing (§3 L.4).
 */
export function levelDatBlockedKey(levelDat: LevelDatPresence | null): TranslationKey | null {
  switch (levelDat) {
    case 'absent':
      return 'worlds.datapacks.blockedNoLevelDat';
    case 'only_old':
      return 'worlds.datapacks.blockedOnlyOld';
    case 'present':
    case null:
      return null;
  }
}

export interface WorldDatapacksGateState extends DatapacksGateState {
  /** The world's level.dat presence; `null` until loaded or if the load failed. */
  levelDat: LevelDatPresence | null;
}

/**
 * The world tab's gate: running, then level.dat, then busy. The game owning
 * the world outranks everything; a level.dat reason is a lasting state, and
 * "busy" only a moment. `datapacksDisabledKey` stays for the library screen,
 * whose rows span many worlds.
 */
export function worldDatapacksDisabledKey(s: WorldDatapacksGateState): TranslationKey | null {
  if (s.running) return 'worlds.datapacks.blockedRunning';
  const levelDat = levelDatBlockedKey(s.levelDat);
  if (levelDat !== null) return levelDat;
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
 * A world Lucerna could not check — a placement whose state is unknown
 * (`null`: the world's data pack folder or level file could not be read, or
 * whether it has a level file could not be told), or an entry it could not
 * read (`ignored_reason === 'unreadable'`, which is "could not tell", not
 * "the game ignores it", Fallback discipline Q2) — is counted in
 * neither number. Counted in "of N" it read as "not enabled there": a pack
 * whose only world could not be checked read "Enabled in 0 of 1 world". It is
 * `unchecked`, shown apart; and while there is one, no claim about every
 * world holds: not «Выключен везде», and not «Ни в одном мире» — `checked` is
 * `null` when no world could be checked at all.
 *
 * §0.5 A22, as amended: a world the pack is not live in is not counted at
 * all — an entry the game ignores (`state === 'ignored'`), or a folder with
 * no level file (`absent`), which the game never opens. An only-old world IS
 * counted, by the state `level.dat_old` holds: the game restores the world
 * from it and loads those packs, so leaving it out made a pack live only
 * there read "In no world" (honesty over A22's letter).
 */
export function datapackWorldSummary(
  placements: Pick<DatapackPlacementView, 'state' | 'level_dat' | 'ignored_reason'>[],
): {
  /** What the worlds Lucerna could check say; `null` when it could check none. */
  checked: {
    key: TranslationKey;
    args: { enabled: number; total: number };
    emphasis: 'accent' | 'muted' | 'normal';
  } | null;
  /** Worlds Lucerna could not check, shown apart (`addons.datapacks.summaryUnchecked`). */
  unchecked: number;
} {
  const live = placements.filter(
    (p) => (p.state !== 'ignored' || p.ignored_reason === 'unreadable') && p.level_dat !== 'absent',
  );
  // The remove dialog's `isUnchecked`, on the rows left once `absent` is out.
  const isUnknown = (p: (typeof live)[number]) =>
    p.state === null || p.ignored_reason === 'unreadable';
  const unchecked = live.filter(isUnknown).length;
  const known = live.filter((p) => !isUnknown(p));
  const total = known.length;
  if (total === 0) {
    return {
      checked:
        unchecked > 0
          ? null
          : {
              key: 'addons.datapacks.summaryInNoWorld',
              args: { enabled: 0, total: 0 },
              emphasis: 'accent',
            },
      unchecked,
    };
  }
  const enabled = known.filter((p) => p.state === 'enabled').length;
  if (unchecked === 0 && known.every((p) => p.state === 'disabled')) {
    return {
      checked: {
        key: 'addons.datapacks.summaryDisabledEverywhere',
        args: { enabled: 0, total },
        emphasis: 'muted',
      },
      unchecked,
    };
  }
  return {
    checked: {
      key: 'addons.datapacks.summaryEnabledIn',
      args: { enabled, total },
      emphasis: 'normal',
    },
    unchecked,
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
