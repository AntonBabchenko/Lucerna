// Pure helpers for the mod-data transfer (spec 2026-10-15 Part A): item labels
// and icons, refusal wording, the selection the dialog sends, and the summary
// lines of the completion toast. No component state, so each is testable alone.

import type { TranslationKey } from '$lib/i18n/keys.generated';
import type { Translate } from '$lib/i18n/typed';
import type {
  Refusal,
  TransferItemId,
  TransferOutcome,
  TransferSelection,
} from '$lib/ipc/bindings';
import type { IconName } from '$lib/ui/icons';

export const ITEM_ICON: Record<TransferItemId, IconName> = {
  schematics: 'blocks',
  xaero: 'locate',
  journeymap: 'locate',
  flashback: 'play',
  replaymod: 'play',
  distant_horizons: 'globe',
  voxy: 'globe',
  bobby: 'globe',
};

const ITEM_LABEL: Record<TransferItemId, TranslationKey> = {
  schematics: 'instance.transfer.item.schematics',
  xaero: 'instance.transfer.item.xaero',
  journeymap: 'instance.transfer.item.journeymap',
  flashback: 'instance.transfer.item.flashback',
  replaymod: 'instance.transfer.item.replaymod',
  distant_horizons: 'instance.transfer.item.distant_horizons',
  voxy: 'instance.transfer.item.voxy',
  bobby: 'instance.transfer.item.bobby',
};

export function itemLabelKey(id: TransferItemId): TranslationKey {
  return ITEM_LABEL[id];
}

/** Items whose data a newer mod or Minecraft version may write in a format an
 *  older one cannot read (research note §3): the dialog carries a caution. */
export const VERSION_SENSITIVE: ReadonlySet<TransferItemId> = new Set<TransferItemId>([
  'xaero',
  'journeymap',
  'flashback',
  'replaymod',
  'distant_horizons',
  'voxy',
  'bobby',
]);

/** Map mods whose transfer takes multiplayer data only (spec §A2). */
export const MULTIPLAYER_ONLY: ReadonlySet<TransferItemId> = new Set<TransferItemId>([
  'xaero',
  'journeymap',
]);

/** LOD mods whose single-player data lives inside each world. */
export const LOD_MULTIPLAYER: ReadonlySet<TransferItemId> = new Set<TransferItemId>([
  'distant_horizons',
  'voxy',
]);

export function selectionFrom(
  items: Record<string, boolean>,
  other: Record<string, boolean>,
): TransferSelection {
  return {
    items: Object.entries(items)
      .filter(([, on]) => on)
      .map(([id]) => id as TransferItemId),
    other: Object.entries(other)
      .filter(([, on]) => on)
      .map(([name]) => name),
  };
}

export function outcomeLines(tr: Translate, o: TransferOutcome): string[] {
  const lines = [tr('instance.transfer.copiedLine', { count: o.copied })];
  if (o.replaced > 0) {
    lines.push(
      o.replaced_kept_at === null
        ? tr('instance.transfer.replacedBinLine', { count: o.replaced })
        : tr('instance.transfer.replacedKeptLine', {
            count: o.replaced,
            folder: o.replaced_kept_at,
          }),
    );
  }
  if (o.kept_existing > 0) lines.push(tr('instance.transfer.keptLine', { count: o.kept_existing }));
  if (o.skipped > 0) lines.push(tr('instance.transfer.skippedLine', { count: o.skipped }));
  return lines;
}

/** Why a row cannot be copied, in the user's words. */
export function refusalText(tr: Translate, r: Refusal): string {
  switch (r.kind) {
    case 'broken_link':
      return tr('instance.transfer.refusal.broken_link', { rel: r.rel });
    case 'unreadable':
      return tr('instance.transfer.refusal.unreadable', { rel: r.rel, reason: r.reason });
    case 'prefix_clash':
      return tr('instance.transfer.refusal.prefix_clash', { rel: r.rel });
    case 'inner_link':
      return tr('instance.transfer.refusal.inner_link', { rel: r.rel });
    case 'overlap':
      return tr('instance.transfer.refusal.overlap', { rel: r.rel });
    default: {
      const _exhaustive: never = r;
      return JSON.stringify(_exhaustive);
    }
  }
}
