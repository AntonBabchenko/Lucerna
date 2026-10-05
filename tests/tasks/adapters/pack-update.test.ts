import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/modpacks/update-runner', () => ({ runUpdate: vi.fn() }));

import { runUpdate } from '$lib/modpacks/update-runner';
import { applyModpackUpdate } from '$lib/tasks/adapters/pack-update';
import { __resetTasksForTest, taskList } from '$lib/tasks/registry.svelte';

describe('pack-update adapter', () => {
  beforeEach(() => {
    __resetTasksForTest();
    vi.clearAllMocks();
  });

  it('registers a task scoped to the instance, feeds progress, and finishes ok', async () => {
    vi.mocked(runUpdate).mockImplementation(
      async (_instanceId, _tempPath, _newVersionId, _backupWorlds, onProgress) => {
        onProgress(
          { phase: 'installing_file', current: 1, total: 3, file_name: 'b.jar' } as never,
          null,
        );
        return { status: 'ok', inst: { id: 'inst-1', name: 'Pack' } } as never;
      },
    );

    await applyModpackUpdate('Pack', 'inst-1', '/tmp/new.mrpack', 'v2', false);

    const task = taskList()[0];
    expect(task.kind).toBe('pack-update');
    expect(task.scope).toEqual({ instanceId: 'inst-1' });
    expect(task.state).toBe('ok');
  });

  it('marks the task failed when the runner throws', async () => {
    vi.mocked(runUpdate).mockRejectedValue(new Error('bridge died'));
    await applyModpackUpdate('Pack', 'inst-1', '/tmp/new.mrpack', 'v2', false);
    expect(taskList()[0].state).toBe('failed');
  });

  it('marks the task failed on an error outcome', async () => {
    vi.mocked(runUpdate).mockResolvedValue({ status: 'error', message: 'nope' } as never);
    await applyModpackUpdate('Pack', 'inst-1', '/tmp/new.mrpack', 'v2', false);
    expect(taskList()[0].state).toBe('failed');
  });

  it('passes the backup choice through and counts worlds while they are backed up', async () => {
    let progressDuringBackup: unknown = null;
    vi.mocked(runUpdate).mockImplementation(
      async (_instanceId, _tempPath, _newVersionId, backupWorlds, onProgress) => {
        expect(backupWorlds).toBe(true);
        // The last byte tick of the downloads is still the latest one here.
        onProgress(
          { phase: 'backing_up_world', current: 1, total: 2, world_name: 'Survival' } as never,
          { phase: 'downloading', current: 9, total: 10 } as never,
        );
        progressDuringBackup = taskList()[0].progress;
        return { status: 'ok', inst: { id: 'inst-1', name: 'Pack' } } as never;
      },
    );

    await applyModpackUpdate('Pack', 'inst-1', '/tmp/new.mrpack', 'v2', true);

    // World 1 of 2 is being zipped: none is done yet.
    expect(progressDuringBackup).toEqual({ current: 0, total: 2, unit: 'files' });
  });

  it('drops the last download tick once the changes are being applied', async () => {
    let during: { phase: unknown; progress: unknown } | null = null;
    vi.mocked(runUpdate).mockImplementation(
      async (_instanceId, _tempPath, _newVersionId, _backupWorlds, onProgress) => {
        onProgress(
          { phase: 'applying_changes' } as never,
          // Still the latest byte tick: the downloads ended at 100%.
          { phase: 'downloading', current: 10, total: 10 } as never,
        );
        during = { phase: taskList()[0].phase, progress: taskList()[0].progress };
        return { status: 'ok', inst: { id: 'inst-1', name: 'Pack' } } as never;
      },
    );

    await applyModpackUpdate('Pack', 'inst-1', '/tmp/new.mrpack', 'v2', false);

    expect(during).toEqual({ phase: 'applying_changes', progress: null });
  });
});
