import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/modpacks/update-runner', () => ({ runUpdate: vi.fn() }));

import { runUpdate } from '$lib/modpacks/update-runner';
import { __resetTasksForTest, taskList } from '$lib/tasks/registry.svelte';
import { applyModpackUpdate } from '$lib/tasks/adapters/pack-update';

describe('pack-update adapter', () => {
  beforeEach(() => {
    __resetTasksForTest();
    vi.clearAllMocks();
  });

  it('registers a task scoped to the instance, feeds progress, and finishes ok', async () => {
    vi.mocked(runUpdate).mockImplementation(
      async (_instanceId, _tempPath, _newVersionId, onProgress) => {
        onProgress(
          { phase: 'installing_file', current: 1, total: 3, file_name: 'b.jar' } as never,
          null,
        );
        return { status: 'ok', inst: { id: 'inst-1', name: 'Pack' } } as never;
      },
    );

    await applyModpackUpdate('Pack', 'inst-1', '/tmp/new.mrpack', 'v2');

    const task = taskList()[0];
    expect(task.kind).toBe('pack-update');
    expect(task.scope).toEqual({ instanceId: 'inst-1' });
    expect(task.state).toBe('ok');
  });

  it('marks the task failed when the runner throws', async () => {
    vi.mocked(runUpdate).mockRejectedValue(new Error('bridge died'));
    await applyModpackUpdate('Pack', 'inst-1', '/tmp/new.mrpack', 'v2');
    expect(taskList()[0].state).toBe('failed');
  });

  it('marks the task failed on an error outcome', async () => {
    vi.mocked(runUpdate).mockResolvedValue({ status: 'error', message: 'nope' } as never);
    await applyModpackUpdate('Pack', 'inst-1', '/tmp/new.mrpack', 'v2');
    expect(taskList()[0].state).toBe('failed');
  });
});
