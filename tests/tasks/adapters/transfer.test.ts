import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/instances/transfer/transfer-runner', () => ({ runTransfer: vi.fn() }));

import { runTransfer } from '$lib/instances/transfer/transfer-runner';
import { transferData } from '$lib/tasks/adapters/transfer';
import { __resetTasksForTest, taskList } from '$lib/tasks/registry.svelte';

describe('transfer adapter', () => {
  beforeEach(() => {
    __resetTasksForTest();
    vi.clearAllMocks();
  });

  it('registers a task scoped to the target instance, feeds progress, finishes ok', async () => {
    vi.mocked(runTransfer).mockImplementation(async (_req, onProgress) => {
      onProgress({ phase: 'copying', current: 1, total: 4 });
      return {
        status: 'ok',
        outcome: { copied: 4, replaced: 0, kept_existing: 0, skipped: 0, replaced_kept_at: null },
      };
    });
    await transferData('New', {
      sourceId: 'inst-2',
      sourceName: 'Old',
      targetId: 'inst-1',
      targetName: 'New',
      selection: { items: ['schematics'], other: [] },
      onConflict: 'replace',
    });
    const task = taskList()[0];
    expect(task.kind).toBe('transfer');
    expect(task.scope).toEqual({ instanceId: 'inst-1' });
    expect(task.state).toBe('ok');
  });
});
