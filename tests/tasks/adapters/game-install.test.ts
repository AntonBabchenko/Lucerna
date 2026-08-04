import { beforeEach, describe, expect, it, vi } from 'vitest';

const listeners = vi.hoisted(() => ({
  install: null as ((event: { payload: unknown }) => void) | null,
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: { installInstance: vi.fn() },
  events: {
    installProgress: {
      listen: vi.fn((cb: (event: { payload: unknown }) => void) => {
        listeners.install = cb;
        return Promise.resolve(() => {
          listeners.install = null;
        });
      }),
    },
  },
}));

import { commands } from '$lib/ipc/bindings';
import { installGame } from '$lib/tasks/adapters/game-install';
import { __resetTasksForTest, taskList } from '$lib/tasks/registry.svelte';

describe('game-install adapter', () => {
  beforeEach(() => {
    __resetTasksForTest();
    vi.clearAllMocks();
    listeners.install = null;
  });

  it('registers a concurrent task, feeds progress off the global event, finishes ok', async () => {
    vi.mocked(commands.installInstance).mockImplementation(async () => {
      listeners.install?.({
        payload: {
          version_id: 'v1',
          phase: 'assets',
          files_done: 3,
          files_total: 10,
          bytes_done: null,
          current_step: null,
        },
      });
      return { status: 'ok', data: null };
    });

    await installGame('inst-1', 'My Instance');

    const task = taskList()[0];
    expect(task.kind).toBe('game-install');
    expect(task.lane).toBe('concurrent');
    expect(task.scope).toEqual({ instanceId: 'inst-1' });
    expect(task.state).toBe('ok');
  });

  it('marks the task failed on an error result', async () => {
    vi.mocked(commands.installInstance).mockResolvedValue({
      status: 'error',
      error: { kind: 'instance_not_found', id: 'inst-1' },
    } as never);

    await installGame('inst-1', 'My Instance');
    expect(taskList()[0].state).toBe('failed');
  });

  it('marks the task failed when the command throws', async () => {
    vi.mocked(commands.installInstance).mockRejectedValue(new Error('bridge died'));
    await installGame('inst-1', 'My Instance');
    expect(taskList()[0].state).toBe('failed');
  });

  it('unsubscribes the global listener once the call settles', async () => {
    vi.mocked(commands.installInstance).mockResolvedValue({ status: 'ok', data: null });
    await installGame('inst-1', 'My Instance');
    expect(listeners.install).toBeNull();
  });
});
