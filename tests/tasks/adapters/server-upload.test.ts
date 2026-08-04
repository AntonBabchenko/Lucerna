import { beforeEach, describe, expect, it, vi } from 'vitest';

const listeners = vi.hoisted(() => ({
  serverUploadProgress: null as ((event: { payload: unknown }) => void) | null,
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    serverUpload: vi.fn(),
  },
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

import { commands } from '$lib/ipc/bindings';
import {
  __resetServerUploadAdapterForTest,
  ensureServerUploadListener,
  uploadToServer,
} from '$lib/tasks/adapters/server-upload';
import { __resetTasksForTest, taskList } from '$lib/tasks/registry.svelte';

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

describe('server-upload adapter — passive listener (progress-only, for uploads not made through uploadToServer)', () => {
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

describe('server-upload adapter — uploadToServer (call-wrapping, detects failure)', () => {
  beforeEach(() => {
    __resetTasksForTest();
    __resetServerUploadAdapterForTest();
    vi.clearAllMocks();
    listeners.serverUploadProgress = null;
  });

  it('starts a concurrent server-upload task scoped to the server', async () => {
    vi.mocked(commands.serverUpload).mockResolvedValue({ status: 'ok', data: null } as never);
    await uploadToServer('s1', 'My Server', false, false, null, false);
    const task = taskList()[0];
    expect(task.kind).toBe('server-upload');
    expect(task.lane).toBe('concurrent');
    expect(task.scope).toEqual({ serverId: 's1' });
    expect(task.state).toBe('ok');
  });

  it('feeds progress ticks filtered to the server onto the task', async () => {
    vi.mocked(commands.serverUpload).mockImplementation(async () => {
      emit(tick('s1', 1, 10, 1000, 100_000));
      return { status: 'ok', data: null };
    });
    await uploadToServer('s1', 'My Server', false, false, null, false);
    expect(taskList()[0].progress).toEqual({ current: 1000, total: 100_000, unit: 'bytes' });
  });

  it('ignores a progress tick for a different server', async () => {
    vi.mocked(commands.serverUpload).mockImplementation(async () => {
      emit(tick('OTHER', 1, 10, 1000, 100_000));
      return { status: 'ok', data: null };
    });
    await uploadToServer('s1', 'My Server', false, false, null, false);
    expect(taskList()[0].progress).toBeNull();
  });

  it('a failed upload produces state failed, not a task stuck running — the gap this adapter used to have', async () => {
    vi.mocked(commands.serverUpload).mockResolvedValue({
      status: 'error',
      error: { kind: 'sftp_transfer_failed', details: 'connection reset' },
    } as never);

    const outcome = await uploadToServer('s1', 'My Server', false, false, null, false);
    expect(taskList()[0].state).toBe('failed');
    expect(outcome.status).toBe('error');
  });

  it('classifies upload_cancelled as cancelled, not failed — no double-report with the re-trust/cancel UI', async () => {
    vi.mocked(commands.serverUpload).mockResolvedValue({
      status: 'error',
      error: { kind: 'upload_cancelled' },
    } as never);

    const outcome = await uploadToServer('s1', 'My Server', false, false, null, false);
    expect(taskList()[0].state).toBe('cancelled');
    expect(taskList()[0].state).not.toBe('failed');
    expect(outcome.status).toBe('cancelled');
  });

  it('classifies sftp_host_key_mismatch as cancelled, not failed — the re-trust dialog owns that message', async () => {
    vi.mocked(commands.serverUpload).mockResolvedValue({
      status: 'error',
      error: { kind: 'sftp_host_key_mismatch' },
    } as never);

    const outcome = await uploadToServer('s1', 'My Server', false, false, null, false);
    expect(taskList()[0].state).toBe('cancelled');
    expect(taskList()[0].state).not.toBe('failed');
    expect(outcome.status).toBe('cancelled');
  });

  it('marks the task failed when the command throws (bridge failure)', async () => {
    vi.mocked(commands.serverUpload).mockRejectedValue(new Error('bridge died'));
    await uploadToServer('s1', 'My Server', false, false, null, false);
    expect(taskList()[0].state).toBe('failed');
  });

  it('unsubscribes its listener once the call settles', async () => {
    vi.mocked(commands.serverUpload).mockResolvedValue({ status: 'ok', data: null } as never);
    await uploadToServer('s1', 'My Server', false, false, null, false);
    expect(listeners.serverUploadProgress).toBeNull();
  });
});
