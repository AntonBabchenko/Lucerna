import type { TranslationKey } from '$lib/i18n/keys.generated';
import type { DatapackPlacementView } from '$lib/ipc/bindings';

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
 */
export function datapackWorldSummary(placements: Pick<DatapackPlacementView, 'state'>[]): {
  key: TranslationKey;
  args: { enabled: number; total: number };
  emphasis: 'accent' | 'muted' | 'normal';
} {
  const total = placements.length;
  if (total === 0) {
    return {
      key: 'addons.datapacks.summaryInNoWorld',
      args: { enabled: 0, total: 0 },
      emphasis: 'accent',
    };
  }
  const enabled = placements.filter((p) => p.state === 'enabled').length;
  if (enabled === 0 && placements.every((p) => p.state === 'disabled')) {
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
