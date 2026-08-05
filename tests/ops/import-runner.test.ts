import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/ipc/bindings', () => ({ commands: { modpackImport: vi.fn() } }));
vi.mock('$lib/ipc/format-error', () => ({
  formatError: vi.fn((e: { kind: string }) => `formatted:${e.kind}`),
}));
// Channel stub: capture the instances passed so the test can drive onmessage.
vi.mock('@tauri-apps/api/core', () => ({
  Channel: class {
    onmessage: ((m: unknown) => void) | null = null;
  },
}));

import { commands } from '$lib/ipc/bindings';
import { runImport } from '$lib/ops/import-runner';

const modpackImport = commands.modpackImport as ReturnType<typeof vi.fn>;

const req = {
  path: '/tmp/pack.mrpack',
  selectedShas: [],
  projectId: null,
  source: null,
  versionId: null,
};

const row = {
  name: 'capes-1.5.4+1.20.5-fabric.jar',
  install_path: 'mods/capes-1.5.4+1.20.5-fabric.jar',
  origin: 'modrinth',
  host: 'cdn.modrinth.com',
  bytes: 77259,
  sha1: 'b8a4d24d',
  outcome: { kind: 'installed', fetched: 'cached', placement: 'linked' },
};

/** Resolve the command WITHOUT ever driving the phase channel.
 *
 *  This is the production ordering, not a convenience: Tauri delivers a
 *  channel payload of 8192 bytes or more through a second async IPC round
 *  trip that lands after the command's own response, so by the time the
 *  runner reads its result nothing has arrived. Any outcome field that a
 *  test can only satisfy by calling `onmessage` first is a field the real
 *  app loses. */
function resolvesWithoutAnyChannelMessage(data: unknown) {
  modpackImport.mockResolvedValue({ status: 'ok', data });
}

beforeEach(() => vi.clearAllMocks());

describe('runImport', () => {
  it('takes the whole outcome from the command result, never from a done message', async () => {
    resolvesWithoutAnyChannelMessage({
      instance: { name: 'My Pack', id: 'i1' },
      skipped_overrides: [{ path: 'overrides/big.zip', size: 1024 }],
      inert_loader_jars: [{ filename: 'x-Fabric.jar', detected_loader: 'Fabric' }],
      details: [row],
    });

    const out = await runImport(req, () => {});

    expect(out).toEqual({
      status: 'ok',
      name: 'My Pack',
      instanceId: 'i1',
      skipped: [{ path: 'overrides/big.zip', size: 1024 }],
      inertLoaderJars: [{ filename: 'x-Fabric.jar', detected_loader: 'Fabric' }],
      details: [row],
    });
  });

  it('reports the report rows even when the pack installed nothing else of note', async () => {
    resolvesWithoutAnyChannelMessage({
      instance: { name: 'My Pack', id: 'i1' },
      skipped_overrides: [],
      inert_loader_jars: [],
      details: [row],
    });

    const out = await runImport(req, () => {});

    expect((out as { details: unknown }).details).toEqual([row]);
  });

  it('maps modpack_partial_failure to a partial outcome with file basenames', async () => {
    modpackImport.mockResolvedValue({
      status: 'error',
      error: { kind: 'modpack_partial_failure', failed: [['mods/x.jar', 'e']] },
    });
    const out = await runImport(req, () => {});
    expect(out).toEqual({ status: 'partial', failed: ['x.jar'] });
  });

  it('maps any other error to an error outcome via formatError', async () => {
    modpackImport.mockResolvedValue({ status: 'error', error: { kind: 'io' } });
    const out = await runImport(req, () => {});
    expect(out).toEqual({ status: 'error', message: 'formatted:io' });
  });

  it('streams phase + bytes to onProgress', async () => {
    const seen: Array<[unknown, unknown]> = [];
    modpackImport.mockImplementation(async (...args: unknown[]) => {
      const phaseCh = args[6] as { onmessage: (m: unknown) => void };
      const tickCh = args[7] as { onmessage: (m: unknown) => void };
      phaseCh.onmessage({ phase: 'enriching' });
      tickCh.onmessage({ current: 5, total: 10 });
      return {
        status: 'ok',
        data: {
          instance: { name: 'P', id: 'i1' },
          skipped_overrides: [],
          inert_loader_jars: [],
          details: [],
        },
      };
    });
    await runImport(req, (phase, bytes) => seen.push([phase, bytes]));
    // First call resets to (null,null); then phase, then bytes.
    expect(seen).toContainEqual([{ phase: 'enriching' }, null]);
    expect(seen).toContainEqual([{ phase: 'enriching' }, { current: 5, total: 10 }]);
  });
});
