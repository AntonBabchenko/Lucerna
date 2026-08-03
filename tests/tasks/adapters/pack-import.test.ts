import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/ops/import-runner', () => ({ runImport: vi.fn() }));

import { runImport } from '$lib/ops/import-runner';
import { __resetTasksForTest, taskList } from '$lib/tasks/registry.svelte';
import { importModpack } from '$lib/tasks/adapters/pack-import';

describe('pack-import adapter', () => {
  beforeEach(() => {
    __resetTasksForTest();
    vi.clearAllMocks();
  });

  it('registers a task, feeds progress, and keeps the details on success', async () => {
    vi.mocked(runImport).mockImplementation(async (_req, onProgress) => {
      onProgress(
        { phase: 'installing_file', current: 1, total: 2, file_name: 'a.jar' } as never,
        null,
      );
      return {
        status: 'ok',
        name: 'Pack',
        instanceId: 'inst-1',
        skipped: [],
        inertLoaderJars: [],
        details: [{ name: 'a.jar' } as never],
      } as never;
    });

    await importModpack('Pack', { path: '/tmp/p.mrpack' } as never);

    const task = taskList()[0];
    expect(task.kind).toBe('pack-import');
    expect(task.state).toBe('ok');
    expect(task.details).toHaveLength(1);
  });

  it('marks the task failed when the runner throws', async () => {
    vi.mocked(runImport).mockRejectedValue(new Error('bridge died'));
    await importModpack('Pack', { path: '/tmp/p.mrpack' } as never);
    expect(taskList()[0].state).toBe('failed');
  });

  it('marks the task partial and does not throw on a partial-failure outcome', async () => {
    vi.mocked(runImport).mockResolvedValue({
      status: 'partial',
      failed: ['a.jar', 'b.jar'],
    } as never);

    await importModpack('Pack', { path: '/tmp/p.mrpack' } as never);

    const task = taskList()[0];
    expect(task.kind).toBe('pack-import');
    expect(task.state).toBe('partial');
  });
});
