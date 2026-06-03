import { describe, expect, it } from 'vitest';
import { depGraphCache } from '$lib/mods/dep-graph-cache';
import { createLru } from '$lib/mods/lru';
import { updateCheckCache } from '$lib/mods/update-check-cache';

// Test the LRU primitive directly with a fresh instance per test (no shared
// module-singleton state), then a smoke test that both exported session caches
// are LRU-bounded at cap 5.
describe('createLru', () => {
  it('evicts the least-recently-used entry past the cap', () => {
    const lru = createLru<number>(5);
    for (let i = 0; i < 5; i++) lru.set(`i${i}`, i);
    lru.get('i0'); // promote i0 to MRU → i1 is now the LRU
    lru.set('i5', 5); // overflow → evict i1
    expect(lru.get('i1')).toBeUndefined();
    expect(lru.get('i0')).toBe(0);
    expect(lru.get('i5')).toBe(5);
  });

  it('set on an existing key refreshes recency and updates the value', () => {
    const lru = createLru<number>(2);
    lru.set('a', 1);
    lru.set('b', 2);
    lru.set('a', 11); // a is now MRU and updated; b is LRU
    lru.set('c', 3); // overflow → evict b (not a)
    expect(lru.get('b')).toBeUndefined();
    expect(lru.get('a')).toBe(11);
    expect(lru.get('c')).toBe(3);
  });

  it('delete removes an entry', () => {
    const lru = createLru<number>(5);
    lru.set('x', 1);
    lru.delete('x');
    expect(lru.get('x')).toBeUndefined();
  });

  it('cap 0 evicts everything immediately', () => {
    const lru = createLru<number>(0);
    lru.set('a', 1);
    expect(lru.get('a')).toBeUndefined();
  });
});

describe('session caches are LRU-bounded', () => {
  // Both exported caches wrap createLru(5). Verify the bound holds on each by
  // overflowing with fresh, uniquely-prefixed keys (so the assertion is
  // independent of any entries other tests may have left in the singleton).
  it('depGraphCache evicts past cap 5', () => {
    const k = (n: number) => `lru-test-dg-${n}`;
    const g = () => ({ roots: [] }) as never;
    for (let i = 0; i < 6; i++) depGraphCache.set(k(i), g());
    // After inserting 6 fresh keys with no intervening get, the first is evicted.
    expect(depGraphCache.get(k(0))).toBeUndefined();
    expect(depGraphCache.get(k(5))).toBeDefined();
  });

  it('updateCheckCache evicts past cap 5', () => {
    const k = (n: number) => `lru-test-uc-${n}`;
    for (let i = 0; i < 6; i++) updateCheckCache.set(k(i), []);
    expect(updateCheckCache.get(k(0))).toBeUndefined();
    expect(updateCheckCache.get(k(5))).toBeDefined();
  });
});
