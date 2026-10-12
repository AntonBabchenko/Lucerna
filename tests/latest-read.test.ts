/**
 * The page's instance-list refresh. Reads overlap (a blur-save beside an
 * activation, a game exit): only the newest may land, and a caller that awaits
 * an older one reads the page state right after — it must find the newest
 * read applied, not the state from before it.
 */
import { describe, expect, it } from 'vitest';
import { latestRead } from '$lib/instances/latest-read';

function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}

describe('latestRead', () => {
  it('applies what a read returns', async () => {
    const applied: number[] = [];
    const refresh = latestRead(
      () => Promise.resolve(1),
      (v) => applied.push(v),
    );
    await refresh();
    expect(applied).toEqual([1]);
  });

  it('drops an older read that lands after a newer one', async () => {
    const reads = [deferred<string>(), deferred<string>()];
    let n = 0;
    const applied: string[] = [];
    const refresh = latestRead(
      () => reads[n++].promise,
      (v) => applied.push(v),
    );
    const first = refresh();
    const second = refresh();
    reads[1].resolve('new');
    await second;
    reads[0].resolve('old');
    await first;
    expect(applied).toEqual(['new']);
  });

  it('an overtaken call resolves only once the newest read has landed', async () => {
    const reads = [deferred<string>(), deferred<string>()];
    let n = 0;
    const applied: string[] = [];
    const refresh = latestRead(
      () => reads[n++].promise,
      (v) => applied.push(v),
    );
    let firstDone = false;
    const first = refresh().then(() => (firstDone = true));
    refresh();
    reads[0].resolve('old');
    await new Promise((r) => setTimeout(r, 0));
    expect(firstDone).toBe(false);
    reads[1].resolve('new');
    await first;
    expect(applied).toEqual(['new']);
  });
});
