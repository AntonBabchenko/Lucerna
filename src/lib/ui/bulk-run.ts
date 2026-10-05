// The shared half of every bulk action. Each list keeps its own operation (which command, which
// identifier, which gate), but how a batch runs and how it is reported is written once:
//   * sequential — every target directory is written in place, and parallel calls would race
//     it (the rule every per-list «Update all» already follows);
//   * a failure never stops the run — the items are independent files, and the user asked for
//     all of them; it is counted, its reason kept with the item's name, and the run goes on;
//   * one notice per run, saying «{ok} of {total}» — never a bare «done» — with each reason once
//     and the names it stopped under it (the app's one grouping, `reasonLines`).
import { get } from 'svelte/store';
import { type ReasonLine, reasonLines } from '$lib/format/reason-lines';
import { t } from '$lib/i18n';
import type { TranslationKey } from '$lib/i18n/keys.generated';
import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';

export type BulkOutcome = {
  total: number;
  ok: number;
  failed: number;
  /** Each reason once, with the names it stopped — «Disabled 0 of 5, 5 failed» alone cannot
   *  tell one busy profile from five unrelated failures. */
  reasons: ReasonLine[];
};

type ResultLike<E> = { status: 'ok'; data: unknown } | { status: 'error'; error: E };

export async function runBulk<T, E>(
  items: readonly T[],
  op: (item: T) => Promise<ResultLike<E>>,
  format: (e: E) => string,
  nameOf: (item: T) => string,
  onOk?: (item: T) => void,
): Promise<BulkOutcome> {
  let ok = 0;
  const failures: { name: string; reason: string }[] = [];
  for (const item of items) {
    let res: ResultLike<E>;
    try {
      res = await op(item);
    } catch (e) {
      // A thrown invoke is a failure of this item, worded by its message — never the end of
      // the run, and never a silent skip.
      failures.push({ name: nameOf(item), reason: e instanceof Error ? e.message : String(e) });
      continue;
    }
    if (res.status === 'ok') {
      ok += 1;
      onOk?.(item);
    } else {
      failures.push({ name: nameOf(item), reason: format(res.error) });
    }
  }
  return { total: items.length, ok, failed: items.length - ok, reasons: reasonLines(failures) };
}

/** One notice for the run: a success with the counts, or a warning with the reasons. */
export function reportBulk(
  outcome: BulkOutcome,
  keys: { done: TranslationKey; partial: TranslationKey },
): void {
  const values = { ok: outcome.ok, total: outcome.total, failed: outcome.failed };
  if (outcome.failed === 0) pushSuccess(get(t)(keys.done, values));
  else pushWarning(get(t)(keys.partial, values), outcome.reasons);
}

/** A confirm lists this many names, then «and N more». */
export const BULK_NAME_CAP = 5;

export function bulkNameLines(names: readonly string[]): string[] {
  if (names.length <= BULK_NAME_CAP + 1) return [...names];
  return [
    ...names.slice(0, BULK_NAME_CAP),
    get(t)('ui.bulk.andMore', { count: names.length - BULK_NAME_CAP }),
  ];
}
