import { beforeEach, describe, expect, it, vi } from 'vitest';

// refresh() calls commands.serverList(); init()'s event listeners aren't
// registered (we never call init()), but the module imports `events`.
vi.mock('$lib/ipc/bindings', () => ({
  commands: { serverList: vi.fn() },
  events: {
    serverLogLine: { listen: vi.fn() },
    serverSpawned: { listen: vi.fn() },
    serverExited: { listen: vi.fn() },
    serverUploadProgress: { listen: vi.fn() },
  },
}));

import { commands } from '$lib/ipc/bindings';
import { serverState } from '$lib/servers/server-state.svelte';

describe('serverState list load state (#23)', () => {
  beforeEach(() => {
    vi.mocked(commands.serverList).mockReset();
  });

  it('clears the error and finishes loading on a successful refresh', async () => {
    vi.mocked(commands.serverList).mockResolvedValue({ status: 'ok', data: [] });
    await serverState.refresh();
    expect(serverState.listError).toBeNull();
    expect(serverState.listLoading).toBe(false);
  });

  it('records the error instead of silently rendering empty', async () => {
    vi.mocked(commands.serverList).mockResolvedValue({
      status: 'error',
      error: { kind: 'Io', message: 'disk gone' } as unknown as never,
    });
    await serverState.refresh();
    expect(serverState.listError).not.toBeNull();
    expect(serverState.listLoading).toBe(false);
  });

  it('a thrown (non-Result) IPC error lands in listError, not a rejection', async () => {
    const boom = new Error('boom');
    vi.mocked(commands.serverList).mockRejectedValue(boom);
    // Must resolve (no unhandled rejection) — a `void`-ing caller would
    // otherwise swallow the throw with no user-visible trace.
    await serverState.refresh();
    expect(serverState.listError).toBe(boom);
    expect(serverState.listLoading).toBe(false);
    expect(serverState.listLoadedOnce).toBe(true);
  });

  it('clears a prior error when a later refresh succeeds', async () => {
    vi.mocked(commands.serverList).mockResolvedValue({
      status: 'error',
      error: { kind: 'Io', message: 'transient' } as unknown as never,
    });
    await serverState.refresh();
    expect(serverState.listError).not.toBeNull();

    vi.mocked(commands.serverList).mockResolvedValue({ status: 'ok', data: [] });
    await serverState.refresh();
    expect(serverState.listError).toBeNull();
  });
});
