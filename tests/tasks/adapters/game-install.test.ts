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

  // Pins the drop-in-replacement contract: the wrapper must resolve with
  // EXACTLY the `Result` shape `commands.installInstance` itself returns —
  // `{status:'error', error: IpcError}`, not a pre-formatted `{message}` —
  // so +page.svelte's existing `result.status === 'error'` /
  // `formatError(result.error)` call site keeps compiling and behaving
  // identically after the one-line swap from `commands.installInstance` to
  // `installGame`.
  it('resolves with the same Result shape as commands.installInstance (ok)', async () => {
    vi.mocked(commands.installInstance).mockResolvedValue({ status: 'ok', data: null });

    const result = await installGame('inst-1', 'My Instance');
    expect(result).toEqual({ status: 'ok', data: null });
  });

  it('resolves with the same Result shape as commands.installInstance (error, raw IpcError untouched)', async () => {
    const error = { kind: 'instance_not_found', id: 'inst-1' } as const;
    vi.mocked(commands.installInstance).mockResolvedValue({ status: 'error', error } as never);

    const result = await installGame('inst-1', 'My Instance');
    expect(result).toEqual({ status: 'error', error });
  });

  // A bridge failure (a real thrown Error, not a typed IpcError) propagates
  // out of `commands.installInstance` itself per `typedError`'s doc comment
  // at the bottom of bindings.ts — the wrapper must not swallow it into a
  // resolved `{status:'error'}`, or its behavior would diverge from the
  // command it replaces. The task still lands in a terminal `failed` state
  // first (via the catch's `finish()`) so a thrown error never wedges the
  // operations strip.
  it('marks the task failed AND rethrows when the command throws (matches commands.installInstance)', async () => {
    vi.mocked(commands.installInstance).mockRejectedValue(new Error('bridge died'));

    await expect(installGame('inst-1', 'My Instance')).rejects.toThrow('bridge died');
    expect(taskList()[0].state).toBe('failed');
  });

  it('unsubscribes the global listener once the call settles', async () => {
    vi.mocked(commands.installInstance).mockResolvedValue({ status: 'ok', data: null });
    await installGame('inst-1', 'My Instance');
    expect(listeners.install).toBeNull();
  });
});
