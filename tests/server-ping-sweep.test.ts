// The ping sweep. The property that matters is that the launcher never turns
// into a scanner: the pool is bounded and each address is dialed once. The hard
// ceiling is the Rust semaphore, but the frontend must not queue a flood behind
// it either, and results must land progressively so rows fill in as they come.

import { describe, expect, it, vi } from 'vitest';
import { PING_POOL, sweepPings } from '$lib/worlds/server-ping';

const noAnswer = { kind: 'no_answer' } as const;

describe('sweepPings', () => {
  it('reports each result as it lands and never exceeds the pool', async () => {
    let active = 0;
    let peak = 0;
    const seen: string[] = [];
    const ping = vi.fn(async () => {
      active++;
      peak = Math.max(peak, active);
      await new Promise((r) => setTimeout(r, 2));
      active--;
      return noAnswer;
    });
    const addresses = Array.from({ length: 12 }, (_, i) => `h${i}`);
    await sweepPings(addresses, ping, (address) => seen.push(address));
    expect(seen).toHaveLength(12);
    expect(peak).toBeLessThanOrEqual(PING_POOL);
    expect(peak).toBeGreaterThan(1); // did run in parallel up to the cap
  });

  it('pings each distinct address once', async () => {
    const ping = vi.fn(async () => noAnswer);
    await sweepPings(['a', 'a', 'b'], ping, () => {});
    expect(ping).toHaveBeenCalledTimes(2);
  });

  it('passes a null outcome through so the caller can drop the row', async () => {
    // null = the call failed (e.g. the permission was revoked mid-sweep).
    const results: (string | null)[] = [];
    await sweepPings(
      ['a'],
      async () => null,
      (address, outcome) => results.push(outcome === null ? `${address}:null` : address),
    );
    expect(results).toEqual(['a:null']);
  });

  it('does nothing for an empty list', async () => {
    const ping = vi.fn(async () => noAnswer);
    await sweepPings([], ping, () => {});
    expect(ping).not.toHaveBeenCalled();
  });
});
