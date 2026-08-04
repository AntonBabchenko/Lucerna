import { beforeEach, describe, expect, it, vi } from 'vitest';

const listeners = vi.hoisted(() => ({
  downloadProgress: null as ((event: { payload: unknown }) => void) | null,
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    updateInstall: vi.fn(),
  },
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

import { commands } from '$lib/ipc/bindings';
import {
  __resetAppUpdateAdapterForTest,
  ensureAppUpdateListener,
  runAppUpdate,
} from '$lib/tasks/adapters/app-update';
import { __resetTasksForTest, taskList } from '$lib/tasks/registry.svelte';
import { updateState } from '$lib/update/state.svelte';

function emit(payload: unknown) {
  listeners.downloadProgress?.({ payload });
}

const INSTALLER_URL = 'https://example.com/lucerna-setup.exe';
const withInstaller = () => {
  updateState.value = {
    current: '1.0.0',
    latest: '1.1.0',
    available: true,
    release_url: 'https://example.com/releases/1.1.0',
    installer: { name: 'setup.exe', url: INSTALLER_URL, size: 1000 },
    sha256sums: null,
    cosign_bundle: null,
  };
};

describe('app-update adapter — passive listener (progress-only, for updates not made through runAppUpdate)', () => {
  beforeEach(() => {
    __resetTasksForTest();
    __resetAppUpdateAdapterForTest();
    vi.clearAllMocks();
    listeners.downloadProgress = null;
    updateState.value = null;
    ensureAppUpdateListener();
  });

  it('ignores downloadProgress ticks for a URL other than the installer (mod/JRE downloads)', () => {
    withInstaller();
    emit({ url: 'https://example.com/some-mod.jar', bytes_done: 10, bytes_total: 100 });
    expect(taskList().length).toBe(0);
  });

  it('starts a modal app-update task once a tick matches the installer URL', () => {
    withInstaller();
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
    withInstaller();
    emit({ url: INSTALLER_URL, bytes_done: 100, bytes_total: 1000 });
    emit({ url: INSTALLER_URL, bytes_done: 900, bytes_total: 1000 });
    expect(taskList().length).toBe(1);
    expect(taskList()[0].progress).toEqual({ current: 900, total: 1000, unit: 'bytes' });
  });

  it('never finishes the task — a successful update terminates by app.exit(0), not an event', () => {
    withInstaller();
    emit({ url: INSTALLER_URL, bytes_done: 1000, bytes_total: 1000 });
    expect(taskList()[0].state).toBe('running');
  });
});

describe('app-update adapter — runAppUpdate (call-wrapping, detects failure)', () => {
  beforeEach(() => {
    __resetTasksForTest();
    __resetAppUpdateAdapterForTest();
    vi.clearAllMocks();
    listeners.downloadProgress = null;
    updateState.value = null;
  });

  it('starts a modal app-update task', async () => {
    vi.mocked(commands.updateInstall).mockReturnValue(new Promise(() => {}) as never);
    void runAppUpdate();
    await Promise.resolve();
    const task = taskList()[0];
    expect(task.kind).toBe('app-update');
    expect(task.lane).toBe('modal');
    expect(task.scope).toEqual({});
    expect(task.state).toBe('running');
  });

  it('a rejected/error Result from updateInstall reaches finish failed — the gap this adapter used to have', async () => {
    vi.mocked(commands.updateInstall).mockResolvedValue({
      status: 'error',
      error: { kind: 'update_install_failed', details: 'disk full' },
    } as never);

    const outcome = await runAppUpdate();
    expect(taskList()[0].state).toBe('failed');
    expect(outcome.status).toBe('error');
  });

  it('marks the task failed when the command throws (bridge failure)', async () => {
    vi.mocked(commands.updateInstall).mockRejectedValue(new Error('bridge died'));
    await runAppUpdate();
    expect(taskList()[0].state).toBe('failed');
  });

  it('feeds progress ticks matching the installer URL onto the task while the call is in flight', async () => {
    withInstaller();
    vi.mocked(commands.updateInstall).mockImplementation(async () => {
      emit({ url: INSTALLER_URL, bytes_done: 400, bytes_total: 1000 });
      return { status: 'error', error: { kind: 'update_install_failed', details: 'x' } };
    });
    await runAppUpdate();
    expect(taskList()[0].progress).toEqual({ current: 400, total: 1000, unit: 'bytes' });
  });

  it('does NOT invent a success completion on an ok Result — leaves the task running, same as the passive listener does on success', async () => {
    vi.mocked(commands.updateInstall).mockResolvedValue({ status: 'ok', data: null } as never);
    const outcome = await runAppUpdate();
    expect(taskList()[0].state).toBe('running');
    expect(outcome.status).toBe('ok');
  });
});
