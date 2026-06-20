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
    expect(r.preview?.token).toBe('t1');
  });

  it('importInspect surfaces error on failure', async () => {
    inspect.mockResolvedValue({ status: 'error', error: { kind: 'server_import_not_a_server' } });
    const r = await serverState.importInspect('C:/x/bad');
    expect(r.ok).toBe(false);
    expect(r.error).toBeDefined();
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
  });

  it('importCancel calls the command', async () => {
    cancel.mockResolvedValue({ status: 'ok', data: null });
    await serverState.importCancel('t1');
    expect(cancel).toHaveBeenCalledWith('t1');
  });
});
