import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/instances/clone-runner', () => ({ runClone: vi.fn() }));

import { runClone } from '$lib/instances/clone-runner';
import { cloneInstance } from '$lib/tasks/adapters/clone';
import { __resetTasksForTest, taskList } from '$lib/tasks/registry.svelte';

describe('clone adapter', () => {
  beforeEach(() => {
    __resetTasksForTest();
    vi.clearAllMocks();
  });

  it('registers a task scoped to the source instance, feeds progress, finishes ok', async () => {
    vi.mocked(runClone).mockImplementation(async (_req, onProgress) => {
      onProgress({ category: 'mods', current: 1, total: 4 } as never);
      return { status: 'ok', instanceId: 'inst-2', name: 'Copy' } as never;
    });

    await cloneInstance('Copy', { sourceId: 'inst-1', newName: 'Copy', options: {} } as never);

    const task = taskList()[0];
    expect(task.kind).toBe('clone');
    expect(task.scope).toEqual({ instanceId: 'inst-1' });
    expect(task.state).toBe('ok');
  });

  it('marks the task failed when the runner throws', async () => {
    vi.mocked(runClone).mockRejectedValue(new Error('bridge died'));
    await cloneInstance('Copy', { sourceId: 'inst-1', newName: 'Copy', options: {} } as never);
    expect(taskList()[0].state).toBe('failed');
  });
});
