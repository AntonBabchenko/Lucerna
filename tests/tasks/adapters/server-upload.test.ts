import { beforeEach, describe, expect, it, vi } from 'vitest';

const listeners = vi.hoisted(() => ({
  serverUploadProgress: null as ((event: { payload: unknown }) => void) | null,
}));

vi.mock('$lib/ipc/bindings', () => ({
  events: {
    serverUploadProgress: {
      listen: (cb: (event: { payload: unknown }) => void) => {
        listeners.serverUploadProgress = cb;
        return Promise.resolve(() => {
          listeners.serverUploadProgress = null;
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
  __resetServerUploadAdapterForTest,
  ensureServerUploadListener,
} from '$lib/tasks/adapters/server-upload';

function emit(payload: unknown) {
  listeners.serverUploadProgress?.({ payload });
}

const tick = (
  serverId: string,
  filesDone: number,
  filesTotal: number,
  bytesDone: number | null,
  bytesTotal: number | null,
  file = 'world/level.dat',
) => ({
  server_id: serverId,
  current_file: file,
  files_done: filesDone,
  files_total: filesTotal,
  bytes_done: bytesDone,
  bytes_total: bytesTotal,
});

describe('server-upload adapter', () => {
  beforeEach(() => {
    __resetTasksForTest();
    __resetServerUploadAdapterForTest();
    vi.clearAllMocks();
    listeners.serverUploadProgress = null;
    ensureServerUploadListener();
  });

  it('registers a concurrent server-upload task scoped to the server on the first tick', () => {
    emit(tick('s1', 1, 10, 1000, 100_000));
    const task = taskList()[0];
    expect(task.kind).toBe('server-upload');
    expect(task.lane).toBe('concurrent');
    expect(task.scope).toEqual({ serverId: 's1' });
    expect(task.state).toBe('running');
  });

  it('prefers byte progress when bytes_total is known', () => {
    emit(tick('s1', 1, 10, 1000, 100_000));
    expect(taskList()[0].progress).toEqual({ current: 1000, total: 100_000, unit: 'bytes' });
  });

  it('falls back to file-count progress when bytes_total is null', () => {
    emit(tick('s1', 2, 5, null, null));
    expect(taskList()[0].progress).toEqual({ current: 2, total: 5, unit: 'files' });
  });

  it('sets phase to the current file being transferred', () => {
    emit(tick('s1', 1, 10, 0, 100, 'mods/sodium.jar'));
    expect(taskList()[0].phase).toBe('mods/sodium.jar');
  });

  it('does not mint a second task for a second tick on the same server', () => {
    emit(tick('s1', 1, 10, 1000, 100_000));
    emit(tick('s1', 2, 10, 2000, 100_000));
    expect(taskList().length).toBe(1);
    expect(taskList()[0].progress).toEqual({ current: 2000, total: 100_000, unit: 'bytes' });
  });

  it('finishes ok once files_done reaches files_total', () => {
    emit(tick('s1', 1, 2, 500, 1000));
    emit(tick('s1', 2, 2, 1000, 1000));
    expect(taskList()[0].state).toBe('ok');
  });

  it('runs uploads to different servers in parallel (concurrent lane)', () => {
    emit(tick('s1', 1, 10, 100, 10_000));
    emit(tick('s2', 1, 10, 100, 10_000));
    expect(taskList().length).toBe(2);
    expect(taskList().every((t) => t.state === 'running')).toBe(true);
  });

  it('a fresh tick after completion starts a new task for the same server', () => {
    emit(tick('s1', 1, 1, 100, 100));
    expect(taskList()[0].state).toBe('ok');
    emit(tick('s1', 1, 5, 10, 500));
    expect(taskList().length).toBe(2);
    expect(taskList()[1].state).toBe('running');
  });
});
