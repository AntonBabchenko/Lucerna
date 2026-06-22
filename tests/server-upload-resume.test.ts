import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    serverList: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    serverUpload: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    serverCancelUpload: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    serverUploadResumeState: vi.fn(),
  },
  events: {
    serverLogLine: { listen: vi.fn() },
    serverSpawned: { listen: vi.fn() },
    serverExited: { listen: vi.fn() },
    serverUploadProgress: { listen: vi.fn() },
  },
}));

// formatError calls get(t) from i18n which is not available in tests.
vi.mock('$lib/ipc/format-error', () => ({
  formatError: (e: { kind: string }) => `err:${e.kind}`,
}));

import { commands } from '$lib/ipc/bindings';
import { serverState } from '$lib/servers/server-state.svelte';

describe('upload resume', () => {
  beforeEach(() => vi.clearAllMocks());

  it('maps a resumable backend snapshot to UploadResumeInfo', async () => {
    (commands.serverUploadResumeState as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: { resumable: true, files_total: 100, files_done: 42, bytes_total: 5_000_000 },
    });
    const info = await serverState.uploadResumeState('srv-1');
    expect(info).toEqual({
      resumable: true,
      filesTotal: 100,
      filesDone: 42,
      bytesTotal: 5_000_000,
    });
  });

  it('returns null when no resumable upload exists', async () => {
    (commands.serverUploadResumeState as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: { resumable: false, files_total: 0, files_done: 0, bytes_total: 0 },
    });
    expect(await serverState.uploadResumeState('srv-2')).toBeNull();
  });

  it('returns null on IPC error', async () => {
    (commands.serverUploadResumeState as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'error',
      error: { kind: 'io', details: 'x' },
    });
    expect(await serverState.uploadResumeState('srv-3')).toBeNull();
  });

  // The real binding is serverUpload(id, acceptNewHostKey, skipWorlds, password, resume).
  // `upload(id, accept, skipWorlds, password?, resume = false)` forwards all five.
  it('upload forwards the resume flag to the command', async () => {
    await serverState.upload('srv-4', false, false, null, true);
    expect(commands.serverUpload).toHaveBeenCalledWith('srv-4', false, false, null, true);
  });

  it('upload defaults resume to false', async () => {
    await serverState.upload('srv-5', true, false);
    expect(commands.serverUpload).toHaveBeenCalledWith('srv-5', true, false, null, false);
  });
});
