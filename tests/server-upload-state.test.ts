import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    serverList: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    serverUpload: vi.fn(),
    serverCancelUpload: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
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

describe('upload state lifecycle', () => {
  beforeEach(() => vi.clearAllMocks());

  it('marks uploading, then done on success', async () => {
    (commands.serverUpload as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: null,
    });
    const p = serverState.upload('srv-1', true);
    expect(serverState.isUploading('srv-1')).toBe(true);
    expect(serverState.anyUploading).toBe(true);
    await p;
    expect(serverState.uploadStateFor('srv-1')?.phase).toBe('done');
    expect(serverState.isUploading('srv-1')).toBe(false);
  });

  it('captures an error and keeps it after the call resolves', async () => {
    (commands.serverUpload as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'error',
      error: { kind: 'sftp_auth_failed', details: 'nope' },
    });
    await serverState.upload('srv-2', true);
    const s = serverState.uploadStateFor('srv-2');
    expect(s?.phase).toBe('error');
    expect(s?.error).toBe('err:sftp_auth_failed');
    expect(serverState.isUploading('srv-2')).toBe(false);
  });

  it('transitions to cancelled when the upload returns upload_cancelled', async () => {
    (commands.serverUpload as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'error',
      error: { kind: 'upload_cancelled' },
    });
    await serverState.upload('srv-3', true);
    expect(commands.serverCancelUpload).not.toHaveBeenCalled();
    expect(serverState.uploadStateFor('srv-3')?.phase).toBe('cancelled');
    // error field is undefined for cancelled
    expect(serverState.uploadStateFor('srv-3')?.error).toBeUndefined();
  });

  it('cancelUpload delegates to the command', async () => {
    await serverState.cancelUpload('srv-4');
    expect(commands.serverCancelUpload).toHaveBeenCalledWith('srv-4');
  });

  it('anyUploading is false when all uploads have settled', async () => {
    (commands.serverUpload as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: null,
    });
    await serverState.upload('srv-5', false);
    expect(serverState.anyUploading).toBe(false);
  });
});
