// Regression coverage for the serial-lane gate: `registry.svelte.ts`'s
// `start()` correctly MODELS queueing (a second serial task is recorded as
// `queued`), but until this fix the adapters called their backend command
// immediately regardless of what the registry decided — so the "queued"
// state was cosmetic and the work ran anyway. These tests prove the
// COMMAND itself is held back, not just the registry's bookkeeping — a
// registry-only assertion could not catch the bug, since the registry's
// state was already correct before this fix (see registry.test.ts).
//
// Uses two REAL adapters: `cloneInstance` (serial) as both the "first" and
// "second" queued op, and `installGame` (concurrent) to prove the
// concurrent lane is never held back by a serial task in flight.

import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/instances/clone-runner', () => ({ runClone: vi.fn() }));

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

import { runClone } from '$lib/instances/clone-runner';
import { commands } from '$lib/ipc/bindings';
import { cloneInstance } from '$lib/tasks/adapters/clone';
import { installGame } from '$lib/tasks/adapters/game-install';
import { __resetTasksForTest, cancelQueued, taskList } from '$lib/tasks/registry.svelte';

const cloneReq = (sourceId: string) => ({ sourceId, newName: 'Copy', options: {} }) as never;

describe('serial lane gate', () => {
  beforeEach(() => {
    __resetTasksForTest();
    vi.clearAllMocks();
    listeners.install = null;
  });

  it('does not run a second serial task until the first finishes', async () => {
    let resolveFirst: (v: unknown) => void = () => {};
    const firstCall = new Promise((resolve) => {
      resolveFirst = resolve;
    });
    vi.mocked(runClone).mockReturnValueOnce(firstCall as never);
    vi.mocked(runClone).mockResolvedValueOnce({
      status: 'ok',
      instanceId: 'inst-2',
      name: 'Second',
    } as never);

    const first = cloneInstance('First', cloneReq('a'));
    const second = cloneInstance('Second', cloneReq('b'));

    // Flush pending microtasks without ever settling `firstCall` — if the
    // second task's command fired here, that is the bug this test exists
    // to catch.
    await Promise.resolve();
    await Promise.resolve();

    expect(taskList().find((t) => t.title === 'Second')?.state).toBe('queued');
    expect(runClone).toHaveBeenCalledTimes(1);

    resolveFirst({ status: 'ok', instanceId: 'inst-1', name: 'First' });
    await Promise.all([first, second]);

    expect(runClone).toHaveBeenCalledTimes(2);
  });

  it('lets a concurrent task run immediately alongside a serial one', async () => {
    // The serial task never resolves during this test — proving the
    // concurrent task does not wait on it even a little.
    vi.mocked(runClone).mockReturnValue(new Promise(() => {}) as never);
    void cloneInstance('First', cloneReq('a'));

    await Promise.resolve();
    expect(taskList().find((t) => t.title === 'First')?.state).toBe('running');

    vi.mocked(commands.installInstance).mockResolvedValue({ status: 'ok', data: null } as never);
    const outcome = await installGame('inst-1', 'Game');

    expect(outcome.status).toBe('ok');
    expect(commands.installInstance).toHaveBeenCalledTimes(1);
  });

  it('never fires the command for a task cancelled while queued', async () => {
    let resolveFirst: (v: unknown) => void = () => {};
    const firstCall = new Promise((resolve) => {
      resolveFirst = resolve;
    });
    vi.mocked(runClone).mockReturnValueOnce(firstCall as never);

    const first = cloneInstance('First', cloneReq('a'));
    const second = cloneInstance('Second', cloneReq('b'));
    await Promise.resolve();
    await Promise.resolve();

    const queuedTask = taskList().find((t) => t.state === 'queued');
    expect(queuedTask).toBeDefined();
    cancelQueued(queuedTask!.id);

    resolveFirst({ status: 'ok', instanceId: 'inst-1', name: 'First' });
    await first;
    // Must settle (not hang forever — no leaked pending gate promise).
    const secondOutcome = await second;

    expect(runClone).toHaveBeenCalledTimes(1);
    expect(secondOutcome.status).not.toBe('ok');
    expect(taskList().find((t) => t.title === 'Second')).toBeUndefined();
  });

  it('promotes the next serial task even when the running one throws', async () => {
    vi.mocked(runClone).mockRejectedValueOnce(new Error('disk full'));
    vi.mocked(runClone).mockResolvedValueOnce({
      status: 'ok',
      instanceId: 'inst-2',
      name: 'Second',
    } as never);

    const first = cloneInstance('First', cloneReq('a'));
    const second = cloneInstance('Second', cloneReq('b'));
    await Promise.all([first, second]);

    expect(runClone).toHaveBeenCalledTimes(2);
    expect(taskList().find((t) => t.title === 'First')?.state).toBe('failed');
    expect(taskList().find((t) => t.title === 'Second')?.state).toBe('ok');
  });
});
