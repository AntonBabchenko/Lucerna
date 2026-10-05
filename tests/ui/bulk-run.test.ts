// The shared runner behind every bulk action: sequential, never aborting on a failure, one
// notice per run that says «{ok} of {total}» and groups the failed names under each reason.
import { describe, expect, it, vi } from 'vitest';

const toasts = vi.hoisted(() => ({ pushSuccess: vi.fn(), pushWarning: vi.fn() }));
vi.mock('$lib/toasts/toasts.svelte', () => toasts);
vi.mock('$lib/i18n', () => ({ t: { subscribe: () => () => {} } }));
vi.mock('svelte/store', () => ({
  get: () => (key: string, values?: Record<string, unknown>) =>
    `${key}${values ? ` ${JSON.stringify(values)}` : ''}`,
}));

import { bulkNameLines, reportBulk, runBulk } from '$lib/ui/bulk-run';

type Res = { status: 'ok'; data: null } | { status: 'error'; error: string };
const ok: Res = { status: 'ok', data: null };
const err = (e: string): Res => ({ status: 'error', error: e });
const self = (item: string) => item;

describe('runBulk', () => {
  it('runs every item in order, counts, and groups the failed names under each reason, in first-seen order', async () => {
    const seen: string[] = [];
    const answers: Record<string, Res> = { a: ok, b: err('busy'), c: err('busy'), d: err('io') };
    const outcome = await runBulk(
      ['a', 'b', 'c', 'd'],
      async (item) => {
        seen.push(item);
        return answers[item];
      },
      (e) => `reason:${e}`,
      self,
    );
    expect(seen).toEqual(['a', 'b', 'c', 'd']);
    expect(outcome).toEqual({
      total: 4,
      ok: 1,
      failed: 3,
      reasons: [
        { names: 'b, c', reason: 'reason:busy' },
        { names: 'd', reason: 'reason:io' },
      ],
    });
  });

  it('a thrown call counts as a failure with its message and does not stop the run', async () => {
    const outcome = await runBulk(
      ['a', 'b'],
      async (item) => {
        if (item === 'a') throw new Error('transport down');
        return ok;
      },
      (e) => String(e),
      self,
    );
    expect(outcome).toEqual({
      total: 2,
      ok: 1,
      failed: 1,
      reasons: [{ names: 'a', reason: 'transport down' }],
    });
  });

  it('calls onOk for each success, with the item', async () => {
    const okItems: string[] = [];
    await runBulk(
      ['a', 'b'],
      async (item) => (item === 'a' ? ok : err('x')),
      String,
      self,
      (item) => okItems.push(item),
    );
    expect(okItems).toEqual(['a']);
  });
});

describe('reportBulk', () => {
  it('a clean run is one success notice with the counts', () => {
    reportBulk(
      { total: 3, ok: 3, failed: 0, reasons: [] },
      { done: 'ui.bulk.removed', partial: 'ui.bulk.removedFailed' },
    );
    expect(toasts.pushSuccess).toHaveBeenCalledWith(
      'ui.bulk.removed {"ok":3,"total":3,"failed":0}',
    );
    expect(toasts.pushWarning).not.toHaveBeenCalled();
  });

  it('a run with failures is one warning with the grouped reasons as lines', () => {
    const reasons = [{ names: 'b, c', reason: 'busy' }];
    reportBulk(
      { total: 3, ok: 1, failed: 2, reasons },
      { done: 'ui.bulk.removed', partial: 'ui.bulk.removedFailed' },
    );
    expect(toasts.pushWarning).toHaveBeenCalledWith(
      'ui.bulk.removedFailed {"ok":1,"total":3,"failed":2}',
      reasons,
    );
  });
});

describe('bulkNameLines', () => {
  it('lists up to six names whole, and seven or more as five plus «and N more»', () => {
    const six = ['1', '2', '3', '4', '5', '6'];
    expect(bulkNameLines(six)).toEqual(six);
    const seven = [...six, '7'];
    expect(bulkNameLines(seven)).toEqual(['1', '2', '3', '4', '5', 'ui.bulk.andMore {"count":2}']);
  });
});
