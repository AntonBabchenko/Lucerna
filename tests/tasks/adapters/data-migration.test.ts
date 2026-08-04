import { beforeEach, describe, expect, it, vi } from 'vitest';

const listeners = vi.hoisted(() => ({
  dataMigrationProgress: null as ((event: { payload: unknown }) => void) | null,
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    setDataLocation: vi.fn(),
    adoptDataLocation: vi.fn(),
  },
  events: {
    dataMigrationProgress: {
      listen: (cb: (event: { payload: unknown }) => void) => {
        listeners.dataMigrationProgress = cb;
        return Promise.resolve(() => {
          listeners.dataMigrationProgress = null;
        });
      },
    },
  },
}));

vi.mock('$lib/i18n', () => ({
  t: {
    subscribe: (run: (v: unknown) => void) => {
      run((key: string) => key);
      return () => {};
    },
  },
}));

import { commands } from '$lib/ipc/bindings';
import {
  __resetDataMigrationAdapterForTest,
  adoptDataLocationTask,
  ensureDataMigrationListener,
  migrateDataLocation,
  resetDataLocation,
} from '$lib/tasks/adapters/data-migration';
import { __resetTasksForTest, taskList } from '$lib/tasks/registry.svelte';

function emit(payload: unknown) {
  listeners.dataMigrationProgress?.({ payload });
}

describe('data-migration adapter — passive listener (progress-only, for migrations not made through this module)', () => {
  beforeEach(() => {
    __resetTasksForTest();
    __resetDataMigrationAdapterForTest();
    vi.clearAllMocks();
    listeners.dataMigrationProgress = null;
    ensureDataMigrationListener();
  });

  it('registers an uncancellable, unreorderable modal task on the first tick', () => {
    emit({ copied_bytes: 0, total_bytes: 1000, phase: 'copying' });
    const task = taskList()[0];
    expect(task.kind).toBe('data-migration');
    expect(task.lane).toBe('modal');
    expect(task.scope).toEqual({});
    expect(task.state).toBe('running');
    expect(task.caps).toEqual({ cancellable: false, reorderable: false });
  });

  it('reports byte progress when total_bytes is known', () => {
    emit({ copied_bytes: 250, total_bytes: 1000, phase: 'copying' });
    expect(taskList()[0].progress).toEqual({ current: 250, total: 1000, unit: 'bytes' });
  });

  it('treats a missing total_bytes as indeterminate', () => {
    emit({ copied_bytes: null, total_bytes: null, phase: 'verifying' });
    expect(taskList()[0].progress).toBeNull();
  });

  it('feeds later ticks onto the same task without minting a new one', () => {
    emit({ copied_bytes: 0, total_bytes: 1000, phase: 'copying' });
    emit({ copied_bytes: 500, total_bytes: 1000, phase: 'copying' });
    emit({ copied_bytes: null, total_bytes: null, phase: 'deleting' });
    expect(taskList().length).toBe(1);
    expect(taskList()[0].phase).toBe('deleting');
  });

  it('never finishes the task — a successful migration terminates by app restart, not an event', () => {
    emit({ copied_bytes: 0, total_bytes: 1000, phase: 'copying' });
    emit({ copied_bytes: 1000, total_bytes: 1000, phase: 'verifying' });
    emit({ copied_bytes: null, total_bytes: null, phase: 'deleting' });
    expect(taskList()[0].state).toBe('running');
  });
});

describe('data-migration adapter — call-wrapping entry points (detect failure)', () => {
  beforeEach(() => {
    __resetTasksForTest();
    __resetDataMigrationAdapterForTest();
    vi.clearAllMocks();
    listeners.dataMigrationProgress = null;
  });

  it('resetDataLocation starts a modal data-migration task', async () => {
    // Never actually resolves on success in production (see the module
    // comment) — model that faithfully with a pending promise so this test
    // only exercises the synchronous `start()` call.
    vi.mocked(commands.setDataLocation).mockReturnValue(new Promise(() => {}) as never);
    void resetDataLocation();
    await Promise.resolve();
    const task = taskList()[0];
    expect(task.kind).toBe('data-migration');
    expect(task.lane).toBe('modal');
    expect(task.state).toBe('running');
  });

  it('a rejected/error Result from setDataLocation(null) (reset) reaches finish failed — the gap this adapter used to have', async () => {
    vi.mocked(commands.setDataLocation).mockResolvedValue({
      status: 'error',
      error: { kind: 'data_location_busy' },
    } as never);

    const outcome = await resetDataLocation();
    expect(taskList()[0].state).toBe('failed');
    expect(outcome.status).toBe('error');
  });

  it('a rejected/error Result from setDataLocation(path) (migrate) reaches finish failed', async () => {
    vi.mocked(commands.setDataLocation).mockResolvedValue({
      status: 'error',
      error: { kind: 'data_location_busy' },
    } as never);

    const outcome = await migrateDataLocation('D:/data');
    expect(taskList()[0].state).toBe('failed');
    expect(outcome.status).toBe('error');
  });

  it('a rejected/error Result from adoptDataLocation reaches finish failed', async () => {
    vi.mocked(commands.adoptDataLocation).mockResolvedValue({
      status: 'error',
      error: { kind: 'data_location_invalid', reason: 'not_a_data_root' },
    } as never);

    const outcome = await adoptDataLocationTask('D:/data');
    expect(taskList()[0].state).toBe('failed');
    expect(outcome.status).toBe('error');
  });

  it('marks the task failed when the command throws (bridge failure)', async () => {
    vi.mocked(commands.setDataLocation).mockRejectedValue(new Error('bridge died'));
    await resetDataLocation();
    expect(taskList()[0].state).toBe('failed');
  });

  it('feeds progress ticks onto the task while the call is in flight', async () => {
    vi.mocked(commands.setDataLocation).mockImplementation(async () => {
      emit({ copied_bytes: 500, total_bytes: 1000, phase: 'copying' });
      return { status: 'error', error: { kind: 'data_location_busy' } };
    });
    await migrateDataLocation('D:/data');
    // The task is finished 'failed' by now, but its last progress patch
    // before that should reflect the mid-flight tick.
    expect(taskList()[0].progress).toEqual({ current: 500, total: 1000, unit: 'bytes' });
  });

  it('does NOT invent a success completion on an ok Result — leaves the task running, same as the passive listener does on success', async () => {
    vi.mocked(commands.setDataLocation).mockResolvedValue({ status: 'ok', data: null } as never);
    const outcome = await resetDataLocation();
    expect(taskList()[0].state).toBe('running');
    expect(outcome.status).toBe('ok');
  });
});
