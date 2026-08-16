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
  // The store renders BOTH halves through the shared helpers now: typed Result
  // errors keep the `err:<kind>` shape, thrown transport failures show their
  // own message (that split is what describeStoreError exists for).
  describeStoreError: (e: unknown) =>
    typeof e === 'object' && e !== null && typeof (e as { kind?: unknown }).kind === 'string'
      ? `err:${(e as { kind: string }).kind}`
      : e instanceof Error
        ? e.message
        : String(e),
  isIpcError: (e: unknown) =>
    typeof e === 'object' && e !== null && typeof (e as { kind?: unknown }).kind === 'string',
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
    const p = serverState.upload('srv-1', true, false);
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
    await serverState.upload('srv-2', true, false);
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
    await serverState.upload('srv-3', true, false);
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
    await serverState.upload('srv-5', false, false);
    expect(serverState.anyUploading).toBe(false);
  });

  it('forwards a transient password to serverUpload', async () => {
    (commands.serverUpload as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: null,
    });
    await serverState.upload('srv-pw', true, false, 'hunter2');
    expect(commands.serverUpload).toHaveBeenCalledWith('srv-pw', true, false, 'hunter2', false);
  });

  it('passes null when no password is provided', async () => {
    (commands.serverUpload as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: null,
    });
    await serverState.upload('srv-pw', true, false);
    expect(commands.serverUpload).toHaveBeenCalledWith('srv-pw', true, false, null, false);
  });

  // Regression pin for the SILENT_UPLOAD_KINDS retyping (readonly IpcError['kind'][]
  // instead of a bare string[]): the second silent kind must stay silent, because
  // the hosting tab opens the re-trust dialog for it and a persisted generic error
  // line alongside that dialog is noise. The non-silent side is pinned above by
  // 'captures an error and keeps it after the call resolves'.
  it('treats a host-key mismatch as cancelled, not an error', async () => {
    (commands.serverUpload as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'error',
      error: { kind: 'sftp_host_key_mismatch', expected: 'a', got: 'b' },
    });
    await serverState.upload('srv-hk', false, false, null);
    expect(serverState.uploadStateFor('srv-hk')?.phase).toBe('cancelled');
    expect(serverState.uploadStateFor('srv-hk')?.error).toBeUndefined();
  });

  // The one honest `unknown` in the store: typedError re-throws JS Errors, so
  // serverUpload can reject. That must land as a readable message, not "{}".
  it('a thrown transport failure renders its message, not JSON', async () => {
    (commands.serverUpload as ReturnType<typeof vi.fn>).mockRejectedValue(
      new Error('ipc channel closed'),
    );
    const r = await serverState.upload('srv-throw', false, false, null);
    expect(r.status).toBe('error');
    expect(serverState.uploadStateFor('srv-throw')?.phase).toBe('error');
    expect(serverState.uploadStateFor('srv-throw')?.error).toBe('ipc channel closed');
  });
});
