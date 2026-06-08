import { beforeEach, describe, expect, it, vi } from 'vitest';

// Mock the IPC layer before importing the store.
const executeRepair = vi.fn();
vi.mock('$lib/ipc/bindings', () => ({
  commands: { executeRepair: (...a: unknown[]) => executeRepair(...a) },
}));
const pushSuccess = vi.fn();
const pushWarning = vi.fn();
vi.mock('$lib/toasts/toasts.svelte', () => ({
  pushSuccess: (...a: unknown[]) => pushSuccess(...a),
  pushWarning: (...a: unknown[]) => pushWarning(...a),
}));
vi.mock('$lib/i18n', () => ({
  t: {
    subscribe: (fn: (v: (k: string) => string) => void) => {
      fn((k) => k);
      return () => {};
    },
  },
}));

import { enqueueRepair, repairRunningFor, __resetRepairOpsForTest } from '$lib/logs/repair-ops.svelte';

describe('repair-ops store', () => {
  beforeEach(() => {
    __resetRepairOpsForTest();
    executeRepair.mockReset();
    pushSuccess.mockReset();
    pushWarning.mockReset();
  });

  it('runs a queued repair and toasts success', async () => {
    executeRepair.mockResolvedValue({ status: 'ok', data: null });
    await enqueueRepair('inst-1', 'My Pack', { kind: 'raise_heap', to_mb: 4096 });
    expect(executeRepair).toHaveBeenCalledWith('inst-1', { kind: 'raise_heap', to_mb: 4096 });
    expect(pushSuccess).toHaveBeenCalled();
    expect(repairRunningFor('inst-1')).toBeNull();
  });

  it('toasts a warning when the command errors', async () => {
    executeRepair.mockResolvedValue({ status: 'error', error: { type: 'InstanceBusy' } });
    await enqueueRepair('inst-1', 'My Pack', { kind: 'reinstall_loader' });
    expect(pushWarning).toHaveBeenCalled();
  });

  it('dedupes a second enqueue for an instance already in flight', async () => {
    let resolve!: (v: unknown) => void;
    executeRepair.mockReturnValue(
      new Promise((r) => {
        resolve = r;
      }),
    );
    const p1 = enqueueRepair('inst-1', 'P', { kind: 'reinstall_loader' });
    const p2 = enqueueRepair('inst-1', 'P', { kind: 'reinstall_loader' });
    resolve({ status: 'ok', data: null });
    await Promise.all([p1, p2]);
    expect(executeRepair).toHaveBeenCalledTimes(1);
  });
});
