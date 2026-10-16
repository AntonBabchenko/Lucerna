import { describe, expect, it } from 'vitest';
import type { TranslationKey } from '$lib/i18n/keys.generated';
import {
  itemLabelKey,
  outcomeLines,
  refusalText,
  selectionFrom,
} from '$lib/instances/transfer/transfer-view';

const tr = (k: TranslationKey, p?: Record<string, unknown>) => `${k}${p ? JSON.stringify(p) : ''}`;

describe('transfer view', () => {
  it('lists only the outcome lines that apply', () => {
    expect(
      outcomeLines(tr, {
        copied: 3,
        replaced: 0,
        kept_existing: 0,
        skipped: 0,
        replaced_kept_at: null,
      }),
    ).toEqual(['instance.transfer.copiedLine{"count":3}']);
    expect(
      outcomeLines(tr, {
        copied: 1,
        replaced: 2,
        kept_existing: 1,
        skipped: 4,
        replaced_kept_at: 'C:/x',
      }),
    ).toEqual([
      'instance.transfer.copiedLine{"count":1}',
      'instance.transfer.replacedKeptLine{"count":2,"folder":"C:/x"}',
      'instance.transfer.keptLine{"count":1}',
      'instance.transfer.skippedLine{"count":4}',
    ]);
  });

  it('says the replaced files are in the Recycle Bin when nothing was kept', () => {
    expect(
      outcomeLines(tr, {
        copied: 1,
        replaced: 1,
        kept_existing: 0,
        skipped: 0,
        replaced_kept_at: null,
      })[1],
    ).toBe('instance.transfer.replacedBinLine{"count":1}');
  });

  it('builds the selection from the checked rows only', () => {
    expect(
      selectionFrom({ schematics: true, xaero: false }, { kubejs: true, cache: false }),
    ).toEqual({ items: ['schematics'], other: ['kubejs'] });
  });

  it('words every refusal with its path', () => {
    expect(refusalText(tr, { kind: 'overlap', rel: 'schematics' })).toBe(
      'instance.transfer.refusal.overlap{"rel":"schematics"}',
    );
    expect(refusalText(tr, { kind: 'unreadable', rel: 'xaero', reason: 'denied' })).toBe(
      'instance.transfer.refusal.unreadable{"rel":"xaero","reason":"denied"}',
    );
  });

  it('labels an item by its own key', () => {
    expect(itemLabelKey('distant_horizons')).toBe('instance.transfer.item.distant_horizons');
  });
});
