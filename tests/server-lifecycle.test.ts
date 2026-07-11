import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ServerWithStatus_Serialize } from '$lib/ipc/bindings';

const { serverStart, serverStop, serverRestart, serverList, serverDiagnose, serverDelete } =
  vi.hoisted(() => ({
    serverStart: vi.fn(),
    serverStop: vi.fn(),
    serverRestart: vi.fn(),
    serverList: vi.fn(),
    serverDiagnose: vi.fn(),
    serverDelete: vi.fn(),
  }));

vi.mock('$lib/ipc/bindings', () => ({
  commands: { serverStart, serverStop, serverRestart, serverList, serverDiagnose, serverDelete },
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

describe('serverState lifecycle helpers', () => {
  beforeEach(() => {
    serverStart.mockReset();
    serverStop.mockReset();
    serverRestart.mockReset();
    serverList.mockReset();
    serverDiagnose.mockReset();
    serverDelete.mockReset();
    serverList.mockResolvedValue({ status: 'ok', data: [makeServer('a', true)] });
    serverDiagnose.mockResolvedValue({ status: 'error', error: { kind: 'x' } });
    serverState.clearActionError('a');
  });

  it('start: happy path refreshes the list and reports ok', async () => {
    serverStart.mockResolvedValue({ status: 'ok', data: 4321 });
    const r = await serverState.start('a');
    expect(r.ok).toBe(true);
    expect(serverStart).toHaveBeenCalledWith('a');
    expect(serverList).toHaveBeenCalled();
    expect(serverState.actionFor('a')).toBeNull();
    expect(serverState.actionErrorFor('a')).toBeUndefined();
  });

  it('start: failure records the error, diagnoses, does not refresh', async () => {
    serverStart.mockResolvedValue({
      status: 'error',
      error: { kind: 'server_spawn_failed', details: 'no' },
    });
    const r = await serverState.start('a');
    expect(r.ok).toBe(false);
    expect(serverState.actionErrorFor('a')).toEqual({ kind: 'server_spawn_failed', details: 'no' });
    expect(serverDiagnose).toHaveBeenCalledWith('a');
    expect(serverList).not.toHaveBeenCalled();
  });

  it('start: a thrown (non-Result) IPC error lands in actionErrors, not a rejection', async () => {
    const boom = new Error('boom');
    serverStart.mockRejectedValue(boom);
    const r = await serverState.start('a');
    expect(r.ok).toBe(false);
    expect(serverState.actionErrorFor('a')).toBe(boom);
    expect(serverDiagnose).toHaveBeenCalledWith('a');
    expect(serverState.actionFor('a')).toBeNull();
  });

  it('stop: failure records the error and does NOT diagnose', async () => {
    serverStop.mockResolvedValue({ status: 'error', error: { kind: 'x' } });
    const r = await serverState.stop('a');
    expect(r.ok).toBe(false);
    expect(serverState.actionErrorFor('a')).toEqual({ kind: 'x' });
    expect(serverDiagnose).not.toHaveBeenCalled();
  });

  it('restart: happy path', async () => {
    serverRestart.mockResolvedValue({ status: 'ok', data: 4321 });
    const r = await serverState.restart('a');
    expect(r.ok).toBe(true);
    expect(serverRestart).toHaveBeenCalledWith('a');
  });

  it('actionFor reports the in-flight action and rejects a concurrent call', async () => {
    let resolveStart!: (v: unknown) => void;
    serverStart.mockReturnValue(new Promise((res) => (resolveStart = res)));
    const p = serverState.start('a');
    expect(serverState.actionFor('a')).toBe('start');
    const second = await serverState.stop('a'); // busy — refused
    expect(second.ok).toBe(false);
    expect(serverStop).not.toHaveBeenCalled();
    resolveStart({ status: 'ok', data: 1 });
    await p;
    expect(serverState.actionFor('a')).toBeNull();
  });

  it('remove clears the per-id action error so a recreated same-slug server starts clean', async () => {
    serverStart.mockResolvedValue({ status: 'error', error: { kind: 'x' } });
    await serverState.start('a');
    expect(serverState.actionErrorFor('a')).toBeDefined();

    serverDelete.mockResolvedValue({ status: 'ok', data: null });
    await serverState.remove('a');
    expect(serverState.actionErrorFor('a')).toBeUndefined();
  });

  it('clearActionError removes a recorded error', async () => {
    serverStart.mockResolvedValue({ status: 'error', error: { kind: 'x' } });
    await serverState.start('a');
    expect(serverState.actionErrorFor('a')).toBeDefined();
    serverState.clearActionError('a');
    expect(serverState.actionErrorFor('a')).toBeUndefined();
  });
});
