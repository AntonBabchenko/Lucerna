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

beforeEach(() => vi.clearAllMocks());

describe('runImport', () => {
  it('returns ok with the instance id from r.data.id', async () => {
    modpackImport.mockImplementation(async (...args: unknown[]) => {
      const phaseCh = args[6] as { onmessage: (m: unknown) => void };
      phaseCh.onmessage({ phase: 'done', skipped_overrides: [] });
      return { status: 'ok', data: { name: 'My Pack', id: 'i1' } };
    });
    const out = await runImport(req, () => {});
    expect(out).toEqual({ status: 'ok', name: 'My Pack', instanceId: 'i1', skipped: [] });
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
      return { status: 'ok', data: { name: 'P' } };
    });
    await runImport(req, (phase, bytes) => seen.push([phase, bytes]));
    // First call resets to (null,null); then phase, then bytes.
    expect(seen).toContainEqual([{ phase: 'enriching' }, null]);
    expect(seen).toContainEqual([{ phase: 'enriching' }, { current: 5, total: 10 }]);
  });
});
