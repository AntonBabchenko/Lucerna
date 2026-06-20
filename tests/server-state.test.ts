import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ServerWithStatus_Serialize } from '$lib/ipc/bindings';

// Replace IPC: refresh() calls commands.serverList(); the event listeners in
// init() are never registered here (we don't call init()), but the module
// imports `events`, so it must exist on the mock.
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

function makeServer(id: string, running: boolean): ServerWithStatus_Serialize {
  return {
    id,
    name: id,
    mc_version: '1.21',
    loader: 'vanilla',
    loader_version: null,
    max_heap_mb: 2048,
    extra_jvm_args: '',
    created_unix_ms: null,
    eula_accepted: true,
    created_from_instance: null,
    running,
    pid: running ? 1234 : null,
    port: 25565,
    upload: null,
    upload_password_set: false,
    last_exit_code: null,
    diagnosis_status: 'none',
  };
}

describe('serverState.anyRunning', () => {
  beforeEach(() => {
    vi.mocked(commands.serverList).mockReset();
  });

  it('is true when at least one server is running', async () => {
    vi.mocked(commands.serverList).mockResolvedValue({
      status: 'ok',
      data: [makeServer('srv-a', false), makeServer('srv-b', true)],
    });
    await serverState.refresh();
    expect(serverState.anyRunning).toBe(true);
  });

  it('is false when no server is running', async () => {
    vi.mocked(commands.serverList).mockResolvedValue({
      status: 'ok',
      data: [makeServer('srv-a', false), makeServer('srv-b', false)],
    });
    await serverState.refresh();
    expect(serverState.anyRunning).toBe(false);
  });

  it('is false when the server list is empty', async () => {
    vi.mocked(commands.serverList).mockResolvedValue({ status: 'ok', data: [] });
    await serverState.refresh();
    expect(serverState.anyRunning).toBe(false);
  });
});
