import { describe, expect, it } from 'vitest';
import { mapLimit } from '$lib/mods/concurrency';

describe('mapLimit', () => {
  it('preserves input order in the result', async () => {
    const out = await mapLimit([1, 2, 3, 4, 5], 2, async (n) => n * 2);
    expect(out).toEqual([2, 4, 6, 8, 10]);
  });

  it('never exceeds the concurrency limit', async () => {
    let active = 0;
    let peak = 0;
    await mapLimit([...Array(12).keys()], 3, async (n) => {
      active++;
      peak = Math.max(peak, active);
      await new Promise((r) => setTimeout(r, 3));
      active--;
      return n;
    });
    expect(peak).toBeLessThanOrEqual(3);
    expect(peak).toBeGreaterThan(1); // actually ran in parallel up to the cap
  });

  it('returns [] for an empty input', async () => {
    expect(await mapLimit([], 4, async (x) => x)).toEqual([]);
  });
});
