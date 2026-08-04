import { beforeEach, describe, expect, it, vi } from 'vitest';

const listeners = vi.hoisted(() => ({
  dataMigrationProgress: null as ((event: { payload: unknown }) => void) | null,
}));

vi.mock('$lib/ipc/bindings', () => ({
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

import { __resetTasksForTest, taskList } from '$lib/tasks/registry.svelte';
import {
  __resetDataMigrationAdapterForTest,
  ensureDataMigrationListener,
} from '$lib/tasks/adapters/data-migration';

function emit(payload: unknown) {
  listeners.dataMigrationProgress?.({ payload });
}

describe('data-migration adapter', () => {
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
    // "deleting" is the LAST phase before the backend calls app.restart() on
    // success (see DataLocationProgressDialog.svelte's phase doc comment).
    // There is no "done" event: the process exits before one could arrive.
    emit({ copied_bytes: 0, total_bytes: 1000, phase: 'copying' });
    emit({ copied_bytes: 1000, total_bytes: 1000, phase: 'verifying' });
    emit({ copied_bytes: null, total_bytes: null, phase: 'deleting' });
    expect(taskList()[0].state).toBe('running');
  });
});
