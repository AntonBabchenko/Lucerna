import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/ipc/bindings', () => ({ commands: { modpackApplyUpdate: vi.fn() } }));
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
import { runUpdate } from '$lib/modpacks/update-runner';

const modpackApplyUpdate = commands.modpackApplyUpdate as ReturnType<typeof vi.fn>;

beforeEach(() => vi.clearAllMocks());

describe('runUpdate', () => {
  it('returns the instance, the inert jars and the report rows from r.data', async () => {
    // All three are RESULTS. They travel on the command's return value, never
    // on the phase channel: a channel payload of 8192 bytes or more is
    // delivered by a second async IPC round trip that lands after this promise
    // resolves, which is exactly how the install report used to come back
    // empty. This mock never touches `onmessage` on purpose.
    const row = {
      name: 'Sodium',
      install_path: 'mods/sodium.jar',
      origin: 'modrinth',
      host: 'cdn.modrinth.com',
      bytes: 123,
      sha1: 'abc',
      outcome: { kind: 'installed', fetched: 'downloaded', placement: 'linked' },
    };
    modpackApplyUpdate.mockResolvedValue({
      status: 'ok',
      data: {
        instance: { id: 'i1', name: 'Pack' },
        inert_loader_jars: [{ filename: 'a-Fabric.jar', detected_loader: 'Fabric' }],
        details: [row],
      },
    });
    const out = await runUpdate('i1', '/tmp/p.mrpack', 'v2', () => {});
    expect(out).toEqual({
      status: 'ok',
      inst: { id: 'i1', name: 'Pack' },
      inertLoaderJars: [{ filename: 'a-Fabric.jar', detected_loader: 'Fabric' }],
      details: [row],
    });
  });

  it('maps an error result via formatError', async () => {
    modpackApplyUpdate.mockResolvedValue({ status: 'error', error: { kind: 'io' } });
    const out = await runUpdate('i1', '/tmp/p.mrpack', 'v2', () => {});
    expect(out).toEqual({ status: 'error', message: 'formatted:io' });
  });

  it('streams phase + bytes to onProgress (latest-wins, reset first)', async () => {
    const seen: Array<[unknown, unknown]> = [];
    modpackApplyUpdate.mockImplementation(async (...args: unknown[]) => {
      const phaseCh = args[3] as { onmessage: (m: unknown) => void };
      const tickCh = args[4] as { onmessage: (m: unknown) => void };
      phaseCh.onmessage({ phase: 'installing_file', current: 3, total: 12, file_name: 'Sodium' });
      tickCh.onmessage({ phase: 'downloading', current: 5, total: 10 });
      return {
        status: 'ok',
        data: { instance: { id: 'i1' }, inert_loader_jars: [], details: [] },
      };
    });
    await runUpdate('i1', '/tmp/p.mrpack', 'v2', (phase, bytes) => seen.push([phase, bytes]));
    expect(seen[0]).toEqual([null, null]);
    expect(seen).toContainEqual([
      { phase: 'installing_file', current: 3, total: 12, file_name: 'Sodium' },
      null,
    ]);
    expect(seen).toContainEqual([
      { phase: 'installing_file', current: 3, total: 12, file_name: 'Sodium' },
      { phase: 'downloading', current: 5, total: 10 },
    ]);
  });
});
