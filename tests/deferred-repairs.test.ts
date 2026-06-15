import { beforeEach, describe, expect, it, vi } from 'vitest';

const executeRepair = vi.fn();
const pushSuccess = vi.fn();
const pushWarning = vi.fn();

vi.mock('$lib/ipc/bindings', () => ({
  commands: { executeRepair: (...a: unknown[]) => executeRepair(...a) },
}));
vi.mock('$lib/ipc/format-error', () => ({ formatError: (e: unknown) => `fmt:${String(e)}` }));
vi.mock('$lib/toasts/toasts.svelte', () => ({
  pushSuccess: (...a: unknown[]) => pushSuccess(...a),
  pushWarning: (...a: unknown[]) => pushWarning(...a),
}));
// get(t) must yield an identity translate fn so toast calls don't throw.
vi.mock('$lib/i18n', () => ({
  t: {
    subscribe: (run: (v: (k: string, p?: unknown) => string) => void) => {
      run((k) => k);
      return () => {};
    },
  },
}));

import {
  deferOrRunRepair,
  drainDeferredRepairs,
  isCompleted,
  isDeferred,
} from '$lib/logs/deferred-repairs.svelte';

beforeEach(() => vi.clearAllMocks());

const choice = { kind: 'disable_mod', sha1: 'a' } as const;

describe('deferred-repairs', () => {
  it('runs immediately when not running', async () => {
    executeRepair.mockResolvedValue({ status: 'ok', data: null });
    const r = await deferOrRunRepair(false, {
      instanceId: 'i-run',
      sha1: 'a',
      label: 'Mod A',
      choice,
    });
    expect(r).toEqual({ deferred: false, ok: true });
    expect(executeRepair).toHaveBeenCalledWith('i-run', choice);
    expect(isCompleted('i-run', 'a')).toBe(true);
    expect(pushSuccess).toHaveBeenCalled();
  });

  it('defers (no executeRepair) when running, then drain applies it', async () => {
    executeRepair.mockResolvedValue({ status: 'ok', data: null });
    const r = await deferOrRunRepair(true, {
      instanceId: 'i-def',
      sha1: 'b',
      label: 'Mod B',
      choice,
    });
    expect(r).toEqual({ deferred: true, ok: true });
    expect(executeRepair).not.toHaveBeenCalled();
    expect(isDeferred('i-def', 'b')).toBe(true);
    expect(pushSuccess).toHaveBeenCalledTimes(1); // queued toast

    await drainDeferredRepairs();
    expect(executeRepair).toHaveBeenCalledWith('i-def', choice);
    expect(isDeferred('i-def', 'b')).toBe(false);
    expect(isCompleted('i-def', 'b')).toBe(true);
  });

  it('reports failure via warning toast on immediate run', async () => {
    executeRepair.mockResolvedValue({ status: 'error', error: 'boom' });
    const r = await deferOrRunRepair(false, {
      instanceId: 'i-err',
      sha1: 'c',
      label: 'Mod C',
      choice,
    });
    expect(r.ok).toBe(false);
    expect(pushWarning).toHaveBeenCalled();
    expect(isCompleted('i-err', 'c')).toBe(false);
  });
});
