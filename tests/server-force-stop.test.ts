import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ServerWithStatus_Serialize } from '$lib/ipc/bindings';

const { serverStart, serverStop, serverKill, serverRestart, serverList, serverDiagnose } =
  vi.hoisted(() => ({
    serverStart: vi.fn(),
    serverStop: vi.fn(),
    serverKill: vi.fn(),
    serverRestart: vi.fn(),
    serverList: vi.fn(),
    serverDiagnose: vi.fn(),
  }));

vi.mock('$lib/ipc/bindings', () => ({
  commands: { serverStart, serverStop, serverKill, serverRestart, serverList, serverDiagnose },
  events: {
    serverLogLine: { listen: vi.fn() },
    serverSpawned: { listen: vi.fn() },
    serverExited: { listen: vi.fn() },
    serverUploadProgress: { listen: vi.fn() },
  },
}));

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

describe('serverState.kill (force-stop escalation)', () => {
  beforeEach(() => {
    for (const m of [serverStart, serverStop, serverKill, serverList, serverDiagnose])
      m.mockReset();
    serverList.mockResolvedValue({ status: 'ok', data: [makeServer('a', true)] });
    serverState.clearActionError('a');
  });

  it('kills and refreshes the list on success', async () => {
    serverKill.mockResolvedValue({ status: 'ok', data: null });
    const r = await serverState.kill('a');
    expect(r.ok).toBe(true);
    expect(serverKill).toHaveBeenCalledWith('a');
    expect(serverList).toHaveBeenCalled();
    expect(serverState.isKilling('a')).toBe(false);
  });

  it('is NOT blocked while a graceful stop is in flight — that is the whole escalation', async () => {
    let resolveStop!: (v: unknown) => void;
    serverStop.mockReturnValue(new Promise((res) => (resolveStop = res)));
    const stopping = serverState.stop('a');
    expect(serverState.actionFor('a')).toBe('stop');

    serverKill.mockResolvedValue({ status: 'ok', data: null });
    const killed = await serverState.kill('a'); // must proceed despite the busy stop
    expect(killed.ok).toBe(true);
    expect(serverKill).toHaveBeenCalledWith('a');

    resolveStop({ status: 'ok', data: null });
    await stopping;
  });

  it('reports isKilling while in flight and refuses a concurrent kill', async () => {
    let resolveKill!: (v: unknown) => void;
    serverKill.mockReturnValue(new Promise((res) => (resolveKill = res)));
    const p = serverState.kill('a');
    expect(serverState.isKilling('a')).toBe(true);
    const second = await serverState.kill('a'); // busy → refused, no second IPC call
    expect(second.ok).toBe(false);
    expect(serverKill).toHaveBeenCalledTimes(1);
    resolveKill({ status: 'ok', data: null });
    await p;
    expect(serverState.isKilling('a')).toBe(false);
  });

  it('a real failure records the error and does NOT diagnose (like stop)', async () => {
    serverKill.mockResolvedValue({ status: 'error', error: { kind: 'server_spawn_failed' } });
    const r = await serverState.kill('a');
    expect(r.ok).toBe(false);
    expect(serverState.actionErrorFor('a')).toEqual({ kind: 'server_spawn_failed' });
    expect(serverDiagnose).not.toHaveBeenCalled();
  });

  it('treats "already stopped" (raced graceful stop) as success, not an error', async () => {
    serverKill.mockResolvedValue({ status: 'error', error: { kind: 'server_not_running' } });
    const r = await serverState.kill('a');
    expect(r.ok).toBe(true);
    expect(serverState.actionErrorFor('a')).toBeUndefined();
    expect(serverList).toHaveBeenCalled();
  });
});
