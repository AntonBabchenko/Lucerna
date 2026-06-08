import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ModSource } from '$lib/ipc/bindings';

const scanMock = vi.fn();
const versionsMock = vi.fn();
const liveMock = vi.fn();
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    scanInstanceModCompat: (...a: unknown[]) => scanMock(...a),
    modsVersions: (...a: unknown[]) => versionsMock(...a),
    checkInstanceModCompat: (...a: unknown[]) => liveMock(...a),
  },
}));
vi.mock('$lib/ipc/format-error', () => ({ formatError: (e: unknown) => String(e) }));

import { createCompatCheck } from '$lib/mods/installed/compat-check.svelte';

function lc(
  sha: string,
  loader_mismatch: boolean,
  live_checkable: boolean,
  detected_loader: string | null = null,
) {
  return { sha1: sha, loader_mismatch, live_checkable, detected_loader };
}
function row(sha: string, source: ModSource | null, project_id: string | null) {
  return { installed: { sha1: sha, source, project_id } };
}

describe('createCompatCheck two-stage pipeline', () => {
  beforeEach(() => {
    scanMock.mockReset();
    versionsMock.mockReset();
    liveMock.mockReset();
  });

  it('manual suspect is flagged from the offline verdict (loader hint)', async () => {
    scanMock.mockResolvedValue({ status: 'ok', data: [lc('a', true, false, 'Fabric')] });
    const c = createCompatCheck(
      () => 'i',
      () => '1.21',
      () => 'forge',
      () => [row('a', null, null)],
    );
    await c.runOfflineScan();
    expect([...c.incompatibleShas]).toEqual(['a']);
    expect(c.hintFor('a')).toEqual({ key: 'loader', detected: 'Fabric' });
    expect(versionsMock).not.toHaveBeenCalled();
    c.dispose();
  });

  it('platform suspect auto-confirmed compatible -> NOT flagged', async () => {
    scanMock.mockResolvedValue({ status: 'ok', data: [lc('a', true, true, 'Fabric')] });
    versionsMock.mockResolvedValue({ status: 'ok', data: [{ version_id: 'v1' }] });
    const c = createCompatCheck(
      () => 'i',
      () => '1.21',
      () => 'forge',
      () => [row('a', 'modrinth', 'px')],
    );
    await c.runOfflineScan();
    expect(c.incompatibleCount).toBe(0);
    expect(c.hintFor('a')).toBeNull();
    expect(versionsMock).toHaveBeenCalledWith('modrinth', 'px', '1.21', 'forge');
    c.dispose();
  });

  it('platform suspect auto-confirmed incompatible -> flagged (noRelease)', async () => {
    scanMock.mockResolvedValue({ status: 'ok', data: [lc('a', true, true, 'Forge')] });
    versionsMock.mockResolvedValue({ status: 'ok', data: [] });
    const c = createCompatCheck(
      () => 'i',
      () => '1.21',
      () => 'forge',
      () => [row('a', 'modrinth', 'px')],
    );
    await c.runOfflineScan();
    expect([...c.incompatibleShas]).toEqual(['a']);
    expect(c.hintFor('a')).toEqual({ key: 'noRelease' });
    c.dispose();
  });

  it('platform suspect auto-confirm fails (429/error) -> NOT flagged', async () => {
    scanMock.mockResolvedValue({ status: 'ok', data: [lc('a', true, true, 'Forge')] });
    versionsMock.mockResolvedValue({ status: 'error', error: 'HTTP 429' });
    const c = createCompatCheck(
      () => 'i',
      () => '1.21',
      () => 'forge',
      () => [row('a', 'modrinth', 'px')],
    );
    await c.runOfflineScan();
    expect(c.incompatibleCount).toBe(0);
    c.dispose();
  });

  it('non-suspect is not flagged and fires no live query', async () => {
    scanMock.mockResolvedValue({ status: 'ok', data: [lc('a', false, true)] });
    const c = createCompatCheck(
      () => 'i',
      () => '1.21',
      () => 'forge',
      () => [row('a', 'modrinth', 'px')],
    );
    await c.runOfflineScan();
    expect(c.incompatibleCount).toBe(0);
    expect(versionsMock).not.toHaveBeenCalled();
    c.dispose();
  });

  it('drops a superseded scan: a newer run wins, the stale write is dropped', async () => {
    scanMock.mockResolvedValue({ status: 'ok', data: [lc('a', true, true, 'Fabric')] });
    // Run A's live query hangs; run B's resolves "compatible".
    let releaseA: (v: unknown) => void = () => {};
    const aPending = new Promise((res) => {
      releaseA = res;
    });
    versionsMock
      .mockImplementationOnce(() => aPending)
      .mockResolvedValue({ status: 'ok', data: [{ version_id: 'v' }] });
    const c = createCompatCheck(
      () => 'i',
      () => '1.21',
      () => 'forge',
      () => [row('a', 'modrinth', 'px')],
    );
    const runA = c.runOfflineScan(); // will reach autoConfirm and park on the pending query
    // Flush microtasks so A gets past its offline scan and into the live query
    // (consuming the pending mock) BEFORE B starts.
    await new Promise((r) => setTimeout(r, 0));
    const runB = c.runOfflineScan(); // supersedes A (bumps generation)
    await runB; // B confirms 'a' compatible
    releaseA({ status: 'ok', data: [] }); // A would resolve to "incompatible"...
    await runA; // ...but A is stale → its write is dropped by the generation guard
    expect(c.incompatibleCount).toBe(0);
    c.dispose();
  });
});
