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

function makeServer(
  id: string,
  running: boolean,
  lastExitCode: number | null = null,
  diagnosisStatus: ServerWithStatus_Serialize['diagnosis_status'] = 'none',
): ServerWithStatus_Serialize {
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
    last_exit_code: lastExitCode,
    diagnosis_status: diagnosisStatus,
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

describe('serverState.serversNavStatus', () => {
  beforeEach(() => {
    vi.mocked(commands.serverList).mockReset();
  });

  async function load(data: ServerWithStatus_Serialize[]) {
    vi.mocked(commands.serverList).mockResolvedValue({ status: 'ok', data });
    await serverState.refresh();
  }

  it("is 'fixable' when a server has an actionable diagnosis, outranking a running server", async () => {
    await load([
      makeServer('running', true),
      makeServer('broken', false, 1, 'actionable'),
    ]);
    expect(serverState.serversNavStatus).toBe('fixable');
  });

  it("is 'crashed' for a non-zero exit with no actionable fix, outranking running", async () => {
    await load([
      makeServer('running', true),
      makeServer('dead', false, 1, 'none'),
    ]);
    expect(serverState.serversNavStatus).toBe('crashed');
  });

  it("is 'running' when a server runs and none crashed/fixable", async () => {
    await load([makeServer('up', true), makeServer('stopped', false, 0, 'none')]);
    expect(serverState.serversNavStatus).toBe('running');
  });

  it("is 'idle' when all servers are cleanly stopped", async () => {
    await load([makeServer('a', false, null, 'none'), makeServer('b', false, 0, 'none')]);
    expect(serverState.serversNavStatus).toBe('idle');
  });

  it("is 'idle' for an empty list", async () => {
    await load([]);
    expect(serverState.serversNavStatus).toBe('idle');
  });
});
