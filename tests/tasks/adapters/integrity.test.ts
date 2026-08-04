import { beforeEach, describe, expect, it, vi } from 'vitest';

// This adapter now subscribes PER CALL (mirrors game-install.test.ts's
// shape), not at module load, so there is only ever one live listener at a
// time in these tests.
const listeners = vi.hoisted(() => ({
  verifyProgress: null as ((event: { payload: unknown }) => void) | null,
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    verifyInstance: vi.fn(),
    repairInstance: vi.fn(),
  },
  events: {
    verifyProgress: {
      listen: vi.fn((cb: (event: { payload: unknown }) => void) => {
        listeners.verifyProgress = cb;
        return Promise.resolve(() => {
          listeners.verifyProgress = null;
        });
      }),
    },
  },
}));

import { commands } from '$lib/ipc/bindings';
import { runRepair, runVerify } from '$lib/tasks/adapters/integrity';
import { __resetTasksForTest, taskList } from '$lib/tasks/registry.svelte';

function emit(payload: unknown) {
  listeners.verifyProgress?.({ payload });
}

const report = (instanceId: string, healthy = true) => ({
  instance_id: instanceId,
  effective_version_id: 'v1',
  categories: [],
  problems: [],
  healthy,
  manifest_recoverable: false,
});

const manifest = (instanceId: string) => ({
  instance_id: instanceId,
  phase: 'manifest',
  files_done: 0,
  files_total: 1,
  bytes_done: 0,
  current_category: null,
});
const hashing = (instanceId: string, done: number, total: number) => ({
  instance_id: instanceId,
  phase: 'hashing',
  files_done: done,
  files_total: total,
  bytes_done: 0,
  current_category: 'assets',
});
const complete = (instanceId: string) => ({
  instance_id: instanceId,
  phase: 'complete',
  files_done: 1,
  files_total: 1,
  bytes_done: 0,
  current_category: null,
});
const repairing = (instanceId: string, done: number, total: number) => ({
  instance_id: instanceId,
  phase: 'repairing',
  files_done: done,
  files_total: total,
  bytes_done: 0,
  current_category: null,
});

describe('integrity adapter', () => {
  beforeEach(() => {
    __resetTasksForTest();
    vi.clearAllMocks();
    listeners.verifyProgress = null;
  });

  it('tags a plain verify as kind verify, scoped to the instance, serial lane', async () => {
    vi.mocked(commands.verifyInstance).mockResolvedValue({
      status: 'ok',
      data: report('i'),
    } as never);

    await runVerify('i', 'My Instance');

    const task = taskList()[0];
    expect(task.kind).toBe('verify');
    expect(task.lane).toBe('serial');
    expect(task.scope).toEqual({ instanceId: 'i' });
    expect(task.state).toBe('ok');
  });

  it('tags a repair as kind repair, not verify — the whole point of this adapter', async () => {
    vi.mocked(commands.repairInstance).mockResolvedValue({
      status: 'ok',
      data: report('i'),
    } as never);

    await runRepair('i', 'My Instance');

    const task = taskList()[0];
    expect(task.kind).toBe('repair');
    expect(task.state).toBe('ok');
  });

  it('feeds files_done/files_total ticks onto the task', async () => {
    vi.mocked(commands.verifyInstance).mockImplementation(async () => {
      emit(manifest('i'));
      emit(hashing('i', 5, 20));
      return { status: 'ok', data: report('i') };
    });

    await runVerify('i', 'My Instance');
    // The final tick before the call settled is what's left on the task —
    // confirms progress really does flow through the event during the call.
    expect(taskList()[0].phase).toBe('hashing');
  });

  it('ignores a progress tick for a different instance', async () => {
    vi.mocked(commands.verifyInstance).mockImplementation(async () => {
      emit(manifest('OTHER'));
      return { status: 'ok', data: report('i') };
    });

    await runVerify('i', 'My Instance');
    expect(taskList()[0].phase).toBeNull();
  });

  it('a repair-with-problems tick sequence (double Complete, late repairing(1,1)) never finishes the task early — only the awaited Result does', async () => {
    vi.mocked(commands.repairInstance).mockImplementation(async () => {
      // Pre-repair scan.
      emit(manifest('i'));
      emit(hashing('i', 3, 3));
      emit(complete('i'));
      // Repair begins, does its fix work, then re-verifies (a second
      // manifest/hashing/complete cycle), then emits its true last tick.
      emit(repairing('i', 0, 1));
      emit(repairing('i', 1, 2));
      emit(repairing('i', 2, 2));
      emit(manifest('i'));
      emit(hashing('i', 3, 3));
      emit(complete('i'));
      // Mid-call: none of the ticks above should have finished the task —
      // only the awaited command's Result does that now.
      expect(taskList()[0].state).toBe('running');
      emit(repairing('i', 1, 1));
      return { status: 'ok', data: report('i') };
    });

    await runRepair('i', 'My Instance');
    expect(taskList().length).toBe(1);
    expect(taskList()[0].state).toBe('ok');
  });

  it('marks the task failed on an error result (verify)', async () => {
    vi.mocked(commands.verifyInstance).mockResolvedValue({
      status: 'error',
      error: { kind: 'instance_not_found', id: 'i' },
    } as never);

    const outcome = await runVerify('i', 'My Instance');
    expect(taskList()[0].state).toBe('failed');
    expect(outcome.status).toBe('error');
  });

  it('marks the task failed on an error result (repair)', async () => {
    vi.mocked(commands.repairInstance).mockResolvedValue({
      status: 'error',
      error: { kind: 'instance_not_found', id: 'i' },
    } as never);

    const outcome = await runRepair('i', 'My Instance');
    expect(taskList()[0].state).toBe('failed');
    expect(outcome.status).toBe('error');
  });

  it('marks the task failed when the command throws (bridge failure)', async () => {
    vi.mocked(commands.verifyInstance).mockRejectedValue(new Error('bridge died'));
    await runVerify('i', 'My Instance');
    expect(taskList()[0].state).toBe('failed');
  });

  it('unsubscribes the listener once the call settles', async () => {
    vi.mocked(commands.verifyInstance).mockResolvedValue({
      status: 'ok',
      data: report('i'),
    } as never);
    await runVerify('i', 'My Instance');
    expect(listeners.verifyProgress).toBeNull();
  });

  it('queues a second serial integrity task behind the first', async () => {
    // Bind the resolver synchronously, up front — a lazy `mockImplementation`
    // executor only runs once `commands.verifyInstance` is actually invoked,
    // which (because this adapter awaits `events.verifyProgress.listen`
    // first) happens on a later microtask than this line, not synchronously
    // when `runVerify` is called.
    let resolveFirst: (v: unknown) => void = () => {};
    const firstCall = new Promise((resolve) => {
      resolveFirst = resolve;
    });
    vi.mocked(commands.verifyInstance).mockReturnValue(firstCall as never);
    const first = runVerify('a', 'A');
    // Second call registers while the first is still in flight.
    vi.mocked(commands.repairInstance).mockResolvedValue({
      status: 'ok',
      data: report('b'),
    } as never);
    const second = runRepair('b', 'B');

    expect(taskList().length).toBe(2);
    expect(taskList()[0].state).toBe('running');
    expect(taskList()[1].state).toBe('queued');

    resolveFirst({ status: 'ok', data: report('a') });
    await Promise.all([first, second]);
  });
});
