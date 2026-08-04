import { beforeEach, describe, expect, it, vi } from 'vitest';

// Capture listener callbacks at module-eval time via vi.hoisted so the
// vi.mock factory (which is hoisted itself) can access them. Mirrors
// tests/phase-status-row.test.ts's shape, but for a plain module (no
// component render) — this adapter is a module-level singleton, like
// $lib/ops/op-queue.svelte, not a mounted component.
const listeners = vi.hoisted(() => ({
  modInstallProgress: null as ((event: { payload: unknown }) => void) | null,
  modInstalled: null as ((event: { payload: unknown }) => void) | null,
  modInstallFailed: null as ((event: { payload: unknown }) => void) | null,
}));

vi.mock('$lib/ipc/bindings', () => {
  type Key = 'modInstallProgress' | 'modInstalled' | 'modInstallFailed';
  const makeEvent = (key: Key) => ({
    listen: (cb: (event: { payload: unknown }) => void) => {
      listeners[key] = cb;
      return Promise.resolve(() => {
        listeners[key] = null;
      });
    },
  });
  return {
    events: {
      modInstallProgress: makeEvent('modInstallProgress'),
      modInstalled: makeEvent('modInstalled'),
      modInstallFailed: makeEvent('modInstallFailed'),
    },
  };
});

vi.mock('$lib/i18n', () => ({
  t: {
    subscribe: (run: (v: unknown) => void) => {
      // Identity translator: returns the key so tests can assert which
      // i18n key a fallback title resolved to.
      run((key: string) => key);
      return () => {};
    },
  },
}));

import { __resetTasksForTest, taskList } from '$lib/tasks/registry.svelte';
import {
  __resetModInstallAdapterForTest,
  ensureModInstallListener,
} from '$lib/tasks/adapters/mod-install';

function emitModInstallProgress(payload: unknown) {
  listeners.modInstallProgress?.({ payload });
}
function emitModInstalled(payload: unknown) {
  listeners.modInstalled?.({ payload });
}
function emitModInstallFailed(payload: unknown) {
  listeners.modInstallFailed?.({ payload });
}

describe('mod-install adapter', () => {
  beforeEach(() => {
    __resetTasksForTest();
    __resetModInstallAdapterForTest();
    vi.clearAllMocks();
    listeners.modInstallProgress = null;
    listeners.modInstalled = null;
    listeners.modInstallFailed = null;
    ensureModInstallListener();
  });

  it('shows x of N once the backend knows the total', () => {
    emitModInstallProgress({
      phase: 'downloading',
      instance_id: 'i',
      project_id: 'p',
      bytes_done: 10,
      bytes_total: 100,
      current: 3,
      total: 8,
    });
    expect(taskList()[0].progress).toEqual({ current: 3, total: 8, unit: 'files' });
  });

  it('treats total = 0 as indeterminate', () => {
    // Manifest extras download before the install set is resolved, so the
    // backend genuinely cannot know a total yet.
    emitModInstallProgress({
      phase: 'downloading',
      instance_id: 'i',
      project_id: 'p',
      current: 0,
      total: 0,
    } as never);
    expect(taskList()[0].progress).toBeNull();
  });

  it('registers a concurrent mod-install task scoped to the instance', () => {
    emitModInstallProgress({
      phase: 'downloading',
      instance_id: 'i',
      project_id: 'p',
      current: 1,
      total: 3,
    });
    const task = taskList()[0];
    expect(task.kind).toBe('mod-install');
    expect(task.lane).toBe('concurrent');
    expect(task.scope).toEqual({ instanceId: 'i' });
    expect(task.state).toBe('running');
  });

  it('does not mint a second task for a second tick on the same instance', () => {
    emitModInstallProgress({
      phase: 'downloading',
      instance_id: 'i',
      project_id: 'p',
      current: 1,
      total: 3,
    });
    emitModInstallProgress({
      phase: 'downloading',
      instance_id: 'i',
      project_id: 'p',
      current: 2,
      total: 3,
    });
    expect(taskList().length).toBe(1);
    expect(taskList()[0].progress).toEqual({ current: 2, total: 3, unit: 'files' });
  });

  it('finishes ok on modInstalled', () => {
    emitModInstallProgress({
      phase: 'downloading',
      instance_id: 'i',
      project_id: 'p',
      current: 1,
      total: 1,
    });
    emitModInstalled({ instance_id: 'i', sha1: 'x', filename: 'x.jar', name: 'X' });
    expect(taskList()[0].state).toBe('ok');
  });

  it('finishes failed on modInstallFailed', () => {
    emitModInstallProgress({
      phase: 'downloading',
      instance_id: 'i',
      project_id: 'p',
      current: 1,
      total: 1,
    });
    emitModInstallFailed({
      instance_id: 'i',
      project_id: 'p',
      error: { kind: 'io', path: 'x', details: 'y' },
    });
    expect(taskList()[0].state).toBe('failed');
  });

  it('ignores modInstalled for an instance with no active task', () => {
    emitModInstalled({ instance_id: 'unknown', sha1: 'x', filename: 'x.jar', name: 'X' });
    expect(taskList().length).toBe(0);
  });

  it('keeps two different instances independent under the concurrent lane', () => {
    emitModInstallProgress({
      phase: 'downloading',
      instance_id: 'a',
      project_id: 'p',
      current: 1,
      total: 2,
    });
    emitModInstallProgress({
      phase: 'downloading',
      instance_id: 'b',
      project_id: 'p',
      current: 1,
      total: 2,
    });
    expect(taskList().length).toBe(2);
    expect(taskList().every((t) => t.state === 'running')).toBe(true);
  });
});
