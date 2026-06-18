import { beforeEach, describe, expect, it, vi } from 'vitest';

// Inline-factory mocks (same pattern as tests/ops/import-runner.test.ts);
// grab typed refs AFTER import to avoid any vi.mock hoisting pitfalls.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modpackFetchToTemp: vi.fn(),
    modpackComputeUpdate: vi.fn(),
    modpackApplyUpdate: vi.fn(),
  },
}));
vi.mock('$lib/ipc/format-error', () => ({
  formatError: vi.fn((e: { kind: string }) => `formatted:${e.kind}`),
}));
vi.mock('@tauri-apps/api/core', () => ({
  Channel: class {
    onmessage: ((m: unknown) => void) | null = null;
  },
}));

import type { InstanceWithStatus, ModpackVersionEntry } from '$lib/ipc/bindings';
import { commands } from '$lib/ipc/bindings';
import { createModpackUpdateFlow } from '$lib/modpacks/modpack-update-flow.svelte';

const fetchToTemp = commands.modpackFetchToTemp as ReturnType<typeof vi.fn>;
const computeUpdate = commands.modpackComputeUpdate as ReturnType<typeof vi.fn>;
const applyUpdate = commands.modpackApplyUpdate as ReturnType<typeof vi.fn>;

const inst = { id: 'i1', mrpack_source: 'modrinth', mrpack_project_id: 'p1' } as InstanceWithStatus;
const entry = { id: 'v2', version_number: '1.3.0' } as ModpackVersionEntry;
const flush = () => new Promise((r) => setTimeout(r, 0));

beforeEach(() => {
  vi.clearAllMocks();
  fetchToTemp.mockResolvedValue({ status: 'ok', data: '/tmp/p.mrpack' });
  computeUpdate.mockResolvedValue({
    status: 'ok',
    data: { added: [], removed: [], updated: [], new_version_number: '1.3.0', version_bump: null },
  });
  applyUpdate.mockResolvedValue({ status: 'ok', data: { id: 'i1' } });
});

describe('createModpackUpdateFlow', () => {
  it('prepare() fetches, computes the diff, and moves to confirming', async () => {
    const flow = createModpackUpdateFlow();
    await flow.prepare(inst, entry);
    expect(fetchToTemp).toHaveBeenCalledWith('modrinth', 'p1', 'v2');
    expect(computeUpdate).toHaveBeenCalledWith('i1', '/tmp/p.mrpack');
    expect(flow.phase).toBe('confirming');
    expect(flow.diff).not.toBeNull();
  });

  it('prepare() surfaces a fetch error and returns to idle', async () => {
    fetchToTemp.mockResolvedValue({ status: 'error', error: { kind: 'io' } });
    const flow = createModpackUpdateFlow();
    await flow.prepare(inst, entry);
    expect(flow.error).toBe('formatted:io');
    expect(flow.phase).toBe('idle');
    expect(flow.diff).toBeNull();
  });

  it('confirm() applies, maps installing_file into progress mid-flight, returns true', async () => {
    let release: (v: unknown) => void = () => {};
    let phaseCh: { onmessage: (m: unknown) => void } | null = null;
    applyUpdate.mockImplementation((...args: unknown[]) => {
      phaseCh = args[3] as { onmessage: (m: unknown) => void };
      return new Promise((r) => {
        release = r;
      });
    });
    const flow = createModpackUpdateFlow();
    await flow.prepare(inst, entry);
    const p = flow.confirm(inst);
    await flush();
    expect(flow.phase).toBe('applying');
    phaseCh!.onmessage({ phase: 'installing_file', current: 3, total: 12, file_name: 'Sodium' });
    expect(flow.progress).toEqual({ current: 3, total: 12, fileName: 'Sodium' });
    release({ status: 'ok', data: { id: 'i1' } });
    expect(await p).toBe(true);
    expect(flow.phase).toBe('idle');
  });

  it('confirm() returns false and sets error on apply failure', async () => {
    applyUpdate.mockResolvedValue({ status: 'error', error: { kind: 'io' } });
    const flow = createModpackUpdateFlow();
    await flow.prepare(inst, entry);
    const ok = await flow.confirm(inst);
    expect(ok).toBe(false);
    expect(flow.error).toBe('formatted:io');
    expect(flow.phase).toBe('idle');
  });
});
