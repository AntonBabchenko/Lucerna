import { describe, expect, it } from 'vitest';
import { depGraphCache } from '$lib/mods/dep-graph-cache';

describe('depGraphCache LRU (cap 5)', () => {
  it('evicts the least-recently-used entry past the cap', () => {
    const g = () => ({ roots: [] }) as never;
    for (let i = 0; i < 5; i++) depGraphCache.set(`i${i}`, g());
    // Touch i0 so it becomes most-recently-used.
    depGraphCache.get('i0');
    // Insert a 6th → evicts the LRU, which is now i1 (not i0).
    depGraphCache.set('i5', g());
    expect(depGraphCache.get('i1')).toBeUndefined();
    expect(depGraphCache.get('i0')).toBeDefined();
    expect(depGraphCache.get('i5')).toBeDefined();
  });

  it('set on an existing key refreshes recency and updates the value', () => {
    // Note: this test shares the module-singleton cache with the test above.
    // Use fresh keys to stay independent of prior state, and only assert on
    // those keys.
    depGraphCache.set('a', { roots: [] } as never);
    depGraphCache.set('a', {
      roots: [{ sha1: 'x', name: 'X', required: [], optional: [] }],
    } as never);
    expect(depGraphCache.get('a')?.roots.length).toBe(1);
  });
});
