import { beforeEach, describe, expect, it, vi } from 'vitest';

const scanMock = vi.fn();
const liveMock = vi.fn();
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    scanInstanceModCompat: (...a: unknown[]) => scanMock(...a),
    checkInstanceModCompat: (...a: unknown[]) => liveMock(...a),
  },
}));
vi.mock('$lib/ipc/format-error', () => ({ formatError: (e: unknown) => String(e) }));

import { createCompatCheck } from '$lib/mods/installed/compat-check.svelte';

function local(sha1: string, loader_mismatch: boolean, detected_loader: string | null = null) {
  return { sha1, loader_mismatch, detected_loader };
}
function live(sha1: string, status: 'compatible' | 'incompatible' | 'unknown') {
  return { sha1, name: sha1, status: { status } };
}

describe('createCompatCheck merge', () => {
  beforeEach(() => {
    scanMock.mockReset();
    liveMock.mockReset();
  });

  it('flags loader mismatch from the offline scan', async () => {
    scanMock.mockResolvedValue({
      status: 'ok',
      data: [local('a', true, 'Fabric'), local('b', false)],
    });
    const c = createCompatCheck(
      () => 'i1',
      () => '1.21',
      () => 'forge',
    );
    await c.runOfflineScan();
    expect([...c.incompatibleShas].sort()).toEqual(['a']);
    expect(c.incompatibleCount).toBe(1);
    expect(c.hintFor('a')).toEqual({ key: 'loader', detected: 'Fabric' });
    expect(c.hintFor('b')).toBeNull();
    c.dispose();
  });

  it('live Compatible does NOT clear a loader mismatch but refines the hint', async () => {
    scanMock.mockResolvedValue({ status: 'ok', data: [local('a', true, 'Fabric')] });
    liveMock.mockResolvedValue({ status: 'ok', data: [live('a', 'compatible')] });
    const c = createCompatCheck(
      () => 'i1',
      () => '1.21',
      () => 'forge',
    );
    await c.runOfflineScan();
    await c.runLiveCheck();
    expect([...c.incompatibleShas]).toEqual(['a']);
    expect(c.hintFor('a')).toEqual({ key: 'fixAvailable' });
    c.dispose();
  });

  it('flags a clean mod when the live check returns Incompatible', async () => {
    scanMock.mockResolvedValue({ status: 'ok', data: [local('a', false)] });
    liveMock.mockResolvedValue({ status: 'ok', data: [live('a', 'incompatible')] });
    const c = createCompatCheck(
      () => 'i1',
      () => '1.21',
      () => 'forge',
    );
    await c.runOfflineScan();
    await c.runLiveCheck();
    expect([...c.incompatibleShas]).toEqual(['a']);
    expect(c.hintFor('a')).toEqual({ key: 'noRelease' });
    c.dispose();
  });

  it('live Unknown adds no flag', async () => {
    scanMock.mockResolvedValue({ status: 'ok', data: [local('a', false)] });
    liveMock.mockResolvedValue({ status: 'ok', data: [live('a', 'unknown')] });
    const c = createCompatCheck(
      () => 'i1',
      () => '1.21',
      () => 'forge',
    );
    await c.runOfflineScan();
    await c.runLiveCheck();
    expect(c.incompatibleCount).toBe(0);
    expect(c.hintFor('a')).toBeNull();
    c.dispose();
  });
});
