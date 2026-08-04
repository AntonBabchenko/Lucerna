import { beforeEach, describe, expect, it, vi } from 'vitest';

const listeners = vi.hoisted(() => ({
  verifyProgress: null as ((event: { payload: unknown }) => void) | null,
}));

vi.mock('$lib/ipc/bindings', () => ({
  events: {
    verifyProgress: {
      listen: (cb: (event: { payload: unknown }) => void) => {
        listeners.verifyProgress = cb;
        return Promise.resolve(() => {
          listeners.verifyProgress = null;
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
  __resetIntegrityAdapterForTest,
  ensureIntegrityListener,
} from '$lib/tasks/adapters/integrity';

function emit(payload: unknown) {
  listeners.verifyProgress?.({ payload });
}

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
    __resetIntegrityAdapterForTest();
    vi.clearAllMocks();
    listeners.verifyProgress = null;
    ensureIntegrityListener();
  });

  it('registers a serial verify task on the first tick, scoped to the instance', () => {
    emit(manifest('i'));
    const task = taskList()[0];
    expect(task.kind).toBe('verify');
    expect(task.lane).toBe('serial');
    expect(task.scope).toEqual({ instanceId: 'i' });
    expect(task.state).toBe('running');
    expect(task.progress).toEqual({ current: 0, total: 1, unit: 'files' });
  });

  it('feeds hashing ticks onto the same task without minting a new one', () => {
    emit(manifest('i'));
    emit(hashing('i', 5, 20));
    expect(taskList().length).toBe(1);
    expect(taskList()[0].progress).toEqual({ current: 5, total: 20, unit: 'files' });
    expect(taskList()[0].phase).toBe('hashing');
  });

  it('finishes ok on the complete phase (plain verify, or repair on a healthy instance)', () => {
    emit(manifest('i'));
    emit(hashing('i', 20, 20));
    emit(complete('i'));
    expect(taskList()[0].state).toBe('ok');
  });

  it('a fresh manifest tick after completion starts a new task for the same instance', () => {
    emit(manifest('i'));
    emit(complete('i'));
    expect(taskList().length).toBe(1);
    expect(taskList()[0].state).toBe('ok');

    emit(manifest('i'));
    expect(taskList().length).toBe(2);
    expect(taskList()[1].state).toBe('running');
  });

  it('a repair-with-problems sequence never leaves a task stuck running', () => {
    // Pre-repair scan.
    emit(manifest('i'));
    emit(hashing('i', 3, 3));
    emit(complete('i'));
    // Repair begins (emit_repairing(0,1) marker), does its fix work, then
    // re-verifies (a second manifest/hashing/complete cycle), then emits its
    // true last tick (emit_repairing(1,1)).
    emit(repairing('i', 0, 1));
    emit(repairing('i', 1, 2)); // one of two targeted downloads finished
    emit(repairing('i', 2, 2)); // the other finished — done === total here too
    emit(manifest('i'));
    emit(hashing('i', 3, 3));
    emit(complete('i'));
    emit(repairing('i', 1, 1)); // the operation's true final marker

    expect(taskList().some((task) => task.state === 'running')).toBe(false);
  });

  it('a repairing tick with files_done === files_total finishes immediately', () => {
    emit(repairing('i', 1, 1));
    expect(taskList().length).toBe(1);
    expect(taskList()[0].state).toBe('ok');
  });

  it('queues a second instance behind the first under the serial lane', () => {
    emit(manifest('a'));
    emit(manifest('b'));
    expect(taskList().length).toBe(2);
    expect(taskList()[0].state).toBe('running');
    expect(taskList()[1].state).toBe('queued');
  });
});
