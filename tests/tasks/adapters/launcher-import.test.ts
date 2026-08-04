import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/instances/import/launcher-import-runner', () => ({
  runLauncherImport: vi.fn(),
}));

import { runLauncherImport } from '$lib/instances/import/launcher-import-runner';
import { importLauncherInstance } from '$lib/tasks/adapters/launcher-import';
import { __resetTasksForTest, taskList } from '$lib/tasks/registry.svelte';

describe('launcher-import adapter', () => {
  beforeEach(() => {
    __resetTasksForTest();
    vi.clearAllMocks();
  });

  it('registers a task, feeds progress, and finishes ok', async () => {
    vi.mocked(runLauncherImport).mockImplementation(async (_req, onProgress) => {
      onProgress({ phase: 'copying', category: 'mods', current: 1, total: 5 } as never);
      return { status: 'ok', name: 'Legacy', instanceId: 'inst-3', untrackedMods: 2 } as never;
    });

    await importLauncherInstance('Legacy', { foreign: { root: '/x' } } as never);

    const task = taskList()[0];
    expect(task.kind).toBe('launcher-import');
    expect(task.state).toBe('ok');
  });

  it('marks the task failed when the runner throws', async () => {
    vi.mocked(runLauncherImport).mockRejectedValue(new Error('bridge died'));
    await importLauncherInstance('Legacy', { foreign: { root: '/x' } } as never);
    expect(taskList()[0].state).toBe('failed');
  });
});
