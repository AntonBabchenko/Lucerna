import { beforeEach, describe, expect, it, vi } from 'vitest';

const listeners = vi.hoisted(() => ({
  downloadProgress: null as ((event: { payload: unknown }) => void) | null,
}));

vi.mock('$lib/ipc/bindings', () => ({
  events: {
    downloadProgress: {
      listen: (cb: (event: { payload: unknown }) => void) => {
        listeners.downloadProgress = cb;
        return Promise.resolve(() => {
          listeners.downloadProgress = null;
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
import { updateState } from '$lib/update/state.svelte';
import {
  __resetAppUpdateAdapterForTest,
  ensureAppUpdateListener,
} from '$lib/tasks/adapters/app-update';

function emit(payload: unknown) {
  listeners.downloadProgress?.({ payload });
}

const INSTALLER_URL = 'https://example.com/lucerna-setup.exe';

describe('app-update adapter', () => {
  beforeEach(() => {
    __resetTasksForTest();
    __resetAppUpdateAdapterForTest();
    vi.clearAllMocks();
    listeners.downloadProgress = null;
    updateState.value = null;
    ensureAppUpdateListener();
  });

  it('ignores downloadProgress ticks for a URL other than the installer (mod/JRE downloads)', () => {
    updateState.value = {
      current: '1.0.0',
      latest: '1.1.0',
      available: true,
      release_url: 'https://example.com/releases/1.1.0',
      installer: { name: 'setup.exe', url: INSTALLER_URL, size: 1000 },
      sha256sums: null,
      cosign_bundle: null,
    };
    emit({ url: 'https://example.com/some-mod.jar', bytes_done: 10, bytes_total: 100 });
    expect(taskList().length).toBe(0);
  });

  it('starts a modal app-update task once a tick matches the installer URL', () => {
    updateState.value = {
      current: '1.0.0',
      latest: '1.1.0',
      available: true,
      release_url: 'https://example.com/releases/1.1.0',
      installer: { name: 'setup.exe', url: INSTALLER_URL, size: 1000 },
      sha256sums: null,
      cosign_bundle: null,
    };
    emit({ url: INSTALLER_URL, bytes_done: 250, bytes_total: 1000 });
    const task = taskList()[0];
    expect(task.kind).toBe('app-update');
    expect(task.lane).toBe('modal');
    expect(task.scope).toEqual({});
    expect(task.state).toBe('running');
    expect(task.progress).toEqual({ current: 250, total: 1000, unit: 'bytes' });
  });

  it('ignores every tick when no update is in flight (installer unset)', () => {
    updateState.value = null;
    emit({ url: INSTALLER_URL, bytes_done: 10, bytes_total: 100 });
    expect(taskList().length).toBe(0);
  });

  it('feeds later ticks onto the same task without minting a new one', () => {
    updateState.value = {
      current: '1.0.0',
      latest: '1.1.0',
      available: true,
      release_url: 'https://example.com/releases/1.1.0',
      installer: { name: 'setup.exe', url: INSTALLER_URL, size: 1000 },
      sha256sums: null,
      cosign_bundle: null,
    };
    emit({ url: INSTALLER_URL, bytes_done: 100, bytes_total: 1000 });
    emit({ url: INSTALLER_URL, bytes_done: 900, bytes_total: 1000 });
    expect(taskList().length).toBe(1);
    expect(taskList()[0].progress).toEqual({ current: 900, total: 1000, unit: 'bytes' });
  });

  it('never finishes the task — a successful update terminates by app.exit(0), not an event', () => {
    updateState.value = {
      current: '1.0.0',
      latest: '1.1.0',
      available: true,
      release_url: 'https://example.com/releases/1.1.0',
      installer: { name: 'setup.exe', url: INSTALLER_URL, size: 1000 },
      sha256sums: null,
      cosign_bundle: null,
    };
    emit({ url: INSTALLER_URL, bytes_done: 1000, bytes_total: 1000 });
    expect(taskList()[0].state).toBe('running');
  });
});
