import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ServerWithStatus_Serialize } from '$lib/ipc/bindings';

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    serverRename: vi.fn(),
    serverUpdateRuntimeConfig: vi.fn(),
    serverDelete: vi.fn(),
    serverList: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  },
  events: {
    serverLogLine: { listen: vi.fn().mockResolvedValue(() => {}) },
    serverSpawned: { listen: vi.fn().mockResolvedValue(() => {}) },
    serverExited: { listen: vi.fn().mockResolvedValue(() => {}) },
    serverUploadProgress: { listen: vi.fn().mockResolvedValue(() => {}) },
  },
}));

import { commands } from '$lib/ipc/bindings';
import { serverState } from '$lib/servers/server-state.svelte';

const srv = (over: Partial<ServerWithStatus_Serialize> = {}): ServerWithStatus_Serialize => ({
  id: 'srv-1',
  name: 'Old',
  mc_version: '1.21.1',
  loader: 'fabric',
  loader_version: '0.16.0',
  max_heap_mb: 4096,
  extra_jvm_args: '',
  created_unix_ms: 1,
  eula_accepted: true,
  created_from_instance: null,
  running: false,
  pid: null,
  port: null,
  upload: null,
  upload_password_set: false,
  ...over,
});

describe('serverState mutations', () => {
  beforeEach(() => {
    vi.mocked(commands.serverRename).mockReset();
    vi.mocked(commands.serverUpdateRuntimeConfig).mockReset();
    vi.mocked(commands.serverDelete).mockReset();
    vi.mocked(commands.serverList).mockResolvedValue({ status: 'ok', data: [srv()] });
  });

  it('rename calls the command and replaces the server in the list', async () => {
    await serverState.refresh();
    vi.mocked(commands.serverRename).mockResolvedValue({
      status: 'ok',
      data: srv({ name: 'New' }),
    });
    const r = await serverState.rename('srv-1', 'New');
    expect(commands.serverRename).toHaveBeenCalledWith('srv-1', 'New');
    expect(r.ok).toBe(true);
    expect(serverState.list.find((s) => s.id === 'srv-1')?.name).toBe('New');
  });

  it('updateRuntimeConfig calls the command and replaces the server', async () => {
    await serverState.refresh();
    vi.mocked(commands.serverUpdateRuntimeConfig).mockResolvedValue({
      status: 'ok',
      data: srv({ max_heap_mb: 8192 }),
    });
    const r = await serverState.updateRuntimeConfig('srv-1', 8192, '-Xfoo');
    expect(commands.serverUpdateRuntimeConfig).toHaveBeenCalledWith('srv-1', 8192, '-Xfoo');
    expect(r.ok).toBe(true);
    expect(serverState.list.find((s) => s.id === 'srv-1')?.max_heap_mb).toBe(8192);
  });

  it('remove calls delete and drops the server from the list', async () => {
    await serverState.refresh();
    vi.mocked(commands.serverDelete).mockResolvedValue({ status: 'ok', data: null });
    const r = await serverState.remove('srv-1');
    expect(commands.serverDelete).toHaveBeenCalledWith('srv-1');
    expect(r.ok).toBe(true);
    expect(serverState.list.find((s) => s.id === 'srv-1')).toBeUndefined();
  });

  it('remove surfaces the error and keeps the server when the command fails', async () => {
    await serverState.refresh();
    vi.mocked(commands.serverDelete).mockResolvedValue({
      status: 'error',
      error: { kind: 'server_already_running', id: 'srv-1' },
    });
    const r = await serverState.remove('srv-1');
    expect(r.ok).toBe(false);
    expect(serverState.list.find((s) => s.id === 'srv-1')).toBeDefined();
  });
});
