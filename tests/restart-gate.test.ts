import { beforeEach, describe, expect, it, vi } from 'vitest';

const h = vi.hoisted(() => ({ restartBlocked: vi.fn() }));
vi.mock('$lib/ipc/bindings', () => ({ commands: { restartBlocked: h.restartBlocked } }));

import { createRestartGate } from '$lib/settings/restart-gate.svelte';

// The shared core of the "may Lucerna restart / close right now?" gate: StoragePanel's rules,
// extracted so the Updates panel cannot drift from them. Each panel maps `block` to its own
// sentence and decides WHEN to ask; this only knows how to ask and what counts as an answer.
beforeEach(() => vi.clearAllMocks());

describe('createRestartGate', () => {
  it('starts blocked — nothing is allowed before the first answer', () => {
    const gate = createRestartGate();
    expect(gate.block).toBe('checking');
  });

  it('is "checking" while the answer is pending, then exactly what the backend said', async () => {
    let resolve!: (v: unknown) => void;
    h.restartBlocked.mockReturnValue(new Promise((r) => (resolve = r)));
    const gate = createRestartGate();
    const p = gate.recheck();
    expect(gate.block).toBe('checking');
    resolve('running');
    await p;
    expect(gate.block).toBe('running');
  });

  it('only an exact "none" opens the gate — a malformed answer or a rejection is "unknown"', async () => {
    const gate = createRestartGate();
    h.restartBlocked.mockResolvedValue('none');
    await gate.recheck();
    expect(gate.block).toBe('none');

    h.restartBlocked.mockResolvedValue('bogus');
    await gate.recheck();
    expect(gate.block).toBe('unknown');

    h.restartBlocked.mockResolvedValue(null);
    await gate.recheck();
    expect(gate.block).toBe('unknown');

    // Not wrapped in typedError: a failure is a rejection. "Could not ask" is "could not tell".
    h.restartBlocked.mockRejectedValue(new Error('ipc down'));
    await gate.recheck();
    expect(gate.block).toBe('unknown');
  });

  it('a slower, older answer never overwrites a newer one', async () => {
    const gate = createRestartGate();
    let resolveFirst!: (v: unknown) => void;
    h.restartBlocked
      .mockReturnValueOnce(new Promise((r) => (resolveFirst = r)))
      .mockResolvedValueOnce('none');
    const first = gate.recheck();
    await gate.recheck(); // the second query answers first
    expect(gate.block).toBe('none');
    resolveFirst('running'); // the stale first answer arrives last
    await first;
    expect(gate.block).toBe('none');
  });
});
