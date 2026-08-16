import { beforeEach, describe, expect, it, vi } from 'vitest';

const { listen, inspect, commit, cancel, list } = vi.hoisted(() => ({
  listen: vi.fn().mockResolvedValue(() => {}),
  inspect: vi.fn(),
  commit: vi.fn(),
  cancel: vi.fn(),
  list: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    serverImportInspect: inspect,
    serverImportCommit: commit,
    serverImportCancel: cancel,
    serverList: list,
  },
  events: {
    serverLogLine: { listen },
    serverSpawned: { listen },
    serverExited: { listen },
    serverUploadProgress: { listen },
  },
}));

import { serverState } from '$lib/servers/server-state.svelte';

describe('serverState import wrappers', () => {
  beforeEach(() => {
    inspect.mockReset();
    commit.mockReset();
    cancel.mockReset();
  });

  it('importInspect returns the preview on ok', async () => {
    inspect.mockResolvedValue({ status: 'ok', data: { token: 't1', detected_name: 'S' } });
    const r = await serverState.importInspect('C:/x/srv.zip');
    expect(inspect).toHaveBeenCalledWith('C:/x/srv.zip');
    expect(r.ok).toBe(true);
    // Narrow before reading the payload. The wrapper returns a discriminated
    // union, so `preview` exists only on the ok arm -- and the throw makes an
    // unexpected failure blow up here instead of silently reading undefined.
    if (!r.ok) throw new Error('unreachable: mocked an ok Result');
    expect(r.preview.token).toBe('t1');
  });

  it('importInspect surfaces error on failure', async () => {
    inspect.mockResolvedValue({ status: 'error', error: { kind: 'server_import_not_a_server' } });
    const r = await serverState.importInspect('C:/x/bad');
    expect(r.ok).toBe(false);
    if (r.ok) throw new Error('unreachable: mocked an error Result');
    // Stronger than toBeDefined(): the typed error must survive the wrapper
    // unchanged rather than being flattened to `unknown`.
    expect(r.error).toEqual({ kind: 'server_import_not_a_server' });
  });

  it('importCommit calls the command with all fields and refreshes', async () => {
    commit.mockResolvedValue({ status: 'ok', data: { id: 'srv-9' } });
    const r = await serverState.importCommit(
      't1',
      'Name',
      '1.20.4',
      'fabric',
      '0.16.5',
      4096,
      true,
    );
    expect(commit).toHaveBeenCalledWith('t1', 'Name', '1.20.4', 'fabric', '0.16.5', 4096, true);
    expect(r.ok).toBe(true);
    if (!r.ok) throw new Error('unreachable: mocked an ok Result');
    // The created server is guaranteed on the ok arm -- that guarantee is what
    // let ServerImportView drop its `r.server?.id` hedge.
    expect(r.server.id).toBe('srv-9');
  });

  it('importCancel calls the command', async () => {
    cancel.mockResolvedValue({ status: 'ok', data: null });
    await serverState.importCancel('t1');
    expect(cancel).toHaveBeenCalledWith('t1');
  });
});
